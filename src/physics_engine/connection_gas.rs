//! Connection Gas Analysis
//!
//! Tracks gas readings during drilling connections (pipe added/removed events).
//! Connection gas is a key indicator of pore pressure — rising connection gas
//! deltas across consecutive connections indicate increasing formation pressure.
//!
//! ## Detection Logic
//!
//! 1. During Drilling: track background gas via EMA
//! 2. Drilling → non-Drilling transition: record pre-connection gas
//! 3. During connection: track peak gas
//! 4. non-Drilling → Drilling transition: finalize event with post-connection gas
//! 5. Compute delta and trend across multiple events

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use crate::types::{RigState, WitsPacket};

/// Maximum connection gas events kept in memory
const MAX_EVENTS: usize = 10;

/// EMA alpha for background gas tracking
const BACKGROUND_GAS_ALPHA: f64 = 0.05;

/// Connection state machine
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum ConnectionState {
    /// Actively drilling — tracking background gas
    Drilling {
        last_gas: f64,
    },
    /// In a connection (non-drilling) — tracking peak gas
    InConnection {
        pre_gas: f64,
        peak_gas: f64,
        start_ts: u64,
        depth_ft: f64,
    },
}

/// A single connection gas event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionGasEvent {
    /// Unix timestamp when connection ended (drilling resumed)
    pub timestamp: u64,
    /// Bit depth at time of connection (ft)
    pub depth_ft: f64,
    /// Gas reading just before connection (units)
    pub pre_gas: f64,
    /// Peak gas reading during connection (units)
    pub peak_gas: f64,
    /// Gas reading after connection (units) — first drilling packet post-connection
    pub post_gas: f64,
    /// Peak - pre gas delta (units)
    pub delta: f64,
    /// Delta above background gas (units)
    pub above_background: f64,
}

/// Tracks connection gas events and trends
#[derive(Debug, Clone)]
pub struct ConnectionGasTracker {
    state: ConnectionState,
    events: VecDeque<ConnectionGasEvent>,
    background_gas: f64,
    background_initialized: bool,
}

impl Default for ConnectionGasTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionGasTracker {
    pub fn new() -> Self {
        Self {
            state: ConnectionState::Drilling { last_gas: 0.0 },
            events: VecDeque::with_capacity(MAX_EVENTS),
            background_gas: 0.0,
            background_initialized: false,
        }
    }

    /// Update the tracker with a new packet and rig state.
    ///
    /// Returns a `ConnectionGasEvent` when a connection ends (transition back to drilling).
    pub fn update(
        &mut self,
        packet: &WitsPacket,
        rig_state: RigState,
    ) -> Option<ConnectionGasEvent> {
        let gas = packet.gas_units;
        let is_drilling = rig_state == RigState::Drilling || rig_state == RigState::Reaming;

        match &mut self.state {
            ConnectionState::Drilling { last_gas } => {
                if is_drilling {
                    // Still drilling — update background gas EMA
                    *last_gas = gas;
                    if !self.background_initialized {
                        self.background_gas = gas;
                        self.background_initialized = true;
                    } else {
                        self.background_gas = self.background_gas * (1.0 - BACKGROUND_GAS_ALPHA)
                            + gas * BACKGROUND_GAS_ALPHA;
                    }
                    None
                } else {
                    // Drilling → Connection: record pre-connection gas
                    let pre_gas = *last_gas;
                    self.state = ConnectionState::InConnection {
                        pre_gas,
                        peak_gas: gas,
                        start_ts: packet.timestamp,
                        depth_ft: packet.bit_depth,
                    };
                    None
                }
            }
            ConnectionState::InConnection {
                pre_gas,
                peak_gas,
                depth_ft,
                ..
            } => {
                if !is_drilling {
                    // Still in connection — track peak gas
                    if gas > *peak_gas {
                        *peak_gas = gas;
                    }
                    None
                } else {
                    // Connection → Drilling: finalize event
                    let event = ConnectionGasEvent {
                        timestamp: packet.timestamp,
                        depth_ft: *depth_ft,
                        pre_gas: *pre_gas,
                        peak_gas: *peak_gas,
                        post_gas: gas,
                        delta: *peak_gas - *pre_gas,
                        above_background: (*peak_gas - self.background_gas).max(0.0),
                    };

                    // Store event
                    if self.events.len() >= MAX_EVENTS {
                        self.events.pop_front();
                    }
                    self.events.push_back(event.clone());

                    // Return to drilling state
                    self.state = ConnectionState::Drilling { last_gas: gas };

                    Some(event)
                }
            }
        }
    }

    /// Compute linear regression slope on event deltas.
    /// Positive slope = connection gas increasing over successive connections.
    pub fn trend_slope(&self) -> f64 {
        let n = self.events.len();
        if n < 2 {
            return 0.0;
        }
        let n_f = n as f64;
        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        let mut sum_xy = 0.0;
        let mut sum_x2 = 0.0;

        for (i, event) in self.events.iter().enumerate() {
            let x = i as f64;
            let y = event.delta;
            sum_x += x;
            sum_y += y;
            sum_xy += x * y;
            sum_x2 += x * x;
        }

        let denom = n_f * sum_x2 - sum_x * sum_x;
        if denom.abs() < 1e-10 {
            return 0.0;
        }

        (n_f * sum_xy - sum_x * sum_y) / denom
    }

    /// Get the latest connection gas events
    pub fn latest_events(&self) -> &VecDeque<ConnectionGasEvent> {
        &self.events
    }

    /// Check if connection gas is trending up.
    ///
    /// Requires >= 5 events and a meaningful positive slope (> 0.1 gas units per
    /// connection) to avoid false positives from noise in the first few events.
    pub fn is_trending_up(&self) -> bool {
        self.events.len() >= 5 && self.trend_slope() > 0.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::WitsPacket;

    fn make_packet(timestamp: u64, depth: f64, gas: f64) -> WitsPacket {
        let mut p = WitsPacket::default();
        p.timestamp = timestamp;
        p.bit_depth = depth;
        p.gas_units = gas;
        p
    }

    #[test]
    fn test_drilling_connection_drilling_produces_event() {
        let mut tracker = ConnectionGasTracker::new();

        // Drilling phase — establish background
        for i in 0..10 {
            let p = make_packet(i, 5000.0, 20.0);
            let ev = tracker.update(&p, RigState::Drilling);
            assert!(ev.is_none());
        }

        // Transition to connection (non-drilling)
        let p = make_packet(10, 5000.0, 25.0);
        let ev = tracker.update(&p, RigState::Connection);
        assert!(ev.is_none());

        // Peak gas during connection
        let p = make_packet(11, 5000.0, 50.0);
        let ev = tracker.update(&p, RigState::Connection);
        assert!(ev.is_none());

        // Gas decreasing during connection
        let p = make_packet(12, 5000.0, 35.0);
        let ev = tracker.update(&p, RigState::Connection);
        assert!(ev.is_none());

        // Resume drilling — event produced
        let p = make_packet(13, 5000.0, 22.0);
        let ev = tracker.update(&p, RigState::Drilling);
        assert!(ev.is_some());
        let ev = ev.unwrap();
        assert_eq!(ev.pre_gas, 20.0);
        assert_eq!(ev.peak_gas, 50.0);
        assert_eq!(ev.post_gas, 22.0);
        assert_eq!(ev.delta, 30.0);
    }

    #[test]
    fn test_increasing_deltas_positive_trend() {
        let mut tracker = ConnectionGasTracker::new();

        // Simulate 5 connections with increasing deltas
        let deltas = [10.0, 15.0, 20.0, 25.0, 30.0];

        for (i, delta) in deltas.iter().enumerate() {
            let base_ts = (i * 100) as u64;
            let depth = 5000.0 + (i as f64 * 30.0); // 30 ft per connection

            // Drilling phase
            for j in 0..5 {
                let p = make_packet(base_ts + j, depth, 20.0);
                tracker.update(&p, RigState::Drilling);
            }

            // Connection with peak gas
            let p = make_packet(base_ts + 5, depth, 20.0);
            tracker.update(&p, RigState::Connection);

            let p = make_packet(base_ts + 6, depth, 20.0 + delta);
            tracker.update(&p, RigState::Connection);

            // Resume drilling
            let p = make_packet(base_ts + 7, depth, 20.0);
            tracker.update(&p, RigState::Drilling);
        }

        assert_eq!(tracker.latest_events().len(), 5);
        assert!(
            tracker.trend_slope() > 0.0,
            "Trend slope should be positive: {}",
            tracker.trend_slope()
        );
        assert!(tracker.is_trending_up());
    }

    #[test]
    fn test_three_events_does_not_trigger_trending() {
        let mut tracker = ConnectionGasTracker::new();

        // Simulate 3 connections with increasing deltas
        let deltas = [10.0, 20.0, 30.0];

        for (i, delta) in deltas.iter().enumerate() {
            let base_ts = (i * 100) as u64;
            let depth = 5000.0 + (i as f64 * 30.0);

            for j in 0..5 {
                let p = make_packet(base_ts + j, depth, 20.0);
                tracker.update(&p, RigState::Drilling);
            }

            let p = make_packet(base_ts + 5, depth, 20.0);
            tracker.update(&p, RigState::Connection);

            let p = make_packet(base_ts + 6, depth, 20.0 + delta);
            tracker.update(&p, RigState::Connection);

            let p = make_packet(base_ts + 7, depth, 20.0);
            tracker.update(&p, RigState::Drilling);
        }

        assert_eq!(tracker.latest_events().len(), 3);
        assert!(
            !tracker.is_trending_up(),
            "3 events should not trigger trending (requires >= 5)"
        );
    }

    #[test]
    fn test_five_events_flat_slope_not_trending() {
        let mut tracker = ConnectionGasTracker::new();

        // Simulate 5 connections with constant deltas (slope ≈ 0)
        for i in 0..5 {
            let base_ts = (i * 100) as u64;
            let depth = 5000.0 + (i as f64 * 30.0);

            for j in 0..5 {
                let p = make_packet(base_ts + j, depth, 20.0);
                tracker.update(&p, RigState::Drilling);
            }

            let p = make_packet(base_ts + 5, depth, 20.0);
            tracker.update(&p, RigState::Connection);

            // Constant delta = 15.0 for all events
            let p = make_packet(base_ts + 6, depth, 35.0);
            tracker.update(&p, RigState::Connection);

            let p = make_packet(base_ts + 7, depth, 20.0);
            tracker.update(&p, RigState::Drilling);
        }

        assert_eq!(tracker.latest_events().len(), 5);
        assert!(
            !tracker.is_trending_up(),
            "Flat slope should not trigger trending"
        );
    }
}
