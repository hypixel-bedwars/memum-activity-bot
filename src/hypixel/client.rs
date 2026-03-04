/// Hypixel API client with built-in caching and rate limiting.
///
/// The client wraps `reqwest::Client` and adds:
/// - A `TimedCache` so repeated lookups for the same UUID within ~30 seconds
///   return cached results without hitting the API.
/// - A `tokio::sync::Semaphore` that limits concurrent requests to avoid
///   exceeding Hypixel's rate limits (~120 requests/minute).
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Client;
use tokio::sync::Semaphore;
use tracing::{debug, warn};

use super::models::{BedwarsStats, HypixelPlayerResponse, MojangProfile};
use crate::shared::cache::TimedCache;

/// Default cache TTL for Hypixel stat lookups (30 seconds).
const CACHE_TTL_SECS: u64 = 30;

/// Maximum number of concurrent Hypixel API requests.
const MAX_CONCURRENT_REQUESTS: usize = 2;

/// The Hypixel API client.
pub struct HypixelClient {
    /// Underlying HTTP client (connection pooling handled internally).
    http: Client,

    /// Hypixel API key sent in the `API-Key` header.
    api_key: String,

    /// TTL cache keyed by Minecraft UUID.
    cache: TimedCache<String, BedwarsStats>,

    /// Semaphore used to limit concurrent outgoing requests.
    rate_limiter: Semaphore,
}

impl HypixelClient {
    /// Create a new client with the given API key.
    pub fn new(api_key: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
            cache: TimedCache::new(Duration::from_secs(CACHE_TTL_SECS)),
            rate_limiter: Semaphore::new(MAX_CONCURRENT_REQUESTS),
        }
    }

    // ---------------------------------------------------------------------
    // Mojang username -> UUID
    // ---------------------------------------------------------------------

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

    // ---------------------------------------------------------------------
    // Hypixel stats
    // ---------------------------------------------------------------------

    /// Fetch Bedwars stats for the given UUID.
    ///
    /// Results are cached for `CACHE_TTL_SECS` seconds. Concurrent requests
    /// beyond `MAX_CONCURRENT_REQUESTS` will wait on the semaphore.
    pub async fn fetch_bedwars_stats(self: &Arc<Self>, uuid: &str) -> Result<BedwarsStats> {
        // Check cache first.
        if let Some(cached) = self.cache.get(&uuid.to_string()).await {
            debug!(uuid, "Returning cached Bedwars stats");
            return Ok(cached);
        }

        // Acquire a rate-limit permit before making the request.
        let _permit = self
            .rate_limiter
            .acquire()
            .await
            .expect("Semaphore closed unexpectedly");

        // Double-check cache after acquiring the permit (another task may have
        // populated it while we were waiting).
        if let Some(cached) = self.cache.get(&uuid.to_string()).await {
            return Ok(cached);
        }

        let url = format!("https://api.hypixel.net/v2/player?uuid={}", uuid);

        let resp = self
            .http
            .get(&url)
            .header("API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to contact Hypixel API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(uuid, %status, body, "Hypixel API returned non-success status");
            bail!("Hypixel API error: {} — {}", status, body);
        }

        let data: HypixelPlayerResponse = resp
            .json()
            .await
            .context("Failed to parse Hypixel API response")?;

        let stats = match data.player {
            Some(player) => match player.stats {
                Some(game_stats) => match game_stats.bedwars {
                    Some(ref bw) => BedwarsStats::from_raw(bw),
                    None => {
                        debug!(uuid, "Player has no Bedwars stats");
                        BedwarsStats::empty()
                    }
                },
                None => BedwarsStats::empty(),
            },
            None => BedwarsStats::empty(),
        };

        // Store in cache.
        self.cache.insert(uuid.to_string(), stats.clone()).await;
        debug!(uuid, "Fetched and cached Bedwars stats");

        Ok(stats)
    }
}
