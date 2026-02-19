//! WITS Drilling Simulation
//!
//! Generates realistic WITS Level 0 drilling data for testing SAIREN-OS.
//! Simulates various drilling scenarios including:
//! - Normal drilling operations
//! - MSE inefficiency (bit wear, formation change)
//! - Well control events (kick, lost circulation)
//! - Mechanical issues (pack-off, stick-slip)
//!
//! # Usage
//! ```bash
//! ./simulation --hours 1 --speed 100 | ./sairen-os --stdin
//! ```

use clap::Parser;
use rand::prelude::*;
use rand_distr::{Normal, Distribution};
use std::io::{self, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sairen_os::types::{RigState, WitsPacket};

// ============================================================================
// Drilling Constants
// ============================================================================

/// Standard bit diameter (inches)
const BIT_DIAMETER: f64 = 8.5;
/// Baseline ROP (ft/hr)
const BASE_ROP: f64 = 50.0;
/// Baseline WOB (klbs)
const BASE_WOB: f64 = 25.0;
/// Baseline RPM
const BASE_RPM: f64 = 120.0;
/// Baseline torque (kft-lbs)
const BASE_TORQUE: f64 = 15.0;
/// Baseline SPP (psi)
const BASE_SPP: f64 = 2800.0;
/// Baseline flow rate (gpm)
const BASE_FLOW: f64 = 500.0;
/// Baseline mud weight (ppg)
const BASE_MUD_WEIGHT: f64 = 12.0;
/// Baseline gas units
const BASE_GAS: f64 = 50.0;

// ============================================================================
// CLI Arguments
// ============================================================================

#[derive(Parser, Debug)]
#[command(name = "wits-simulation")]
#[command(about = "WITS drilling data simulation for SAIREN-OS testing")]
#[command(version = "1.0")]
struct Args {
    /// Simulation duration in hours (1-24)
    #[arg(short = 'H', long, default_value = "1", value_parser = clap::value_parser!(u32).range(1..=24))]
    hours: u32,

    /// Time compression factor (1 = real-time, 100 = 100x faster)
    #[arg(short, long, default_value = "100", value_parser = clap::value_parser!(u32).range(1..=1000))]
    speed: u32,

    /// Output format: json or csv
    #[arg(short, long, default_value = "json")]
    format: String,

    /// Suppress mission log (only output sensor data)
    #[arg(short, long)]
    quiet: bool,

    /// Output sample rate in Hz
    #[arg(long, default_value = "1")]
    sample_rate: u32,

    /// Random seed for reproducibility
    #[arg(long)]
    seed: Option<u64>,

    /// Drilling scenario to simulate
    #[arg(long, default_value = "full")]
    scenario: String,
}

// ============================================================================
// Simulation Phases
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// System warmup and baseline learning (0-40%)
    BaselineLearning,
    /// Normal efficient drilling (40-55%)
    NormalDrilling,
    /// MSE inefficiency - bit wear or wrong parameters (55-70%)
    MseInefficiency,
    /// Well control event - kick scenario (70-80%)
    KickEvent,
    /// Pack-off scenario (80-90%)
    PackOff,
    /// Return to normal (90-100%)
    Recovery,
}

impl Phase {
    fn name(&self) -> &'static str {
        match self {
            Phase::BaselineLearning => "Baseline Learning (System Warmup)",
            Phase::NormalDrilling => "Normal Drilling (Optimal Parameters)",
            Phase::MseInefficiency => "MSE Inefficiency (Bit Wear / Formation Change)",
            Phase::KickEvent => "Well Control Event (Kick)",
            Phase::PackOff => "Mechanical Issue (Pack-Off)",
            Phase::Recovery => "Recovery (Return to Normal)",
        }
    }

    fn from_progress(progress: f64) -> Self {
        match progress {
            p if p < 0.40 => Phase::BaselineLearning,
            p if p < 0.55 => Phase::NormalDrilling,
            p if p < 0.70 => Phase::MseInefficiency,
            p if p < 0.80 => Phase::KickEvent,
            p if p < 0.90 => Phase::PackOff,
            _ => Phase::Recovery,
        }
    }

    fn rig_state(&self) -> RigState {
        match self {
            Phase::BaselineLearning => RigState::Drilling,
            Phase::NormalDrilling => RigState::Drilling,
            Phase::MseInefficiency => RigState::Drilling,
            Phase::KickEvent => RigState::Drilling,
            Phase::PackOff => RigState::Reaming,
            Phase::Recovery => RigState::Drilling,
        }
    }
}

// ============================================================================
// Simulation State
// ============================================================================

struct SimulationState {
    rng: StdRng,
    current_phase: Phase,
    sim_time_seconds: f64,
    total_duration_seconds: f64,
    output_sample_rate: f64,

    // Current drilling depth
    current_depth: f64,
    hole_depth: f64,

    // Baseline values (established during learning phase)
    baseline_mse: f64,
    baseline_d_exp: f64,

    // Current state values
    rop: f64,
    wob: f64,
    rpm: f64,
    torque: f64,
    spp: f64,
    flow_in: f64,
    flow_out: f64,
    pit_volume: f64,
    mud_weight_in: f64,
    mud_weight_out: f64,
    gas_units: f64,
    mud_temp_in: f64,
    mud_temp_out: f64,

    // Derived values
    mse: f64,
    d_exponent: f64,

    // Statistics
    packets_generated: u64,
    anomaly_packets: u64,

    // Normal distributions
    small_noise: Normal<f64>,
    medium_noise: Normal<f64>,
}

impl SimulationState {
    fn new(duration_hours: u32, sample_rate: u32, seed: Option<u64>) -> Self {
        let rng = match seed {
            Some(s) => StdRng::seed_from_u64(s),
            None => StdRng::from_entropy(),
        };

        Self {
            rng,
            current_phase: Phase::BaselineLearning,
            sim_time_seconds: 0.0,
            total_duration_seconds: duration_hours as f64 * 3600.0,
            output_sample_rate: sample_rate as f64,
            current_depth: 10000.0,
            hole_depth: 10050.0,
            baseline_mse: 35000.0,
            baseline_d_exp: 1.5,
            rop: BASE_ROP,
            wob: BASE_WOB,
            rpm: BASE_RPM,
            torque: BASE_TORQUE,
            spp: BASE_SPP,
            flow_in: BASE_FLOW,
            flow_out: BASE_FLOW + 2.0,
            pit_volume: 500.0,
            mud_weight_in: BASE_MUD_WEIGHT,
            mud_weight_out: BASE_MUD_WEIGHT + 0.1,
            gas_units: BASE_GAS,
            mud_temp_in: 100.0,
            mud_temp_out: 120.0,
            mse: 35000.0,
            d_exponent: 1.5,
            packets_generated: 0,
            anomaly_packets: 0,
            small_noise: Normal::new(0.0, 0.02).unwrap(),
            medium_noise: Normal::new(0.0, 0.1).unwrap(),
        }
    }

    fn progress(&self) -> f64 {
        self.sim_time_seconds / self.total_duration_seconds
    }

    fn update_phase(&mut self) -> bool {
        let new_phase = Phase::from_progress(self.progress());
        if new_phase != self.current_phase {
            self.current_phase = new_phase;
            true
        } else {
            false
        }
    }

    /// Calculate MSE: MSE = (480 * T * RPM) / (D^2 * ROP) + (4 * WOB) / (pi * D^2)
    fn calculate_mse(&self) -> f64 {
        if self.rop < 0.1 {
            return 0.0;
        }
        let d_sq = BIT_DIAMETER * BIT_DIAMETER;
        let rotary_term = (480.0 * self.torque * self.rpm) / (d_sq * self.rop);
        let wob_term = (4.0 * self.wob * 1000.0) / (std::f64::consts::PI * d_sq);
        rotary_term + wob_term
    }

    /// Calculate d-exponent: d = log(ROP / 60*RPM) / log(12*WOB / 1000*D)
    fn calculate_d_exponent(&self) -> f64 {
        if self.rpm < 1.0 || self.wob < 0.1 {
            return 1.0;
        }
        let rop_term = self.rop / (60.0 * self.rpm);
        let wob_term = (12.0 * self.wob) / (1000.0 * BIT_DIAMETER);

        if rop_term <= 0.0 || wob_term <= 0.0 {
            return 1.0;
        }

        rop_term.log10() / wob_term.log10()
    }

    /// Update drilling parameters based on current phase
    fn update_parameters(&mut self) {
        let noise_small = self.small_noise.sample(&mut self.rng);
        let noise_med = self.medium_noise.sample(&mut self.rng);

        match self.current_phase {
            Phase::BaselineLearning | Phase::NormalDrilling | Phase::Recovery => {
                // Normal drilling - efficient parameters
                self.rop = BASE_ROP * (1.0 + noise_small);
                self.wob = BASE_WOB * (1.0 + noise_small);
                self.rpm = BASE_RPM * (1.0 + noise_small * 0.5);
                self.torque = BASE_TORQUE * (1.0 + noise_small);
                self.spp = BASE_SPP * (1.0 + noise_small);
                self.flow_in = BASE_FLOW * (1.0 + noise_small);
                self.flow_out = self.flow_in + 2.0 + noise_small * 5.0;
                self.pit_volume = 500.0 + noise_small * 2.0;
                self.gas_units = BASE_GAS * (1.0 + noise_small);
                self.mud_temp_out = 120.0 + noise_small * 5.0;
            }

            Phase::MseInefficiency => {
                // MSE inefficiency - higher energy, lower ROP
                let phase_progress = (self.progress() - 0.55) / 0.15;
                let inefficiency = phase_progress.clamp(0.0, 1.0);

                self.rop = BASE_ROP * (0.5 + 0.3 * (1.0 - inefficiency)) * (1.0 + noise_small);
                self.wob = (BASE_WOB + 8.0 * inefficiency) * (1.0 + noise_small);
                self.torque = (BASE_TORQUE + 5.0 * inefficiency) * (1.0 + noise_small);
                self.rpm = (BASE_RPM - 20.0 * inefficiency) * (1.0 + noise_small * 0.5);
                self.spp = (BASE_SPP + 150.0 * inefficiency) * (1.0 + noise_small);
                self.flow_in = BASE_FLOW * (1.0 + noise_small);
                self.flow_out = self.flow_in + 2.0 + noise_small * 5.0;
                self.gas_units = (BASE_GAS + 30.0 * inefficiency) * (1.0 + noise_med);
                self.mud_temp_out = 120.0 + 10.0 * inefficiency + noise_small * 5.0;

                self.anomaly_packets += 1;
            }

            Phase::KickEvent => {
                // Kick - flow out > flow in, pit gain, gas increase
                // Made more severe to trigger CRITICAL alerts
                let phase_progress = (self.progress() - 0.70) / 0.10;
                let kick_severity = phase_progress.clamp(0.0, 1.0);

                self.rop = BASE_ROP * 0.3 * (1.0 + noise_small); // Severe ROP drop
                self.wob = BASE_WOB * 0.5 * (1.0 + noise_small);
                self.torque = BASE_TORQUE * 0.7 * (1.0 + noise_small);

                // Severe flow imbalance (major kick indicator)
                self.flow_in = BASE_FLOW * (1.0 + noise_small);
                self.flow_out = self.flow_in + 40.0 + 80.0 * kick_severity + noise_med * 15.0;

                // Major pit gain
                self.pit_volume = 500.0 + 20.0 + 40.0 * kick_severity;

                // Severe gas increase
                self.gas_units = BASE_GAS + 300.0 * kick_severity + 500.0 * kick_severity * kick_severity;

                // Major SPP drop (formation fluid influx)
                self.spp = (BASE_SPP - 400.0 * kick_severity) * (1.0 + noise_small);

                // Significant mud weight out drops (dilution)
                self.mud_weight_out = BASE_MUD_WEIGHT - 0.8 * kick_severity;

                self.mud_temp_out = 120.0 + 25.0 * kick_severity + noise_small * 5.0;

                self.anomaly_packets += 1;
            }

            Phase::PackOff => {
                // Pack-off - torque spike, SPP increase
                let phase_progress = (self.progress() - 0.80) / 0.10;
                let packoff_severity = phase_progress.clamp(0.0, 1.0);

                self.rop = BASE_ROP * (0.3 - 0.2 * packoff_severity) * (1.0 + noise_small);
                self.wob = BASE_WOB * (1.2 + 0.3 * packoff_severity) * (1.0 + noise_small);

                // Torque increase (key indicator)
                self.torque = BASE_TORQUE * (1.3 + 0.5 * packoff_severity) * (1.0 + noise_med);

                // SPP increase (annular restriction)
                self.spp = (BASE_SPP + 300.0 * packoff_severity) * (1.0 + noise_small);

                self.rpm = BASE_RPM * 0.8 * (1.0 + noise_small * 0.5);
                self.flow_in = BASE_FLOW * (1.0 + noise_small);
                self.flow_out = self.flow_in - 5.0 * packoff_severity + noise_small * 5.0;
                self.gas_units = BASE_GAS * (1.0 + noise_small);
                self.mud_temp_out = 120.0 + 5.0 * packoff_severity + noise_small * 5.0;

                self.anomaly_packets += 1;
            }
        }

        // Update derived values
        self.mse = self.calculate_mse();
        self.d_exponent = self.calculate_d_exponent();

        // Update depth (only when drilling)
        if self.current_phase != Phase::PackOff {
            let depth_increment = self.rop / 3600.0 / self.output_sample_rate;
            self.current_depth += depth_increment;
            if self.current_depth > self.hole_depth {
                self.hole_depth = self.current_depth;
            }
        }
    }

    /// Generate a WITS packet
    fn generate_packet(&mut self) -> WitsPacket {
        self.packets_generated += 1;
        self.update_parameters();

        let timestamp = self.sim_time_seconds as u64;

        // Calculate additional derived values
        let ecd = self.mud_weight_in + 0.3 + self.rng.gen_range(-0.05..0.05);
        let rop_delta = if self.current_phase == Phase::MseInefficiency {
            -25.0 + self.rng.gen_range(-5.0..5.0)
        } else {
            self.rng.gen_range(-3.0..3.0)
        };

        let _mse_delta = ((self.mse - self.baseline_mse) / self.baseline_mse * 100.0).abs();
        let torque_delta = if self.current_phase == Phase::PackOff {
            20.0 + self.rng.gen_range(0.0..10.0)
        } else {
            self.rng.gen_range(-2.0..2.0)
        };

        let spp_delta = if self.current_phase == Phase::PackOff {
            200.0 + self.rng.gen_range(0.0..100.0)
        } else if self.current_phase == Phase::KickEvent {
            -100.0 + self.rng.gen_range(-50.0..0.0)
        } else {
            self.rng.gen_range(-20.0..20.0)
        };

        WitsPacket {
            timestamp,
            bit_depth: self.current_depth,
            hole_depth: self.hole_depth,
            rop: self.rop,
            hook_load: 200.0 + self.rng.gen_range(-10.0..10.0),
            wob: self.wob,
            rpm: self.rpm,
            torque: self.torque,
            bit_diameter: BIT_DIAMETER,
            spp: self.spp,
            pump_spm: 120.0 + self.rng.gen_range(-2.0..2.0),
            flow_in: self.flow_in,
            flow_out: self.flow_out,
            pit_volume: self.pit_volume,
            pit_volume_change: if self.current_phase == Phase::KickEvent {
                (self.pit_volume - 500.0).max(0.0)
            } else {
                0.0
            },
            mud_weight_in: self.mud_weight_in,
            mud_weight_out: self.mud_weight_out,
            ecd,
            mud_temp_in: self.mud_temp_in,
            mud_temp_out: self.mud_temp_out,
            gas_units: self.gas_units,
            background_gas: self.gas_units * 0.8,
            connection_gas: self.gas_units * 0.2,
            h2s: 0.0,
            co2: 0.1 + self.rng.gen_range(-0.02..0.02),
            casing_pressure: if self.current_phase == Phase::KickEvent {
                50.0 + 100.0 * ((self.progress() - 0.70) / 0.10).clamp(0.0, 1.0)
            } else {
                self.rng.gen_range(0.0..10.0)
            },
            annular_pressure: self.rng.gen_range(0.0..20.0),
            pore_pressure: 10.5,
            fracture_gradient: 14.0,
            mse: self.mse,
            d_exponent: self.d_exponent,
            dxc: self.d_exponent * (BASE_MUD_WEIGHT / self.mud_weight_in),
            rop_delta,
            torque_delta_percent: torque_delta,
            spp_delta,
            rig_state: self.current_phase.rig_state(),
            waveform_snapshot: Arc::new(Vec::new()),
        }
    }
}

// ============================================================================
// Logging Utilities
// ============================================================================

fn format_time(seconds: f64) -> String {
    let hours = (seconds / 3600.0) as u32;
    let minutes = ((seconds % 3600.0) / 60.0) as u32;
    let secs = (seconds % 60.0) as u32;
    format!("{:02}:{:02}:{:02}", hours, minutes, secs)
}

fn log_mission(time: f64, message: &str, quiet: bool) {
    if !quiet {
        eprintln!("[{}] {}", format_time(time), message);
    }
}

// ============================================================================
// Main Entry Point
// ============================================================================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Initialize simulation
    let mut state = SimulationState::new(args.hours, args.sample_rate, args.seed);

    // Timing calculations
    let total_samples = (state.total_duration_seconds * state.output_sample_rate) as u64;
    let sample_interval_real = Duration::from_secs_f64(1.0 / (state.output_sample_rate * args.speed as f64));
    let sample_interval_sim = 1.0 / state.output_sample_rate;

    // Mission briefing
    log_mission(0.0, &"=".repeat(70), args.quiet);
    log_mission(0.0, "WITS DRILLING SIMULATION v1.0", args.quiet);
    log_mission(0.0, "SAIREN-OS Operational Intelligence Test Data Generator", args.quiet);
    log_mission(0.0, &"=".repeat(70), args.quiet);
    log_mission(0.0, "", args.quiet);
    log_mission(0.0, "WELL PARAMETERS:", args.quiet);
    log_mission(0.0, &format!("  Starting Depth: {:.0} ft", state.current_depth), args.quiet);
    log_mission(0.0, &format!("  Bit Diameter: {:.1} in", BIT_DIAMETER), args.quiet);
    log_mission(0.0, &format!("  Mud Weight: {:.1} ppg", BASE_MUD_WEIGHT), args.quiet);
    log_mission(0.0, "", args.quiet);
    log_mission(0.0, "DRILLING PARAMETERS:", args.quiet);
    log_mission(0.0, &format!("  Target ROP: {:.0} ft/hr", BASE_ROP), args.quiet);
    log_mission(0.0, &format!("  WOB: {:.0} klbs", BASE_WOB), args.quiet);
    log_mission(0.0, &format!("  RPM: {:.0}", BASE_RPM), args.quiet);
    log_mission(0.0, &format!("  Flow Rate: {:.0} gpm", BASE_FLOW), args.quiet);
    log_mission(0.0, "", args.quiet);
    log_mission(0.0, "SIMULATION PARAMETERS:", args.quiet);
    log_mission(0.0, &format!("  Duration: {} hours ({} samples)", args.hours, total_samples), args.quiet);
    log_mission(0.0, &format!("  Speed: {}x compression", args.speed), args.quiet);
    log_mission(0.0, &format!("  Output sample rate: {} Hz", args.sample_rate), args.quiet);
    if let Some(seed) = args.seed {
        log_mission(0.0, &format!("  Random seed: {}", seed), args.quiet);
    }
    log_mission(0.0, "", args.quiet);
    log_mission(0.0, "SCENARIO PHASES:", args.quiet);
    log_mission(0.0, "  0-40%:  Baseline Learning (normal drilling)", args.quiet);
    log_mission(0.0, "  40-55%: Normal Drilling (optimal parameters)", args.quiet);
    log_mission(0.0, "  55-70%: MSE Inefficiency (bit wear simulation)", args.quiet);
    log_mission(0.0, "  70-80%: Well Control Event (kick simulation)", args.quiet);
    log_mission(0.0, "  80-90%: Mechanical Issue (pack-off simulation)", args.quiet);
    log_mission(0.0, "  90-100%: Recovery (return to normal)", args.quiet);
    log_mission(0.0, &"=".repeat(70), args.quiet);
    log_mission(0.0, "SIMULATION START", args.quiet);
    log_mission(0.0, &"=".repeat(70), args.quiet);

    // CSV header if needed
    if args.format == "csv" {
        println!("timestamp,bit_depth,rop,wob,rpm,torque,spp,flow_in,flow_out,gas_units,mse,rig_state");
    }

    let start_time = Instant::now();
    let mut last_log_percent = 0;

    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();

    // Main simulation loop
    while state.sim_time_seconds < state.total_duration_seconds {
        let loop_start = Instant::now();

        // Phase transition logging
        if state.update_phase() {
            log_mission(state.sim_time_seconds, "", args.quiet);
            log_mission(state.sim_time_seconds, &format!(">>> PHASE: {}", state.current_phase.name()), args.quiet);

            match state.current_phase {
                Phase::BaselineLearning => {
                    log_mission(state.sim_time_seconds, "    Normal drilling - system learning baseline", args.quiet);
                    log_mission(state.sim_time_seconds, "    Expected: No advisories", args.quiet);
                }
                Phase::NormalDrilling => {
                    log_mission(state.sim_time_seconds, "    Optimal drilling parameters", args.quiet);
                    log_mission(state.sim_time_seconds, "    Expected: No advisories", args.quiet);
                }
                Phase::MseInefficiency => {
                    log_mission(state.sim_time_seconds, "    Simulating bit wear / formation change", args.quiet);
                    log_mission(state.sim_time_seconds, "    Expected: MSE inefficiency advisory", args.quiet);
                }
                Phase::KickEvent => {
                    log_mission(state.sim_time_seconds, "    SIMULATING KICK - Flow imbalance, pit gain, gas", args.quiet);
                    log_mission(state.sim_time_seconds, "    Expected: CRITICAL well control advisory", args.quiet);
                }
                Phase::PackOff => {
                    log_mission(state.sim_time_seconds, "    Simulating pack-off condition", args.quiet);
                    log_mission(state.sim_time_seconds, "    Expected: Mechanical issue advisory", args.quiet);
                }
                Phase::Recovery => {
                    log_mission(state.sim_time_seconds, "    Returning to normal operations", args.quiet);
                    log_mission(state.sim_time_seconds, "    Expected: Advisories clearing", args.quiet);
                }
            }
            log_mission(state.sim_time_seconds, "", args.quiet);
        }

        // Progress logging (every 10%)
        let current_percent = (state.progress() * 100.0) as u32 / 10 * 10;
        if current_percent > last_log_percent && current_percent <= 100 {
            log_mission(state.sim_time_seconds, &format!(
                "Progress: {}% | Depth: {:.0}ft | ROP: {:.1}ft/hr | MSE: {:.0}psi",
                current_percent, state.current_depth, state.rop, state.mse
            ), args.quiet);
            last_log_percent = current_percent;
        }

        // Generate and output packet
        let packet = state.generate_packet();

        match args.format.as_str() {
            "json" => {
                let json = serde_json::to_string(&packet)?;
                writeln!(stdout_lock, "{}", json)?;
            }
            "csv" => {
                writeln!(
                    stdout_lock,
                    "{},{:.1},{:.1},{:.1},{:.1},{:.2},{:.0},{:.1},{:.1},{:.1},{:.0},{:?}",
                    packet.timestamp,
                    packet.bit_depth,
                    packet.rop,
                    packet.wob,
                    packet.rpm,
                    packet.torque,
                    packet.spp,
                    packet.flow_in,
                    packet.flow_out,
                    packet.gas_units,
                    packet.mse,
                    packet.rig_state,
                )?;
            }
            _ => {
                let json = serde_json::to_string(&packet)?;
                writeln!(stdout_lock, "{}", json)?;
            }
        }

        stdout_lock.flush()?;

        // Advance time
        state.sim_time_seconds += sample_interval_sim;

        // Sleep for time compression
        if args.speed < 1000 {
            let elapsed = loop_start.elapsed();
            if elapsed < sample_interval_real {
                std::thread::sleep(sample_interval_real - elapsed);
            }
        }
    }

    stdout_lock.flush()?;
    drop(stdout_lock);

    // Mission debrief
    let total_elapsed = start_time.elapsed();

    log_mission(state.sim_time_seconds, &"=".repeat(70), args.quiet);
    log_mission(state.sim_time_seconds, "SIMULATION COMPLETE", args.quiet);
    log_mission(state.sim_time_seconds, &"=".repeat(70), args.quiet);
    log_mission(state.sim_time_seconds, &format!("Total packets: {}", state.packets_generated), args.quiet);
    log_mission(state.sim_time_seconds, &format!("Anomaly packets: {}", state.anomaly_packets), args.quiet);
    log_mission(state.sim_time_seconds, &format!("Final depth: {:.0} ft", state.current_depth), args.quiet);
    log_mission(state.sim_time_seconds, &format!("Real time: {:.1}s", total_elapsed.as_secs_f64()), args.quiet);
    log_mission(state.sim_time_seconds, &"=".repeat(70), args.quiet);

    Ok(())
}
