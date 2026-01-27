You are helping me implement dynamic thresholds (baseline learning + z-score anomaly detection) in my Rust-based TDS Guardian / RigWatch multi-agent monitoring system.

Goal: Replace hardcoded anomaly thresholds (for example kurtosis > 4.0, BPFO amplitude > 0.3g, max vibration > 0.3g) with dynamic, learned thresholds based on each machine’s baseline behavior.

This should make the system vendor-agnostic and robust across different bearings, motors, rigs, sensor placements, and natural baseline vibration differences.

Please explain clearly what needs to be built and where it plugs into the pipeline. Do not output full code; use structured pseudocode and clear implementation steps only.

Context: Current pipeline (10-phase)

    Phase 2: Calculate metrics (kurtosis, BPFO amplitude, temps, state, etc.)

    Phase 3: Tactical decision gate currently uses fixed thresholds and creates tickets

    Phase 4: History buffer stores last N packets

    Phase 5: Strategic verification uses physics and trends (L10 life, Miners rule, wear acceleration, trend consistency, confidence)

    Phase 6-10: Context lookup, LLM diagnosis, ensemble voting, report generation, dashboard updates

What I need you to do:

    Add baseline learning as a new step

    Implement a "Phase 1.5 Baseline Learning" concept that runs during the initial healthy period (e.g., first 30 minutes of operation, or any configured commissioning window).

    During baseline learning, for each metric we care about (at minimum vibration RMS, kurtosis, BPFO/BPFI amplitude, temperatures):

        Continuously accumulate samples.

        Compute baseline_mean and baseline_std for each metric.

    At the end of the baseline window, freeze/lock the baseline into a DynamicThresholds object per metric (and per equipment / per sensor where appropriate).

Important: Baseline learning should not create operational alerts. It is a commissioning step whose job is to establish "normal".

    Define the DynamicThresholds behavior

    For each metric, store:

        baseline_mean

        baseline_std

        warning_sigma (default 3.0)

        critical_sigma (default 5.0)

        locked flag / locked timestamp

        sample_count

    Provide functions/logic to compute:

        warning_threshold = baseline_mean + warning_sigma * baseline_std

        critical_threshold = baseline_mean + critical_sigma * baseline_std

        z_score = (current_value - baseline_mean) / baseline_std

    Ensure safe handling for baseline_std near zero (avoid divide-by-zero and false anomalies).

    Modify Phase 3 to use dynamic thresholds

    Replace the fixed-threshold checks with z-score checks:

        If z_score(metric) > warning_sigma -> create WARNING ticket

        If z_score(metric) > critical_sigma -> create CRITICAL ticket (or escalate)

    Keep the existing rule: if State is not Drilling, suppress tickets (unchanged).

    Add rate-of-change detection as a secondary trigger:

        Detect rapid increases relative to baseline (example: current_value > baseline_mean + some multiple, or delta exceeds 0.5 sigma between packets).

        Require the trend to be increasing over the last few packets (use history buffer) to avoid one-sample spikes.

    Use Phase 4 history buffer to support trends

    Continue storing last N packets.

    In addition to raw metrics, store derived info that helps verification:

        Rolling z-scores

        Trend slope estimates (optional)

    Make sure this remains lightweight (tactical loop must stay fast).

    Strengthen Phase 5 verification using baseline context

    When Phase 3 creates a ticket based on z-score, Phase 5 should confirm or reject using:

        Consistency: was z-score > 3 consistently for multiple packets (not a fluke)?

        Acceleration: is the trend rising (slope positive)?

        Physics agreement: do physics indicators (L10 life, Miners rule, wear acceleration, etc.) support the anomaly?

    Output one of: REJECTED, UNCERTAIN, CONFIRMED.

    Only CONFIRMED should proceed to later phases and dashboard alerts (same as current behavior).

    Add baseline contamination safeguards
    Baseline can be wrong if the machine starts already damaged. Add a policy for baseline lock:

    Require a minimum sample count before locking.

    Detect contamination during baseline learning:

        If too many samples are outliers (for example > 5% beyond 3 sigma), flag baseline as contaminated and do not lock automatically.

    Provide a fallback strategy:

        Continue learning longer until stable

        Or require an operator manual confirmation to lock baseline

    Explain a practical approach that avoids false confidence.

    Persist thresholds so restarts do not reset commissioning

    After baseline is locked, persist DynamicThresholds to disk (e.g., JSON per rig or per equipment).

    On restart, load thresholds and continue monitoring without re-learning (unless operator requests re-commissioning).

    Provide a safe versioning approach so future schema changes do not break old saved baselines.

    Update reporting/dashboard output for transparency

    In Phase 9 strategic report, include baseline stats for relevant metrics so users can see:

        Baseline mean and std

        Warning and critical thresholds

        Current z-score

        Sample count and locked status

    Keep it concise, but make it auditable.

    Testing plan
    Give me a clear test plan with expected outcomes:

    Clean baseline: locks after 30 minutes; no tickets during baseline; faults detected after.

    Contaminated baseline: baseline does not lock; system requests extended learning or manual check.

    Progressive failure sim: earlier detection than fixed thresholds (z-score triggers before absolute thresholds).

    Multi-equipment: each equipment learns its own baseline independently (no shared thresholds).

Deliverable expected from you (Claude):

    A step-by-step implementation guide (no full code)

    Pseudocode for the key logic (baseline accumulation, lock decision, Phase 3 gate using z-scores, Phase 5 verification checks)

    Any design pitfalls to avoid (std near zero, baseline drift, speed/load changes)

    Recommended default parameter values (learning duration, min samples, sigmas, contamination threshold)
