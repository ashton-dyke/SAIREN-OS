
**Authors:** Claude (Anthropic) + Perplexity — Merged Technical Architecture for SAIREN Ltd  
**Date:** 12 February 2026  
**Status:** Definitive Architecture Specification  
**Context:** This document merges the V2.5 Liquid Intelligence Architecture with bleeding-edge technology research from both Claude and Perplexity. Every technology included meets the bar: peer-reviewed, working implementations exist, and runs on RTX 4060 class edge hardware. Technologies are integrated into a staged build sequence where each phase compounds the value of the last.

-----

## 1. Design Philosophy

### 1.1 Constraint Hierarchy (Unchanged from V2.5)

1. **Safety** — Cannot miss a kick. Cannot recommend unsafe parameters.
2. **Trust** — Operators must believe the system. Physics grounding + explainability.
3. **Reliability** — Cannot crash offshore. Every component degrades gracefully.
4. **Speed** — Tactical <100ms, strategic <5 minutes.
5. **Depth** — Campaign-level context (7+ day history).
6. **Efficiency** — Edge-deployed, no cloud dependencies.

### 1.2 The Compounding Moat Principle

V3.0 is designed so that every technology creates a flywheel that accelerates with deployment:

- **More rigs** → more fleet gradients → better LNN specialists → better advisories → more rigs
- **More wells** → more causal graphs discovered → better root cause analysis → faster problem resolution → more trust
- **More formations** → more symbolic equations discovered → better physics models → publishable IP → industry authority
- **More events** → richer GNN fault propagation library → earlier detection → less NPT → higher ROI demonstrated

A competitor starting from zero faces not one moat but twelve interlocking moats, each requiring different expertise and irreplaceable proprietary data.

### 1.3 One Product, Hardware-Adaptive

No tiers. No “Standard/Professional/Premium.” One SAIREN-OS installation that auto-detects available hardware and configures accordingly:

- GPU available (RTX 4060/4060 Ti/2000 Ada) → full stack, larger models, faster inference
- CPU only → smaller models, acceptable latency, full capability
- GPU failure mid-operation → automatic fallback to CPU path, no advisory gap

Hardware evaluation: parallel field testing of RTX 4060 Ti (desktop, 16GB VRAM, 160W) vs RTX 2000 Ada (wall-mount, 16GB VRAM, 70W) to determine which form factor wins with OIMs. Architecture identical on both.

-----

## 2. The Seven-Layer Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│  Layer 7: World Model / Counterfactual Engine                        │
│           "What happens if we change WOB to 28 klbs?"                │
├──────────────────────────────────────────────────────────────────────┤
│  Layer 6: Strategic Reasoning (Qwen 3B / Template Fallback)          │
│           Synthesis, contextualisation, natural language advisory     │
├──────────────────────────────────────────────────────────────────────┤
│  Layer 5: Neurosymbolic Safety & Causal Reasoning                    │
│           Clingo ASP constraints + PCMCI+ causal graphs              │
├──────────────────────────────────────────────────────────────────────┤
│  Layer 4: Integration & Uncertainty Quantification                   │
│           Meta-CfC fusion + ACI conformal + EDL epistemic + EVT tail │
├──────────────────────────────────────────────────────────────────────┤
│  Layer 3: Specialist Intelligence Mesh                               │
│           5× LNN specialists + ST-GNN system graph + xLSTM anomaly   │
├──────────────────────────────────────────────────────────────────────┤
│  Layer 2: Feature Engineering                                        │
│           Wavelet Scattering + HHT/EMD + Mamba temporal encoding     │
├──────────────────────────────────────────────────────────────────────┤
│  Layer 1: Physics Engine (Deterministic, CPU-only, Absolute Authority)│
│           MSE, d-exp, ECD, flow balance, bearing freqs, torque-drag  │
├──────────────────────────────────────────────────────────────────────┤
│  Layer 0: Multi-Modal Ingestion                                      │
│           WITS 1Hz + Vibration 20kHz + Temperature 1Hz + SCADA       │
└──────────────────────────────────────────────────────────────────────┘
```

### 2.1 Layer 0: Multi-Modal Ingestion (Unchanged)

WITS Level 0 TCP broadcast (1-5 Hz), vibration accelerometers (20kHz), temperature sensors (1Hz), SCADA digital I/O (variable rate). All ingested into a unified timestamped buffer.

### 2.2 Layer 1: Physics Engine (Unchanged from V2.5)

**Latency:** <5ms on CPU  
**Authority:** Absolute — no higher layer overrides physics calculations

Deterministic calculations:

- MSE (Mechanical Specific Energy) — drilling efficiency
- D-exponent — normalised formation hardness
- ECD (Equivalent Circulating Density) — effective mud weight
- Flow balance — kick/loss indicator
- Pit rate — smoothed rate of change of pit volume
- Bearing fault frequencies — BPFO, BPFI, BSF, FTF from FFT
- Hydraulic pressure decay — BOP seal integrity
- Stroke pressure variance — pump liner wear
- Torque-drag models — string friction and hole cleaning

**Output:** 64-dimensional physics feature vector + discrete safety flags.

**Non-negotiable:** When physics says ECD margin < 0.1 ppg = CRITICAL, that’s CRITICAL regardless of what any neural network thinks.

-----

### 2.3 Layer 2: Feature Engineering (NEW — From Both Research Streams)

**Purpose:** Transform raw and physics-derived signals into rich feature representations before the specialist mesh. This layer is entirely deterministic or uses fixed (non-learned) transforms — no training required, no failure modes.

**Latency:** ~5ms combined on CPU, <2ms on GPU

#### 2.3.1 Wavelet Scattering Transform (Claude Research)

**What:** Translation-invariant, deformation-stable features from vibration signals using fixed wavelet filters. Zero training required.

**Why:** On the CWRU bearing benchmark, scattering features achieve 100% classification accuracy across 15 bearing conditions. Data-efficient — critical when labelled fault events are rare offshore.

**Implementation:** ~500 lines Rust using `rustfft` and `fcwt` crates. Core operations are wavelet convolution → modulus → averaging, repeated across scales. Produces a fixed-length feature vector per time window.

**Output feeds:** Equipment specialist (Layer 3) with rich spectral features that complement physics-derived BPFO/BPFI frequencies.

**Maturity:** Production-ready. Kymatio library (Python) is BSD-licensed with GPU acceleration. Rust port is straightforward fixed-filter DSP.

#### 2.3.2 Hilbert-Huang Transform / EMD (Claude Research, Perplexity Validated)

**What:** Fully adaptive signal decomposition for nonlinear, non-stationary drilling signals. Decomposes signals into physically meaningful Intrinsic Mode Functions (IMFs) without pre-selected basis functions.

**Why:** Proven specifically for drilling vibration analysis — a 2023 paper in *Journal of Petroleum Exploration and Production Technology* demonstrated lithology identification from drill-bit vibration using HHT. Unlike FFT or wavelets, HHT adapts to the signal rather than forcing a fixed basis.

**Implementation:** ~300-500 lines Rust using cubic spline interpolation + `rustfft`. EMD runs in ~10-100ms on CPU for a 10-second signal at 10kHz. Ensemble EMD (EEMD) can be GPU-parallelised.

**Output feeds:** Formation specialist (lithology signatures), Equipment specialist (non-stationary vibration patterns that FFT misses), Operations specialist (adaptive drilling dynamics decomposition).

**Maturity:** NASA-heritage technology (developed at NASA Goddard). Used in seismology, oceanography, and structural health monitoring for 20+ years.

#### 2.3.3 Mamba Temporal Encoding (Claude Research)

**What:** State Space Model that processes long sensor sequences with O(L) linear complexity versus O(L²) for transformers. Mamba-2 achieves SOTA on 8+ time-series forecasting benchmarks.

**Why:** The specialist LNNs process short temporal windows (seconds to minutes). Some patterns require longer context — 2-hour ROP trends, 6-hour d-exponent evolution, 24-hour torque baseline drift. Mamba provides this long-range temporal encoding efficiently.

**Implementation:** Two Rust implementations exist: `mamba.rs` (pure Rust inference) and `mamba-ssm` (Candle-based, supports models up to 2.8B). The Liquid-S4 hybrid from Hasani et al. embeds linearised LNN dynamics inside S4 transition matrices — natural bridge from the CfC architecture.

**Architecture:** Small Mamba encoder (2-4M parameters) processes the rolling 2-hour physics feature buffer and produces a 64-dim temporal context vector. This vector is appended to the physics feature vector, giving every specialist access to long-range trends without requiring them to maintain long memory themselves.

**Latency:** ~3-5ms on GPU, ~15-25ms on CPU for 2-hour window at 1Hz (7,200 timesteps).

**Maturity:** Mamba-2 published at ICML 2024, Rust code exists, eMamba demonstrates FPGA/ASIC acceleration. Production-ready for inference; training uses PyTorch then exports.

-----

### 2.4 Layer 3: Specialist Intelligence Mesh (Extended from V2.5)

**Purpose:** Detect temporal patterns in enriched feature representations that threshold-based rules miss.

This layer combines three complementary architectures, each attacking a different failure mode detection strength:

#### 2.4.1 Five CfC/NCP Specialist Networks (From V2.5)

|Specialist      |Neurons|Inputs                                                                                  |Learns                                                                            |Output                                             |
|----------------|-------|----------------------------------------------------------------------------------------|----------------------------------------------------------------------------------|---------------------------------------------------|
|**Operations**  |50     |WOB/ROP/RPM/torque/MSE/hookload/d-exp/block + Mamba context (24 dims)                   |Drilling state dynamics, founder onset, pack-off precursors, stick-slip signatures|State (6 classes) + Health (0-1) + Optimal params  |
|**Hydraulics**  |32     |SPP/ECD/flow/mud weight/pump rate/pit vol + Mamba context (20 dims)                     |Pressure envelope evolution, ECD margin trend, pump efficiency                    |Envelope status + Trend + Confidence               |
|**Well Control**|40     |Pit vol/rate/gas/H2S/flow balance/ECD/SPP + Mamba context (18 dims)                     |Kick onset (including slow kicks), loss signatures, gas migration                 |Risk (0-1) + Event probabilities + Time-to-critical|
|**Formation**   |25     |D-exp/ROP/WOB/RPM/mud weight/torque + HHT IMFs + Mamba context (16 dims)                |Boundary approach, pore pressure trending, hardness transitions                   |Formation type + Boundary proximity + Hardness     |
|**Equipment**   |50     |FFT bands + Wavelet Scattering features + kurtosis/temps/SCADA + Mamba context (26 dims)|Bearing degradation curves, seal wear, pump liner wear                            |Per-component health + TTF + Failure modes         |

**Total:** ~197 neurons, ~5,000-8,000 parameters. CfC with NCP wiring within each specialist. ~20ms combined latency on CPU.

**Test-Time Training (NEW — Perplexity’s Best Finding):**

Each specialist implements distribution shift detection via KL-divergence between current batch statistics and running averages. When shift detected (new formation, new rig, equipment change):

- Batch normalisation statistics adapt in real-time (~50 lines of code)
- Lightweight CfC time constant parameters update without backpropagation
- Overhead: 1.2× normal inference time during adaptation, negligible when stable
- Adaptation completes within ~100 samples (1.5 hours of drilling at 1Hz)

**Fleet compound:** When Rig-001 adapts to a new Angola formation, the adapted normalisation parameters (5KB) are uploaded to the hub. Rig-002 starting in the same basin receives pre-adapted parameters — zero learning phase needed.

**Why this is devastating:** Competitors’ models trained on historical data perform poorly for 5-10 wells in new formations until retrained. SAIREN-OS adapts within hours.

#### 2.4.2 xLSTM Anomaly Detection Layer (NEW — Claude Research)

**What:** Dedicated anomaly detection network using Hochreiter’s extended LSTM architecture (xLSTMAD variant), running parallel to the specialist mesh.

**Why:** xLSTMAD outperforms 23 baselines on the TSB-AD-M benchmark, achieving near-perfect scores on industrial control datasets (SWaT: VUS-PR 0.91, VUS-ROC 0.95). The mLSTM variant uses matrix memory with query-key-value retrieval that captures complex temporal dependencies the CfC specialists may miss.

**Architecture:** Single xLSTM network (~2-4M parameters) receives the combined physics feature vector + Mamba temporal context. Trained to reconstruct normal operating patterns; anomalies are detected when reconstruction error exceeds learned thresholds. Outputs:

- Anomaly score (0-1) per timestep
- Reconstruction error breakdown per channel (which parameters are anomalous?)
- Novelty flag: “this pattern doesn’t match any training distribution”

**Why parallel to specialists, not replacement:** The specialists detect *known* failure modes (pack-off, stick-slip, kick). xLSTM detects *unknown* anomalies — patterns that don’t match any training data. This is the black swan detector. Together they cover both known-unknown and unknown-unknown failure modes.

**Latency:** ~3-5ms on GPU, ~10-15ms on CPU.

**Maturity:** xLSTMAD published at ICDM 2025, xLSTM-Mixer at NeurIPS 2025. NXAI (Hochreiter’s company) has AMD partnership for industrial edge deployment. No Rust implementation yet; LSTM operations are straightforward to port to Burn framework.

#### 2.4.3 Spatio-Temporal Graph Neural Network (NEW — Claude Research)

**What:** GNN that models the rig as a connected system where faults propagate through physical connections.

**Why:** A kick detected in the mud system affects the BOP, pumps, and drill string — and the propagation follows physical connections. ST-GNNs achieve 92-97% fault diagnosis accuracy on the Tennessee Eastman Process benchmark. For oil and gas specifically, GCN+GAT achieved 92% accuracy in pipeline defect recognition and 85% accuracy in 24-hour advance leakage prediction.

**Architecture:**

Graph topology derived from rig P&ID (piping and instrumentation diagram):

- **Nodes** (~50-200): Sensors and components (drill string, mud pit, shale shakers, mud pumps, BOP stack, choke manifold, top drive, rotary table)
- **Edges** (~100-500): Material flows (mud circuit), energy flows (torque transmission), signal flows (pressure propagation)
- Each node carries the relevant specialist’s hidden state as its feature vector

The GNN performs message passing: when the Hydraulics specialist detects rising SPP, the GNN propagates this signal through the graph topology to check whether connected Equipment nodes (pump bearings, drill string) show correlated anomalies. This distinguishes formation-induced pressure changes (affecting only the wellbore path) from mechanical failures (affecting the equipment path).

**Output:**

- Fault localisation: which component is the likely root cause?
- Propagation prediction: which downstream components will be affected?
- System-level health score that accounts for connected failure modes

**Latency:** <2ms inference on RTX 4060 via ONNX (graph is small). Train in PyTorch Geometric, export to ONNX, deploy via `ort` Rust crate.

**Compounding moat:** As SAIREN-OS deploys across rigs with different configurations, the GNN learns which graph topologies produce which fault propagation patterns. Each new rig type enriches the library.

#### 2.4.4 Continuous Optimiser (Unchanged from V2.5)

50-neuron CfC producing optimal WOB/RPM/Flow every 30-60 seconds. Runs parallel to ML Engine V2.2 for validation. Separate from specialist mesh because it answers “what should we do?” not “what’s happening?”

-----

### 2.5 Layer 4: Integration & Uncertainty Quantification (Extended from V2.5)

**Purpose:** Fuse all specialist signals into unified intelligence vectors with calibrated uncertainty.

#### 2.5.1 Meta-CfC Fusion (From V2.5)

128-neuron CfC with FullyConnected wiring. Compression layer (200→64 dims) reads all specialist hidden states. Four readout heads (extended from three):

1. **Anomaly confidence** — scalar (0-1), combines specialist signals + xLSTM novelty + GNN fault localisation
2. **Top-10 correlations** — learned inter-domain patterns (e.g., torque_instability + bearing_kurtosis → gearbox wear not formation)
3. **Urgency scoring** — GREEN/YELLOW/RED accounting for compound risk
4. **Epistemic uncertainty** — “how far outside training distribution is this signal?” (NEW — from Perplexity’s UQ recommendation + Claude’s Evidential DL research)

**Latency:** ~8-12ms

#### 2.5.2 Adaptive Conformal Inference — ACI (NEW — Claude Research)

**What:** Wraps EVERY prediction from the system with a calibrated confidence interval that self-corrects under distribution shift.

**Why:** Standard prediction intervals assume data is stationary. ACI from Gibbs & Candès (NeurIPS 2021) provides finite-sample coverage guarantees even for non-exchangeable data — drilling conditions that change mid-operation. Where standard conformal prediction drops to 81-84% coverage under shift, ACI maintains ~90%.

**Mechanism:** Single scalar update per prediction: α_{t+1} = α_t + γ(α − err_t). Dynamically widens/narrows prediction intervals based on recent accuracy.

**Implementation:** ~20 lines of Rust. Wraps the optimiser’s parameter recommendations: “Optimal WOB: 35 klbs [32-38 klbs, 90% coverage]” instead of just “Optimal WOB: 35 klbs.”

**Why this matters for trust:** An operator told “increase WOB to 35” might hesitate. An operator told “increase WOB to 35, safe range 32-38 based on current formation response” has the confidence to act.

#### 2.5.3 Evidential Deep Learning — EDL (NEW — Claude Research)

**What:** Single-forward-pass uncertainty estimation by placing a Dirichlet distribution on specialist output probabilities. Explicitly separates:

- **Aleatoric uncertainty** — inherent noise in drilling data (sensor noise, formation heterogeneity)
- **Epistemic uncertainty** — model ignorance (never seen this formation type, novel equipment configuration)

**Why:** High anomaly confidence + low epistemic uncertainty = known failure pattern (act). High anomaly + high epistemic uncertainty = black swan (proceed with extreme caution, escalate). This distinction changes the advisory.

**Implementation:** Modified output layer on each specialist — no additional compute overhead whatsoever. Deploy via ONNX with EDL-modified architecture.

#### 2.5.4 Extreme Value Theory — EVT (NEW — Claude Research)

**What:** Models the tail distribution of sensor signals using Generalised Pareto Distribution fitted to the top 1-5% of historical observations.

**Why:** Estimates probability of never-before-seen extreme values — kick magnitudes beyond training data, vibration amplitudes exceeding all historical precedent. ACI and EDL can’t handle this because they require calibration data; EVT extrapolates from the tail.

**Implementation:** Periodic recalibration (monthly/quarterly) on the hub server. Lightweight lookup table deployed to edge. Computational cost at inference: negligible.

**Combined UQ Architecture:**

```
Specialist output
      ↓
EDL: aleatoric vs epistemic uncertainty (single pass, zero overhead)
      ↓
ACI: calibrated conformal interval (one scalar update, ~20 lines)
      ↓
EVT: tail risk estimate for extreme scenarios (lookup table)
      ↓
Advisory includes: prediction + confidence interval + uncertainty type + tail risk
```

-----

### 2.6 Layer 5: Neurosymbolic Safety & Causal Reasoning (NEW)

**Purpose:** Provide explainable safety constraints and root cause analysis that regulators and operators can inspect, verify, and trust.

#### 2.6.1 Clingo ASP Safety Reasoning (Claude Research)

**What:** Answer Set Programming solver that evaluates neural model outputs against hard-coded drilling safety rules.

**Why:** Neural networks can be wrong. A symbolic safety layer that operates on logical rules provides a verifiable safety net. When a neural model flags an anomaly, Clingo evaluates it against the full body of drilling rules, physics constraints, and operational procedures, producing an explainable chain of reasoning.

**Example rules (drilling domain):**

```prolog
% If mud weight below fracture gradient AND losses detected → kick risk HIGH
kick_risk(high) :- 
    mud_weight(MW), fracture_gradient(FG), MW < FG,
    losses_detected(true).

% If ECD margin < 0.1 ppg → CRITICAL regardless of neural model output
override_critical :- ecd_margin(M), M < 0.1.

% Neural model recommends WOB increase but torque already elevated → VETO
veto_recommendation(increase_wob) :-
    neural_recommends(increase_wob),
    torque_cv(CV), CV > 12.
```

**Architecture:** Clingo acts as a “safety voter” in the multi-agent voting system. It can:

- Confirm neural advisory (agrees with symbolic rules)
- Augment neural advisory (adds context from rules the neural model doesn’t encode)
- Veto neural advisory (recommendation violates a safety constraint)

**Implementation:** C++ solver with existing Rust bindings via `clingo-rs` crate. ~5MB binary, millisecond solve times for drilling-scale rule sets (~100-500 rules).

**Audit trail:** Every decision produces: “BHA vibration exceeded threshold (neural) → drillstring_resonance_risk (symbolic rule #47) → recommended ROP reduction (action) → counterfactual: prediction changes to SAFE if WOB reduced by 2 klbs (world model).”

#### 2.6.2 PCMCI+ Causal Discovery (Claude Research)

**What:** Algorithm that discovers causal relationships (not just correlations) between time-series variables. Published in *Science Advances* 2019.

**Why:** Correlation says “torque and SPP both rose.” Causation says “torque rise caused SPP rise because the drill string is packing off, increasing friction which increases standpipe pressure.” This distinction changes the recommended action entirely.

**Implementation:** PCMCI+ runs as a periodic batch job (every shift change or daily) using the Tigramite library (Python, called from Rust via subprocess or embedded Python). Processes ~50-100 variables with appropriate lag settings in minutes on RTX 4060.

**Output:** Causal graph stored in Rust’s `petgraph` crate. When an anomaly is detected in real-time, the system traverses the graph upstream to identify root causes — not just symptoms.

**Compounding moat:** After 100+ wells, the discovered causal graphs encode formation-specific and rig-specific causal relationships that represent genuine drilling knowledge. This is not reproducible without equivalent operational data.

#### 2.6.3 Drilling Knowledge Graph (Claude Research)

**What:** Lightweight SPARQL database (Oxigraph, Rust-native) hosting a drilling domain knowledge graph based on ISO 15926 (oil and gas lifecycle data standard).

**Contents:** Equipment taxonomy, formation properties, fault patterns, operational procedures, fleet-discovered causal relationships. 10K-100K triples, <100MB RAM, sub-millisecond query times.

**Purpose:** Contextual enrichment for all specialist agents. When the Equipment specialist detects bearing degradation, the knowledge graph provides: bearing model, manufacturer, expected L10 life, maintenance history, similar failures on fleet rigs, recommended interventions.

**Growth:** Every deployment adds rig-specific knowledge (equipment configurations, formation encounters, fault resolutions). The knowledge graph becomes an irreplaceable institutional memory.

-----

### 2.7 Layer 6: Strategic Reasoning (Extended from V2.5)

**Qwen 2.5 3B** (GPU) or **Qwen 2.5 1.5B** (CPU fallback) — parallel evaluation of both to resolve the debate.

Receives:

- TicketContext from tactical agent
- 5 LNN specialist signal vectors
- xLSTM anomaly score + novelty flag
- GNN fault localisation + propagation prediction
- Meta-CfC fusion output (anomaly, correlations, urgency, epistemic uncertainty)
- ACI calibrated intervals
- Clingo safety evaluation (confirm/augment/veto)
- Causal graph traversal (root cause identification)
- Continuous optimiser recommendation
- Fleet RAG precedents
- Knowledge graph context

**Jobs:**

1. **Synthesise** — combine all signals into coherent diagnosis
2. **Contextualise** — reference campaign history (256K context window if Jamba validated)
3. **Communicate** — natural language advisory the OIM actually reads
4. **Reason counterfactually** — “if WOB reduced to 25 klbs, expected ROP 85 ft/hr” (informed by world model)
5. **Explain causally** — “SPP rise caused by pack-off (causal graph), not pump efficiency change”

**Template fallback (enhanced):** LNN signals + UQ calibration + causal graph + Clingo reasoning populate templates with temporal context, confidence scores, correlation patterns, and root cause chains. Fully operational without LLM. Regulatory safety net.

**Latency:** 300-400ms at 3B (GPU), 3-8s at 3B (CPU), 35ms template-only.

**Model evaluation plan:** Parallel benchmarking of Qwen 3B vs Qwen 1.5B vs Jamba 3B against advisory test set (10 historical wells including edge cases where fleet RAG returns nothing). If 1.5B produces advisories that OIMs would act on correctly, it’s promoted. If not, 3B confirmed.

-----

### 2.8 Layer 7: World Model / Counterfactual Engine (NEW — Perplexity Concept, Claude Architecture)

**What:** A thin prediction layer built on top of the existing architecture that simulates “what happens next” given proposed parameter changes.

**Why:** An OIM who can *see* the predicted outcome of a parameter change is far more likely to follow the advisory. “Reduce WOB” is weak. “If you reduce WOB from 35→28, ROP drops to 70 initially but recovers to 76 by minute 20, and pack-off risk drops from 40% to <5%” is compelling.

**Architecture (simplified from Perplexity’s proposal):**

The world model is NOT a separate standalone model. It’s a thin CfC dynamics layer that sits on top of the existing stack:

```
Current state (from specialists + physics)
      ↓
Proposed parameter change (from optimiser or operator)
      ↓
CfC Dynamics Model (64 neurons, trained on historical rollouts)
      ↓
Predicted trajectory: 30-minute forward simulation
      ↓
PINN Physics Surrogate validates trajectory (FNO, 5-50MB, sub-10ms)
      ↓
ACI wraps predictions with calibrated intervals
      ↓
Dashboard renders split-screen: "current path" vs "recommended path"
```

The PINN (Physics-Informed Neural Network) / FNO (Fourier Neural Operator) acts as a fast physics simulator that validates the CfC dynamics model’s predictions against physical laws. FNOs deliver 100,000× speedup over traditional FVM solvers with comparable accuracy. Train offline on HPC, deploy inference model (5-50MB) on RTX 4060 via ONNX.

**Trust factor:** The dashboard shows a split-screen animation:

- Left: “If no change” → predicted ROP, MSE, risk trajectory
- Right: “If recommended change” → predicted improvement with confidence intervals

Operators don’t just trust recommendations — they see the predicted future before committing.

**Latency:** ~15-25ms for 30-minute rollout on GPU (1,800 timesteps through small CfC + PINN validation).

**Digital twin divergence detection:** When the world model predicts X and reality delivers Y, the divergence IS the anomaly signal. A model that expects stable SPP but sees it rising detects developing problems earlier than threshold-based monitoring.

-----

## 3. Fleet Intelligence Network (Extended from V2.5)

### 3.1 Three-Tier Hierarchical Sync (From V2.5)

**Tier 1: Emergency Pattern Broadcast (Real-Time)**

- Trigger: CRITICAL event unpredicted by LNNs
- Payload: <1KB (physics features + label)
- Distribution: Hub validates, broadcasts to ALL rigs within 60 seconds
- Frequency: 1-2/week fleet-wide

**Tier 2: Gradient Exchange (Hourly, Significance-Gated)**

- Cosine similarity check: skip if >0.95 vs last upload (~60% traffic reduction)
- Payload: 5-25KB compressed weight deltas
- Hub performs federated averaging: 85% local, 15% fleet

**Tier 3: Daily Episode Batch**

- All AMBER/RED episodes + random 10% GREEN sample
- Payload: 50-200KB/rig/day
- Hub curator scores, adds to fleet library

### 3.2 Clustered Federated Learning (NEW — Claude Research)

**What:** Groups rigs with similar geological/operational profiles and trains per-cluster models.

**Why:** Rigs drilling Gulf of Mexico carbonates face fundamentally different dynamics than North Sea shales. Flat FedAvg averaging dilutes formation-specific knowledge. Clustered FL creates **formation-type knowledge pools** where every well drilled in a geological basin improves predictions for all subsequent wells in that basin.

**Implementation:** IFCA (Iterative Federated Clustering Algorithm) or FedGroup. Clusters emerge automatically from gradient similarity. Rig-007 drilling sandstone receives fleet knowledge weighted toward other sandstone-drilling rigs, not an average across all formations.

### 3.3 FedPer — Personalised Layers (NEW — Claude Research)

**What:** Split models into shared base layers (universal drilling physics) and personal head layers (local formation characteristics).

**Why:** Fluid mechanics and drill string dynamics are universal. Formation response varies. FedPer shares the universal layers across the fleet while keeping formation-specific heads local.

**Implementation:** Built into NVIDIA FLARE. Minimal overhead. The base layers improve with every rig; the personal heads remain rig-specific.

### 3.4 Communication Efficiency (NEW — Claude Research)

**FedSMU:** 10× bandwidth reduction by transmitting only the sign (±) of each weight change.  
**FedKD:** 94.89% communication reduction through mutual knowledge distillation + dynamic gradient compression.

For a 10M parameter model: full transmission = ~40MB/round. With sign compression + FedPer shared-only = **<500KB** — feasible on the most constrained satellite links.

### 3.5 Test-Time Training Fleet Propagation (NEW — Perplexity)

When a rig adapts to new conditions via TTT:

1. Adapted normalisation parameters extracted (5KB)
2. Uploaded to hub with geological context metadata
3. Hub indexes by formation type, depth range, basin
4. Other rigs entering similar conditions receive pre-adapted parameters
5. Zero learning phase for the second rig in any given basin

### 3.6 New Rig Onboarding Sequence

1. Receives fleet-averaged LNN weights (warm start from clustered FL)
2. Receives formation-appropriate TTT parameters (if basin previously encountered)
3. Shadow mode during first well (all systems log but don’t alert)
4. Validation against threshold-based system
5. Promotion to active advisory
6. Begins contributing gradients + episodes to fleet

-----

## 4. Symbolic Regression — The IP Engine (NEW — Perplexity’s Best Long-Term Idea)

### 4.1 What It Does

After accumulating data from 50+ wells across the fleet, run symbolic regression (PySR or equivalent) on consolidated drilling data to discover **human-readable equations** that describe formation-specific drilling efficiency.

### 4.2 Why This Is Transformative

The current industry-standard ROP model is Bourgoyne-Young (1974). If SAIREN-OS discovers a formation-specific drilling efficiency law that outperforms it — and the fleet data proves it does — that’s:

1. **Publishable at SPE** (Society of Petroleum Engineers) — your name on the paper
2. **An industry reference** — drilling engineers cite SAIREN equations
3. **Not reverse-engineerable** — it’s physics, not code. Knowing the equation doesn’t help a competitor who lacks the data to validate it
4. **A validation of the ML models** — discovered equations serve as independent ground truth

### 4.3 How It Works

```
Fleet data: 50+ wells × 10,000 samples × 40 channels
      ↓
Symbolic regression (12-24 hours on hub server)
      ↓
Discovered equations: ROP = f(WOB, RPM, mud_weight, d_exponent, ...)
      ↓
Validation against held-out wells
      ↓
If validated: embed in physics engine as formation-specific models
              + publish at SPE as discovered drilling laws
```

### 4.4 Example Output

```
Basin: North Sea, Kimmeridge Clay Formation
Discovered: ROP_opt = 0.34 × WOB^0.82 × RPM^0.31 × (16.5 / MW) × exp(-0.019 × d_exp)
R²: 0.94 across 12 wells
Improvement over Bourgoyne-Young: 23% better ROP prediction
```

### 4.5 Implementation

Run on hub server (not edge). Batch job after sufficient data accumulation. Discovered equations are lightweight (a few hundred bytes) and deploy to all edge nodes as enhanced physics engine models.

**Timeline:** Phase 3 (requires 50+ wells of fleet data). This is the permanent, compounding IP moat.

-----

## 5. Differentiable Physics — Certification Strategy (Deferred to V3.5)

### 5.1 What It Is

Embed physics equations as differentiable operations inside the LNN computational graph so gradients flow through physics during training.

### 5.2 Why It Matters (Perplexity Was Right)

The certification angle is the real value. Being able to prove **structurally** that the system cannot recommend unsafe parameters — not just that it’s unlikely to — is a genuine differentiator with regulatory bodies (NOPSEMA, HSE, DNV). Oil majors like Equinor and Shell require formal safety cases for autonomous systems. Differentiable physics enables formal verification: “this architecture is mathematically incapable of recommending WOB above X in formation Y.”

### 5.3 Why It’s Deferred

Implementation requires custom autograd implementations for every physics equation in Rust using the Burn framework, testing against analytical solutions, and validation that constraints don’t prevent learning useful patterns. Estimated 3-6 months of research work.

### 5.4 Status

Research branch. Prioritised for post-pilot R&D. If the gradients-through-physics approach produces meaningfully better results than the “physics feeds LNN” approach in V3.0, promote to V3.5 production.

-----

## 6. Offline Reinforcement Learning — Safe Parameter Optimisation (NEW — Claude Research)

### 6.1 Implicit Q-Learning (IQL)

**What:** Offline RL algorithm that learns optimal drilling parameter policies from historical data without dangerous exploration. IQL uses expectile regression and never evaluates actions outside the historical dataset — inherently safe for drilling.

**Why:** Current competitors use supervised ML for ROP prediction. Nobody has deployed RL that optimises drilling parameters holistically while respecting safety constraints. SAIREN-OS would be first.

### 6.2 PICNN Safety Layer

**What:** Partially Input-Convex Neural Networks provide deployment-time safety correction. At each timestep, the RL policy proposes an action, and the PICNN corrects it by gradient descent over a convex cost surface encoding safety constraints. Tested on industrial process control (exothermic CSTR with realistic constraints).

### 6.3 Architecture

```
Historical drilling data (1000+ wells from fleet)
      ↓
IQL training (offline, on hub server)
      ↓
Policy network (2-4 layer MLP, <2M params)
      ↓
Deploy to edge → proposes WOB/RPM/Flow adjustments
      ↓
PICNN safety layer corrects for constraint violations (<0.5ms)
      ↓
Clingo symbolic rules verify final recommendation
      ↓
Advisory: "Optimal parameters: WOB 35 [32-38], RPM 120 [110-130]"
```

### 6.4 Timeline

Phase 3 (requires sufficient fleet data for offline training). The Decision Transformer variant (RL via sequence modelling) is an alternative if transformer inference is already available.

-----

## 7. Neuromorphic Computing — The Efficiency Endgame (Phase 4+)

### 7.1 Current Assessment (Agree with Perplexity’s Vision, Disagree on Timing)

Spiking Neural Networks on neuromorphic hardware (BrainChip AKD1500, Intel Loihi 2, Innatera Pulsar) offer 60× power reduction and 3× speed improvement for event-driven processing. Vibration sensors naturally produce spike-like data — perfect match.

### 7.2 Why Phase 4, Not Phase 1

- BrainChip AKD1500 volume production not until Q3 2026 at earliest
- No working “Spiking CfC” implementation exists yet (Perplexity’s code is pseudocode)
- Intel Loihi 2 has limited commercial availability
- The RTX 4060 path delivers all required capabilities at acceptable power

### 7.3 Strategic Monitoring

Track BrainChip and Innatera commercial deployment. When neuromorphic hardware is available at volume with proven toolchains, the Equipment specialist (vibration analysis) is the natural first candidate for migration. The event-driven architecture matches the sensor modality perfectly.

-----

## 8. Hardware Specification

### 8.1 One Product, Hardware-Adaptive

Industrial Mini-ITX workstation:

- **GPU option A:** RTX 4060 Ti — 16GB VRAM, 160W TDP, desktop form factor
- **GPU option B:** RTX 2000 Ada — 16GB VRAM, 70W TDP, wall-mount form factor
- **CPU:** Intel Core i7-13700K (8P+8E cores)
- **RAM:** 32GB DDR5
- **Storage:** 1TB NVMe SSD, full disk encryption (LUKS)
- **Power:** <450W total (GPU option A) / <250W (GPU option B)
- **Cost:** ~£4,500/unit

**Parallel hardware evaluation:** Deploy one rig with 4060 Ti, one with 2000 Ada. Compare thermal performance, form factor feedback from OIMs, and whether the power/space savings of wall-mount changes the installation conversation.

### 8.2 Inference Budget (Full V3.0 Stack on RTX 4060)

|Component             |Latency (GPU)|Latency (CPU)|Memory      |
|----------------------|-------------|-------------|------------|
|Physics Engine        |<5ms         |<5ms         |~10MB       |
|Wavelet Scattering    |~1ms         |~3ms         |~5MB        |
|HHT/EMD               |~2ms         |~5ms         |~5MB        |
|Mamba Temporal Encoder|~3ms         |~15ms        |~50MB       |
|5× CfC Specialists    |~5ms         |~20ms        |~20MB       |
|xLSTM Anomaly Detector|~3ms         |~10ms        |~30MB       |
|ST-GNN System Graph   |~2ms         |~5ms         |~20MB       |
|Meta-CfC Integration  |~3ms         |~8ms         |~15MB       |
|UQ Stack (ACI+EDL+EVT)|<1ms         |<1ms         |~5MB        |
|Clingo Safety Rules   |~2ms         |~2ms         |~5MB        |
|**Subtotal (Pre-LLM)**|**~27ms**    |**~74ms**    |**~165MB**  |
|Qwen 3B Strategic     |~350ms       |~5,000ms     |~2,000MB    |
|World Model Rollout   |~15ms        |~40ms        |~50MB       |
|**Total**             |**~392ms**   |**~5,114ms** |**~2,215MB**|
|Template-only (no LLM)|**~42ms**    |**~114ms**   |**~215MB**  |

VRAM headroom with 3B model: ~14GB free (16GB card). Ample room for model upgrades, dashboard rendering, and future additions.

### 8.3 Degradation Modes

|Mode             |Condition               |Capability                              |Latency|
|-----------------|------------------------|----------------------------------------|-------|
|**Full**         |GPU + all models healthy|Complete advisory with world model      |~400ms |
|**CPU**          |GPU failure             |Smaller LLM, all other layers functional|~5s    |
|**LNN-only**     |GPU + LLM failure       |Specialists + UQ + Clingo, no synthesis |~75ms  |
|**Template-only**|All neural models failed|Physics + rules + templates             |~42ms  |
|**Physics-only** |Everything failed       |Deterministic calculations + hard limits|<5ms   |

Five layers of graceful degradation. Even in worst case, the system provides physics-validated safety monitoring.

-----

## 9. Validation Protocol

### 9.1 Four Core Tests (From V2.5, Extended)

**Test 1 — Historical Wells:**
V3.0 detects ≥90% of V1.0 events + ≥2 events V1.0 missed + false positive rate ≤ V1.0 + causal graph correctly identifies root cause in ≥80% of multi-symptom events.

**Test 2 — Fleet Learning:**
Novel kick pattern on Rig 1 day 10 → Rigs 2-5 achieve ≥80% detection within 24 hours via gradient sharing. TTT adaptation completes within 100 samples. Clustered FL correctly groups rigs by formation type.

**Test 3 — Resilience:**
7-day continuous operation with 10% packet loss, zero-value injections, phase transitions, GPU failure, network dropout, thermal throttling. Zero crashes, <30s recovery, graceful degradation documented through all five modes.

**Test 4 — V72 Pilot Shadow Mode:**
All new components (xLSTM, GNN, Mamba, UQ stack, Clingo) log votes without acting. Compare detection timing and false positive rates vs current threshold system.

### 9.2 Additional Validation (NEW)

**Test 5 — World Model Accuracy:**
30-minute forward predictions validated against actual outcomes on historical wells. Prediction error ≤ 15% for ROP, ≤ 10% for SPP, ≤ 5% for ECD. Conformal intervals achieve ≥90% coverage.

**Test 6 — Causal Discovery Validation:**
PCMCI+ discovered causal graphs validated against known drilling physics relationships. At least 80% of discovered edges correspond to physically meaningful connections.

**Test 7 — Symbolic Regression Validation:**
Discovered equations validated on held-out wells not used in discovery. R² ≥ 0.85. Improvement over Bourgoyne-Young ≥ 10% for formation-specific predictions.

-----

## 10. Build Sequence

### Phase 1: V72 Pilot (Now — Q1 2026)

**Ship:** Current V1.0 threshold system + shadow logging for:

- Single Operations CfC specialist
- ACI conformal intervals on existing predictions (~20 lines)
- Wavelet Scattering features logged (not yet consumed by models)
- HHT/EMD features logged

**Validate:** Shadow system detection timing vs threshold system. Zero operational impact.

### Phase 1.5: LNN Mesh Shadow (Q2 2026)

**Ship:** V1.0 remains primary + all 5 drilling LNNs shadow:

- All specialists trained on V72 pilot data
- Continuous Optimiser parallel to ML V2.2
- Meta-CfC integration layer shadow
- Mamba temporal encoder providing context to specialists
- TTT distribution shift detection active
- xLSTM anomaly detector shadow
- Clingo safety rules evaluating (logging only)

**Validate:** Compare specialist mesh accuracy to threshold system across multiple wells.

### Phase 2.0: LNN Promotion (Q3 2026)

**Ship:** LNN specialists promoted to active advisory pipeline:

- Strategic model drops from 7B to 3B (LNN preprocessing simplifies transformer job)
- Fleet gradient sharing enabled (FedProx + significance gating)
- Clustered FL groups rigs by formation
- TTT fleet propagation active
- Equipment Health specialist shadow (Guardian TDS data)
- ST-GNN system graph shadow
- Jamba 3B parallel evaluation begins
- xLSTM promoted to active (parallel anomaly channel)

**Validate:** OIM-assessed advisory accuracy ≥95%. False positive rate <3%.

### Phase 2.5: Full Mesh + UQ (Q4 2026)

**Ship:**

- Full UQ stack active (ACI + EDL + EVT on every prediction)
- ST-GNN promoted to active (fault propagation)
- Equipment specialist active with Wavelet + HHT features
- PCMCI+ causal discovery running daily batch
- Knowledge graph (Oxigraph) with basic drilling ontology
- Multi-modal fusion (WITS + vibration + temperature) at Layer 2

### Phase 3.0: World Model + RL + Symbolic Regression (2027)

**Ship:**

- World model / counterfactual engine active (30-min forward simulation)
- PINN/FNO digital twin surrogate for physics validation
- IQL offline RL with PICNN safety layer (parameter optimisation)
- Symbolic regression on 50+ well fleet data (discover drilling equations)
- Differentiable physics research branch (V3.5 candidate)
- BOP/Pump equipment LNNs
- Autonomous parameter adjustment with human approval

### Phase 4.0: Efficiency + Scale (2027-2028)

**Ship:**

- Neuromorphic evaluation (BrainChip AKD1500 for Equipment specialist)
- FedSMU/FedKD communication compression for 100+ rig fleet
- Decision Transformer for sequence-based drilling policy
- ISO 15926 full knowledge graph integration
- Third hub (Singapore for APAC expansion)
- Multi-operator federated learning (with differential privacy)

-----

## 11. Non-Negotiable Principles

1. **Physics layer always authoritative** — LNNs, GNNs, xLSTM, RL all augment, never override
2. **Well control thresholds stay manual** — kick detection limits are regulatory, not learned
3. **Every layer degrades gracefully** — five degradation modes, none catastrophic
4. **Template-only mode must remain viable** — regulatory safety net
5. **One product, hardware-adaptive** — no tiers, no fragmentation
6. **Shadow mode before promotion** — every component validated before influencing advisories
7. **Clingo can veto any neural recommendation** — symbolic safety constraints are absolute
8. **Causal, not just correlational** — root cause analysis, not symptom matching

-----

## 12. Moat Analysis — Honest Assessment

### Permanent Moats (Cannot Be Replicated Without Equivalent Deployment)

|Moat                                     |Mechanism                                  |Compounding Rate                       |
|-----------------------------------------|-------------------------------------------|---------------------------------------|
|**Fleet data corpus**                    |Every well adds irreplaceable labelled data|Linear per rig, quadratic network value|
|**Causal graph library**                 |Formation-specific causal relationships    |Grows with every well in every basin   |
|**Discovered drilling equations**        |Symbolic regression IP                     |Permanent — published physics          |
|**GNN fault propagation library**        |Rig-type-specific fault patterns           |Grows with rig diversity               |
|**Knowledge graph**                      |Institutional memory of every event        |Grows with every deployment            |
|**Data sovereignty architecture**        |Federated gradients, raw data stays on rig |Regulatory requirement, NOC preference |
|**Operator trust & workflow integration**|SOPs built around SAIREN-OS                |Switching cost increases with time     |

### Temporary Moats (12-24 Months Before Competitors Can Replicate)

|Moat                       |Mechanism                      |Decay Rate                                   |
|---------------------------|-------------------------------|---------------------------------------------|
|**Architecture**           |Seven-layer integrated stack   |SLB/Halliburton can replicate in 18-24 months|
|**Edge LNN/Mamba/xLSTM**   |Specific model choices         |Open research, implementations available     |
|**UQ stack**               |ACI + EDL + EVT combination    |Techniques are published                     |
|**Neuromorphic efficiency**|If deployed, hardware advantage|Hardware becomes commodity                   |

### The Critical Insight

The architecture is defensible for 18-24 months. The data network effect is defensible permanently. Every month of deployment widens the permanent moat while the temporary moats slowly erode. A competitor starting in 2027 needs:

1. Equivalent multi-rig deployment experience (years to accumulate)
2. Formation-specific causal graphs (requires drilling actual wells)
3. Validated symbolic drilling equations (requires fleet data that doesn’t exist yet)
4. Operator trust built through demonstrated accuracy (years of track record)

The technology can be copied. The data cannot.

-----

## 13. Total System Specifications

|Specification                          |Value                                                |
|---------------------------------------|-----------------------------------------------------|
|**End-to-end latency**                 |~400ms (GPU full), ~5s (CPU), ~42ms (template-only)  |
|**Pre-LLM intelligence latency**       |~27ms (GPU), ~74ms (CPU)                             |
|**VRAM usage**                         |~2.2GB (full stack with 3B LLM)                      |
|**System RAM**                         |~4GB operational                                     |
|**LNN mesh parameters**                |~8,000 (5 specialists + optimiser)                   |
|**xLSTM parameters**                   |~2-4M                                                |
|**Mamba encoder parameters**           |~2-4M                                                |
|**GNN parameters**                     |~1-10M                                               |
|**Total neural parameters (excl. LLM)**|~10-20M                                              |
|**Target advisory accuracy**           |≥95% safe and actionable                             |
|**False positive rate**                |<3%                                                  |
|**UQ coverage guarantee**              |≥90% (ACI calibrated)                                |
|**Fleet gradient sync**                |5-25KB/hour (significance-gated)                     |
|**Episode sync**                       |50-200KB/day                                         |
|**TTT adaptation time**                |~100 samples (1.5 hours)                             |
|**World model rollout**                |30-minute prediction in ~15ms                        |
|**Degradation modes**                  |5 layers (Full → CPU → LNN-only → Template → Physics)|
|**Power consumption**                  |150-250W (GPU A) / 100-170W (GPU B)                  |

-----

## 14. Technology Attribution

|Technology                    |Source                    |Priority|Status    |
|------------------------------|--------------------------|--------|----------|
|CfC/NCP Liquid Neural Networks|V2.5 Architecture         |P0      |Foundation|
|Meta-CfC Integration Layer    |Perplexity V2.5 Critique  |P0      |Foundation|
|Hierarchical Fleet Sync       |Perplexity V2.5 Critique  |P0      |Foundation|
|Adaptive Conformal Inference  |Claude Research           |P0      |Phase 1   |
|Wavelet Scattering Transform  |Claude Research           |P0      |Phase 1   |
|HHT / EMD                     |Claude Research           |P0      |Phase 1   |
|Test-Time Training            |Perplexity Research       |P0      |Phase 1.5 |
|Mamba Temporal Encoding       |Claude Research           |P1      |Phase 1.5 |
|xLSTM Anomaly Detection       |Claude Research           |P1      |Phase 1.5 |
|Clingo ASP Safety             |Claude Research           |P1      |Phase 1.5 |
|PCMCI+ Causal Discovery       |Claude Research           |P1      |Phase 2.5 |
|Clustered Federated Learning  |Claude Research           |P1      |Phase 2.0 |
|FedPer Personalised Layers    |Claude Research           |P1      |Phase 2.0 |
|ST-GNN System Graph           |Claude Research           |P2      |Phase 2.5 |
|Evidential Deep Learning      |Claude Research           |P2      |Phase 2.5 |
|EVT Tail Risk                 |Claude Research           |P2      |Phase 2.5 |
|Drilling Knowledge Graph      |Claude Research           |P2      |Phase 2.5 |
|World Model / Counterfactual  |Perplexity Research       |P2      |Phase 3.0 |
|PINN / FNO Digital Twin       |Claude Research           |P2      |Phase 3.0 |
|IQL Offline RL + PICNN        |Claude Research           |P2      |Phase 3.0 |
|Symbolic Regression           |Perplexity Research       |P2      |Phase 3.0 |
|Differentiable Physics        |Perplexity V2.5 Critique  |P3      |Phase 3.5 |
|FedSMU/FedKD Compression      |Claude Research           |P3      |Phase 4.0 |
|Neuromorphic Computing        |Both (Perplexity detailed)|P3      |Phase 4.0 |
|Decision Transformer          |Claude Research           |P3      |Phase 4.0 |

-----

**This is the merged architecture. Every technology has a clear home in the seven-layer stack, a specific phase in the build sequence, and a defined validation criterion. The system is designed so that no single technology is a dependency — remove any one layer and the rest still function. Add any one layer and the whole system gets smarter.**

**Build it layer by layer. Validate each before promoting. Let the data compound.**
