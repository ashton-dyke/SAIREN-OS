//! Strategic Actor - Aggregates tactical analyses and generates strategic reports

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use super::aggregation::{DailyAggregate, HourlyAggregate, TacticalAnalysis};
use super::parsing::{
    parse_daily_report_with_score, parse_hourly_report_with_score, DailyReport, HourlyReport,
};
#[cfg(feature = "llm")]
use crate::llm::SchedulerHandle;
use crate::storage::StrategicStorage;

// ============================================================================
// Commands
// ============================================================================

/// Commands for StrategicActor
#[derive(Debug)]
pub enum StrategicCommand {
    /// Add a tactical analysis to the buffer
    AddTactical(TacticalAnalysis),
    /// Get recent hourly reports
    GetHourlyReports {
        limit: usize,
        response_tx: tokio::sync::oneshot::Sender<Result<Vec<HourlyReport>>>,
    },
    /// Get recent daily reports
    GetDailyReports {
        limit: usize,
        response_tx: tokio::sync::oneshot::Sender<Result<Vec<DailyReport>>>,
    },
}

// ============================================================================
// Actor Handle
// ============================================================================

/// Handle to interact with StrategicActor
#[derive(Clone)]
pub struct StrategicActorHandle {
    tx: mpsc::Sender<StrategicCommand>,
}

impl StrategicActorHandle {
    /// Send a tactical analysis to be buffered
    pub async fn send_tactical(&self, analysis: TacticalAnalysis) -> Result<()> {
        self.tx
            .send(StrategicCommand::AddTactical(analysis))
            .await
            .context("Strategic actor channel closed")
    }

    /// Get recent hourly reports
    pub async fn get_hourly_reports(&self, limit: usize) -> Result<Vec<HourlyReport>> {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StrategicCommand::GetHourlyReports { limit, response_tx })
            .await
            .context("Strategic actor channel closed")?;
        response_rx.await.context("Response channel closed")?
    }

    /// Get recent daily reports
    pub async fn get_daily_reports(&self, limit: usize) -> Result<Vec<DailyReport>> {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StrategicCommand::GetDailyReports { limit, response_tx })
            .await
            .context("Strategic actor channel closed")?;
        response_rx.await.context("Response channel closed")?
    }
}

// ============================================================================
// Strategic Actor
// ============================================================================

/// Strategic Actor - manages aggregation and strategic report generation
pub struct StrategicActor {
    /// Scheduler handle for submitting strategic requests
    scheduler: SchedulerHandle,
    /// Storage for strategic reports
    storage: StrategicStorage,
    /// Command receiver
    rx: mpsc::Receiver<StrategicCommand>,
    /// Buffer for tactical analyses (up to 60)
    tactical_buffer: Vec<TacticalAnalysis>,
    /// Buffer for hourly aggregates (up to 24)
    hourly_buffer: Vec<HourlyAggregate>,
    /// Counter for tracking
    tactical_count: usize,
    hourly_count: usize,
}

impl StrategicActor {
    /// Create new strategic actor and handle
    pub fn new(
        scheduler: SchedulerHandle,
        storage: StrategicStorage,
    ) -> (Self, StrategicActorHandle) {
        let (tx, rx) = mpsc::channel(100);

        let actor = Self {
            scheduler,
            storage,
            rx,
            tactical_buffer: Vec::with_capacity(60),
            hourly_buffer: Vec::with_capacity(24),
            tactical_count: 0,
            hourly_count: 0,
        };

        let handle = StrategicActorHandle { tx };

        (actor, handle)
    }

    /// Run the strategic actor loop
    pub async fn run(mut self) {
        info!("StrategicActor starting");

        while let Some(cmd) = self.rx.recv().await {
            match cmd {
                StrategicCommand::AddTactical(analysis) => {
                    self.handle_tactical(analysis).await;
                }
                StrategicCommand::GetHourlyReports { limit, response_tx } => {
                    let result = self.storage.get_hourly(limit);
                    let _ = response_tx.send(result);
                }
                StrategicCommand::GetDailyReports { limit, response_tx } => {
                    let result = self.storage.get_daily(limit);
                    let _ = response_tx.send(result);
                }
            }
        }

        info!("StrategicActor stopped");
    }

    /// Handle incoming tactical analysis
    async fn handle_tactical(&mut self, analysis: TacticalAnalysis) {
        self.tactical_buffer.push(analysis);
        self.tactical_count += 1;

        debug!(
            tactical_count = self.tactical_count,
            buffer_size = self.tactical_buffer.len(),
            "Tactical analysis buffered"
        );

        // Check if we have 60 tactical analyses for hourly aggregate
        if self.tactical_buffer.len() >= 60 {
            info!("Generating hourly aggregate from {} tactical analyses", self.tactical_buffer.len());
            self.generate_hourly().await;
        }
    }

    /// Generate hourly aggregate and report
    async fn generate_hourly(&mut self) {
        // Compute aggregate
        let aggregate = HourlyAggregate::from_tactical(&self.tactical_buffer);

        // Calculate deterministic health score from aggregate data
        // Use the mean of tactical scores (which are already deterministic)
        let health_score = aggregate.mean_health_score;

        // Adjust score based on trends (penalize negative trends)
        let trend_penalty = self.calculate_trend_penalty(
            aggregate.bpfo_delta_per_hour,
            aggregate.bpfi_delta_per_hour,
            aggregate.motor_temp_delta_per_hour,
            aggregate.gearbox_temp_delta_per_hour,
        );
        let health_score = (health_score - trend_penalty).max(0.0).min(100.0);

        // Determine severity from score
        let severity = Self::severity_from_score(health_score);

        info!(
            health_score = health_score,
            severity = %severity,
            trend_penalty = trend_penalty,
            "Deterministic hourly score calculated"
        );

        // Build prompt with pre-calculated score
        let prompt = self.build_hourly_prompt_with_score(&aggregate, health_score, &severity);

        // Submit to scheduler (P1 priority)
        info!("Submitting hourly report request to scheduler (P1)");
        match self.scheduler.infer_strategic(prompt, 100, 0.2).await {
            Ok(response) => {
                debug!(response_len = response.len() as usize, "Hourly report received");

                // Parse response using deterministic score
                match parse_hourly_report_with_score(&response, health_score, &severity) {
                    Ok(report) => {
                        info!(
                            health_score = report.health_score,
                            severity = %report.severity,
                            "Hourly report generated with deterministic score"
                        );

                        // Store report
                        if let Err(e) = self.storage.store_hourly(&report) {
                            error!("Failed to store hourly report: {}", e);
                        }

                        // Add to hourly buffer
                        self.hourly_buffer.push(aggregate);
                        self.hourly_count += 1;

                        // Check if we have 24 hourly reports for daily
                        if self.hourly_buffer.len() >= 24 {
                            info!("Generating daily aggregate from {} hourly reports", self.hourly_buffer.len());
                            self.generate_daily().await;
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse hourly report: {}", e);
                        warn!("Raw response: {}", response);
                    }
                }
            }
            Err(e) => {
                error!("Hourly report generation failed: {}", e);
            }
        }

        // Clear tactical buffer
        self.tactical_buffer.clear();
    }

    /// Generate daily aggregate and report
    async fn generate_daily(&mut self) {
        // Compute aggregate
        let aggregate = DailyAggregate::from_hourly(&self.hourly_buffer);

        // Calculate deterministic health score from aggregate data
        let health_score = aggregate.mean_health_score;

        // Adjust based on trend (penalize negative trends more for daily)
        let trend_adjustment = aggregate.health_score_trend * 2.0; // More weight for daily trend
        let health_score = (health_score + trend_adjustment).max(0.0).min(100.0);

        // Determine severity from score
        let severity = Self::severity_from_score(health_score);

        info!(
            health_score = health_score,
            severity = %severity,
            trend_adjustment = trend_adjustment,
            "Deterministic daily score calculated"
        );

        // Build prompt with pre-calculated score
        let prompt = self.build_daily_prompt_with_score(&aggregate, health_score, &severity);

        // Submit to scheduler (P1 priority)
        info!("Submitting daily report request to scheduler (P1)");
        match self.scheduler.infer_strategic(prompt, 100, 0.2).await {
            Ok(response) => {
                debug!(response_len = response.len() as usize, "Daily report received");

                // Parse response using deterministic score
                match parse_daily_report_with_score(&response, health_score, &severity) {
                    Ok(report) => {
                        info!(
                            health_score = report.health_score,
                            severity = %report.severity,
                            has_details = report.details.is_some(),
                            "Daily report generated"
                        );

                        // Store report
                        if let Err(e) = self.storage.store_daily(&report) {
                            error!("Failed to store daily report: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse daily report: {}", e);
                        warn!("Raw response: {}", response);
                    }
                }
            }
            Err(e) => {
                error!("Daily report generation failed: {}", e);
            }
        }

        // Clear hourly buffer
        self.hourly_buffer.clear();
    }

    /// Calculate severity level from health score
    fn severity_from_score(score: f64) -> String {
        if score >= 80.0 {
            "Healthy".to_string()
        } else if score >= 60.0 {
            "Watch".to_string()
        } else if score >= 40.0 {
            "Warning".to_string()
        } else {
            "Critical".to_string()
        }
    }

    /// Calculate trend penalty for hourly reports
    ///
    /// Penalizes negative trends (increasing faults, rising temperatures)
    fn calculate_trend_penalty(
        &self,
        bpfo_delta: f64,
        bpfi_delta: f64,
        motor_temp_delta: f64,
        gearbox_temp_delta: f64,
    ) -> f64 {
        let mut penalty = 0.0;

        // Penalize increasing bearing faults (positive deltas are bad)
        if bpfo_delta > 0.0 {
            penalty += (bpfo_delta * 100.0).min(10.0); // Max 10 points per fault
        }
        if bpfi_delta > 0.0 {
            penalty += (bpfi_delta * 100.0).min(10.0);
        }

        // Penalize rising temperatures (positive deltas above 2°C/hour are concerning)
        if motor_temp_delta > 2.0 {
            penalty += ((motor_temp_delta - 2.0) * 2.0).min(10.0);
        }
        if gearbox_temp_delta > 2.0 {
            penalty += ((gearbox_temp_delta - 2.0) * 2.0).min(10.0);
        }

        penalty
    }

    /// Build prompt for hourly report with pre-calculated score
    fn build_hourly_prompt_with_score(
        &self,
        aggregate: &HourlyAggregate,
        health_score: f64,
        severity: &str,
    ) -> String {
        format!(
            r#"Hourly drilling analysis. Score: {:.0}/100 ({}).
Data: BPFO {:+.4}g/h, Temp {:+.1}°C/h, RMS {:.4}g, RPM {:.0}

Reply format:
DIAGNOSIS: <2-3 sentences analyzing current drilling conditions, trends, and any concerns>
ACTION: <1-2 sentences with specific recommended actions>"#,
            health_score,
            severity,
            aggregate.bpfo_delta_per_hour,
            aggregate.motor_temp_delta_per_hour,
            aggregate.mean_rms,
            aggregate.mean_rpm,
        )
    }

    /// Build prompt for hourly report (DEPRECATED - use build_hourly_prompt_with_score)
    fn build_hourly_prompt(&self, aggregate: &HourlyAggregate) -> String {
        format!(
            r#"You are analyzing equipment health over the past HOUR (60 measurements).

DATA SUMMARY:
- Time period: {} to {}
- Mean health score: {:.1} (min: {:.1}, max: {:.1})
- Mean RPM: {:.0}
- Mean motor temp: {:.1}°C (delta: {:+.1}°C/hour)
- Mean gearbox temp: {:.1}°C (delta: {:+.1}°C/hour)
- Mean RMS vibration: {:.4} g
- Mean BPFO amplitude: {:.4} g (delta: {:+.4} g/hour)
- Mean BPFI amplitude: {:.4} g (delta: {:+.4} g/hour)

TASK: Analyze the hourly trend and provide strategic assessment.

OUTPUT FORMAT (MANDATORY - EXACTLY 4 LINES):
HEALTHSCORE: <number 0-100>
SEVERITY: <HEALTHY|WATCH|WARNING|CRITICAL>
DIAGNOSIS: <single sentence under 20 words>
ACTION: <single sentence under 20 words>

RULES:
- Start immediately with "HEALTHSCORE:" - no preamble
- No markdown code fences
- Each field must be ONE line
- DIAGNOSIS and ACTION: exactly ONE sentence, under 20 words each
- Focus on TRENDS (deltas per hour) not absolute values"#,
            aggregate.start_time.format("%H:%M"),
            aggregate.end_time.format("%H:%M"),
            aggregate.mean_health_score,
            aggregate.min_health_score,
            aggregate.max_health_score,
            aggregate.mean_rpm,
            aggregate.mean_motor_temp,
            aggregate.motor_temp_delta_per_hour,
            aggregate.mean_gearbox_temp,
            aggregate.gearbox_temp_delta_per_hour,
            aggregate.mean_rms,
            aggregate.mean_bpfo,
            aggregate.bpfo_delta_per_hour,
            aggregate.mean_bpfi,
            aggregate.bpfi_delta_per_hour,
        )
    }

    /// Build prompt for daily report with pre-calculated score
    fn build_daily_prompt_with_score(
        &self,
        aggregate: &DailyAggregate,
        health_score: f64,
        severity: &str,
    ) -> String {
        format!(
            r#"Daily drilling summary. Score: {:.0}/100 ({}).
Data: Trend {:+.2}/h, BPFO {:.4}g, Motor {:.1}°C, Gearbox {:.1}°C

Reply format:
DIAGNOSIS: <2-3 sentences summarizing daily performance, key trends, and any issues observed>
ACTION: <1-2 sentences with recommended actions for the next shift>"#,
            health_score,
            severity,
            aggregate.health_score_trend,
            aggregate.mean_bpfo,
            aggregate.mean_motor_temp,
            aggregate.mean_gearbox_temp,
        )
    }

    /// Build prompt for daily report (DEPRECATED - use build_daily_prompt_with_score)
    fn build_daily_prompt(&self, aggregate: &DailyAggregate) -> String {
        format!(
            r#"You are analyzing equipment health over the past DAY (24 hourly reports).

DATA SUMMARY:
- Time period: {} to {}
- Mean health score: {:.1} (min: {:.1}, max: {:.1})
- Health trend: {:+.2} points per hour
- Mean motor temp: {:.1}°C
- Mean gearbox temp: {:.1}°C
- Mean BPFO: {:.4} g
- Mean BPFI: {:.4} g

TASK: Provide strategic daily assessment with optional details.

OUTPUT FORMAT (MANDATORY):
First 4 lines EXACTLY like this:
HEALTHSCORE: <number 0-100>
SEVERITY: <HEALTHY|WATCH|WARNING|CRITICAL>
DIAGNOSIS: <single sentence under 20 words>
ACTION: <single sentence under 20 words>

Then OPTIONALLY add:
DETAILS:
TREND: <one bullet, under 18 words>
TOP_DRIVERS: <one bullet, under 18 words>
CONFIDENCE: <Low|Medium|High>
NEXT_CHECK: <e.g. "Reassess in 24h">

RULES:
- Start with "HEALTHSCORE:" - no preamble
- No markdown fences
- First 4 lines are MANDATORY
- DETAILS section is OPTIONAL
- Total response: max 900 characters
- Focus on day-scale trends and strategic recommendations"#,
            aggregate.start_time.format("%Y-%m-%d %H:%M"),
            aggregate.end_time.format("%Y-%m-%d %H:%M"),
            aggregate.mean_health_score,
            aggregate.min_health_score,
            aggregate.max_health_score,
            aggregate.health_score_trend,
            aggregate.mean_motor_temp,
            aggregate.mean_gearbox_temp,
            aggregate.mean_bpfo,
            aggregate.mean_bpfi,
        )
    }
}
