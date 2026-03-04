/// Points calculation engine.
///
/// The calculator is stateless — it takes a guild's points configuration and a
/// slice of `StatDelta` values, and returns the total points earned. This makes
/// it trivial to test and to extend with new stat sources.
use crate::config::GuildConfig;
use crate::shared::types::StatDelta;

/// Calculate the total points earned from a set of stat deltas.
///
/// For each delta, the function looks up the stat name in the guild's
/// `points_config` map. If the stat is present and has a positive multiplier,
/// the delta's `difference` is multiplied by that value and added to the total.
///
/// Stats not present in the config contribute zero points.
///
/// # Example
///
/// ```ignore
/// // Guild config: { "wins": 10, "kills": 1, "beds_broken": 5 }
/// // Deltas: [ wins +2, kills +5, beds_broken +1 ]
/// // Points = (2 * 10) + (5 * 1) + (1 * 5) = 30
/// ```
pub fn calculate_points(config: &GuildConfig, deltas: &[StatDelta]) -> f64 {
    let mut total = 0.0;

    for delta in deltas {
        // Only award points for positive changes (stats going up).
        if delta.difference <= 0.0 {
            continue;
        }

        if let Some(&multiplier) = config.points_config.get(&delta.stat_name) {
            total += delta.difference * multiplier;
        }
    }

    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_config(map: HashMap<String, f64>) -> GuildConfig {
        GuildConfig {
            registered_role_id: None,
            points_config: map,
            discord_stats_enabled: false,
        }
    }

    #[test]
    fn test_basic_calculation() {
        let mut pts = HashMap::new();
        pts.insert("wins".to_string(), 10.0);
        pts.insert("kills".to_string(), 1.0);
        pts.insert("beds_broken".to_string(), 5.0);
        let config = make_config(pts);

        let deltas = vec![
            StatDelta::new(1, "wins".to_string(), 0.0, 2.0),
            StatDelta::new(1, "kills".to_string(), 0.0, 5.0),
            StatDelta::new(1, "beds_broken".to_string(), 0.0, 1.0),
        ];

        let points = calculate_points(&config, &deltas);
        assert!((points - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_unknown_stats_ignored() {
        let config = make_config(HashMap::new());
        let deltas = vec![StatDelta::new(1, "unknown_stat".to_string(), 0.0, 100.0)];

        let points = calculate_points(&config, &deltas);
        assert!((points - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_negative_deltas_ignored() {
        let mut pts = HashMap::new();
        pts.insert("wins".to_string(), 10.0);
        let config = make_config(pts);

        // A negative delta (stat went down) should not award points.
        let deltas = vec![StatDelta::new(1, "wins".to_string(), 10.0, 5.0)];

        let points = calculate_points(&config, &deltas);
        assert!((points - 0.0).abs() < f64::EPSILON);
    }
}
