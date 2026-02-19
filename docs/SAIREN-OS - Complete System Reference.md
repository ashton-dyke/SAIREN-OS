
**Document:** Definitive Technical & Business Architecture  
**Version:** 3.0  
**Author:** Ashton Jay Dyke — SAIREN Ltd (17014476)  
**Date:** February 2026  
**Classification:** Internal — Commercial in Confidence

-----

## Executive Summary

SAIREN-OS is a federated industrial intelligence platform that processes sensor data locally on edge hardware, generates real-time operational advisories, and shares anonymised learning across a fleet network where raw data never leaves the source. The first deployment vertical is offshore drilling optimisation, but the architecture is domain-agnostic.

The system operates on a single principle: **physics cannot be wrong, AI can be.** Every layer in the architecture exists to augment — never override — deterministic physics calculations. The result is a system that is safe by design, explainable by default, and gets exponentially smarter with every asset connected to the network.

**In one sentence:** SAIREN-OS is a self-writing operational textbook — every rig that connects makes it smarter, and disconnecting means losing access to the collective intelligence of the entire fleet.

-----

## Full Architectural Map

```
╔══════════════════════════════════════════════════════════════════════════════╗
║                        SAIREN-OS V3.0 ARCHITECTURE                         ║
╚══════════════════════════════════════════════════════════════════════════════╝

                    ┌─────────────────────────────────┐
                    │  FLEET INTELLIGENCE NETWORK      │
                    │  (Hub-and-Spoke / WireGuard VPN) │
                    │                                   │
                    │  ┌───────────┐   ┌───────────┐   │
                    │  │ ABERDEEN  │◄─►│ HOUSTON   │   │
                    │  │   HUB     │   │   HUB     │   │
                    │  └─────┬─────┘   └─────┬─────┘   │
                    │        │               │         │
                    │   ┌────┴────┐     ┌────┴────┐   │
                    │   │Clustered│     │Clustered│   │
                    │   │FedAvg   │     │FedAvg   │   │
                    │   │FedPer   │     │FedPer   │   │
                    │   │TTT Prop │     │TTT Prop │   │
                    │   └─────────┘     └─────────┘   │
                    └──────────┬──────────────────────┘
                               │ Gradients, Events,
                               │ TTT Parameters (5-25KB)
                               │ Raw data NEVER leaves rig
                               ▼
╔══════════════════════════════════════════════════════════════════════════════╗
║                          EDGE NODE (PER RIG)                               ║
║                    RTX 4060 Ti / RTX 2000 Ada + i7-13700K                  ║
╠══════════════════════════════════════════════════════════════════════════════╣
║                                                                            ║
║  ┌──────────────────────────────────────────────────────────────────────┐  ║
║  │ LAYER 7: WORLD MODEL / COUNTERFACTUAL ENGINE            (~15-25ms) │  ║
║  │                                                                      │  ║
║  │  Current State ──► CfC Dynamics Model (64 neurons)                   │  ║
║  │                         │                                             │  ║
║  │                         ▼                                             │  ║
║  │              PINN/FNO Physics Surrogate (validates trajectory)        │  ║
║  │                         │                                             │  ║
║  │                         ▼                                             │  ║
║  │              30-min Forward Prediction + ACI Intervals                │  ║
║  │              Split-screen: "Current Path" vs "Recommended Path"       │  ║
║  └──────────────────────────────────────────────────────────────────────┘  ║
║                                    ▲                                       ║
║  ┌──────────────────────────────────────────────────────────────────────┐  ║
║  │ LAYER 6: STRATEGIC REASONING                          (~350-400ms) │  ║
║  │                                                                      │  ║
║  │  Qwen 2.5 3B (GPU) ──or── Qwen 2.5 1.5B (CPU) ──or── Templates    │  ║
║  │                                                                      │  ║
║  │  Receives: All specialist signals + UQ + Clingo evaluation           │  ║
║  │            + Causal graph + Fleet RAG + Knowledge graph context       │  ║
║  │                                                                      │  ║
║  │  Produces: Natural language advisory with causal explanation,         │  ║
║  │            confidence intervals, and counterfactual reasoning         │  ║
║  └──────────────────────────────────────────────────────────────────────┘  ║
║                                    ▲                                       ║
║  ┌──────────────────────────────────────────────────────────────────────┐  ║
║  │ LAYER 5: NEUROSYMBOLIC SAFETY & CAUSAL REASONING         (~5-8ms) │  ║
║  │                                                                      │  ║
║  │  ┌─────────────────┐  ┌──────────────────┐  ┌──────────────────┐   │  ║
║  │  │ Clingo ASP      │  │ PCMCI+ Causal    │  │ Knowledge Graph  │   │  ║
║  │  │ Safety Solver    │  │ Discovery        │  │ (Oxigraph/       │   │  ║
║  │  │                  │  │                  │  │  ISO 15926)      │   │  ║
║  │  │ CONFIRM /        │  │ Root cause       │  │                  │   │  ║
║  │  │ AUGMENT /        │  │ identification   │  │ Equipment +      │   │  ║
║  │  │ VETO neural      │  │ via causal       │  │ Formation +      │   │  ║
║  │  │ recommendations  │  │ graph traversal  │  │ Fleet knowledge  │   │  ║
║  │  └─────────────────┘  └──────────────────┘  └──────────────────┘   │  ║
║  └──────────────────────────────────────────────────────────────────────┘  ║
║                                    ▲                                       ║
║  ┌──────────────────────────────────────────────────────────────────────┐  ║
║  │ LAYER 4: INTEGRATION & UNCERTAINTY QUANTIFICATION        (~12-15ms)│  ║
║  │                                                                      │  ║
║  │  ┌────────────────────────────────────────────────────────────────┐  │  ║
║  │  │ Meta-CfC Fusion (128 neurons, FullyConnected wiring)          │  │  ║
║  │  │                                                                │  │  ║
║  │  │ Inputs: All specialist hidden states (200 dims → 64 dims)     │  │  ║
║  │  │                                                                │  │  ║
║  │  │ Four Readout Heads:                                            │  │  ║
║  │  │   1. Anomaly confidence (0-1)                                  │  │  ║
║  │  │   2. Top-10 cross-domain correlations                          │  │  ║
║  │  │   3. Urgency scoring (GREEN/YELLOW/RED)                        │  │  ║
║  │  │   4. Epistemic uncertainty                                     │  │  ║
║  │  └────────────────────────────────────────────────────────────────┘  │  ║
║  │                              │                                       │  ║
║  │  ┌───────────┐  ┌───────────┴───┐  ┌──────────────┐                │  ║
║  │  │    EDL    │  │     ACI       │  │     EVT      │                │  ║
║  │  │ Aleatoric │  │  Calibrated   │  │  Tail risk   │                │  ║
║  │  │    vs     │  │  conformal    │  │  (Generalised │                │  ║
║  │  │ Epistemic │  │  intervals    │  │   Pareto)    │                │  ║
║  │  │ (0 cost)  │  │  (~20 lines)  │  │  (lookup)    │                │  ║
║  │  └───────────┘  └───────────────┘  └──────────────┘                │  ║
║  └──────────────────────────────────────────────────────────────────────┘  ║
║                                    ▲                                       ║
║  ┌──────────────────────────────────────────────────────────────────────┐  ║
║  │ LAYER 3: SPECIALIST INTELLIGENCE MESH                   (~10-20ms) │  ║
║  │                                                                      │  ║
║  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐ │  ║
║  │  │Operations│ │Hydraulics│ │   Well   │ │Formation │ │Equipment │ │  ║
║  │  │CfC 50n   │ │CfC 32n   │ │ Control  │ │CfC 25n   │ │CfC 50n   │ │  ║
║  │  │          │ │          │ │ CfC 40n  │ │          │ │          │ │  ║
║  │  │State +   │ │Envelope +│ │Risk +    │ │Type +    │ │Health +  │ │  ║
║  │  │Health +  │ │Trend +   │ │Event     │ │Boundary +│ │TTF +     │ │  ║
║  │  │Optimal   │ │Confidence│ │probs +   │ │Hardness  │ │Failure   │ │  ║
║  │  │params    │ │          │ │Time-to-  │ │          │ │modes     │ │  ║
║  │  │          │ │          │ │critical  │ │          │ │          │ │  ║
║  │  └──────────┘ └──────────┘ └──────────┘ └──────────┘ └──────────┘ │  ║
║  │       ~197 CfC/NCP neurons total, ~5,000-8,000 parameters          │  ║
║  │       + Test-Time Training (adapts in ~100 samples / 1.5 hours)    │  ║
║  │                                                                      │  ║
║  │  ┌──────────────────────┐  ┌──────────────────────────────────────┐ │  ║
║  │  │ xLSTM Anomaly        │  │ Spatio-Temporal GNN                  │ │  ║
║  │  │ Detector (~2-4M)     │  │ (Rig system graph, ~1-10M)           │ │  ║
║  │  │                      │  │                                      │ │  ║
║  │  │ Black swan detector: │  │ Models rig as connected system:      │ │  ║
║  │  │ finds patterns that  │  │ fault propagation through physical   │ │  ║
║  │  │ don't match ANY      │  │ connections. Distinguishes formation │ │  ║
║  │  │ training data        │  │ vs mechanical root causes.           │ │  ║
║  │  └──────────────────────┘  └──────────────────────────────────────┘ │  ║
║  │                                                                      │  ║
║  │  ┌──────────────────────────────────────────────────────────────┐   │  ║
║  │  │ Continuous Optimiser (50 CfC neurons)                        │   │  ║
║  │  │ Produces optimal WOB/RPM/Flow every 30-60 seconds            │   │  ║
║  │  └──────────────────────────────────────────────────────────────┘   │  ║
║  └──────────────────────────────────────────────────────────────────────┘  ║
║                                    ▲                                       ║
║  ┌──────────────────────────────────────────────────────────────────────┐  ║
║  │ LAYER 2: FEATURE ENGINEERING                               (~5ms)  │  ║
║  │                                                                      │  ║
║  │  ┌──────────────────┐ ┌─────────────────┐ ┌──────────────────────┐ │  ║
║  │  │ Wavelet           │ │ Hilbert-Huang   │ │ Mamba Temporal       │ │  ║
║  │  │ Scattering        │ │ Transform / EMD │ │ Encoder              │ │  ║
║  │  │                   │ │                 │ │                      │ │  ║
║  │  │ Fixed wavelet     │ │ Adaptive signal │ │ State Space Model    │ │  ║
║  │  │ filters. Zero     │ │ decomposition.  │ │ O(L) linear.         │ │  ║
║  │  │ training needed.  │ │ NASA heritage.  │ │ 2-hour context       │ │  ║
║  │  │ 100% accuracy on  │ │ Proven for      │ │ window → 64-dim      │ │  ║
║  │  │ CWRU bearing      │ │ drilling        │ │ temporal vector       │ │  ║
║  │  │ benchmark.        │ │ vibration.      │ │ for all specialists. │ │  ║
║  │  │                   │ │                 │ │                      │ │  ║
║  │  │ → Equipment       │ │ → Formation     │ │ → All specialists    │ │  ║
║  │  │   specialist      │ │ → Equipment     │ │   (long-range        │ │  ║
║  │  │                   │ │ → Operations    │ │    trend context)     │ │  ║
║  │  └──────────────────┘ └─────────────────┘ └──────────────────────┘ │  ║
║  └──────────────────────────────────────────────────────────────────────┘  ║
║                                    ▲                                       ║
║  ┌──────────────────────────────────────────────────────────────────────┐  ║
║  │ LAYER 1: PHYSICS ENGINE (Deterministic, CPU-only)           (<5ms) │  ║
║  │                                       *** ABSOLUTE AUTHORITY ***     │  ║
║  │                                                                      │  ║
║  │  MSE │ D-exponent │ ECD │ Flow Balance │ Pit Rate │ Bearing Freqs  │  ║
║  │  Hydraulic Pressure Decay │ Stroke Pressure Variance │ Torque-Drag  │  ║
║  │                                                                      │  ║
║  │  Output: 64-dim physics feature vector + discrete safety flags       │  ║
║  │  Rule: NO higher layer overrides physics. ECD < 0.1 = CRITICAL.     │  ║
║  └──────────────────────────────────────────────────────────────────────┘  ║
║                                    ▲                                       ║
║  ┌──────────────────────────────────────────────────────────────────────┐  ║
║  │ LAYER 0: MULTI-MODAL INGESTION                                      │  ║
║  │                                                                      │  ║
║  │  WITS Level 0 (1-5 Hz) │ Vibration (20kHz) │ Temperature (1Hz)     │  ║
║  │  SCADA Digital I/O (variable rate)                                   │  ║
║  │                                                                      │  ║
║  │  All sources → Unified timestamped buffer                            │  ║
║  └──────────────────────────────────────────────────────────────────────┘  ║
║                                                                            ║
╠════════════════════════════════════════════════════════════════════════════╣
║  DEGRADATION HIERARCHY (5 Modes — No Single Point of Failure)            ║
║                                                                            ║
║  Full (~400ms) → CPU (~5s) → LNN-only (~75ms) → Template (~42ms)        ║
║  → Physics-only (<5ms)                                                    ║
╚══════════════════════════════════════════════════════════════════════════════╝
```

-----

## Constraint Hierarchy

Every design decision in SAIREN-OS is governed by six constraints in strict priority order:

1. **Safety** — Cannot miss a kick. Cannot recommend unsafe parameters. No exceptions.
2. **Trust** — Operators must believe the system. Physics grounding and explainability are mandatory.
3. **Reliability** — Cannot crash offshore. Every component degrades gracefully.
4. **Speed** — Tactical responses under 100ms. Strategic advisories under 5 minutes.
5. **Depth** — Campaign-level context spanning 7+ days of operational history.
6. **Efficiency** — Edge-deployed. No cloud dependencies. Fully air-gapped operation.

If any two constraints conflict, the higher-numbered constraint always yields to the lower.

-----

## Layer-by-Layer Detail

### Layer 0: Multi-Modal Ingestion

**What it does:** Receives raw sensor data from every available source on the rig and unifies it into a single timestamped buffer that all higher layers consume.

**Data sources:**

|Source                  |Sample Rate|Data                                                                        |
|------------------------|-----------|----------------------------------------------------------------------------|
|WITS Level 0            |1-5 Hz     |WOB, ROP, RPM, torque, SPP, flow, pit volume, hookload, mud weight, gas, H2S|
|Vibration accelerometers|20 kHz     |Top drive, drawworks, mud pump vibration signatures                         |
|Temperature sensors     |1 Hz       |Motor temperatures, bearing temperatures, mud temperature                   |
|SCADA digital I/O       |Variable   |Pump status, valve positions, alarm states                                  |

**How it works:** A TCP listener passively monitors the rig data aggregator’s WITS broadcast. The connection is strictly one-way (ingress only) — the device cannot transmit data back to the rig network. All data is timestamped at ingestion and written to a rolling circular buffer in RAM.

**Why it exists:** Every subsequent layer needs clean, time-aligned data. Without a unified ingestion layer, each specialist would need its own connection and parser. Centralising ingestion also creates a single point where data quality issues (sensor dropouts, zero-value injections, timestamp gaps) can be detected and handled before they corrupt downstream analysis.

**Why it matters:** The system is physically air-gapped. WiFi, Bluetooth, and cellular hardware are removed or disabled. No data leaves the rig. This is not a software configuration — it is a hardware guarantee. The ingestion layer is the only external interface the system has, and it is read-only by design.

-----

### Layer 1: Physics Engine

**What it does:** Computes deterministic drilling physics calculations from raw sensor data. Produces a 64-dimensional feature vector and discrete safety flags that serve as ground truth for every higher layer.

**Latency:** <5ms on CPU. No GPU required.

**Authority:** Absolute. No neural network, no AI model, no higher layer can override a physics calculation. When physics says ECD margin is below 0.1 ppg, that is CRITICAL regardless of what any other component thinks.

**Calculations performed:**

**MSE (Mechanical Specific Energy):** The energy required to remove a unit volume of rock. Calculated from WOB, torque, RPM, ROP, and bit diameter. Lower MSE means more efficient drilling. When MSE exceeds the optimal range for the current formation, the system is wasting energy — either through bit dysfunction, incorrect parameters, or formation change. This is the primary efficiency metric.

**D-exponent:** A normalised measure of formation hardness that accounts for WOB, RPM, and ROP. As the bit drills deeper, changes in d-exponent indicate formation transitions. A rising d-exponent may signal approaching overpressure — a critical well control indicator. The physics engine tracks d-exponent continuously and flags shifts greater than 15% as potential formation boundaries.

**ECD (Equivalent Circulating Density):** The effective mud weight at the bit including the frictional pressure drop from circulating mud. ECD must remain between pore pressure (below which the formation flows into the wellbore — a kick) and fracture gradient (above which the wellbore wall breaks and mud escapes — losses). This is the most safety-critical calculation in the system. Warning at <0.3 ppg margin. Critical at <0.1 ppg margin.

**Flow Balance:** Flow out minus flow in. Positive imbalance means more fluid is coming out of the well than going in — potential kick (formation fluid influx). Negative means fluid is being lost into the formation. Warning at ±10 gpm. Critical at ±20 gpm.

**Pit Rate:** Rate of change of active pit volume, smoothed over a 5-minute rolling average to filter sensor noise. A rising pit volume with no pump rate change is a primary kick indicator.

**Bearing Fault Frequencies:** BPFO (Ball Pass Frequency Outer), BPFI (Ball Pass Frequency Inner), BSF (Ball Spin Frequency), and FTF (Fundamental Train Frequency) derived from FFT analysis of vibration data. These are mathematically deterministic for a given bearing geometry and RPM. When vibration energy concentrates at these specific frequencies, it indicates bearing degradation before it becomes audible or visible.

**Hydraulic Pressure Decay:** Analysis of BOP (Blowout Preventer) pressure test curves. The rate and pattern of pressure decay after a test indicates seal integrity. Deterministic comparison against expected decay profiles for each seal type.

**Stroke Pressure Variance:** Statistical analysis of pump stroke-to-stroke pressure consistency. Increasing variance indicates liner wear, valve degradation, or packing failure.

**Torque-Drag Models:** Calculated friction coefficients for the drill string based on measured hookload, torque, and well trajectory. Deviations from the predicted torque-drag profile indicate hole cleaning problems, formation instability, or mechanical issues.

**Why physics is Layer 1:** AI models can hallucinate. Neural networks can produce confident but wrong predictions. Physics cannot. By placing deterministic calculations as the foundational layer, every AI component in the system operates on mathematically verified ground truth. The physics engine is the system’s anchor to reality.

-----

### Layer 2: Feature Engineering

**What it does:** Transforms raw and physics-derived signals into rich feature representations using fixed (non-learned) mathematical transforms. No training is required. No failure modes exist beyond numerical precision.

**Latency:** ~5ms combined on CPU, <2ms on GPU.

**Three complementary transforms:**

#### Wavelet Scattering Transform

**What:** A cascade of fixed wavelet convolutions followed by modulus and averaging operations. Produces translation-invariant, deformation-stable features from time-series signals — particularly vibration data.

**How:** The signal passes through multiple scales of wavelet filters (no learned parameters — these are fixed mathematical functions). At each scale, the modulus of the wavelet coefficients is computed, then averaged. The process repeats across scales, producing a fixed-length feature vector that captures both fine and coarse spectral structure.

**Why:** On the CWRU bearing benchmark (the standard industrial bearing fault dataset), wavelet scattering features achieve 100% classification accuracy across 15 bearing conditions. Critically, it is data-efficient — it works with very few labelled examples. In offshore drilling, labelled fault events are rare and expensive. A feature extraction method that works with minimal training data is essential.

**Output:** Feeds the Equipment specialist with rich spectral features that complement the physics-derived BPFO/BPFI frequencies from Layer 1.

**Implementation:** ~500 lines of Rust using `rustfft` and `fcwt` crates. Pure DSP — no model weights, no training, no failure modes.

#### Hilbert-Huang Transform / Empirical Mode Decomposition

**What:** A fully adaptive signal decomposition method for nonlinear, non-stationary signals. Decomposes a signal into physically meaningful Intrinsic Mode Functions (IMFs) without pre-selected basis functions.

**How:** EMD (Empirical Mode Decomposition) identifies the signal’s upper and lower envelopes via cubic spline interpolation, computes the mean envelope, subtracts it from the signal to produce the first IMF, then repeats on the residual. Each IMF represents an oscillatory mode at a characteristic timescale. The Hilbert Transform is then applied to each IMF to extract instantaneous frequency and amplitude — the Hilbert-Huang spectrum.

**Why:** Unlike FFT (which assumes stationarity) or wavelets (which use a fixed basis), HHT adapts to the signal itself. Drilling vibration is inherently nonlinear and non-stationary — formation changes, bit wear, and drill string dynamics produce signals that violate FFT assumptions. A 2023 paper in the Journal of Petroleum Exploration and Production Technology demonstrated lithology identification from drill-bit vibration using HHT specifically. NASA developed this method — it has 20+ years of validation in seismology, oceanography, and structural health monitoring.

**Output:** Feeds the Formation specialist (lithology signatures from vibration), Equipment specialist (non-stationary vibration patterns that FFT misses), and Operations specialist (adaptive drilling dynamics decomposition).

**Implementation:** ~300-500 lines of Rust using cubic spline interpolation and `rustfft`. Runs in 10-100ms on CPU for a 10-second signal at 10kHz.

#### Mamba Temporal Encoder

**What:** A State Space Model that processes long sensor sequences with O(L) linear computational complexity (versus O(L²) for transformers).

**How:** A small Mamba encoder (2-4 million parameters) continuously processes a rolling 2-hour window of the physics feature buffer (7,200 timesteps at 1Hz). It produces a 64-dimensional temporal context vector that encodes long-range trends — 2-hour ROP evolution, 6-hour d-exponent drift, 24-hour torque baseline changes. This vector is appended to the physics feature vector, giving every specialist access to long-range temporal context without requiring them to maintain long memory themselves.

**Why:** The CfC specialists in Layer 3 process short temporal windows (seconds to minutes). Some patterns require longer context to detect — gradual formation transitions, slow equipment degradation trends, or multi-hour operational patterns. Mamba provides this efficiently. The Liquid-S4 hybrid from Hasani et al. embeds linearised LNN dynamics inside S4 transition matrices, creating a natural bridge from the CfC architecture used throughout the system.

**Output:** 64-dimensional temporal context vector appended to every specialist’s input.

**Implementation:** Trained in PyTorch, exported to ONNX, deployed via the `ort` crate in Rust. ~3-5ms on GPU, ~15-25ms on CPU.

-----

### Layer 3: Specialist Intelligence Mesh

**What it does:** Detects temporal patterns in the enriched feature representations that threshold-based rules cannot capture. Three complementary architectures run in parallel, each attacking different failure mode detection strengths.

**Latency:** ~10-20ms combined on GPU.

#### 3A: Five CfC/NCP Specialist Networks

The core of the intelligence mesh. Five small neural networks built using Closed-form Continuous-time (CfC) neurons with Neural Circuit Policy (NCP) wiring — a biologically-inspired architecture where neurons have continuous-time dynamics (they evolve between observations, unlike standard neural networks that only compute at discrete timesteps).

**Why CfC/NCP:** Standard neural networks process data as snapshots. CfC neurons maintain internal state that evolves continuously — they “think between observations.” This makes them naturally suited to irregularly-sampled time series (sensor data doesn’t always arrive at perfect intervals), and their continuous dynamics mean they can detect gradual changes that snapshot-based models miss. NCP wiring constrains the connectivity pattern to biologically plausible sparse topologies, which dramatically reduces parameter count while maintaining performance.

**The five specialists:**

|Specialist      |Neurons|What It Learns                                                                                        |What It Outputs                                                                    |
|----------------|-------|------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------|
|**Operations**  |50     |Drilling state dynamics, founder onset, pack-off precursors, stick-slip signatures                    |State classification (6 classes), health score (0-1), optimal parameter suggestions|
|**Hydraulics**  |32     |Pressure envelope evolution, ECD margin trends, pump efficiency degradation                           |Envelope status, trend direction, confidence                                       |
|**Well Control**|40     |Kick onset (including slow kicks that threshold systems miss), loss signatures, gas migration patterns|Risk score (0-1), event probabilities, time-to-critical estimate                   |
|**Formation**   |25     |Boundary approach, pore pressure trending, hardness transitions                                       |Formation type, boundary proximity, hardness estimate                              |
|**Equipment**   |50     |Bearing degradation curves, seal wear patterns, pump liner wear trajectories                          |Per-component health score, time-to-failure, failure mode classification           |

**Total:** ~197 neurons, ~5,000-8,000 parameters. This is deliberately tiny. Small models are fast, auditable, and less prone to overfitting — critical properties for safety-critical systems.

**Test-Time Training (TTT):** Each specialist monitors for distribution shift via KL-divergence between current batch statistics and running averages. When a shift is detected (new formation, new rig, equipment change), the network’s batch normalisation statistics and CfC time constants adapt in real-time without backpropagation. Adaptation completes within ~100 samples (~1.5 hours of drilling). This means the system adapts to a new formation in hours, not the days or weeks that retraining would require. When one rig adapts, the adapted parameters (5KB) are uploaded to the fleet hub and pre-deployed to other rigs entering similar conditions — giving them zero-time adaptation.

#### 3B: xLSTM Anomaly Detector

**What:** A dedicated anomaly detection network running in parallel to the specialist mesh, built on Hochreiter’s extended LSTM architecture (the xLSTMAD variant).

**How:** Trained to reconstruct normal operating patterns. During operation, it continuously attempts to reconstruct the current sensor state. When reconstruction error exceeds learned thresholds, the current state is anomalous. The mLSTM variant uses matrix memory with query-key-value retrieval — a mechanism that captures complex temporal dependencies the CfC specialists may miss.

**Why it runs parallel to specialists, not instead of them:** The specialists detect *known* failure modes — pack-off, stick-slip, kick patterns they were trained to recognise. xLSTM detects *unknown* anomalies — patterns that don’t match any training data. This is the black swan detector. Together they cover both known-unknown failure modes (the specialists) and unknown-unknown failure modes (xLSTM).

**Output:** Anomaly score (0-1) per timestep, reconstruction error breakdown per sensor channel (which parameters are anomalous?), and a novelty flag indicating whether this pattern matches any training distribution.

**Why xLSTM specifically:** xLSTMAD outperforms 23 baseline methods on the TSB-AD-M benchmark, achieving near-perfect scores on industrial control datasets (SWaT: VUS-PR 0.91, VUS-ROC 0.95).

#### 3C: Spatio-Temporal Graph Neural Network (ST-GNN)

**What:** Models the rig as a connected system where faults propagate through physical connections.

**How:** The rig’s piping and instrumentation diagram (P&ID) is encoded as a graph. Nodes (~50-200) represent sensors and components (drill string, mud pit, shale shakers, mud pumps, BOP stack, choke manifold, top drive). Edges (~100-500) represent material flows (mud circuit), energy flows (torque transmission), and signal flows (pressure propagation). Each node carries the relevant specialist’s hidden state as its feature vector. The GNN performs message passing — when one node detects an anomaly, the signal propagates through the graph topology to check whether connected nodes show correlated anomalies.

**Why this matters:** When the Hydraulics specialist detects rising SPP, there are two possible explanations: formation-induced pressure change (which affects only the wellbore path) or mechanical failure (which affects the equipment path). Without the GNN, distinguishing these requires human expertise. The GNN traces the anomaly through the physical connection graph and determines whether it’s localised to the wellbore or propagating through the mechanical system — providing automatic root cause differentiation.

**Output:** Fault localisation (which component is the likely root cause?), propagation prediction (which downstream components will be affected?), and a system-level health score that accounts for connected failure modes.

#### 3D: Continuous Optimiser

**What:** A 50-neuron CfC network that produces optimal WOB/RPM/Flow recommendations every 30-60 seconds.

**Why it’s separate:** The five specialists answer “what’s happening?” The optimiser answers “what should we do?” These are fundamentally different questions. The optimiser runs parallel to the ML Engine V2.2 (the existing statistical optimisation system) for cross-validation. If they agree, confidence is high. If they disagree, the advisory flags the divergence.

-----

### Layer 4: Integration & Uncertainty Quantification

**What it does:** Fuses all specialist signals into unified intelligence vectors and wraps every prediction with calibrated uncertainty estimates.

#### Meta-CfC Fusion Network

**What:** A 128-neuron CfC network with FullyConnected wiring that reads all specialist hidden states through a compression layer (200→64 dimensions) and produces four readout heads.

**The four heads:**

1. **Anomaly confidence (0-1):** Combines specialist signals, xLSTM novelty score, and GNN fault localisation into a single scalar. This is the system’s overall assessment of whether something abnormal is happening.
2. **Top-10 correlations:** Learned inter-domain patterns that span specialist boundaries. Example: torque instability (Operations specialist) combined with bearing kurtosis (Equipment specialist) indicates gearbox wear, not formation change. These cross-domain correlations are patterns that no individual specialist can detect alone.
3. **Urgency scoring (GREEN/YELLOW/RED):** Accounts for compound risk — multiple simultaneous low-level anomalies that individually are benign but collectively indicate a developing problem.
4. **Epistemic uncertainty:** How far outside the training distribution is the current signal? High anomaly confidence with low epistemic uncertainty means the system has seen this failure pattern before (act with confidence). High anomaly with high epistemic uncertainty means this is novel (proceed with extreme caution, escalate to human).

#### Adaptive Conformal Inference (ACI)

**What:** Wraps every prediction with a calibrated confidence interval that self-corrects under distribution shift.

**How:** A single scalar update per prediction: α(t+1) = α(t) + γ(α - err(t)). When the system has been accurate recently, intervals tighten. When accuracy drops (distribution shift), intervals widen automatically. This provides finite-sample coverage guarantees even for non-exchangeable data — drilling conditions that change mid-operation.

**Why:** Standard prediction intervals assume data is stationary. Drilling conditions are not stationary. ACI from Gibbs and Candès (NeurIPS 2021) maintains approximately 90% coverage even under shift, where standard methods drop to 81-84%.

**Impact on trust:** An operator told “increase WOB to 35” might hesitate. An operator told “increase WOB to 35, safe range 32-38 based on current formation response” has the confidence to act. The interval communicates uncertainty honestly.

**Implementation:** ~20 lines of Rust.

#### Evidential Deep Learning (EDL)

**What:** Single-forward-pass uncertainty estimation that explicitly separates two types of uncertainty.

**Aleatoric uncertainty:** Inherent noise in the data. Sensor noise, formation heterogeneity, natural variability. This cannot be reduced with more data.

**Epistemic uncertainty:** Model ignorance. The system has never seen this formation type, this equipment configuration, or this combination of conditions. This CAN be reduced with more data and deployments.

**Why the distinction matters:** Both types produce uncertainty, but they require different responses. High aleatoric uncertainty means “the situation is inherently unpredictable — be cautious.” High epistemic uncertainty means “the system doesn’t know enough — get human input.” Different advisory language, different recommended actions.

**Implementation:** Modified output layer on each specialist. No additional compute overhead — zero cost.

#### Extreme Value Theory (EVT)

**What:** Models the tail distribution of sensor signals using Generalised Pareto Distribution fitted to the top 1-5% of historical observations.

**Why:** ACI and EDL can’t handle scenarios that exceed all training data. EVT extrapolates from the tail to estimate the probability of never-before-seen extreme values — kick magnitudes beyond anything in the training set, vibration amplitudes exceeding all historical precedent. This is the “how bad could this theoretically get?” estimate.

**Implementation:** Periodic recalibration (monthly/quarterly) on the hub server. Lightweight lookup table deployed to edge. Negligible inference cost.

**Combined UQ output:** Every advisory includes: prediction + confidence interval (ACI) + uncertainty type (EDL: aleatoric vs epistemic) + tail risk estimate (EVT). The operator sees not just what the system recommends, but how confident it is, why it might be wrong, and how bad the worst case could be.

-----

### Layer 5: Neurosymbolic Safety & Causal Reasoning

**What it does:** Provides explainable safety constraints and root cause analysis that regulators and operators can inspect, verify, and trust. This is the layer that makes the system auditable.

#### Clingo ASP Safety Solver

**What:** An Answer Set Programming solver that evaluates neural model outputs against hard-coded drilling safety rules written in declarative logic.

**How:** Drilling safety rules are encoded as logical constraints. Example rules:

- If ECD margin < 0.1 ppg → CRITICAL, regardless of neural model output (override)
- If neural model recommends increasing WOB but torque coefficient of variation > 12% → VETO the recommendation (stick-slip risk)
- If losses detected AND mud weight below fracture gradient → kick risk HIGH (augment the neural assessment with additional context)

Clingo acts as a “safety voter” in the system. For every neural advisory, it either confirms (the recommendation is consistent with all safety rules), augments (adds context from rules the neural model doesn’t encode), or vetoes (the recommendation violates a safety constraint).

**Why:** Neural networks cannot be formally verified. You cannot prove mathematically that a neural network will never recommend an unsafe action. But you CAN prove that a set of logical rules will never allow an unsafe recommendation to pass through. Clingo is the safety net — the last line of defence between a neural model’s suggestion and the advisory that reaches the operator.

**Audit trail:** Every decision produces a complete chain: “BHA vibration exceeded threshold (neural) → drillstring resonance risk (symbolic rule #47) → recommended ROP reduction (action) → counterfactual: risk drops to SAFE if WOB reduced by 2 klbs (world model).” This chain is inspectable by regulators, auditors, and operators.

**Implementation:** C++ solver with existing Rust bindings (`clingo-rs` crate). ~5MB binary. Millisecond solve times for drilling-scale rule sets (~100-500 rules).

#### PCMCI+ Causal Discovery

**What:** An algorithm that discovers causal relationships (not just correlations) between time-series variables.

**How:** PCMCI+ uses conditional independence testing with time lags to distinguish causation from correlation. It runs as a periodic batch job (every shift change or daily) using the Tigramite library. It processes 50-100 variables with appropriate lag settings and produces a causal graph stored in Rust’s `petgraph` crate.

**Why:** Correlation says “torque and SPP both rose.” Causation says “torque rise CAUSED SPP rise because the drill string is packing off, increasing friction which increases standpipe pressure.” The distinction changes the recommended action entirely. If SPP rose because of a formation change, the response is to adjust mud weight. If SPP rose because of a pack-off, the response is to reduce WOB and increase flow. Same symptom, different cause, different action. Without causal reasoning, the system can recommend the wrong response to the right observation.

**Compounding moat:** After 100+ wells, the discovered causal graphs encode formation-specific and rig-specific causal relationships that represent genuine drilling knowledge. This knowledge is not reproducible without equivalent operational data.

#### Drilling Knowledge Graph

**What:** A lightweight SPARQL database (Oxigraph, Rust-native) hosting a drilling domain knowledge graph based on ISO 15926 (the oil and gas lifecycle data standard).

**Contents:** Equipment taxonomy, formation properties, fault patterns, operational procedures, fleet-discovered causal relationships. 10K-100K triples, <100MB RAM, sub-millisecond query times.

**Why:** When the Equipment specialist detects bearing degradation, the knowledge graph provides context: bearing model, manufacturer, expected L10 life, maintenance history, similar failures on fleet rigs, recommended interventions. Without this context, the advisory is “bearing degrading.” With it, the advisory is “bearing degrading — this model typically fails at 4,200 hours, you’re at 3,800, fleet data shows 3 similar cases resolved by scheduled replacement during next trip.”

**Growth:** Every deployment adds rig-specific knowledge. The knowledge graph becomes irreplaceable institutional memory that grows permanently.

-----

### Layer 6: Strategic Reasoning

**What it does:** Synthesises all signals from all lower layers into a coherent natural language advisory that the OIM can read, understand, and act on.

**Model:** Qwen 2.5 3B (GPU primary) or Qwen 2.5 1.5B (CPU fallback).

**Inputs:** This layer receives everything — the full intelligence picture:

- Five specialist signal vectors from the CfC mesh
- xLSTM anomaly score and novelty flag
- GNN fault localisation and propagation prediction
- Meta-CfC fusion output (anomaly, correlations, urgency, epistemic uncertainty)
- ACI calibrated intervals
- Clingo safety evaluation (confirm/augment/veto)
- PCMCI+ causal graph traversal (root cause identification)
- Continuous optimiser recommendation
- Fleet RAG precedents from the hub
- Knowledge graph context
- Mamba temporal context (long-range trends)

**Five jobs:**

1. **Synthesise:** Combine all signals into a coherent diagnosis. The specialists each see their domain — the LLM sees the whole picture and identifies how the pieces fit together.
2. **Contextualise:** Reference campaign history. Is this formation change expected based on the well plan? Has this rig seen this pattern before? What happened last time?
3. **Communicate:** Produce natural language that the OIM actually reads. Not a data dump — a clear recommendation with reasoning. “Reduce WOB from 35 to 28 klbs. SPP rise is caused by developing pack-off (causal analysis), not pump efficiency change. Fleet data shows 3 similar cases in this formation resolved by WOB reduction.”
4. **Reason counterfactually:** “If WOB reduced to 25 klbs, expected ROP 85 ft/hr based on world model prediction.” The operator sees the predicted outcome, not just the recommendation.
5. **Explain causally:** Reference the causal graph to explain WHY, not just WHAT. “SPP rise caused by pack-off (causal graph), not pump efficiency change” — this is the difference between a useful advisory and a confusing one.

**Template fallback:** When the LLM is unavailable (GPU failure, model loading error), the system falls back to template-based advisories populated with specialist signals, UQ calibration, causal graph traversal, and Clingo reasoning. These templates produce functional advisories with temporal context, confidence scores, and root cause chains — fully operational without any LLM. This is the regulatory safety net. The system never goes silent.

**Latency:** ~350-400ms at 3B (GPU), ~3-8s at 3B (CPU), ~35ms template-only.

-----

### Layer 7: World Model / Counterfactual Engine

**What it does:** Simulates “what happens next” given proposed parameter changes. Shows the operator the predicted future before they commit to a recommendation.

**How:** This is NOT a separate standalone model. It is a thin CfC dynamics layer (64 neurons) trained on historical parameter rollouts that sits on top of the existing stack. Given the current state (from specialists and physics) and a proposed parameter change (from the optimiser or operator), it produces a 30-minute forward prediction. A PINN (Physics-Informed Neural Network) / FNO (Fourier Neural Operator) validates the trajectory against physical laws — acting as a fast physics simulator that catches physically impossible predictions.

**Why:** “Reduce WOB” is weak advice. “If you reduce WOB from 35 to 28, ROP drops to 70 initially but recovers to 76 by minute 20, and pack-off risk drops from 40% to less than 5%” is compelling. The dashboard renders a split-screen: left shows “if no change” (predicted trajectory under current parameters), right shows “if recommended change” (predicted improvement with confidence intervals). Operators don’t just trust recommendations — they see the predicted future.

**Digital twin divergence detection:** When the world model predicts X and reality delivers Y, the divergence IS the anomaly signal. A model that expects stable SPP but sees it rising detects developing problems earlier than threshold-based monitoring — because the prediction creates an expectation, and violated expectations are the earliest possible anomaly signal.

**Latency:** ~15-25ms for 30-minute rollout on GPU (1,800 timesteps through small CfC + PINN validation).

-----

## Fleet Intelligence Network

### The Core Principle

Every rig processes locally. No raw data ever leaves the rig. What flows through the network is generalised operational knowledge — formation physics patterns, equipment degradation signatures, optimisation parameters — stripped of all identifying information. The intelligence is the network itself.

### Three-Tier Hierarchical Sync

**Tier 1 — Emergency Pattern Broadcast (Real-Time):**
When a CRITICAL event occurs that was unpredicted by the specialist mesh (a true surprise), a <1KB payload containing physics features and event label is uploaded to the hub. The hub validates it and broadcasts to ALL rigs within 60 seconds. Frequency: 1-2 events per week fleet-wide. This ensures that a novel kick pattern detected on one rig is immediately available to every other rig.

**Tier 2 — Gradient Exchange (Hourly, Significance-Gated):**
Each specialist periodically uploads weight deltas (the difference between its current weights and its last-uploaded weights). Before uploading, a cosine similarity check filters out insignificant updates (>0.95 similarity to last upload → skip, reducing traffic by ~60%). Payload: 5-25KB compressed. The hub performs federated averaging: 85% weight on local learning, 15% on fleet average. This ensures each rig retains its own personality while benefiting from collective intelligence.

**Tier 3 — Daily Episode Batch:**
All AMBER and RED episodes from the past 24 hours, plus a random 10% sample of GREEN (normal) episodes, are packaged and uploaded. Payload: 50-200KB per rig per day. The hub curator scores each episode for quality and adds validated events to the fleet library. The GREEN samples prevent the fleet from developing a bias toward anomalies — it needs to know what normal looks like too.

### Clustered Federated Learning

**Problem:** Rigs drilling Gulf of Mexico carbonates face fundamentally different dynamics than North Sea shales. Flat averaging across all rigs dilutes formation-specific knowledge.

**Solution:** IFCA (Iterative Federated Clustering Algorithm) automatically groups rigs with similar geological and operational profiles based on gradient similarity. Rig-007 drilling sandstone receives fleet knowledge weighted toward other sandstone-drilling rigs, not an unweighted average across all formations. Clusters emerge organically from the data — nobody has to manually configure them.

### FedPer — Personalised Layers

**Problem:** Some drilling physics is universal (fluid mechanics, drill string dynamics). Formation response is local. Averaging everything loses the distinction.

**Solution:** Models are split into shared base layers (universal drilling physics) and personal head layers (local formation characteristics). The base layers improve with every rig on the network. The personal heads remain rig-specific. Universal knowledge flows; local knowledge stays.

### Communication Efficiency

**FedSMU:** 10× bandwidth reduction by transmitting only the sign (±) of each weight change instead of the full floating-point value. For formation-level patterns, the direction of change matters more than the magnitude.

**FedKD:** 94.89% communication reduction through mutual knowledge distillation and dynamic gradient compression.

**Result:** A 10M parameter model that would normally require ~40MB per sync round transmits <500KB — feasible on the most bandwidth-constrained satellite links.

### Test-Time Training Fleet Propagation

When one rig adapts to new conditions (new formation, new equipment), the adapted normalisation parameters (5KB) are uploaded to the hub with geological context metadata. The hub indexes them by formation type, depth range, and basin. Other rigs entering similar conditions receive pre-adapted parameters — zero learning phase. The second rig in any given basin already knows what the first rig took 1.5 hours to learn.

### New Rig Onboarding

1. Receives fleet-averaged specialist weights (warm start from clustered FL)
2. Receives formation-appropriate TTT parameters (if basin previously encountered)
3. Shadow mode during first well (all systems log but don’t alert)
4. Validation against threshold-based system
5. Promotion to active advisory
6. Begins contributing gradients and episodes to fleet

-----

## Symbolic Regression — The IP Engine

**What it does:** After accumulating data from 50+ wells across the fleet, symbolic regression (PySR) runs on consolidated drilling data to discover human-readable equations that describe formation-specific drilling efficiency.

**Why this is transformative:** The current industry-standard ROP model is Bourgoyne-Young, published in 1974. If SAIREN-OS discovers a formation-specific drilling efficiency law that outperforms it — and fleet data proves it does — that equation is:

1. **Publishable at SPE** — your name on the paper
2. **An industry reference** — drilling engineers cite SAIREN equations
3. **Not reverse-engineerable** — knowing the equation doesn’t help without the data to validate it
4. **A validation of the ML models** — discovered equations serve as independent ground truth

**Example output:**

```
Basin: North Sea, Kimmeridge Clay Formation
Discovered: ROP_opt = 0.34 × WOB^0.82 × RPM^0.31 × (16.5 / MW) × exp(-0.019 × d_exp)
R²: 0.94 across 12 wells
Improvement over Bourgoyne-Young: 23% better ROP prediction
```

**Implementation:** Runs on the hub server (not edge). Batch job after sufficient fleet data accumulation. Discovered equations are lightweight (a few hundred bytes) and deploy to all edge nodes as enhanced physics engine models.

-----

## Business Architecture

### Product One: SAIREN-OS

Per rig. One price. One product. Gets smarter with every rig on the network. Sold to drilling contractors and operators.

- £50K per rig per year (pilot rigs at reduced rate)
- 3-year contract: £44K per rig per year (12% discount)
- Volume discounts for large fleets
- Includes: edge hardware (amortised), software, 24/7 support, fleet hub access, updates, dashboard, 5-year data retention

The operator experiences a system that keeps getting smarter. They attribute it to good software. The network effect is invisible to them — they simply notice that advisories improve over time and that the system seems to “know” formations it hasn’t drilled before. This is by design.

### Product Two: SAIREN Intelligence (Future — requires 30+ rigs)

Sold separately to equipment manufacturers and service companies. Completely decoupled from rig operations. Different buyer, different contract, different sales conversation. The OIM never sees it.

- Equipment manufacturers pay for anonymised fleet performance benchmarking
- Service companies pay for formation-specific performance data
- Insurers pay for risk profile access

This product does not exist until the network has enough participants to generate meaningful aggregate intelligence. Product One builds the network. Product Two monetises it. Product One is profitable on its own. Product Two is pure upside.

### The Network Effect

Every rig that joins the network makes every other rig’s advisories better. An operator who disconnects doesn’t lose a software subscription — they lose access to the collective intelligence of every other rig on the network. Reconnecting to a competitor means starting from zero because the intelligence is encoded in the network’s model weights, causal graphs, discovered equations, and fleet-learned parameters. There is no file to export. There is no database to migrate. The intelligence IS the network.

This is the permanent, compounding moat. The technology can be copied. The network intelligence cannot.

-----

## Hardware Specification

### Edge Node (Per Rig)

|Component    |Specification                                            |
|-------------|---------------------------------------------------------|
|GPU Option A |RTX 4060 Ti — 16GB VRAM, 160W TDP, desktop form factor   |
|GPU Option B |RTX 2000 Ada — 16GB VRAM, 70W TDP, wall-mount form factor|
|CPU          |Intel Core i7-13700K (8P+8E cores)                       |
|RAM          |32GB DDR5                                                |
|Storage      |1TB NVMe SSD, full disk encryption (LUKS)                |
|Power        |<450W total (Option A) / <250W (Option B)                |
|Certification|Non-Ex (Safe Zone installation only)                     |
|Network      |One-way Ethernet ingress (WITS), air-gapped              |
|Cost         |~£4,500 per unit                                         |

### Inference Budget (Full Stack on RTX 4060)

|Component             |GPU Latency|CPU Latency |Memory      |
|----------------------|-----------|------------|------------|
|Physics Engine        |<5ms       |<5ms        |~10MB       |
|Wavelet Scattering    |~1ms       |~3ms        |~5MB        |
|HHT/EMD               |~2ms       |~5ms        |~5MB        |
|Mamba Temporal Encoder|~3ms       |~15ms       |~50MB       |
|5× CfC Specialists    |~5ms       |~20ms       |~20MB       |
|xLSTM Anomaly Detector|~3ms       |~10ms       |~30MB       |
|ST-GNN System Graph   |~2ms       |~5ms        |~20MB       |
|Meta-CfC Integration  |~3ms       |~8ms        |~15MB       |
|UQ Stack (ACI+EDL+EVT)|<1ms       |<1ms        |~5MB        |
|Clingo Safety Rules   |~2ms       |~2ms        |~5MB        |
|**Subtotal (Pre-LLM)**|**~27ms**  |**~74ms**   |**~165MB**  |
|Qwen 3B Strategic     |~350ms     |~5,000ms    |~2,000MB    |
|World Model Rollout   |~15ms      |~40ms       |~50MB       |
|**Total**             |**~392ms** |**~5,114ms**|**~2,215MB**|

VRAM headroom with 3B model: ~14GB free out of 16GB.

### Five Degradation Modes

|Mode             |Condition               |Capability                              |Latency|
|-----------------|------------------------|----------------------------------------|-------|
|**Full**         |GPU + all models healthy|Complete advisory with world model      |~400ms |
|**CPU**          |GPU failure             |Smaller LLM, all other layers functional|~5s    |
|**LNN-only**     |GPU + LLM failure       |Specialists + UQ + Clingo, no synthesis |~75ms  |
|**Template-only**|All neural models failed|Physics + rules + templates             |~42ms  |
|**Physics-only** |Everything failed       |Deterministic calculations + hard limits|<5ms   |

No single point of failure. Even in worst case, the system provides physics-validated safety monitoring.

-----

## Build Sequence

### Phase 1: V72 Pilot (Q1 2026)

Ship current V1.0 threshold system. Shadow-log: single Operations CfC specialist, ACI conformal intervals, Wavelet Scattering features, HHT/EMD features. Validate shadow system detection timing versus threshold system. Zero operational impact.

### Phase 1.5: LNN Mesh Shadow (Q2 2026)

V1.0 remains primary. All 5 specialists shadow. Continuous Optimiser parallel to ML V2.2. Meta-CfC integration shadow. Mamba encoder active. TTT distribution shift detection active. xLSTM shadow. Clingo logging only.

### Phase 2.0: LNN Promotion (Q3 2026)

Specialists promoted to active advisory. Strategic model drops from 7B to 3B. Fleet gradient sharing enabled. Clustered FL groups rigs by formation. Equipment specialist shadow. ST-GNN shadow.

### Phase 2.5: Full Mesh + UQ (Q4 2026)

Full UQ stack active. ST-GNN promoted. Equipment specialist active. PCMCI+ causal discovery daily batch. Knowledge graph with basic drilling ontology. Multi-modal fusion at Layer 2.

### Phase 3.0: World Model + RL + Symbolic Regression (2027)

World model active. PINN/FNO digital twin. IQL offline RL with PICNN safety layer. Symbolic regression on 50+ well fleet data. BOP/Pump equipment specialists. Autonomous parameter adjustment with human approval.

### Phase 4.0: Efficiency + Scale (2027-2028)

Neuromorphic evaluation. Communication compression for 100+ rig fleet. Multi-operator federated learning with differential privacy. Third hub for APAC expansion.

-----

## Non-Negotiable Principles

1. **Physics layer always authoritative** — no neural network overrides deterministic calculations
2. **Well control thresholds stay manual** — kick detection limits are regulatory, not learned
3. **Every layer degrades gracefully** — five degradation modes, none catastrophic
4. **Template-only mode must remain viable** — regulatory safety net
5. **One product, hardware-adaptive** — no tiers, no fragmentation
6. **Shadow mode before promotion** — every component validated before influencing advisories
7. **Clingo can veto any neural recommendation** — symbolic safety constraints are absolute
8. **Causal, not just correlational** — root cause analysis, not symptom matching
9. **Raw data never leaves the rig** — federated gradients only, privacy by architecture
10. **The network is the product** — every rig makes every other rig smarter

-----

**SAIREN Ltd — Company Number 17014476**  
**Director: Ashton Jay Dyke**  
**February 2026**