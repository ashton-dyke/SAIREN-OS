//! Data aggregation for strategic analysis

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Tactical analysis snapshot for aggregation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TacticalAnalysis {
    pub timestamp: DateTime<Utc>,
    pub health_score: f64,
    pub severity: String,
    pub rpm: f64,
    pub motor_temp_avg: f64,
    pub gearbox_temp_avg: f64,
    pub rms: f64,
    pub bpfo_amp: f64,
    pub bpfi_amp: f64,
}

/// Hourly aggregate (60 tactical analyses)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyAggregate {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub mean_health_score: f64,
    pub min_health_score: f64,
    pub max_health_score: f64,
    pub mean_rpm: f64,
    pub mean_motor_temp: f64,
    pub mean_gearbox_temp: f64,
    pub mean_rms: f64,
    pub mean_bpfo: f64,
    pub mean_bpfi: f64,
    // Trend slopes (per hour)
    pub bpfo_delta_per_hour: f64,
    pub bpfi_delta_per_hour: f64,
    pub motor_temp_delta_per_hour: f64,
    pub gearbox_temp_delta_per_hour: f64,
    pub count: usize,
}

impl HourlyAggregate {
    pub fn from_tactical(analyses: &[TacticalAnalysis]) -> Self {
        if analyses.is_empty() {
            return Self::default();
        }

        let count = analyses.len();
        let start_time = analyses.first().unwrap().timestamp;
        let end_time = analyses.last().unwrap().timestamp;

        let mean_health_score = analyses.iter().map(|a| a.health_score).sum::<f64>() / count as f64;
        let min_health_score = analyses
            .iter()
            .map(|a| a.health_score)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        let max_health_score = analyses
            .iter()
            .map(|a| a.health_score)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();

        let mean_rpm = analyses.iter().map(|a| a.rpm).sum::<f64>() / count as f64;
        let mean_motor_temp = analyses.iter().map(|a| a.motor_temp_avg).sum::<f64>() / count as f64;
        let mean_gearbox_temp = analyses.iter().map(|a| a.gearbox_temp_avg).sum::<f64>() / count as f64;
        let mean_rms = analyses.iter().map(|a| a.rms).sum::<f64>() / count as f64;
        let mean_bpfo = analyses.iter().map(|a| a.bpfo_amp).sum::<f64>() / count as f64;
        let mean_bpfi = analyses.iter().map(|a| a.bpfi_amp).sum::<f64>() / count as f64;

        // Calculate trends (simple linear regression slope)
        let bpfo_delta_per_hour = Self::calculate_slope(analyses, |a| a.bpfo_amp);
        let bpfi_delta_per_hour = Self::calculate_slope(analyses, |a| a.bpfi_amp);
        let motor_temp_delta_per_hour = Self::calculate_slope(analyses, |a| a.motor_temp_avg);
        let gearbox_temp_delta_per_hour = Self::calculate_slope(analyses, |a| a.gearbox_temp_avg);

        Self {
            start_time,
            end_time,
            mean_health_score,
            min_health_score,
            max_health_score,
            mean_rpm,
            mean_motor_temp,
            mean_gearbox_temp,
            mean_rms,
            mean_bpfo,
            mean_bpfi,
            bpfo_delta_per_hour,
            bpfi_delta_per_hour,
            motor_temp_delta_per_hour,
            gearbox_temp_delta_per_hour,
            count,
        }
    }

    fn calculate_slope<F>(analyses: &[TacticalAnalysis], extractor: F) -> f64
    where
        F: Fn(&TacticalAnalysis) -> f64,
    {
        if analyses.len() < 2 {
            return 0.0;
        }

        let n = analyses.len() as f64;
        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        let mut sum_xy = 0.0;
        let mut sum_x2 = 0.0;

        for (i, analysis) in analyses.iter().enumerate() {
            let x = i as f64;
            let y = extractor(analysis);
            sum_x += x;
            sum_y += y;
            sum_xy += x * y;
            sum_x2 += x * x;
        }

        let slope = (n * sum_xy - sum_x * sum_y) / (n * sum_x2 - sum_x * sum_x);
        slope.is_finite().then_some(slope).unwrap_or(0.0)
    }
}

impl Default for HourlyAggregate {
    fn default() -> Self {
        Self {
            start_time: Utc::now(),
            end_time: Utc::now(),
            mean_health_score: 100.0,
            min_health_score: 100.0,
            max_health_score: 100.0,
            mean_rpm: 0.0,
            mean_motor_temp: 55.0,
            mean_gearbox_temp: 48.0,
            mean_rms: 0.0,
            mean_bpfo: 0.0,
            mean_bpfi: 0.0,
            bpfo_delta_per_hour: 0.0,
            bpfi_delta_per_hour: 0.0,
            motor_temp_delta_per_hour: 0.0,
            gearbox_temp_delta_per_hour: 0.0,
            count: 0,
        }
    }
}

/// Daily aggregate (24 hourly aggregates)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyAggregate {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub mean_health_score: f64,
    pub min_health_score: f64,
    pub max_health_score: f64,
    pub health_score_trend: f64,
    pub mean_motor_temp: f64,
    pub mean_gearbox_temp: f64,
    pub mean_bpfo: f64,
    pub mean_bpfi: f64,
    pub count: usize,
}

impl DailyAggregate {
    pub fn from_hourly(aggregates: &[HourlyAggregate]) -> Self {
        if aggregates.is_empty() {
            return Self::default();
        }

        let count = aggregates.len();
        let start_time = aggregates.first().unwrap().start_time;
        let end_time = aggregates.last().unwrap().end_time;

        let mean_health_score =
            aggregates.iter().map(|h| h.mean_health_score).sum::<f64>() / count as f64;
        let min_health_score = aggregates
            .iter()
            .map(|h| h.min_health_score)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        let max_health_score = aggregates
            .iter()
            .map(|h| h.max_health_score)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();

        // Health score trend (slope over the day)
        let health_scores: Vec<f64> = aggregates.iter().map(|h| h.mean_health_score).collect();
        let health_score_trend = Self::simple_slope(&health_scores);

        let mean_motor_temp = aggregates.iter().map(|h| h.mean_motor_temp).sum::<f64>() / count as f64;
        let mean_gearbox_temp =
            aggregates.iter().map(|h| h.mean_gearbox_temp).sum::<f64>() / count as f64;
        let mean_bpfo = aggregates.iter().map(|h| h.mean_bpfo).sum::<f64>() / count as f64;
        let mean_bpfi = aggregates.iter().map(|h| h.mean_bpfi).sum::<f64>() / count as f64;

        Self {
            start_time,
            end_time,
            mean_health_score,
            min_health_score,
            max_health_score,
            health_score_trend,
            mean_motor_temp,
            mean_gearbox_temp,
            mean_bpfo,
            mean_bpfi,
            count,
        }
    }

    fn simple_slope(values: &[f64]) -> f64 {
        if values.len() < 2 {
            return 0.0;
        }

        let n = values.len() as f64;
        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        let mut sum_xy = 0.0;
        let mut sum_x2 = 0.0;

        for (i, &y) in values.iter().enumerate() {
            let x = i as f64;
            sum_x += x;
            sum_y += y;
            sum_xy += x * y;
            sum_x2 += x * x;
        }

        let slope = (n * sum_xy - sum_x * sum_y) / (n * sum_x2 - sum_x * sum_x);
        slope.is_finite().then_some(slope).unwrap_or(0.0)
    }
}

impl Default for DailyAggregate {
    fn default() -> Self {
        Self {
            start_time: Utc::now(),
            end_time: Utc::now(),
            mean_health_score: 100.0,
            min_health_score: 100.0,
            max_health_score: 100.0,
            health_score_trend: 0.0,
            mean_motor_temp: 55.0,
            mean_gearbox_temp: 48.0,
            mean_bpfo: 0.0,
            mean_bpfi: 0.0,
            count: 0,
        }
    }
}
