/// Hypixel API client with built-in caching and rate limiting.
///
/// The client wraps `reqwest::Client` and adds:
/// - A `TimedCache` so repeated lookups for the same UUID within ~60 seconds
///   return cached results without hitting the API.
/// - A `tokio::sync::Semaphore` that limits concurrent requests to avoid
///   exceeding Hypixel's rate limits (~120 requests/minute).
/// - A `known_bedwars_stat_keys` cache that grows over time as players are
///   fetched, providing autocomplete data for the `/edit-stats` command.
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Client;
use std::collections::HashMap;
use tokio::sync::{RwLock, Semaphore};
use tracing::debug;

use super::models::{BedwarsStats, HypixelPlayerResponse, MojangProfile, PlayerData};
use crate::shared::cache::TimedCache;

/// Default cache TTL for Hypixel stat lookups (60 seconds).
const CACHE_TTL_SECS: u64 = 60;

/// Maximum number of concurrent Hypixel API requests.
const MAX_CONCURRENT_REQUESTS: usize = 2;

/// The Hypixel API client.
pub struct HypixelClient {
    /// Underlying HTTP client (connection pooling handled internally).
    http: Client,

    /// Hypixel API key sent in the `API-Key` header.
    api_key: String,

    /// TTL cache keyed by Minecraft UUID.
    cache: TimedCache<String, PlayerData>,

    /// Semaphore used to limit concurrent outgoing requests.
    rate_limiter: Semaphore,

    /// All Bedwars stat keys seen across every `fetch_player` call, sorted
    /// alphabetically. This grows monotonically as more players are fetched.
    /// Used as autocomplete data for the `/edit-stats` command.
    pub known_bedwars_stat_keys: Arc<RwLock<Vec<String>>>,
}

impl HypixelClient {
    /// Create a new client with the given API key.
    pub fn new(api_key: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
            cache: TimedCache::new(Duration::from_secs(CACHE_TTL_SECS)),
            rate_limiter: Semaphore::new(MAX_CONCURRENT_REQUESTS),
            known_bedwars_stat_keys: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Resolve a Minecraft username to a UUID via the Mojang API.
    ///
    /// Returns the UUID as a dashless hex string.
    pub async fn resolve_username(&self, username: &str) -> Result<MojangProfile> {
        let url = format!(
            "https://api.mojang.com/users/profiles/minecraft/{}",
            username
        );

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("Failed to contact Mojang API")?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("Minecraft user '{}' not found", username);
        }

        let profile: MojangProfile = resp
            .json()
            .await
            .context("Failed to parse Mojang API response")?;

        debug!(username, uuid = %profile.id, "Resolved Minecraft username");
        Ok(profile)
    }

    pub async fn resolve_uuid(&self, uuid: &str) -> Result<String> {
        let url = format!(
            "https://sessionserver.mojang.com/session/minecraft/profile/{}",
            uuid
        );

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("Failed to contact Mojang session API")?;

        if !resp.status().is_success() {
            bail!("Failed to resolve UUID {}", uuid);
        }

        #[derive(serde::Deserialize)]
        struct Profile {
            name: String,
        }

        let profile: Profile = resp
            .json()
            .await
            .context("Failed to parse Mojang session response")?;

        Ok(profile.name)
    }

    pub async fn fetch_player(self: &Arc<Self>, uuid: &str) -> Result<PlayerData> {
        if let Some(cached) = self.cache.get(&uuid.to_string()).await {
            return Ok(cached);
        }

        // Acquire a permit to stay within Hypixel rate limits.
        let _permit = self.rate_limiter.acquire().await;

        let url = format!("https://api.hypixel.net/v2/player?uuid={}", uuid);

        let resp = self
            .http
            .get(&url)
            .header("API-Key", &self.api_key)
            .send()
            .await?;

        let data: HypixelPlayerResponse = resp.json().await?;

        let (bedwars, socials) = match data.player {
            Some(player) => {
                let bw = player
                    .stats
                    .and_then(|s| s.bedwars)
                    .map(|bw| BedwarsStats::from_raw(&bw))
                    .unwrap_or_else(BedwarsStats::empty);

                let socials = player.social_media.map(|s| s.links).unwrap_or_default();

                (bw, socials)
            }
            None => (BedwarsStats::empty(), HashMap::new()),
        };

        let result = PlayerData {
            bedwars,
            social_links: socials,
        };

        self.cache.insert(uuid.to_string(), result.clone()).await;

        // Update the global stat key cache: union of existing keys and new keys.
        {
            let new_keys: Vec<String> = result.bedwars.stats.keys().cloned().collect();
            let mut known = self.known_bedwars_stat_keys.write().await;
            known.extend(new_keys);
            known.sort();
            known.dedup();
        }

        Ok(result)
    }
}
