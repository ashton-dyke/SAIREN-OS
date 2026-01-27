# TDS Guardian - Tactical Agent LLM System Extraction

This document contains extracted information about the Tactical Agent LLM system from the TDS Guardian codebase.

---

## 1. TACTICAL AGENT SYSTEM PROMPT

**File:** `src/llm/tactical_llm.rs:124-146`

**Note:** There is **no explicit separate system prompt**. The tactical LLM uses an inline classification prompt within the `classify()` function:

```rust
let prompt = format!(
    r#"Analyze these TDS-11SA top drive sensor metrics and determine if this is a real bearing fault or operational noise.

SENSOR DATA:
- Operational State: {:?}
- Kurtosis: {:.2} (normal < 3.0, warning > 4.0, critical > 8.0)
- Shock Factor: {:.2} (normal < 1.5, warning > 2.0)
- Anomaly Detected: {}
- Description: {}

RULES:
- High kurtosis during DRILLING = likely real fault
- High kurtosis during JARRING = operational noise (expected)
- Shock factor > 2.0 with drilling = bearing impact damage
- Consider the operational state context

Is this a REAL bearing fault? Answer only: YES or NO"#,
    metrics.state,
    metrics.kurtosis,
    metrics.shock_factor,
    metrics.is_anomaly,
    metrics.anomaly_description.as_deref().unwrap_or("None")
);
```

The mistral.rs backend also uses a generic system prompt in `src/llm/mistral_rs.rs:244-246`:
```rust
let system_prompt = "You are a vibration analysis expert for industrial drilling equipment. Reply concisely.";
```

---

## 2. TACTICAL PROMPT CONSTRUCTION CODE

**File:** `src/llm/tactical_llm.rs:119-147`

**Function:** `TacticalLLM::classify()`

```rust
/// Classify whether the detected anomaly is a real fault or noise
///
/// Returns `true` if this is a REAL bearing fault that should generate a ticket.
/// Returns `false` if this is operational noise that should be filtered out.
#[cfg(feature = "llm")]
pub async fn classify(&self, metrics: &TacticalMetrics) -> Result<bool> {
    let start = Instant::now();

    // Build classification prompt
    let prompt = format!(
        r#"Analyze these TDS-11SA top drive sensor metrics and determine if this is a real bearing fault or operational noise.

SENSOR DATA:
- Operational State: {:?}
- Kurtosis: {:.2} (normal < 3.0, warning > 4.0, critical > 8.0)
- Shock Factor: {:.2} (normal < 1.5, warning > 2.0)
- Anomaly Detected: {}
- Description: {}

RULES:
- High kurtosis during DRILLING = likely real fault
- High kurtosis during JARRING = operational noise (expected)
- Shock factor > 2.0 with drilling = bearing impact damage
- Consider the operational state context

Is this a REAL bearing fault? Answer only: YES or NO"#,
        metrics.state,
        metrics.kurtosis,
        metrics.shock_factor,
        metrics.is_anomaly,
        metrics.anomaly_description.as_deref().unwrap_or("None")
    );

    // Generate response with conservative parameters
    let response = self
        .backend
        .generate_with_params(&prompt, 10, 0.1)
        .await
        .context("Tactical inference failed")?;

    // ... rest of function
}
```

---

## 3. TACTICAL INPUT DATA STRUCTURE

**File:** `src/types.rs:106-139`

**Struct:** `TacticalMetrics`

```rust
/// Output from the tactical agent's basic physics calculations (Phase 2)
///
/// Contains fast metrics computed in < 15ms:
/// - Kurtosis (4th moment of vibration signal)
/// - Shock factor based on operational state
/// - BPFO frequency calculated from RPM
/// - Temperature delta from baseline
/// - Operational state classification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TacticalMetrics {
    /// Operational state (Drilling, Jarring, etc.)
    pub state: OperationalState,
    /// Kurtosis value (> 4.0 indicates impulsive behavior)
    pub kurtosis: f64,
    /// Shock factor multiplier based on state
    pub shock_factor: f64,
    /// Ball Pass Frequency Outer race (Hz) at current RPM
    pub bpfo_frequency: f64,
    /// Temperature delta from baseline (Celsius)
    pub temp_delta: f64,
    /// BPFO amplitude in g's (if available from spectrum)
    pub bpfo_amplitude: f64,
    /// Whether physics metrics indicate an anomaly
    pub is_anomaly: bool,
    /// Description of detected anomaly
    pub anomaly_description: Option<String>,
}
```

**Enum:** `OperationalState` at `src/types.rs:79-86`
```rust
pub enum OperationalState {
    Drilling,
    Circulating,
    Tripping,
    Idle,
}
```

---

## 4. TACTICAL LLM CALL CODE

**File:** `src/llm/tactical_llm.rs:148-153`

**The actual LLM call:**
```rust
// Generate response with conservative parameters
let response = self
    .backend
    .generate_with_params(&prompt, 10, 0.1)
    .await
    .context("Tactical inference failed")?;
```

**Parameters:**
- `prompt`: The formatted classification prompt (see #2)
- `max_tokens`: `10` (only needs YES/NO)
- `temperature`: `0.1` (low for deterministic output)

**Returns:** `Result<bool>` - `true` if LLM response contains "YES", `false` otherwise

**Response parsing at line 156:**
```rust
let is_fault = response.trim().to_uppercase().contains("YES");
```

**When called through the scheduler** (`src/llm/scheduler.rs:134-146`):
```rust
pub async fn infer_tactical(&self, prompt: String) -> Result<String> {
    let request = InferenceRequest {
        model_id: ModelId::Tactical,
        priority: Priority::Tactical,
        prompt,
        max_tokens: 40,  // Note: scheduler uses 40 tokens
        temperature: 0.2,
        // ...
    };
    // ...
}
```

---

## 5. TACTICAL OUTPUT DATA STRUCTURE

**The Tactical LLM itself returns:** `Result<bool>`
- `true` = REAL bearing fault (should generate ticket)
- `false` = operational noise (should be filtered out)

**The Tactical Agent (non-LLM) produces:** `VerificationTicket` at `src/types.rs:425-448`

```rust
/// Verification ticket generated by tactical agent for strategic validation
pub struct VerificationTicket {
    /// Unix timestamp when the ticket was created
    pub timestamp: u64,
    /// Suspected fault type (e.g., "BPFO bearing defect", "Elevated kurtosis")
    pub suspected_fault: String,
    /// The value that triggered this ticket (e.g., BPFO amplitude, kurtosis)
    pub trigger_value: f64,
    /// Tactical agent's confidence in this detection (0.0 to 1.0)
    pub confidence: f64,
    /// FFT snapshot at time of detection
    pub fft_snapshot: FftSnapshot,
    /// Operational state at time of detection
    pub operational_state: OperationalState,
    /// Initial severity assessment from tactical agent
    pub initial_severity: TicketSeverity,
    /// The tactical metrics that triggered this ticket
    pub metrics: TacticalMetrics,
    /// Sensor that triggered the ticket
    pub sensor_name: String,
}
```

**Stats output:** `TacticalLLMStats` at `src/llm/tactical_llm.rs:250-257`
```rust
pub struct TacticalLLMStats {
    pub inference_count: u64,
    pub avg_latency_ms: f64,
    pub true_faults: u64,
    pub noise_filtered: u64,
}
```

---

## 6. EXAMPLE TACTICAL LLM OUTPUTS

**NOT FOUND: Actual logged LLM text outputs**

The tactical LLM only outputs `YES` or `NO`. The logs show inference metrics but not raw text. Based on the code, expected outputs are:

**Healthy/Normal Case (returns `false`):**
```
NO
```

**Warning Case (returns `true`):**
```
YES
```

**Critical Case (returns `true`):**
```
YES
```

The LLM is prompted to answer **only** "YES" or "NO", so responses are minimal. The system uses `response.trim().to_uppercase().contains("YES")` to determine the result.

**Test cases from `src/llm/tactical_llm.rs:274-303`:**
```rust
// Normal case - expected: NO (is_fault = false)
let normal_metrics = TacticalMetrics {
    state: OperationalState::Drilling,
    kurtosis: 2.0,
    shock_factor: 1.0,
    is_anomaly: false,
    anomaly_description: None,
    // ...
};

// Fault case - expected: YES (is_fault = true)
let fault_metrics = TacticalMetrics {
    state: OperationalState::Drilling,
    kurtosis: 6.0,
    shock_factor: 3.0,
    is_anomaly: true,
    anomaly_description: Some("Bearing fault signature".to_string()),
    // ...
};
```

---

## 7. TACTICAL MODEL CONFIGURATION

**Model:** Qwen 2.5 1.5B Instruct (Q4_K_M quantized GGUF)

**File:** `src/llm/tactical_llm.rs:24-25`
```rust
/// Default model path for tactical model (1.5B)
const DEFAULT_TACTICAL_MODEL: &str = "models/qwen2.5-1.5b-instruct-q4_k_m.gguf";
```

**Alternative (from logs):** `models/deepseek-r1-distill-qwen-1.5b-q4.gguf`

**Generation Parameters:**

| Parameter | Direct Call | Via Scheduler |
|-----------|-------------|---------------|
| **max_tokens** | 10 | 40 |
| **temperature** | 0.1 | 0.2 |
| **top_k** | 50 | 50 |
| **top_p** | 0.9 | 0.9 |

From `src/llm/mistral_rs.rs:264-279`:
```rust
sampling_params: SamplingParams {
    temperature: Some(temperature),
    top_k: Some(50),
    top_p: Some(0.9),
    max_len: Some(max_tokens),
    // ...
}
```

**Target Latency:** 60ms (from `src/llm/tactical_llm.rs:3-4, 178-185`)
```rust
//! Target latency: 60ms

// Warn if latency exceeds target
if elapsed.as_millis() > 60 {
    tracing::warn!(
        latency_ms = elapsed.as_millis(),
        target_ms = 60,
        "Tactical inference exceeded target latency"
    );
}
```

**Actual Latency (from logs):** ~330-520ms per inference (exceeds 60ms target)

**Scheduler Config** (`src/llm/scheduler.rs:112-119`):
```rust
impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            tactical_deadline_guard_secs: 10,
            tactical_interval_secs: 60,
            channel_buffer_size: 100,
        }
    }
}
```

**Model Loading:** Uses mistral.rs with CUDA/GPU support
- Max sequence length: 4096
- Max batch size: 8
- PagedAttention enabled
- BF16/F16 dtype

---

## Summary Table

| Item | Location | Status |
|------|----------|--------|
| System Prompt | `src/llm/tactical_llm.rs:124-146` | Found (inline in prompt) |
| Prompt Construction | `src/llm/tactical_llm.rs:119-147` | Found |
| Input Structure | `src/types.rs:106-139` (TacticalMetrics) | Found |
| LLM Call | `src/llm/tactical_llm.rs:148-153` | Found |
| Output Structure | `Result<bool>` + TacticalLLMStats | Found |
| Example Outputs | N/A (only YES/NO) | Not logged |
| Model Config | Multiple files | Found |

---

## Architecture Note

The codebase has **two distinct "Tactical" components**:

1. **Tactical Agent** (`src/agents/tactical.rs`)
   - Performs physics calculations (kurtosis, shock factor, BPFO)
   - Threshold-based anomaly detection
   - Creates `VerificationTicket` when anomalies detected
   - Does NOT use LLM

2. **Tactical LLM** (`src/llm/tactical_llm.rs`)
   - Uses small 1.5B LLM to classify anomalies
   - Acts as a "smart filter" to reduce false positives
   - Returns simple YES/NO for fault classification
   - Called when `llm` feature is enabled

The Tactical LLM serves as an additional verification layer on top of the physics-based Tactical Agent.
