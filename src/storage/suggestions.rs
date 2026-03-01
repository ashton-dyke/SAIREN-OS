//! Threshold suggestion engine
//!
//! Pure computation module — no sled storage. Analyses operator feedback
//! records to compute per-category confirmation rates and recommend threshold
//! adjustments when false positive rates are high.

use crate::config::WellConfig;
use crate::types::AnomalyCategory;
use serde::Serialize;
use super::feedback::{FeedbackOutcome, FeedbackRecord};

/// Per-category confirmation statistics.
#[derive(Debug, Clone, Serialize)]
pub struct CategoryStats {
    pub category: AnomalyCategory,
    pub total: usize,
    pub confirmed: usize,
    pub false_positives: usize,
    pub unclear: usize,
    /// confirmed / (confirmed + false_positives), NaN-safe.
    pub confirmation_rate: f64,
}

/// A suggested threshold adjustment based on feedback data.
#[derive(Debug, Clone, Serialize)]
pub struct ThresholdSuggestion {
    pub category: AnomalyCategory,
    /// Dot-path to the threshold key (e.g. "thresholds.mse.efficiency_warning_percent").
    pub threshold_key: String,
    pub current_value: f64,
    pub suggested_value: f64,
    pub rationale: String,
    /// Confidence based on sample size (0.0–1.0).
    pub confidence: f64,
}

/// Compute per-category stats from feedback records.
pub fn compute_stats(records: &[FeedbackRecord]) -> Vec<CategoryStats> {
    use std::collections::HashMap;

    let mut buckets: HashMap<AnomalyCategory, (usize, usize, usize)> = HashMap::new();

    for rec in records {
        let entry = buckets.entry(rec.category).or_insert((0, 0, 0));
        match rec.outcome {
            FeedbackOutcome::Confirmed => entry.0 += 1,
            FeedbackOutcome::FalsePositive => entry.1 += 1,
            FeedbackOutcome::Unclear => entry.2 += 1,
        }
    }

    let mut stats: Vec<CategoryStats> = buckets
        .into_iter()
        .filter(|(cat, _)| *cat != AnomalyCategory::None)
        .map(|(category, (confirmed, false_positives, unclear))| {
            let total = confirmed + false_positives + unclear;
            let denominator = confirmed + false_positives;
            let confirmation_rate = if denominator > 0 {
                confirmed as f64 / denominator as f64
            } else {
                0.0
            };
            CategoryStats {
                category,
                total,
                confirmed,
                false_positives,
                unclear,
                confirmation_rate,
            }
        })
        .collect();

    stats.sort_by(|a, b| format!("{:?}", a.category).cmp(&format!("{:?}", b.category)));
    stats
}

/// Map anomaly category to its primary threshold key path and whether
/// "higher value = more sensitive" (true) or "lower value = more sensitive" (false).
///
/// For thresholds where higher = more sensitive (e.g. efficiency_warning_percent 70%→75%
/// means more alerts), tightening = decrease the value.
/// For thresholds where lower = more sensitive (e.g. stick_slip_cv_warning 0.15→0.12
/// means more alerts), tightening = increase the value.
fn category_threshold_mapping(cat: AnomalyCategory) -> Option<(&'static str, bool)> {
    match cat {
        AnomalyCategory::DrillingEfficiency => {
            // Higher % = more sensitive (more alerts when efficiency drops below)
            Some(("thresholds.mse.efficiency_warning_percent", true))
        }
        AnomalyCategory::Hydraulics => {
            // Higher ppg margin = more sensitive (alerts when ECD is further from frac gradient)
            Some(("thresholds.hydraulics.ecd_margin_warning_ppg", true))
        }
        AnomalyCategory::WellControl => {
            // Lower gpm = more sensitive (alerts on smaller flow imbalance)
            Some(("thresholds.well_control.flow_imbalance_warning_gpm", false))
        }
        AnomalyCategory::Mechanical => {
            // Lower CV = more sensitive (alerts on less stick-slip)
            Some(("thresholds.mechanical.stick_slip_cv_warning", false))
        }
        AnomalyCategory::Formation => {
            // More negative = more sensitive (alerts on smaller dxc reversals)
            // This is a negative threshold; treat as "lower = more sensitive"
            Some(("thresholds.formation.dexp_decrease_warning", false))
        }
        AnomalyCategory::None => None,
    }
}

/// Read the current threshold value for a category from `WellConfig`.
fn current_threshold_value(config: &WellConfig, cat: AnomalyCategory) -> Option<f64> {
    match cat {
        AnomalyCategory::DrillingEfficiency => {
            Some(config.thresholds.mse.efficiency_warning_percent)
        }
        AnomalyCategory::Hydraulics => {
            Some(config.thresholds.hydraulics.ecd_margin_warning_ppg)
        }
        AnomalyCategory::WellControl => {
            Some(config.thresholds.well_control.flow_imbalance_warning_gpm)
        }
        AnomalyCategory::Mechanical => {
            Some(config.thresholds.mechanical.stick_slip_cv_warning)
        }
        AnomalyCategory::Formation => {
            Some(config.thresholds.formation.dexp_decrease_warning)
        }
        AnomalyCategory::None => None,
    }
}

/// Compute threshold adjustment suggestions from feedback data.
///
/// Rules:
/// - Need 10+ rated advisories per category to generate a suggestion.
/// - If confirmation rate < 50%: suggest tightening by 10%.
/// - If confirmation rate > 90% with 20+ samples: suggest loosening by 5%.
/// - Never suggest beyond ±25% of current value.
pub fn compute_suggestions(
    records: &[FeedbackRecord],
    config: &WellConfig,
) -> Vec<ThresholdSuggestion> {
    let stats = compute_stats(records);
    let mut suggestions = Vec::new();

    for stat in &stats {
        let (threshold_key, higher_is_more_sensitive) =
            match category_threshold_mapping(stat.category) {
                Some(m) => m,
                None => continue,
            };

        let current = match current_threshold_value(config, stat.category) {
            Some(v) => v,
            None => continue,
        };

        // Need 10+ rated (non-unclear) advisories to suggest anything
        let rated = stat.confirmed + stat.false_positives;
        if rated < 10 {
            continue;
        }

        let (adjustment_pct, rationale) = if stat.confirmation_rate < 0.50 {
            // Too many false positives — tighten (make less sensitive)
            (
                -0.10,
                format!(
                    "Only {:.0}% confirmation rate ({} confirmed, {} false positive). \
                     Recommend tightening to reduce false alerts.",
                    stat.confirmation_rate * 100.0,
                    stat.confirmed,
                    stat.false_positives
                ),
            )
        } else if stat.confirmation_rate > 0.90 && rated >= 20 {
            // Very high confirmation — can safely loosen (make more sensitive)
            (
                0.05,
                format!(
                    "{:.0}% confirmation rate over {} rated advisories. \
                     Threshold may be too conservative — consider loosening.",
                    stat.confirmation_rate * 100.0,
                    rated
                ),
            )
        } else {
            continue;
        };

        // Direction logic:
        // "Tighten" (adjustment_pct < 0) means make LESS sensitive (fewer alerts).
        //   - If higher_is_more_sensitive: decrease the value → multiply by (1 + adjustment_pct)
        //   - If !higher_is_more_sensitive: increase the value → multiply by (1 - adjustment_pct)
        // "Loosen" (adjustment_pct > 0) means make MORE sensitive (more alerts).
        //   - If higher_is_more_sensitive: increase the value → multiply by (1 + adjustment_pct)
        //   - If !higher_is_more_sensitive: decrease the value → multiply by (1 - adjustment_pct)
        let multiplier = if higher_is_more_sensitive {
            1.0 + adjustment_pct
        } else {
            1.0 - adjustment_pct
        };

        let raw_suggested = current * multiplier;

        // Floor/cap: never suggest beyond ±25% of current value
        let floor = current * 0.75;
        let cap = current * 1.25;
        let suggested = raw_suggested.clamp(floor.min(cap), floor.max(cap));

        // Confidence based on sample size (scales from 0.5 at 10 samples to 1.0 at 50+)
        let confidence = ((rated as f64 - 10.0) / 40.0).clamp(0.0, 1.0) * 0.5 + 0.5;

        suggestions.push(ThresholdSuggestion {
            category: stat.category,
            threshold_key: threshold_key.to_string(),
            current_value: current,
            suggested_value: (suggested * 1000.0).round() / 1000.0, // 3 decimal places
            rationale,
            confidence,
        });
    }

    suggestions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::feedback::FeedbackRecord;

    fn make_feedback(
        cat: AnomalyCategory,
        outcome: FeedbackOutcome,
        ts: u64,
    ) -> FeedbackRecord {
        FeedbackRecord {
            advisory_timestamp: ts,
            outcome,
            category: cat,
            trigger_parameter: "test".to_string(),
            trigger_value: 0.0,
            threshold_value: 0.0,
            submitted_by: "driller".to_string(),
            submitted_at: ts + 100,
            notes: String::new(),
        }
    }

    #[test]
    fn test_compute_stats_mixed() {
        let records: Vec<FeedbackRecord> = vec![
            make_feedback(AnomalyCategory::DrillingEfficiency, FeedbackOutcome::Confirmed, 1),
            make_feedback(AnomalyCategory::DrillingEfficiency, FeedbackOutcome::Confirmed, 2),
            make_feedback(AnomalyCategory::DrillingEfficiency, FeedbackOutcome::FalsePositive, 3),
            make_feedback(AnomalyCategory::DrillingEfficiency, FeedbackOutcome::Unclear, 4),
            make_feedback(AnomalyCategory::WellControl, FeedbackOutcome::Confirmed, 5),
        ];

        let stats = compute_stats(&records);
        let de_stat = stats.iter().find(|s| s.category == AnomalyCategory::DrillingEfficiency).unwrap();
        assert_eq!(de_stat.total, 4);
        assert_eq!(de_stat.confirmed, 2);
        assert_eq!(de_stat.false_positives, 1);
        assert_eq!(de_stat.unclear, 1);
        // confirmation_rate = 2 / (2 + 1) = 0.6667
        assert!((de_stat.confirmation_rate - 0.6667).abs() < 0.001);
    }

    #[test]
    fn test_suggestion_high_false_positive_rate() {
        // 3 confirmed + 10 false positive = 13 rated, conf rate = 23%
        let mut records = Vec::new();
        for i in 0..3 {
            records.push(make_feedback(AnomalyCategory::DrillingEfficiency, FeedbackOutcome::Confirmed, i));
        }
        for i in 3..13 {
            records.push(make_feedback(AnomalyCategory::DrillingEfficiency, FeedbackOutcome::FalsePositive, i));
        }

        let config = WellConfig::default();
        let suggestions = compute_suggestions(&records, &config);

        assert_eq!(suggestions.len(), 1);
        let s = &suggestions[0];
        assert_eq!(s.category, AnomalyCategory::DrillingEfficiency);
        // MSE efficiency_warning_percent: higher = more sensitive
        // Tightening = decrease → suggested < current
        assert!(s.suggested_value < s.current_value);
    }

    #[test]
    fn test_suggestion_high_confirmation_rate() {
        // 19 confirmed + 1 false positive = 20 rated, conf rate = 95%
        let mut records = Vec::new();
        for i in 0..19 {
            records.push(make_feedback(AnomalyCategory::WellControl, FeedbackOutcome::Confirmed, i));
        }
        records.push(make_feedback(AnomalyCategory::WellControl, FeedbackOutcome::FalsePositive, 19));

        let config = WellConfig::default();
        let suggestions = compute_suggestions(&records, &config);

        assert_eq!(suggestions.len(), 1);
        let s = &suggestions[0];
        assert_eq!(s.category, AnomalyCategory::WellControl);
        // flow_imbalance_warning_gpm: lower = more sensitive
        // Loosening = decrease → suggested < current
        assert!(s.suggested_value < s.current_value);
    }

    #[test]
    fn test_no_suggestion_with_insufficient_data() {
        let records: Vec<FeedbackRecord> = (0..5)
            .map(|i| make_feedback(AnomalyCategory::Mechanical, FeedbackOutcome::FalsePositive, i))
            .collect();

        let config = WellConfig::default();
        let suggestions = compute_suggestions(&records, &config);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_no_suggestion_moderate_confirmation_rate() {
        // 7 confirmed + 5 false positive = 12 rated, conf rate = 58%
        // Between 50% and 90% — no suggestion
        let mut records = Vec::new();
        for i in 0..7 {
            records.push(make_feedback(AnomalyCategory::Hydraulics, FeedbackOutcome::Confirmed, i));
        }
        for i in 7..12 {
            records.push(make_feedback(AnomalyCategory::Hydraulics, FeedbackOutcome::FalsePositive, i));
        }

        let config = WellConfig::default();
        let suggestions = compute_suggestions(&records, &config);
        assert!(suggestions.is_empty());
    }
}
