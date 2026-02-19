//! Strategic Report Parsing
//!
//! Parses LLM outputs for hourly and daily strategic reports with strict format validation.

use anyhow::Result;
use serde::{Deserialize, Serialize};

// ============================================================================
// Report Structures
// ============================================================================

/// Hourly strategic report (strict 4-line format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyReport {
    pub health_score: f64,
    pub severity: String,
    pub diagnosis: String,
    pub action: String,
    pub raw: String,
}

/// Daily strategic report (4-line header + optional DETAILS)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyReport {
    pub health_score: f64,
    pub severity: String,
    pub diagnosis: String,
    pub action: String,
    pub details: Option<DetailsSection>,
    pub raw: String,
}

/// Optional DETAILS section for daily reports
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailsSection {
    pub trend: String,
    pub top_drivers: String,
    pub confidence: String,
    pub next_check: String,
}

// ============================================================================
// Parsing Functions
// ============================================================================

/// Parse hourly report (strict 4-line format)
pub fn parse_hourly_report(text: &str) -> Result<HourlyReport> {
    let (health_score, severity, diagnosis, action) = parse_common_header(text)?;

    Ok(HourlyReport {
        health_score,
        severity,
        diagnosis,
        action,
        raw: text.to_string(),
    })
}

/// Parse daily report (4-line header + optional DETAILS)
pub fn parse_daily_report(text: &str) -> Result<DailyReport> {
    let (health_score, severity, diagnosis, action) = parse_common_header(text)?;

    // Try to parse optional DETAILS section
    let details = parse_details_section(text);

    Ok(DailyReport {
        health_score,
        severity,
        diagnosis,
        action,
        details,
        raw: text.to_string(),
    })
}

/// Parse hourly report with pre-calculated health score (2-line format: DIAGNOSIS + ACTION)
pub fn parse_hourly_report_with_score(
    text: &str,
    health_score: f64,
    severity: &str,
) -> Result<HourlyReport> {
    let (diagnosis, action) = parse_diagnosis_action_only(text)?;

    Ok(HourlyReport {
        health_score,
        severity: severity.to_string(),
        diagnosis,
        action,
        raw: text.to_string(),
    })
}

/// Parse daily report with pre-calculated health score (2-line format + optional DETAILS)
pub fn parse_daily_report_with_score(
    text: &str,
    health_score: f64,
    severity: &str,
) -> Result<DailyReport> {
    let (diagnosis, action) = parse_diagnosis_action_only(text)?;

    // Try to parse optional DETAILS section
    let details = parse_details_section(text);

    Ok(DailyReport {
        health_score,
        severity: severity.to_string(),
        diagnosis,
        action,
        details,
        raw: text.to_string(),
    })
}

// ============================================================================
// Internal Parsing Helpers
// ============================================================================

/// Strip DeepSeek-R1 reasoning blocks from response.
///
/// DeepSeek-R1-Distill models output reasoning in `<think>...</think>` blocks
/// before the actual response. This extracts just the final answer.
/// Also handles unclosed `<think>` tags, HTML artifacts, and raw reasoning.
fn strip_think_tags(text: &str) -> String {
    // Strip HTML artifacts
    let text = text
        .replace("</div>", "")
        .replace("<div>", "")
        .trim()
        .to_string();

    let lower = text.to_lowercase();

    // Case 1: Complete <think>...</think> block
    if let Some(end_pos) = lower.find("</think>") {
        return text[end_pos + "</think>".len()..].trim().to_string();
    }

    // Case 2: Unclosed <think> tag - try to find keywords inside
    if let Some(think_start) = lower.find("<think>") {
        let after_think = &text[think_start + "<think>".len()..];
        let after_lower = after_think.to_lowercase();

        if let Some(diag_pos) = after_lower.find("diagnosis:") {
            return after_think[diag_pos..].trim().to_string();
        }
        if let Some(action_pos) = after_lower.find("action:") {
            return after_think[action_pos..].trim().to_string();
        }

        let before = text[..think_start].trim();
        if !before.is_empty() {
            return before.to_string();
        }

        return after_think.trim().to_string();
    }

    // Case 3: No <think> tags - look for conclusion markers
    let conclusion_markers = [
        "therefore,", "so,", "in conclusion,", "the diagnosis is",
        "this indicates", "this suggests", "based on this",
    ];

    for marker in conclusion_markers {
        if let Some(pos) = lower.rfind(marker) {
            let after_marker = &text[pos..];
            if let Some(end) = after_marker.find('.') {
                let sentence = &after_marker[..end + 1];
                if sentence.len() > 20 {
                    return format!("DIAGNOSIS: {}\nACTION: Continue monitoring.", sentence.trim());
                }
            }
        }
    }

    text
}

/// Parse only DIAGNOSIS and ACTION (for deterministic scoring mode)
fn parse_diagnosis_action_only(text: &str) -> Result<(String, String)> {
    // Strip DeepSeek-R1 <think> blocks first
    let text = strip_think_tags(text);

    // Normalize text
    let normalized = text
        .replace("<0x0A>", "\n")
        .replace("\\n", "\n")
        .replace("\\r\\n", "\n")
        .replace("\\r", "\n")
        .trim()
        .trim_start_matches("```")
        .trim_start_matches("text")
        .trim_end_matches("```")
        .trim()
        .to_string();

    let upper = normalized.to_uppercase();

    // Extract DIAGNOSIS
    let diagnosis = extract_diagnosis(&normalized, &upper)?;

    // Extract ACTION
    let action = extract_action(&normalized, &upper)?;

    Ok((diagnosis, action))
}

/// Parse the common 4-line header (HEALTHSCORE, SEVERITY, DIAGNOSIS, ACTION)
fn parse_common_header(text: &str) -> Result<(f64, String, String, String)> {
    // Strip DeepSeek-R1 <think> blocks first
    let text = strip_think_tags(text);

    // Normalize text
    let normalized = text
        .replace("<0x0A>", "\n")
        .replace("\\n", "\n")
        .replace("\\r\\n", "\n")
        .replace("\\r", "\n")
        .trim()
        .trim_start_matches("```")
        .trim_start_matches("text")
        .trim_end_matches("```")
        .trim()
        .to_string();

    let upper = normalized.to_uppercase();

    // Extract HEALTHSCORE
    let health_score = extract_health_score(&normalized, &upper)?;

    // Extract SEVERITY
    let severity = extract_severity(&normalized, &upper)?;

    // Extract DIAGNOSIS
    let diagnosis = extract_diagnosis(&normalized, &upper)?;

    // Extract ACTION
    let action = extract_action(&normalized, &upper)?;

    Ok((health_score, severity, diagnosis, action))
}

/// Extract health score (0-100)
fn extract_health_score(text: &str, upper: &str) -> Result<f64> {
    for variant in ["HEALTHSCORE:", "HEALTH_SCORE:", "HEALTH SCORE:"] {
        if let Some(pos) = upper.find(variant) {
            let after = &text[pos + variant.len()..];
            let num_str: String = after
                .chars()
                .skip_while(|c| c.is_whitespace() || *c == ':' || *c == '=')
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();

            if !num_str.is_empty() {
                if let Ok(score) = num_str.parse::<f64>() {
                    let normalized = if score <= 10.0 { score * 10.0 } else { score };
                    return Ok(normalized.clamp(0.0, 100.0));
                }
            }
        }
    }

    anyhow::bail!("HEALTHSCORE not found or invalid")
}

/// Extract severity (HEALTHY|WATCH|WARNING|CRITICAL)
fn extract_severity(text: &str, upper: &str) -> Result<String> {
    if let Some(pos) = upper.find("SEVERITY:") {
        let after = &text[pos + "SEVERITY:".len()..];
        let line = after
            .lines()
            .next()
            .unwrap_or("")
            .trim();

        if !line.is_empty() {
            let severity_upper = line.to_uppercase();
            if severity_upper.contains("CRITICAL") {
                return Ok("Critical".to_string());
            } else if severity_upper.contains("WARNING") {
                return Ok("Warning".to_string());
            } else if severity_upper.contains("WATCH") {
                return Ok("Watch".to_string());
            } else if severity_upper.contains("HEALTHY") {
                return Ok("Healthy".to_string());
            } else {
                // Return whatever was there
                return Ok(line.to_string());
            }
        }
    }

    anyhow::bail!("SEVERITY not found or invalid")
}

/// Extract diagnosis text
fn extract_diagnosis(text: &str, upper: &str) -> Result<String> {
    if let Some(pos) = upper.find("DIAGNOSIS:") {
        let after = &text[pos + "DIAGNOSIS:".len()..];

        // Find end (next keyword or end of string)
        let end_pos = upper[pos..]
            .find("ACTION:")
            .or_else(|| upper[pos..].find("DETAILS:"))
            .map(|p| p.saturating_sub("DIAGNOSIS:".len()))
            .unwrap_or(after.len());

        let diagnosis = after[..end_pos.min(after.len())]
            .trim()
            .to_string();

        if !diagnosis.is_empty() {
            return Ok(diagnosis);
        }
    }

    anyhow::bail!("DIAGNOSIS not found or empty")
}

/// Extract action text
fn extract_action(text: &str, upper: &str) -> Result<String> {
    for keyword in ["ACTION:", "RECOMMENDED ACTION:", "RECOMMENDED_ACTION:"] {
        if let Some(pos) = upper.find(keyword) {
            let after = &text[pos + keyword.len()..];

            // Find end (DETAILS keyword or end of string)
            let end_pos = upper[pos..]
                .find("DETAILS:")
                .map(|p| p.saturating_sub(keyword.len()))
                .unwrap_or(after.len());

            let action = after[..end_pos.min(after.len())]
                .trim()
                .to_string();

            if !action.is_empty() {
                return Ok(action);
            }
        }
    }

    anyhow::bail!("ACTION not found or empty")
}

/// Parse optional DETAILS section (for daily reports)
fn parse_details_section(text: &str) -> Option<DetailsSection> {
    let upper = text.to_uppercase();

    // Check if DETAILS section exists
    if !upper.contains("DETAILS:") {
        return None;
    }

    // Extract each field
    let trend = extract_detail_field(text, "TREND:")?;
    let top_drivers = extract_detail_field(text, "TOP_DRIVERS:")?;
    let confidence = extract_detail_field(text, "CONFIDENCE:")?;
    let next_check = extract_detail_field(text, "NEXT_CHECK:")?;

    Some(DetailsSection {
        trend,
        top_drivers,
        confidence,
        next_check,
    })
}

/// Extract a single detail field
fn extract_detail_field(text: &str, keyword: &str) -> Option<String> {
    let upper = text.to_uppercase();
    let keyword_upper = keyword.to_uppercase();
    let pos = upper.find(&keyword_upper)?;

    let value_start = pos + keyword.len();
    let after = &text[value_start..];
    let upper_after = &upper[value_start..];

    // Extract until next keyword or end of line/text
    let next_keywords = ["TREND:", "TOP_DRIVERS:", "CONFIDENCE:", "NEXT_CHECK:"];
    let end_pos = next_keywords
        .iter()
        .filter_map(|k| upper_after.find(k))
        .min()
        .unwrap_or(after.len());

    // Also check for newline as end of value
    let newline_pos = after.find('\n').unwrap_or(after.len());
    let actual_end = end_pos.min(newline_pos);

    let value = after[..actual_end].trim().to_string();

    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hourly_basic() {
        let text = r#"
HEALTHSCORE: 75
SEVERITY: Warning
DIAGNOSIS: Increasing vibration at BPFO frequency detected.
ACTION: Monitor closely and schedule inspection.
"#;

        let report = parse_hourly_report(text).unwrap();
        assert_eq!(report.health_score, 75.0);
        assert_eq!(report.severity, "Warning");
        assert!(report.diagnosis.contains("BPFO"));
        assert!(report.action.contains("inspection"));
    }

    #[test]
    fn test_parse_daily_with_details() {
        let text = r#"
HEALTHSCORE: 82
SEVERITY: Healthy
DIAGNOSIS: Overall equipment health stable with minor variations.
ACTION: Continue normal operations with standard monitoring.
DETAILS:
TREND: Health score improving by 2 points per day
TOP_DRIVERS: Reduced motor temperatures and stable bearing signatures
CONFIDENCE: High
NEXT_CHECK: Reassess in 24h
"#;

        let report = parse_daily_report(text).unwrap();
        assert_eq!(report.health_score, 82.0);
        assert_eq!(report.severity, "Healthy");
        assert!(report.details.is_some());

        let details = report.details.unwrap();
        assert!(details.trend.contains("improving"));
        assert!(details.top_drivers.contains("motor"));
        assert_eq!(details.confidence, "High");
        assert!(details.next_check.contains("24h"));
    }

    #[test]
    fn test_parse_daily_without_details() {
        let text = r#"
HEALTHSCORE: 90
SEVERITY: Healthy
DIAGNOSIS: All systems normal.
ACTION: Continue monitoring.
"#;

        let report = parse_daily_report(text).unwrap();
        assert_eq!(report.health_score, 90.0);
        assert!(report.details.is_none());
    }

    #[test]
    fn test_health_score_scaling() {
        // Score 0-10 should be scaled to 0-100
        let text = "HEALTHSCORE: 8.5\nSEVERITY: Healthy\nDIAGNOSIS: Good\nACTION: Continue";
        let report = parse_hourly_report(text).unwrap();
        assert_eq!(report.health_score, 85.0);
    }

    #[test]
    fn test_missing_healthscore() {
        let text = "SEVERITY: Healthy\nDIAGNOSIS: Good\nACTION: Continue";
        let result = parse_hourly_report(text);
        assert!(result.is_err());
    }

    #[test]
    fn test_severity_variations() {
        let text = "HEALTHSCORE: 50\nSEVERITY: CRITICAL\nDIAGNOSIS: Bad\nACTION: Stop";
        let report = parse_hourly_report(text).unwrap();
        assert_eq!(report.severity, "Critical");
    }

    #[test]
    fn test_parse_hourly_with_score() {
        // LLM only outputs DIAGNOSIS and ACTION (no HEALTHSCORE or SEVERITY)
        let text = r#"
DIAGNOSIS: Increasing vibration at BPFO frequency detected.
ACTION: Monitor closely and schedule inspection.
"#;

        let report = parse_hourly_report_with_score(text, 67.5, "Watch").unwrap();
        assert_eq!(report.health_score, 67.5);
        assert_eq!(report.severity, "Watch");
        assert!(report.diagnosis.contains("BPFO"));
        assert!(report.action.contains("inspection"));
    }

    #[test]
    fn test_parse_daily_with_score_and_details() {
        // LLM outputs DIAGNOSIS, ACTION, and optional DETAILS
        let text = r#"
DIAGNOSIS: Overall equipment health stable with minor variations.
ACTION: Continue normal operations with standard monitoring.
DETAILS:
TREND: Health score improving by 2 points per day
TOP_DRIVERS: Reduced motor temperatures and stable bearing signatures
CONFIDENCE: High
NEXT_CHECK: Reassess in 24h
"#;

        let report = parse_daily_report_with_score(text, 82.3, "Healthy").unwrap();
        assert_eq!(report.health_score, 82.3);
        assert_eq!(report.severity, "Healthy");
        assert!(report.diagnosis.contains("stable"));
        assert!(report.details.is_some());

        let details = report.details.unwrap();
        assert!(details.trend.contains("improving"));
        assert!(details.confidence.contains("High"));
    }

    #[test]
    fn test_parse_daily_with_score_no_details() {
        // Minimal format: just DIAGNOSIS and ACTION
        let text = r#"
DIAGNOSIS: All systems operating within normal parameters.
ACTION: Continue monitoring with standard procedures.
"#;

        let report = parse_daily_report_with_score(text, 88.0, "Healthy").unwrap();
        assert_eq!(report.health_score, 88.0);
        assert_eq!(report.severity, "Healthy");
        assert!(report.diagnosis.contains("normal"));
        assert!(report.details.is_none());
    }
}
