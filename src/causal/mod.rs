//! Causal Inference — lightweight Granger-causality on edge
//!
//! Detects which drilling parameters (WOB, RPM, torque, SPP, ROP) causally
//! precede MSE spikes in the 60-packet history buffer.
//!
//! ## Algorithm
//!
//! For each candidate parameter X and target series Y (MSE):
//!   - Compute cross-correlation at lags 1..=`MAX_LAG_SECS`
//!   - Record the lag with highest |Pearson r|
//!   - If |r| ≥ `MIN_CORRELATION`, emit a `CausalLead`
//!
//! Cross-correlation at lag k: r(X[0..n-k], Y[k..n])
//! This answers: "does X at time t-k help predict MSE at time t?"
//!
//! Runs in < 1 ms on a 60-sample buffer — no external crates required.

use crate::types::{CausalLead, HistoryEntry};

/// Minimum |Pearson r| to report a causal lead.
const MIN_CORRELATION: f64 = 0.45;

/// Maximum lag to test (seconds). At 1 Hz this equals packets.
const MAX_LAG_SECS: usize = 20;

/// Minimum history length required to run analysis.
const MIN_HISTORY: usize = 20;

/// Maximum causal leads to return (sorted by |r| descending).
const MAX_LEADS: usize = 3;

/// Detect leading indicators for MSE spikes from the drilling history buffer.
///
/// Returns up to [`MAX_LEADS`] parameters that most strongly precede MSE
/// changes, sorted by correlation strength descending. Returns an empty
/// `Vec` when the history buffer is too short to compute reliable statistics.
pub fn detect_leads(history: &[HistoryEntry]) -> Vec<CausalLead> {
    if history.len() < MIN_HISTORY {
        return Vec::new();
    }

    let max_lag = MAX_LAG_SECS.min(history.len() / 3);

    // Target series: MSE (the metric we want to predict)
    let mse: Vec<f64> = history.iter().map(|e| e.metrics.mse).collect();

    // Candidate input series — each is a (name, values) pair
    let candidates: [(&str, Vec<f64>); 5] = [
        ("WOB",    history.iter().map(|e| e.packet.wob).collect()),
        ("RPM",    history.iter().map(|e| e.packet.rpm).collect()),
        ("Torque", history.iter().map(|e| e.packet.torque).collect()),
        ("SPP",    history.iter().map(|e| e.packet.spp).collect()),
        ("ROP",    history.iter().map(|e| e.packet.rop).collect()),
    ];

    let mut leads: Vec<CausalLead> = Vec::new();

    for (name, series) in &candidates {
        let (best_lag, best_r) = best_lagged_correlation(series, &mse, max_lag);
        if best_r.abs() >= MIN_CORRELATION {
            leads.push(CausalLead {
                parameter: name.to_string(),
                lag_seconds: best_lag as u32,
                pearson_r: best_r,
                correlation_sign: if best_r > 0.0 { 1 } else { -1 },
            });
        }
    }

    // Sort by |r| descending and cap at MAX_LEADS
    leads.sort_by(|a, b| {
        b.pearson_r
            .abs()
            .partial_cmp(&a.pearson_r.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    leads.truncate(MAX_LEADS);
    leads
}

/// Find the lag (1..=`max_lag`) that maximises |Pearson r| between `x` and `mse`.
///
/// Returns `(best_lag, best_r)`. If `max_lag` is 0 or `x` is too short, returns
/// `(0, 0.0)`.
fn best_lagged_correlation(x: &[f64], mse: &[f64], max_lag: usize) -> (usize, f64) {
    let mut best_lag = 0usize;
    let mut best_r = 0.0f64;

    for lag in 1..=max_lag {
        if lag >= x.len() {
            break;
        }
        // cause = x[0..n-lag]  (what happened lag seconds ago)
        // effect = mse[lag..n]  (what MSE is now)
        let cause = &x[..x.len() - lag];
        let effect = &mse[lag..];
        let r = pearson_r(cause, effect);
        if r.abs() > best_r.abs() {
            best_r = r;
            best_lag = lag;
        }
    }

    (best_lag, best_r)
}

/// Pearson correlation coefficient for two equal-length slices.
///
/// Returns 0.0 when either series has zero variance or fewer than 3 points.
fn pearson_r(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len().min(y.len());
    if n < 3 {
        return 0.0;
    }
    let n_f = n as f64;
    let mean_x = x[..n].iter().sum::<f64>() / n_f;
    let mean_y = y[..n].iter().sum::<f64>() / n_f;

    let mut num = 0.0_f64;
    let mut den_x = 0.0_f64;
    let mut den_y = 0.0_f64;

    for i in 0..n {
        let dx = x[i] - mean_x;
        let dy = y[i] - mean_y;
        num += dx * dy;
        den_x += dx * dx;
        den_y += dy * dy;
    }

    let denom = (den_x * den_y).sqrt();
    if denom < 1e-10 {
        0.0
    } else {
        num / denom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pearson_r_perfect_correlation() {
        let x: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let r = pearson_r(&x, &x);
        assert!((r - 1.0).abs() < 1e-9, "Expected 1.0, got {r}");
    }

    #[test]
    fn pearson_r_perfect_anticorrelation() {
        let x: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let y: Vec<f64> = (0..10).map(|i| -(i as f64)).collect();
        let r = pearson_r(&x, &y);
        assert!((r + 1.0).abs() < 1e-9, "Expected -1.0, got {r}");
    }

    #[test]
    fn pearson_r_constant_series_returns_zero() {
        let x = vec![5.0; 10];
        let y: Vec<f64> = (0..10).map(|i| i as f64).collect();
        assert_eq!(pearson_r(&x, &y), 0.0);
    }

    #[test]
    fn detect_leads_empty_history() {
        assert!(detect_leads(&[]).is_empty());
    }

    #[test]
    fn detect_leads_insufficient_history() {
        // Build 10 entries — below MIN_HISTORY (20)
        let entries: Vec<HistoryEntry> = (0..10)
            .map(|_| make_entry(25.0, 120.0, 15.0, 2800.0, 50.0, 30_000.0))
            .collect();
        assert!(detect_leads(&entries).is_empty());
    }

    #[test]
    fn detect_leads_wob_leads_mse() {
        // Construct history where WOB steadily increases 10 packets before MSE rises.
        // First 20 packets: constant WOB=20, MSE=20_000
        // Next 20 packets: WOB=30 (higher), MSE still 20_000 (the "lag" window)
        // Last 20 packets: WOB=30, MSE=40_000 (MSE now reflects earlier WOB rise)
        let mut entries: Vec<HistoryEntry> = Vec::new();
        for _ in 0..20 {
            entries.push(make_entry(20.0, 120.0, 15.0, 2800.0, 50.0, 20_000.0));
        }
        for _ in 0..20 {
            entries.push(make_entry(30.0, 120.0, 15.0, 2800.0, 50.0, 20_000.0));
        }
        for _ in 0..20 {
            entries.push(make_entry(30.0, 120.0, 15.0, 2800.0, 50.0, 40_000.0));
        }
        let leads = detect_leads(&entries);
        // WOB should be detected as a leading indicator with positive r
        let wob_lead = leads.iter().find(|l| l.parameter == "WOB");
        assert!(wob_lead.is_some(), "WOB should be a causal lead");
        if let Some(lead) = wob_lead {
            assert!(lead.pearson_r > 0.0, "WOB→MSE correlation should be positive");
            assert!(lead.lag_seconds > 0, "Lag should be > 0");
        }
    }

    #[test]
    fn detect_leads_max_three_results() {
        // All parameters correlated with MSE (ramp together)
        let entries: Vec<HistoryEntry> = (0..60)
            .map(|i| {
                let v = i as f64;
                make_entry(v, v * 5.0, v * 0.5, v * 20.0, v, v * 500.0)
            })
            .collect();
        let leads = detect_leads(&entries);
        assert!(leads.len() <= MAX_LEADS, "Should return at most {MAX_LEADS} leads");
    }

    // ── helpers ──────────────────────────────────────────────────────────────

    fn make_entry(wob: f64, rpm: f64, torque: f64, spp: f64, rop: f64, mse: f64) -> HistoryEntry {
        use crate::types::{DrillingMetrics, Operation, RigState, WitsPacket};
        use std::sync::Arc;

        let packet = WitsPacket {
            timestamp: 0,
            bit_depth: 10_000.0,
            hole_depth: 10_050.0,
            rop,
            hook_load: 200.0,
            wob,
            rpm,
            torque,
            bit_diameter: 8.5,
            spp,
            pump_spm: 120.0,
            flow_in: 500.0,
            flow_out: 500.0,
            pit_volume: 500.0,
            pit_volume_change: 0.0,
            mud_weight_in: 12.0,
            mud_weight_out: 12.0,
            ecd: 12.4,
            mud_temp_in: 100.0,
            mud_temp_out: 120.0,
            gas_units: 50.0,
            background_gas: 40.0,
            connection_gas: 10.0,
            h2s: 0.0,
            co2: 0.1,
            casing_pressure: 0.0,
            annular_pressure: 0.0,
            pore_pressure: 10.5,
            fracture_gradient: 14.0,
            mse,
            d_exponent: 1.5,
            dxc: 1.45,
            rop_delta: 0.0,
            torque_delta_percent: 0.0,
            spp_delta: 0.0,
            rig_state: RigState::Drilling,
            regime_id: 0,
            seconds_since_param_change: 0,        };

        let metrics = DrillingMetrics {
            mse,
            mse_efficiency: 70.0,
            state: RigState::Drilling,
            operation: Operation::ProductionDrilling,
            ..DrillingMetrics::default()
        };

        HistoryEntry { packet, metrics }
    }
}
