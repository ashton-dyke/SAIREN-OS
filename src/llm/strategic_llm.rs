//! Strategic LLM - Drilling Advisory Intelligence
//!
//! Uses DeepSeek R1 Distill Qwen 7B for comprehensive drilling advisory analysis.
//! Target latency: 800ms (acceptable for strategic analysis)
//!
//! The strategic LLM combines physics analysis, drilling domain knowledge, and
//! deep reasoning to generate actionable drilling optimization and risk prevention advice.

use crate::types::{
    AdvisoryTicket, AnomalyCategory, DrillingMetrics, DrillingPhysicsReport,
    FinalSeverity, RiskLevel, StrategicAdvisory, TicketSeverity, TicketType,
};
use anyhow::Result;
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use tokio::sync::Mutex;

#[cfg(feature = "llm")]
use anyhow::Context;
#[cfg(feature = "llm")]
use regex::Regex;
#[cfg(feature = "llm")]
use std::env;
#[cfg(feature = "llm")]
use super::MistralRsBackend;

/// Default model path for strategic model (Qwen 2.5 7B Instruct)
#[cfg(feature = "llm")]
const DEFAULT_STRATEGIC_MODEL: &str = "/home/ashton/sairen-multiagent/models/qwen2.5-7b-instruct-q4_k_m.gguf";

/// System prompt for drilling advisory output
/// Optimized for drilling operational intelligence
#[cfg(feature = "llm")]
const DRILLING_ADVISORY_PROMPT: &str = r#"You are the Strategic AI for rig operational intelligence.
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

### INSTRUCTIONS
1. Analyze drilling parameters against operational limits.
2. If flow imbalance > 10 bbl/hr, prioritize well control assessment.
3. If MSE efficiency < 70%, identify optimization opportunities.
4. Consider torque trends for mechanical issues.
5. Output ONLY the 6 lines below. No preamble. No markdown.

### OUTPUT FORMAT
TYPE: [OPTIMIZATION | RISK_WARNING | INTERVENTION]
PRIORITY: [LOW | MEDIUM | HIGH | CRITICAL]
CONFIDENCE: [0-100]%
RECOMMENDATION: [Specific actionable advice with target values]
EXPECTED BENEFIT: [Quantified: ROP gain, cost savings, risk reduction]
REASONING: [Technical justification based on drilling physics]"#;

/// P&A (Plug & Abandonment) specific prompt
/// Focuses on cement integrity, pressure testing, and barrier verification
#[cfg(feature = "llm")]
const PA_ADVISORY_PROMPT: &str = r#"You are the Strategic AI for Plug & Abandonment operations.
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

### P&A SPECIFIC INSTRUCTIONS
1. Monitor cement returns - expect returns during cement placement.
2. Track pressure behavior during cement setting.
3. Verify barrier integrity through pressure testing.
4. Watch for fluid migration or gas channeling.
5. Assess squeeze job effectiveness.
6. Output ONLY the 6 lines below. No preamble. No markdown.

### OUTPUT FORMAT
TYPE: [CEMENT_PLACEMENT | PRESSURE_TEST | BARRIER_VERIFICATION | RISK_WARNING]
PRIORITY: [LOW | MEDIUM | HIGH | CRITICAL]
CONFIDENCE: [0-100]%
RECOMMENDATION: [Specific P&A operational advice]
EXPECTED BENEFIT: [Barrier integrity, regulatory compliance, safety]
REASONING: [Technical justification for P&A operations]"#;

/// Well control specific prompt for critical situations
#[cfg(feature = "llm")]
const WELL_CONTROL_PROMPT: &str = r#"You are the Well Control AI for rig safety.
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

/// Global singleton for StrategicLLM
static STRATEGIC_INSTANCE: OnceLock<Arc<StrategicLLM>> = OnceLock::new();

/// Statistics tracking for strategic inference
#[derive(Debug, Default)]
struct StrategicStats {
    inference_count: u64,
    total_latency_ms: f64,
    optimization_advisories: u64,
    risk_warnings: u64,
    interventions: u64,
    well_control_alerts: u64,
    parse_failures: u64,
}

impl StrategicStats {
    fn record_advisory(&mut self, ticket_type: &TicketType) {
        match ticket_type {
            TicketType::Optimization => self.optimization_advisories += 1,
            TicketType::RiskWarning => self.risk_warnings += 1,
            TicketType::Intervention => self.interventions += 1,
        }
    }
}

/// Parsed drilling advisory from LLM
#[derive(Debug, Clone)]
pub struct ParsedAdvisory {
    pub ticket_type: TicketType,
    pub priority: TicketSeverity,
    pub confidence: u8,
    pub recommendation: String,
    pub expected_benefit: String,
    pub reasoning: String,
}

/// Strategic LLM for drilling advisory generation
pub struct StrategicLLM {
    #[cfg(feature = "llm")]
    backend: Arc<MistralRsBackend>,
    stats: Mutex<StrategicStats>,
    #[cfg(not(feature = "llm"))]
    _phantom: std::marker::PhantomData<()>,
}

impl StrategicLLM {
    /// Initialize the strategic LLM singleton
    #[cfg(feature = "llm")]
    pub async fn init() -> Result<Arc<Self>> {
        if let Some(existing) = STRATEGIC_INSTANCE.get() {
            return Ok(Arc::clone(existing));
        }

        let model_path = env::var("STRATEGIC_MODEL_PATH")
            .unwrap_or_else(|_| DEFAULT_STRATEGIC_MODEL.to_string());

        tracing::info!(
            model_path = %model_path,
            "Initializing Strategic LLM for drilling intelligence"
        );

        let backend = MistralRsBackend::load(&model_path)
            .await
            .context("Failed to load strategic model")?;

        let instance = Arc::new(Self {
            backend: Arc::new(backend),
            stats: Mutex::new(StrategicStats::default()),
        });

        let _ = STRATEGIC_INSTANCE.set(Arc::clone(&instance));

        tracing::info!("Strategic LLM initialized for drilling advisory");
        Ok(instance)
    }

    /// Initialize mock strategic LLM (when llm feature disabled)
    #[cfg(not(feature = "llm"))]
    pub async fn init() -> Result<Arc<Self>> {
        if let Some(existing) = STRATEGIC_INSTANCE.get() {
            return Ok(Arc::clone(existing));
        }

        tracing::info!("Initializing Strategic LLM (MOCK - llm feature disabled)");

        let instance = Arc::new(Self {
            stats: Mutex::new(StrategicStats::default()),
            _phantom: std::marker::PhantomData,
        });

        let _ = STRATEGIC_INSTANCE.set(Arc::clone(&instance));
        Ok(instance)
    }

    /// Get the global singleton instance
    pub fn get() -> Option<Arc<Self>> {
        STRATEGIC_INSTANCE.get().cloned()
    }

    /// Build drilling advisory prompt (campaign-aware)
    #[cfg(feature = "llm")]
    fn build_drilling_advisory_prompt(
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

        // Select prompt based on campaign
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
            .replace("{casing_pressure}", &format!("{:.0}", physics.current_casing_pressure))
            .replace("{mse}", &format!("{:.0}", metrics.mse))
            .replace("{mse_opt}", &format!("{:.0}", physics.optimal_mse))
            .replace("{efficiency}", &format!("{:.0}", 100.0 - metrics.mse_delta_percent.abs()))
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
    }

    /// Parse LLM response into advisory
    #[cfg(feature = "llm")]
    fn parse_advisory_response(response: &str) -> Result<ParsedAdvisory> {
        let type_re = Regex::new(r"(?i)TYPE:\s*(.+?)(?:\n|$)").unwrap();
        let priority_re = Regex::new(r"(?i)PRIORITY:\s*(.+?)(?:\n|$)").unwrap();
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

        let priority_str = priority_re
            .captures(response)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_uppercase())
            .unwrap_or_else(|| "MEDIUM".to_string());

        let priority = if priority_str.contains("CRITICAL") {
            TicketSeverity::Critical
        } else if priority_str.contains("HIGH") {
            TicketSeverity::High
        } else if priority_str.contains("LOW") {
            TicketSeverity::Low
        } else {
            TicketSeverity::Medium
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
            priority,
            confidence: confidence.min(100),
            recommendation,
            expected_benefit,
            reasoning,
        })
    }

    /// Generate drilling advisory from ticket and analysis
    #[cfg(feature = "llm")]
    pub async fn generate_advisory(
        &self,
        ticket: &AdvisoryTicket,
        metrics: &DrillingMetrics,
        physics: &DrillingPhysicsReport,
        context: &[String],
        campaign: crate::types::Campaign,
    ) -> Result<StrategicAdvisory> {
        let start = Instant::now();

        let prompt = Self::build_drilling_advisory_prompt(
            ticket, metrics, physics, context, None, campaign,
        );

        let response = self
            .backend
            .generate_with_params(&prompt, 200, 0.3)
            .await
            .context("Strategic drilling advisory failed")?;

        let elapsed = start.elapsed();

        let parsed = match Self::parse_advisory_response(&response) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to parse advisory, using fallback");
                let mut stats = self.stats.lock().await;
                stats.parse_failures += 1;
                Self::fallback_advisory(ticket, metrics, campaign)
            }
        };

        // Calculate efficiency score
        let efficiency_score = Self::calculate_efficiency_score(metrics, physics);

        // Determine risk level
        let risk_level = Self::determine_risk_level(ticket, metrics);

        {
            let mut stats = self.stats.lock().await;
            stats.inference_count += 1;
            stats.total_latency_ms += elapsed.as_secs_f64() * 1000.0;
            stats.record_advisory(&parsed.ticket_type);
        }

        tracing::debug!(
            latency_ms = elapsed.as_millis(),
            ticket_type = ?parsed.ticket_type,
            confidence = parsed.confidence,
            "Drilling advisory generated"
        );

        Ok(StrategicAdvisory {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            efficiency_score,
            risk_level,
            severity: Self::risk_to_severity(&risk_level),
            recommendation: parsed.recommendation,
            expected_benefit: parsed.expected_benefit,
            reasoning: parsed.reasoning,
            votes: Vec::new(),
            physics_report: physics.clone(),
            context_used: context.to_vec(),
            trace_log: ticket.trace_log.clone(),
        })
    }

    /// Generate advisory with trace context
    #[cfg(feature = "llm")]
    pub async fn generate_advisory_with_trace(
        &self,
        ticket: &AdvisoryTicket,
        metrics: &DrillingMetrics,
        physics: &DrillingPhysicsReport,
        context: &[String],
        trace_summary: &str,
        campaign: crate::types::Campaign,
    ) -> Result<StrategicAdvisory> {
        let start = Instant::now();

        let prompt = Self::build_drilling_advisory_prompt(
            ticket, metrics, physics, context, Some(trace_summary), campaign,
        );

        let response = self
            .backend
            .generate_with_params(&prompt, 200, 0.3)
            .await
            .context("Strategic drilling advisory with trace failed")?;

        let elapsed = start.elapsed();

        let parsed = match Self::parse_advisory_response(&response) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to parse advisory");
                let mut stats = self.stats.lock().await;
                stats.parse_failures += 1;
                Self::fallback_advisory(ticket, metrics, campaign)
            }
        };

        let efficiency_score = Self::calculate_efficiency_score(metrics, physics);
        let risk_level = Self::determine_risk_level(ticket, metrics);

        {
            let mut stats = self.stats.lock().await;
            stats.inference_count += 1;
            stats.total_latency_ms += elapsed.as_secs_f64() * 1000.0;
            stats.record_advisory(&parsed.ticket_type);
        }

        tracing::debug!(
            latency_ms = elapsed.as_millis(),
            has_trace = true,
            "Drilling advisory with trace generated"
        );

        Ok(StrategicAdvisory {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            efficiency_score,
            risk_level,
            severity: Self::risk_to_severity(&risk_level),
            recommendation: parsed.recommendation,
            expected_benefit: parsed.expected_benefit,
            reasoning: parsed.reasoning,
            votes: Vec::new(),
            physics_report: physics.clone(),
            context_used: context.to_vec(),
            trace_log: ticket.trace_log.clone(),
        })
    }

    /// Mock advisory generation when LLM feature disabled
    #[cfg(not(feature = "llm"))]
    pub async fn generate_advisory(
        &self,
        ticket: &AdvisoryTicket,
        metrics: &DrillingMetrics,
        physics: &DrillingPhysicsReport,
        context: &[String],
        campaign: crate::types::Campaign,
    ) -> Result<StrategicAdvisory> {
        let start = Instant::now();

        let parsed = Self::fallback_advisory(ticket, metrics, campaign);
        let efficiency_score = Self::calculate_efficiency_score(metrics, physics);
        let risk_level = Self::determine_risk_level(ticket, metrics);

        let elapsed = start.elapsed();

        {
            let mut stats = self.stats.lock().await;
            stats.inference_count += 1;
            stats.total_latency_ms += elapsed.as_secs_f64() * 1000.0;
            stats.record_advisory(&parsed.ticket_type);
        }

        tracing::debug!(
            campaign = %campaign.short_code(),
            "(MOCK) Drilling advisory generated"
        );

        Ok(StrategicAdvisory {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            efficiency_score,
            risk_level,
            severity: Self::risk_to_severity(&risk_level),
            recommendation: parsed.recommendation,
            expected_benefit: parsed.expected_benefit,
            reasoning: parsed.reasoning,
            votes: Vec::new(),
            physics_report: physics.clone(),
            context_used: context.to_vec(),
            trace_log: ticket.trace_log.clone(),
        })
    }

    /// Mock advisory with trace when LLM disabled
    #[cfg(not(feature = "llm"))]
    pub async fn generate_advisory_with_trace(
        &self,
        ticket: &AdvisoryTicket,
        metrics: &DrillingMetrics,
        physics: &DrillingPhysicsReport,
        context: &[String],
        _trace_summary: &str,
        campaign: crate::types::Campaign,
    ) -> Result<StrategicAdvisory> {
        self.generate_advisory(ticket, metrics, physics, context, campaign).await
    }

    /// Fallback advisory when parsing fails (campaign-aware)
    fn fallback_advisory(
        ticket: &AdvisoryTicket,
        metrics: &DrillingMetrics,
        campaign: crate::types::Campaign,
    ) -> ParsedAdvisory {
        use crate::types::Campaign;

        let (recommendation, expected_benefit, reasoning) = match (campaign, &ticket.category) {
            // P&A specific fallbacks
            (Campaign::PlugAbandonment, AnomalyCategory::WellControl) => (
                "Verify cement placement and barrier integrity. Monitor for fluid migration.".to_string(),
                "Barrier integrity verification".to_string(),
                format!(
                    "Flow balance: {:.1} bbl/hr. Monitor for cement returns and pressure behavior.",
                    metrics.flow_balance
                ),
            ),
            (Campaign::PlugAbandonment, AnomalyCategory::Hydraulics) => (
                "Monitor cement pump pressure and returns. Verify displacement volumes.".to_string(),
                "Successful cement placement".to_string(),
                format!("ECD margin: {:.2} ppg. Track cement top location.", metrics.ecd_margin),
            ),
            (Campaign::PlugAbandonment, _) => (
                "Continue P&A operations. Monitor pressure and cement behavior.".to_string(),
                "Regulatory compliance and well integrity".to_string(),
                "P&A operations in progress. Verify barrier requirements.".to_string(),
            ),
            // Production drilling fallbacks
            (Campaign::Production, AnomalyCategory::WellControl) => (
                "Verify flow balance and pit levels. Prepare for well control procedures if needed.".to_string(),
                "Well control incident prevention".to_string(),
                format!(
                    "Flow imbalance of {:.1} bbl/hr detected. Pit rate {:.1} bbl/hr.",
                    metrics.flow_balance, metrics.pit_rate
                ),
            ),
            (Campaign::Production, AnomalyCategory::DrillingEfficiency) => (
                format!(
                    "Consider adjusting WOB/RPM to improve MSE. Current MSE deviation: {:.0}%",
                    metrics.mse_delta_percent
                ),
                "Potential 10-20% ROP improvement".to_string(),
                format!("MSE analysis shows {:.0}% deviation from optimal.", metrics.mse_delta_percent),
            ),
            (Campaign::Production, AnomalyCategory::Hydraulics) => (
                "Monitor standpipe pressure and flow rates. Check for potential washout or plugging.".to_string(),
                "Hydraulic efficiency optimization".to_string(),
                format!("ECD margin: {:.2} ppg. Flow balance: {:.1} bbl/hr.", metrics.ecd_margin, metrics.flow_balance),
            ),
            (Campaign::Production, AnomalyCategory::Mechanical) => (
                "Monitor torque and drag trends. Consider backreaming if torque continues to increase.".to_string(),
                "Pack-off prevention, reduced NPT risk".to_string(),
                "Elevated torque/drag detected. Possible mechanical resistance.".to_string(),
            ),
            (Campaign::Production, AnomalyCategory::Formation) => (
                "Formation change detected. Adjust drilling parameters accordingly.".to_string(),
                "Optimized drilling through formation transition".to_string(),
                format!("D-exponent: {:.2}, DXC: {:.2}. Trend indicates formation change.", metrics.d_exponent, metrics.dxc),
            ),
            (Campaign::Production, AnomalyCategory::None) => (
                "Continue monitoring drilling parameters.".to_string(),
                "Maintained operational efficiency".to_string(),
                "Normal drilling operations.".to_string(),
            ),
        };

        ParsedAdvisory {
            ticket_type: ticket.ticket_type.clone(),
            priority: ticket.severity.clone(),
            confidence: 75,
            recommendation,
            expected_benefit,
            reasoning,
        }
    }

    /// Calculate efficiency score from metrics
    fn calculate_efficiency_score(metrics: &DrillingMetrics, _physics: &DrillingPhysicsReport) -> u8 {
        // Base score from MSE efficiency
        let mse_score: f64 = if metrics.mse_delta_percent.abs() < 10.0 {
            90.0
        } else if metrics.mse_delta_percent.abs() < 25.0 {
            75.0
        } else if metrics.mse_delta_percent.abs() < 50.0 {
            55.0
        } else {
            35.0
        };

        // Penalty for well control issues
        let flow_penalty: f64 = if metrics.flow_balance.abs() > 20.0 {
            25.0
        } else if metrics.flow_balance.abs() > 10.0 {
            15.0
        } else if metrics.flow_balance.abs() > 5.0 {
            5.0
        } else {
            0.0
        };

        // Penalty for low ECD margin
        let ecd_penalty: f64 = if metrics.ecd_margin < 0.2 {
            15.0
        } else if metrics.ecd_margin < 0.3 {
            5.0
        } else {
            0.0
        };

        (mse_score - flow_penalty - ecd_penalty).max(0.0).min(100.0) as u8
    }

    /// Determine risk level from ticket and metrics
    fn determine_risk_level(ticket: &AdvisoryTicket, metrics: &DrillingMetrics) -> RiskLevel {
        // Well control issues are always elevated
        if ticket.category == AnomalyCategory::WellControl {
            if metrics.flow_balance.abs() > 20.0 || metrics.pit_rate.abs() > 10.0 {
                return RiskLevel::Critical;
            }
            if metrics.flow_balance.abs() > 10.0 || metrics.pit_rate.abs() > 5.0 {
                return RiskLevel::High;
            }
            return RiskLevel::Elevated;
        }

        // Map from ticket severity
        match ticket.severity {
            TicketSeverity::Critical => RiskLevel::Critical,
            TicketSeverity::High => RiskLevel::High,
            TicketSeverity::Medium => RiskLevel::Elevated,
            TicketSeverity::Low => RiskLevel::Low,
        }
    }

    /// Convert RiskLevel to FinalSeverity for ensemble output
    fn risk_to_severity(risk: &RiskLevel) -> FinalSeverity {
        match risk {
            RiskLevel::Critical => FinalSeverity::Critical,
            RiskLevel::High => FinalSeverity::High,
            RiskLevel::Elevated => FinalSeverity::Medium,
            RiskLevel::Low => FinalSeverity::Low,
        }
    }

    /// Get inference statistics
    pub async fn stats(&self) -> StrategicLLMStats {
        let stats = self.stats.lock().await;
        StrategicLLMStats {
            inference_count: stats.inference_count,
            avg_latency_ms: if stats.inference_count > 0 {
                stats.total_latency_ms / stats.inference_count as f64
            } else {
                0.0
            },
            optimization_advisories: stats.optimization_advisories,
            risk_warnings: stats.risk_warnings,
            interventions: stats.interventions,
            well_control_alerts: stats.well_control_alerts,
            parse_failures: stats.parse_failures,
        }
    }
}

/// Statistics from strategic LLM
#[derive(Debug, Clone)]
pub struct StrategicLLMStats {
    pub inference_count: u64,
    pub avg_latency_ms: f64,
    pub optimization_advisories: u64,
    pub risk_warnings: u64,
    pub interventions: u64,
    pub well_control_alerts: u64,
    pub parse_failures: u64,
}

impl std::fmt::Display for StrategicLLMStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "StrategicLLM: {} advisories ({:.1}ms avg) - {} optimization, {} risk, {} intervention, {} well control, {} parse failures",
            self.inference_count,
            self.avg_latency_ms,
            self.optimization_advisories,
            self.risk_warnings,
            self.interventions,
            self.well_control_alerts,
            self.parse_failures
        )
    }
}

// VRAM monitoring helper
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
pub fn print_vram_stats() {
    if let Some(vram) = get_vram_usage_mb() {
        tracing::info!(vram_mb = vram, "VRAM usage");
        if vram > 8192.0 {
            tracing::warn!("VRAM usage exceeds 8GB target ({:.0} MB)", vram);
        }
    } else {
        tracing::debug!("VRAM usage: unable to query (nvidia-smi not available)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RigState;

    fn create_test_metrics() -> DrillingMetrics {
        DrillingMetrics {
            state: RigState::Drilling,
            operation: crate::types::Operation::ProductionDrilling,
            mse: 45000.0,
            mse_efficiency: 75.0,
            d_exponent: 1.5,
            dxc: 1.4,
            mse_delta_percent: 25.0,
            flow_balance: 5.0,
            pit_rate: 2.0,
            ecd_margin: 0.4,
            torque_delta_percent: 10.0,
            spp_delta: 50.0,
            is_anomaly: true,
            anomaly_category: AnomalyCategory::DrillingEfficiency,
            anomaly_description: Some("MSE inefficiency detected".to_string()),
        }
    }

    fn create_test_physics() -> DrillingPhysicsReport {
        DrillingPhysicsReport {
            avg_mse: 45000.0,
            mse_trend: 5.0,
            optimal_mse: 35000.0,
            mse_efficiency: 78.0,
            dxc_trend: 2.0,
            flow_balance_trend: 1.0,
            avg_pit_rate: 1.5,
            formation_hardness: 6.0,
            confidence: 0.9,
            detected_dysfunctions: Vec::new(),
            current_depth: 10000.0,
            current_rop: 50.0,
            current_wob: 25.0,
            current_rpm: 120.0,
            current_torque: 15.0,
            current_spp: 2500.0,
            current_casing_pressure: 0.0,
            current_flow_in: 500.0,
            current_flow_out: 505.0,
            current_mud_weight: 12.0,
            current_ecd: 12.4,
            current_gas: 50.0,
            current_pit_volume: 500.0,
            // Founder detection fields (V0.6)
            wob_trend: 0.0,
            rop_trend: 0.0,
            founder_detected: false,
            founder_severity: 0.0,
            optimal_wob_estimate: 0.0,
        }
    }

    fn create_test_ticket() -> AdvisoryTicket {
        AdvisoryTicket {
            timestamp: 1705564800,
            ticket_type: TicketType::Optimization,
            category: AnomalyCategory::DrillingEfficiency,
            severity: TicketSeverity::Medium,
            current_metrics: create_test_metrics(),
            trigger_parameter: "mse_delta".to_string(),
            trigger_value: 25.0,
            threshold_value: 20.0,
            description: "MSE efficiency below threshold".to_string(),
            depth: 10000.0,
            trace_log: Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_mock_advisory_generation() {
        use crate::types::Campaign;

        let llm = StrategicLLM::init().await.unwrap();
        let ticket = create_test_ticket();
        let metrics = create_test_metrics();
        let physics = create_test_physics();

        let advisory = llm
            .generate_advisory(&ticket, &metrics, &physics, &[], Campaign::Production)
            .await
            .unwrap();

        assert!(advisory.efficiency_score > 0);
        assert!(!advisory.recommendation.is_empty());
        assert!(!advisory.reasoning.is_empty());
    }

    #[tokio::test]
    async fn test_mock_advisory_pa_campaign() {
        use crate::types::Campaign;

        let llm = StrategicLLM::init().await.unwrap();
        let ticket = create_test_ticket();
        let metrics = create_test_metrics();
        let physics = create_test_physics();

        let advisory = llm
            .generate_advisory(&ticket, &metrics, &physics, &[], Campaign::PlugAbandonment)
            .await
            .unwrap();

        assert!(advisory.efficiency_score > 0);
        assert!(!advisory.recommendation.is_empty());
    }

    #[tokio::test]
    async fn test_efficiency_score_calculation() {
        let metrics = create_test_metrics();
        let physics = create_test_physics();

        let score = StrategicLLM::calculate_efficiency_score(&metrics, &physics);
        assert!(score > 0 && score <= 100);
    }

    #[tokio::test]
    async fn test_risk_level_determination() {
        let ticket = create_test_ticket();
        let metrics = create_test_metrics();

        let risk = StrategicLLM::determine_risk_level(&ticket, &metrics);
        assert_eq!(risk, RiskLevel::Elevated); // Medium severity -> Elevated
    }

    #[tokio::test]
    async fn test_well_control_critical_risk() {
        let mut ticket = create_test_ticket();
        ticket.category = AnomalyCategory::WellControl;

        let mut metrics = create_test_metrics();
        metrics.flow_balance = 25.0; // High flow imbalance

        let risk = StrategicLLM::determine_risk_level(&ticket, &metrics);
        assert_eq!(risk, RiskLevel::Critical);
    }
}
