You are an expert in vibration signal synthesis for rotating machinery (bearings, motors, gearboxes).

Your task: design and implement the most realistic possible time-domain simulation of a failing AC motor nondrive-end bearing on a TDS4-S top drive, using a physics-inspired model rather than simple sine waves.

Do NOT just add clean sines at BPFO/BPFI. Build a signal that looks like real accelerometer data from a rig.
1. Machine and bearing to model

Model this specific bearing and operating condition:

    Machine: AC motor, nondrive-end bearing, NOV TDS4-S top drive

    Part: 108235-2 -> SKF QJ316 (four-point contact angular contact ball bearing)

    Geometry (for kinematics):

        Bore d = 80 mm

        Outer D = 170 mm

        Width B = 39 mm

        Contact angle phi = 35 degrees

        Number of balls N = 17

        Ball diameter Bd = 20 mm

        Pitch diameter Pd = 125 mm

    Operating speed:

        Motor speed: 1800 RPM = 30 Hz shaft frequency

Use the standard bearing fault frequency formulas with correct units:

    Shaft rotational frequency:
    f_r = RPM / 60 = 1800 / 60 = 30 Hz

    BPFO (outer race fault):
    BPFO = (N / 2) * f_r * (1 - (Bd / Pd) * cos(phi))

    BPFI (inner race fault):
    BPFI = (N / 2) * f_r * (1 + (Bd / Pd) * cos(phi))

Plug in N = 17, Bd = 20 mm, Pd = 125 mm, phi = 35 degrees and compute BPFO and BPFI in Hz, not CPM. Use these as the base fault frequencies.
2. Simulation goals and constraints

Design a 1-hour simulation of this motor bearing, which will be run at accelerated time (e.g., 10x, 100x), but the synthetic signal itself should be physically realistic.

Use four phases:

    Healthy Baseline (0-15 min)

    Normal Drilling / Loaded but Healthy (15-30 min)

    Progressive Failure (Outer race fault developing, 30-50 min)

    Critical Failure (Outer race severely damaged, 50-60 min)

The key constraint:
Do NOT model the fault as a pure sine at BPFO.
Model impacts that excite a structural resonance, plus all the non-idealities seen in real data: sidebands, jitter, broadband HF noise, nonlinear growth, etc.

Assume:

    Sampling rate: pick a realistic value, e.g. 10-20 kHz (justify your choice)

    Sensor: accelerometer mounted on motor housing near nondrive-end bearing

    Output: time series of acceleration in g, plus optional "ground truth" channels (phase, severity, etc.)

3. Signal model requirements (what the code must do)

Design the signal generation around these physics-inspired elements:
3.1 Impulsive excitation at BPFO

    Treat this as an outer race defect:

        Once per BPFO cycle, a rolling element strikes the spall.

    Instead of sin(2 * pi * BPFO * t), generate:

        A train of impulses at BPFO period, each one exciting a structural resonance of the motor housing/top drive structure.

Implement conceptually:

    Compute ideal impact times: t_n,ideal = n / BPFO

    Add timing jitter to each impact to mimic micro-slip and cage/ball irregularities:

        t_n = t_n,ideal + epsilon_n, where epsilon_n is small random noise (tunable std dev)

    For each impact, generate a decaying sinusoid at a chosen resonance frequency:

        Choose a realistic resonance frequency, e.g. 5-10 kHz, based on typical motor/top drive structures.

        Use an exponentially damped response:
        h(tau) = A * exp(-tau / tau_decay) * sin(2 * pi * f_res * tau), tau >= 0

The sum over all impacts gives the core fault signal.
3.2 Amplitude modulation and sidebands

Real bearings show sidebands around BPFO spaced at shaft frequency due to load zone and rotation.

    Modulate impact amplitude by shaft rotation:

        Use a slow modulation term linked to shaft frequency f_r (30 Hz) to represent the load zone effect.

        Conceptually: multiply the fault signal by (1 + m * sin(2 * pi * f_r * t)), with severity-dependent modulation depth m(severity).

This should naturally produce BPFO +/- f_r sidebands and higher-order sidebands in the FFT.
3.3 Nonlinear fault severity progression

Introduce a severity parameter s in that increases from 0 (no fault) at 30 min to 1 (fully developed) at 60 min:​

    From 30-50 min (Phase 3), ramp s from 0 -> 0.8

    From 50-60 min (Phase 4), ramp s from 0.8 -> 1.0

Use nonlinear growth, not linear:

    Impact amplitude should follow an exponential or power law in s, e.g. A(s) ~ s^p with p > 1

    This matches reality: early damage is almost invisible, then the signal "takes off" later.

Define explicit mappings (for you to implement):

    Fault impact amplitude A_fault(s): increasing, convex function of s

    Jitter standard deviation sigma_jitter(s): slowly increases with s to broaden spectral lines

    Modulation depth m(s): increases with s (more pronounced sidebands at high severity)

3.4 Baseline vibration and noise

On top of the fault signal, simulate:

    Baseline motor vibration:

        Dominated by shaft frequency (30 Hz) and maybe 2x, 3x harmonics.

        Low amplitude in Phase 1-2; slowly increasing if you want to model general wear/load.

    Broadband Gaussian noise:

        Always present; amplitude roughly stable across all phases.

        Ensures weak early faults are partially buried, like in real data.

3.5 High-frequency broadband growth

Add an HF broadband component that grows with severity:

    Represents friction, micro-spalling, and plastic deformation in the damaged raceway.

    Generate wideband noise and high-pass filter it (e.g. above 5 kHz).

    Scale its amplitude with a fast-growing function of s (e.g. s^1.5 or s^2).

This gives:

    Quiet HF band in Phases 1-2

    Noticeable HF hiss in Phase 3

    Strong HF noise in Phase 4

3.6 Harmonics and occasional spikes (critical phase)

In Phase 4 (Critical Failure):

    The impulsive model + resonance will naturally produce harmonics and broad spectral content around BPFO.

    On top of this, add:

        Occasional large spikes (rare, high-amplitude impacts) to simulate pieces of material breaking loose:

            Low probability per unit time (e.g. bursts a few times per minute)

            Amplitude higher than the typical impact envelope

        Make sure these spikes look like short, sharp events, not long sinusoids.

4. Phase definitions and parameter evolution

Implement four distinct regimes with smooth transitions:

    Phase 1: Healthy Baseline (0-15 min)

        No BPFO/BPFI impulses.

        Low baseline vibration (e.g., 0.05-0.08 g RMS).

        Low Gaussian noise.

        Temperature channels (if modeled) stable and low.

    Phase 2: Normal Drilling / Loaded Healthy (15-30 min)

        Still no fault impulses.

        Slightly higher baseline vibration due to drilling load.

        Slightly increased RPM variation (plus/minus a few percent around 1800 RPM).

        Noise a bit higher, but still no distinct BPFO peak.

    Phase 3: Progressive Outer Race Failure (30-50 min)

        Start ramping severity s from 0 to about 0.8.

        Introduce impulsive BPFO signal as described:

            Impacts at BPFO with jitter.

            Resonant ringing at f_res.

            Amplitude A_fault(s) increasing nonlinearly.

        Start adding amplitude modulation at shaft frequency to create sidebands.

        HF broadband noise starts to grow with s.

        The FFT should show:

            Emerging BPFO peak.

            Weak sidebands around BPFO +/- f_r.

            Slight broadening of peaks from jitter.

    Phase 4: Critical Failure (50-60 min)

        s -> 1.0, fully developed fault.

        Strong impacts at BPFO, large A_fault(1.0).

        Strong amplitude modulation (distinct sidebands).

        High HF broadband content.

        Occasional very large spikes (rare extreme impacts).

        FFT should show:

            Dominant BPFO and its harmonics.

            Clear sidebands at multiples of shaft frequency.

            Wide spectral smearing and rich HF content.

5. Outputs and diagnostics

The final simulation should output:

    Time-series acceleration signal in g.

    Optionally: metadata or ground truth per sample or per block:

        Current phase (1-4)

        Severity s

        Instantaneous shaft speed

        BPFO frequency at that instant

Ensure that when this signal is:

    Passed through an FFT or envelope spectrum,

    Analysed in time-frequency view,

it exhibits the characteristic patterns expected for an outer race defect on this QJ316 motor bearing at 1800 RPM, evolving from barely detectable to catastrophic over the 1-hour window.
6. Implementation focus areas

Focus on physical realism over simplicity. Use clear comments and structure so another engineer can tune:

    Resonance frequency and decay time

    Jitter level

    Modulation depth

    Noise levels

    Severity curve shape (exponent p, coefficients)

Do not just approximate with a few sinusoids: build it from impacts + structural response + modulation + noise.

When complete, the synthetic data should:

    Show minimal fault signature in Phases 1-2 (buried in baseline noise).

    Show growing impulsive peaks and sidebands in Phase 3.

    Show dominant BPFO and rich spectral content in Phase 4.

    Be suitable for training and testing an LLM-based diagnostic system that learns to detect and classify bearing faults from vibration FFT or envelope spectra.
