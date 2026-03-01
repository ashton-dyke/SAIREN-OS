//! Strategic LLM – prompt building and response parsing
//!
//! Provides the prompt templates and structured response parsing used by the
//! hub-hosted LLM advisory path (`llm::http_client`) and documents the output
//! format expected from any OpenAI-compatible endpoint.
//!
//! The heavy local-inference code (MistralRsBackend, StrategicLLM struct,
//! CUDA/GPU logic) has been removed. Advisory generation now routes through
//! `llm::http_client` (fleet-client feature) with a deterministic template
//! fallback in `strategic::templates`.

use crate::types::{AdvisoryTicket, DrillingMetrics, DrillingPhysicsReport, TicketType};
use anyhow::Result;
use regex::Regex;

// ─── Prompt Constants ────────────────────────────────────────────────────────

/// System prompt for drilling advisory output (production drilling).
/// Optimised for drilling operational intelligence.
pub const DRILLING_ADVISORY_PROMPT: &str = r#"You are the Strategic AI for rig operational intelligence.
Analyze WITS data and provide actionable drilling optimization advice.

### INPUT CONTEXT
{context_str}
{trace_str}

### INPUT DATA
State: {state} | Depth: {depth}ft | ROP: {rop} ft/hr
WOB: {wob} klbs | RPM: {rpm} | Torque: {torque} kft-lbs
MSE: {mse} psi (Optimal: {mse_opt}, Efficiency: {efficiency}%)
Flow In: {flow_in} gpm | Out: {flow_out} gpm | Balance: {balance} bbl/hr
MW: {mw} ppg | ECD: {ecd} ppg | Margin: {margin} ppg
Gas: {gas} units | Pit Volume: {pit_volume} bbl

### TRIGGER
Category: {category} | Parameter: {trigger_param} | Value: {trigger_value}
{tactical_context}
### INSTRUCTIONS
1. Analyze drilling parameters against operational limits.
2. If flow imbalance > 10 bbl/hr, prioritize well control assessment.
3. If MSE efficiency < 70%, identify optimization opportunities.
4. Consider torque trends for mechanical issues.
5. Output ONLY the 6 lines below. No preamble. No markdown.
6. CfC Neural Network anomaly score > 0.5 means the AI sensor model independently detects abnormal behavior. Use to corroborate or weaken diagnosis confidence.

### OUTPUT FORMAT
TYPE: [OPTIMIZATION | RISK_WARNING | INTERVENTION]
PRIORITY: [LOW | MEDIUM | HIGH | CRITICAL]
CONFIDENCE: [0-100]%
RECOMMENDATION: [Specific actionable advice with target values]
EXPECTED BENEFIT: [Quantified: ROP gain, cost savings, risk reduction]
REASONING: [Technical justification based on drilling physics]"#;

/// P&A (Plug & Abandonment) specific prompt.
/// Focuses on cement integrity, pressure testing, and barrier verification.
pub const PA_ADVISORY_PROMPT: &str = r#"You are the Strategic AI for Plug & Abandonment operations.
Analyze WITS data and provide advice for cement operations and barrier integrity.

### CAMPAIGN: PLUG & ABANDONMENT
Focus areas: Cement placement, pressure testing, barrier verification, wellbore integrity

### INPUT CONTEXT
{context_str}
{trace_str}

### INPUT DATA
State: {state} | Depth: {depth}ft
Pump Rate: {flow_in} gpm | Returns: {flow_out} gpm | Balance: {balance} gpm
SPP: {spp} psi | Casing Pressure: {casing_pressure} psi
MW: {mw} ppg | ECD: {ecd} ppg | Margin: {margin} ppg
Pit Volume: {pit_volume} bbl

### TRIGGER
Category: {category} | Parameter: {trigger_param} | Value: {trigger_value}
{tactical_context}
### P&A SPECIFIC INSTRUCTIONS
1. Monitor cement returns - expect returns during cement placement.
2. Track pressure behavior during cement setting.
3. Verify barrier integrity through pressure testing.
4. Watch for fluid migration or gas channeling.
5. Assess squeeze job effectiveness.
6. Output ONLY the 6 lines below. No preamble. No markdown.
7. CfC Neural Network anomaly score > 0.5 means the AI sensor model independently detects abnormal behavior. Use to corroborate or weaken diagnosis confidence.

### OUTPUT FORMAT
TYPE: [CEMENT_PLACEMENT | PRESSURE_TEST | BARRIER_VERIFICATION | RISK_WARNING]
PRIORITY: [LOW | MEDIUM | HIGH | CRITICAL]
CONFIDENCE: [0-100]%
RECOMMENDATION: [Specific P&A operational advice]
EXPECTED BENEFIT: [Barrier integrity, regulatory compliance, safety]
REASONING: [Technical justification for P&A operations]"#;

/// Well control specific prompt for critical situations.
pub const WELL_CONTROL_PROMPT: &str = r#"You are the Well Control AI for rig safety.
CRITICAL ALERT: Possible well control event detected.

### INPUT DATA
Flow In: {flow_in} gpm | Flow Out: {flow_out} gpm | Balance: {balance} bbl/hr
Pit Volume: {pit_volume} bbl | Pit Rate: {pit_rate} bbl/hr
Gas: {gas} units | H2S: {h2s} ppm | CO2: {co2} %
Casing Pressure: {casing_pressure} psi
MW: {mw} ppg | ECD: {ecd} ppg

### ASSESSMENT
{assessment}

### INSTRUCTIONS
Provide immediate well control recommendation. Safety is paramount.
Output ONLY the 4 lines below. No preamble.

### OUTPUT FORMAT
STATUS: [KICK | LOSS | BALLOONING | NORMAL]
ACTION: [Immediate action required]
SEVERITY: [CRITICAL | HIGH | MEDIUM]
REASONING: [Brief technical justification]"#;

// ─── Parsed Advisory ─────────────────────────────────────────────────────────

/// Structured advisory parsed from an LLM response.
#[derive(Debug, Clone)]
pub struct ParsedAdvisory {
    pub ticket_type: TicketType,
    pub confidence: u8,
    pub recommendation: String,
    pub expected_benefit: String,
    pub reasoning: String,
}

// ─── Prompt Builder ──────────────────────────────────────────────────────────

/// Build the drilling advisory prompt for the given ticket, metrics, and campaign.
///
/// Selects the correct template (production vs P&A) and interpolates all metric
/// values. Pass `trace_summary` to embed a verification trace section.
pub fn build_prompt(
    ticket: &AdvisoryTicket,
    metrics: &DrillingMetrics,
    physics: &DrillingPhysicsReport,
    context: &[String],
    trace_summary: Option<&str>,
    campaign: crate::types::Campaign,
) -> String {
    let context_str = if context.is_empty() {
        "No historical context available.".to_string()
    } else {
        context.join("\n")
    };

    let trace_str = match trace_summary {
        Some(summary) if !summary.is_empty() => {
            format!("\n### VERIFICATION TRACE\n{}\n", summary)
        }
        _ => String::new(),
    };

    // Tactical context from deterministic pattern matcher
    let context_section = match &ticket.context {
        Some(ctx) => format!("\n### TACTICAL CONTEXT\n{}", ctx.to_prompt_section()),
        None => String::new(),
    };

    // CfC neural network section
    let cfc_section = match &ticket.cfc_anomaly_score {
        Some(score) => {
            let surprises_str = if ticket.cfc_feature_surprises.is_empty() {
                String::new()
            } else {
                let items: Vec<String> = ticket
                    .cfc_feature_surprises
                    .iter()
                    .take(5)
                    .map(|s| {
                        let dir = if s.error > 0.0 { "above" } else { "below" };
                        format!(
                            "  - {} ({} predicted, {:.2}\u{03c3})",
                            s.name, dir, s.magnitude
                        )
                    })
                    .collect();
                format!("\nSurprising Features:\n{}", items.join("\n"))
            };
            format!(
                "\n### CfC NEURAL NETWORK\nAnomaly Score: {:.2}/1.00 | Health: {:.2}/1.00{}",
                score,
                1.0 - score,
                surprises_str
            )
        }
        None => String::new(),
    };

    let base_prompt = match campaign {
        crate::types::Campaign::Production => DRILLING_ADVISORY_PROMPT,
        crate::types::Campaign::PlugAbandonment => PA_ADVISORY_PROMPT,
    };

    base_prompt
        .replace("{context_str}", &context_str)
        .replace("{trace_str}", &trace_str)
        .replace("{state}", &format!("{:?}", metrics.state))
        .replace("{depth}", &format!("{:.0}", physics.current_depth))
        .replace("{rop}", &format!("{:.1}", physics.current_rop))
        .replace("{wob}", &format!("{:.1}", physics.current_wob))
        .replace("{rpm}", &format!("{:.0}", physics.current_rpm))
        .replace("{torque}", &format!("{:.1}", physics.current_torque))
        .replace("{spp}", &format!("{:.0}", physics.current_spp))
        .replace(
            "{casing_pressure}",
            &format!("{:.0}", physics.current_casing_pressure),
        )
        .replace("{mse}", &format!("{:.0}", metrics.mse))
        .replace("{mse_opt}", &format!("{:.0}", physics.optimal_mse))
        .replace("{efficiency}", &format!("{:.0}", metrics.mse_efficiency))
        .replace("{flow_in}", &format!("{:.0}", physics.current_flow_in))
        .replace("{flow_out}", &format!("{:.0}", physics.current_flow_out))
        .replace("{balance}", &format!("{:.1}", metrics.flow_balance))
        .replace("{mw}", &format!("{:.2}", physics.current_mud_weight))
        .replace("{ecd}", &format!("{:.2}", physics.current_ecd))
        .replace("{margin}", &format!("{:.2}", metrics.ecd_margin))
        .replace("{gas}", &format!("{:.0}", physics.current_gas))
        .replace("{pit_volume}", &format!("{:.1}", physics.current_pit_volume))
        .replace("{category}", &format!("{:?}", ticket.category))
        .replace("{trigger_param}", &ticket.trigger_parameter)
        .replace("{trigger_value}", &format!("{:.2}", ticket.trigger_value))
        .replace(
            "{tactical_context}",
            &format!("{}{}", context_section, cfc_section),
        )
}

// ─── Response Parser ─────────────────────────────────────────────────────────

/// Parse a raw LLM response string into a `ParsedAdvisory`.
///
/// Regex-based extraction of the six output lines defined in the prompt
/// templates. Returns sensible defaults for any field that cannot be parsed.
pub fn parse_response(response: &str) -> Result<ParsedAdvisory> {
    let type_re = Regex::new(r"(?i)TYPE:\s*(.+?)(?:\n|$)").unwrap();
    let confidence_re = Regex::new(r"(?i)CONFIDENCE:\s*(\d+)\s*%?").unwrap();
    let recommendation_re = Regex::new(r"(?i)RECOMMENDATION:\s*(.+?)(?:\n|$)").unwrap();
    let benefit_re = Regex::new(r"(?i)EXPECTED BENEFIT:\s*(.+?)(?:\n|$)").unwrap();
    let reasoning_re = Regex::new(r"(?i)REASONING:\s*(.+?)(?:\n|$)").unwrap();

    let type_str = type_re
        .captures(response)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_uppercase())
        .unwrap_or_else(|| "RISK_WARNING".to_string());

    let ticket_type = if type_str.contains("OPTIMIZATION") {
        TicketType::Optimization
    } else if type_str.contains("INTERVENTION") {
        TicketType::Intervention
    } else {
        TicketType::RiskWarning
    };

    let confidence: u8 = confidence_re
        .captures(response)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(70);

    let recommendation = recommendation_re
        .captures(response)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_else(|| "Monitor situation and verify parameters.".to_string());

    let expected_benefit = benefit_re
        .captures(response)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_else(|| "Risk mitigation".to_string());

    let reasoning = reasoning_re
        .captures(response)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_else(|| "Based on drilling parameter analysis.".to_string());

    Ok(ParsedAdvisory {
        ticket_type,
        confidence: confidence.min(100),
        recommendation,
        expected_benefit,
        reasoning,
    })
}

// ─── GPU Memory Monitoring Utility ───────────────────────────────────────────

/// Query VRAM usage via nvidia-smi. Returns `None` when GPU is absent.
#[allow(dead_code)]
pub fn get_vram_usage_mb() -> Option<f64> {
    std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.used", "--format=csv,noheader,nounits"])
        .output()
        .ok()
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .and_then(|line| line.trim().parse::<f64>().ok())
        })
}

#[allow(dead_code)]
pub fn print_memory_stats() {
    if let Some(vram) = get_vram_usage_mb() {
        tracing::info!(vram_mb = vram, "GPU VRAM usage");
        if vram > 8192.0 {
            tracing::warn!("VRAM usage exceeds 8GB target ({:.0} MB)", vram);
        }
    } else {
        tracing::debug!("GPU VRAM: not available (running on CPU or nvidia-smi not found)");
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        AnomalyCategory, DrillingMetrics, DrillingPhysicsReport, Operation, RigState,
        TicketSeverity, TicketType,
    };

    fn make_ticket() -> AdvisoryTicket {
        AdvisoryTicket {
            timestamp: 1_705_564_800,
            ticket_type: TicketType::Optimization,
            category: AnomalyCategory::DrillingEfficiency,
            severity: TicketSeverity::Medium,
            current_metrics: DrillingMetrics {
                state: RigState::Drilling,
                operation: Operation::ProductionDrilling,
                mse: 45_000.0,
                mse_efficiency: 75.0,
                mse_delta_percent: 25.0,
                flow_balance: 5.0,
                ecd_margin: 0.4,
                ..DrillingMetrics::default()
            },
            trigger_parameter: "mse_delta".to_string(),
            trigger_value: 25.0,
            threshold_value: 20.0,
            description: "MSE efficiency below threshold".to_string(),
            context: None,
            depth: 10_000.0,
            trace_log: Vec::new(),
            cfc_anomaly_score: None,
            cfc_feature_surprises: Vec::new(),
            causal_leads: Vec::new(),
            damping_recommendation: None,
        }
    }

    fn make_physics() -> DrillingPhysicsReport {
        DrillingPhysicsReport {
            avg_mse: 45_000.0,
            optimal_mse: 35_000.0,
            mse_efficiency: 78.0,
            current_depth: 10_000.0,
            current_rop: 50.0,
            current_wob: 25.0,
            current_rpm: 120.0,
            current_torque: 15.0,
            current_spp: 2_500.0,
            current_flow_in: 500.0,
            current_flow_out: 505.0,
            current_mud_weight: 12.0,
            current_ecd: 12.4,
            current_gas: 50.0,
            current_pit_volume: 500.0,
            ..DrillingPhysicsReport::default()
        }
    }

    #[test]
    fn test_build_prompt_contains_key_fields() {
        use crate::types::Campaign;

        let ticket = make_ticket();
        let metrics = ticket.current_metrics.clone();
        let physics = make_physics();

        let prompt = build_prompt(&ticket, &metrics, &physics, &[], None, Campaign::Production);

        assert!(prompt.contains("10000"), "Should include depth");
        assert!(prompt.contains("45000"), "Should include MSE");
        assert!(prompt.contains("mse_delta"), "Should include trigger parameter");
        assert!(prompt.contains("OUTPUT FORMAT"), "Should include output format instructions");
    }

    #[test]
    fn test_build_prompt_pa_uses_pa_template() {
        use crate::types::Campaign;

        let ticket = make_ticket();
        let metrics = ticket.current_metrics.clone();
        let physics = make_physics();

        let prompt = build_prompt(
            &ticket,
            &metrics,
            &physics,
            &[],
            None,
            Campaign::PlugAbandonment,
        );

        assert!(
            prompt.contains("PLUG & ABANDONMENT"),
            "P&A campaign should use P&A template"
        );
    }

    #[test]
    fn test_build_prompt_includes_context() {
        use crate::types::Campaign;

        let ticket = make_ticket();
        let metrics = ticket.current_metrics.clone();
        let physics = make_physics();
        let context = vec![
            "Bit change at 9800ft — dull grade C-3".to_string(),
            "Formation top shale at 9850ft".to_string(),
        ];

        let prompt = build_prompt(&ticket, &metrics, &physics, &context, None, Campaign::Production);
        assert!(prompt.contains("9800ft"), "Context should be embedded in prompt");
    }

    #[test]
    fn test_parse_valid_response() {
        let response = "\
TYPE: OPTIMIZATION
PRIORITY: MEDIUM
CONFIDENCE: 82%
RECOMMENDATION: Reduce WOB by 5 klbs to bring MSE within target range.
EXPECTED BENEFIT: 15% ROP improvement, reduced bit wear.
REASONING: Current MSE 45000 psi vs optimal 35000 psi indicates founder onset.";

        let parsed = parse_response(response).expect("Should parse cleanly");
        assert_eq!(parsed.ticket_type, TicketType::Optimization);
        assert_eq!(parsed.confidence, 82);
        assert!(parsed.recommendation.contains("WOB"));
        assert!(parsed.expected_benefit.contains("ROP"));
        assert!(parsed.reasoning.contains("MSE"));
    }

    #[test]
    fn test_parse_intervention_type() {
        let response = "\
TYPE: INTERVENTION
PRIORITY: HIGH
CONFIDENCE: 90%
RECOMMENDATION: Shut in well immediately.
EXPECTED BENEFIT: Well control.
REASONING: High flow imbalance detected.";

        let parsed = parse_response(response).expect("Should parse");
        assert_eq!(parsed.ticket_type, TicketType::Intervention);
        assert_eq!(parsed.confidence, 90);
    }

    #[test]
    fn test_parse_malformed_uses_defaults() {
        let response = "garbage output with no recognizable format";
        let parsed = parse_response(response).expect("Should not error");
        // Should fall back to defaults
        assert_eq!(parsed.ticket_type, TicketType::RiskWarning);
        assert_eq!(parsed.confidence, 70);
        assert!(!parsed.recommendation.is_empty());
    }

    #[test]
    fn test_confidence_clamped_to_100() {
        let response = "\
TYPE: OPTIMIZATION
CONFIDENCE: 150%
RECOMMENDATION: Test.
EXPECTED BENEFIT: Test.
REASONING: Test.";
        let parsed = parse_response(response).expect("Should parse");
        assert_eq!(parsed.confidence, 100);
    }
}
