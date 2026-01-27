# SAIREN-OS Campaign System

## Overview

The Campaign system enables SAIREN-OS to operate in different operational modes with context-aware thresholds, LLM prompts, and advisory generation. This allows the same system to provide intelligent advisories for both **Production Drilling** and **Plug & Abandonment (P&A)** operations.

## Supported Campaigns

### Production Drilling
- Focus: ROP optimization, MSE efficiency, drilling performance
- Thresholds: Standard flow imbalance (10 gpm warning, 20 gpm critical)
- Specialist weights: Balanced (MSE 25%, Hydraulic 25%, Well Control 30%, Formation 20%)

### Plug & Abandonment (P&A)
- Focus: Cement integrity, pressure testing, barrier verification
- Thresholds: Tighter flow control (5 gpm warning, 10 gpm critical)
- Specialist weights: Well Control priority (MSE 15%, Hydraulic 35%, Well Control 40%, Formation 10%)
- Additional monitoring: Cement returns, pressure hold, barrier integrity

## Architecture

### Data Types (`src/types.rs`)

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Campaign {
    #[default]
    Production,
    PlugAbandonment,
}

pub struct CampaignThresholds {
    // MSE/Efficiency thresholds
    pub mse_efficiency_warning: f64,
    pub mse_efficiency_poor: f64,

    // Pressure thresholds (critical for P&A)
    pub pressure_test_tolerance: f64,
    pub cement_pressure_hold: f64,
    pub barrier_pressure_margin: f64,

    // Flow thresholds (tighter for P&A)
    pub flow_imbalance_warning: f64,
    pub flow_imbalance_critical: f64,

    // Specialist voting weights
    pub weight_mse: f64,
    pub weight_hydraulic: f64,
    pub weight_well_control: f64,
    pub weight_formation: f64,

    // P&A specific
    pub cement_returns_expected: bool,
    pub plug_depth_tolerance: f64,
}
```

### Pipeline State (`src/pipeline/processor.rs`)

The `AppState` struct includes campaign tracking:

```rust
pub struct AppState {
    // ... other fields ...
    pub campaign: Campaign,
    pub campaign_thresholds: CampaignThresholds,
}

impl AppState {
    pub fn set_campaign(&mut self, campaign: Campaign) {
        self.campaign = campaign;
        self.campaign_thresholds = CampaignThresholds::for_campaign(campaign);
        tracing::info!(campaign = %campaign.display_name(), "Campaign switched");
    }
}
```

### LLM Prompts (`src/llm/strategic_llm.rs`)

Campaign-specific prompts guide the LLM to provide contextually appropriate advisories:

**Production Prompt Focus:**
- MSE efficiency optimization
- ROP improvement opportunities
- Formation change detection
- Standard well control monitoring

**P&A Prompt Focus:**
- Cement placement monitoring
- Pressure test evaluation
- Barrier integrity verification
- Fluid migration detection

## API Endpoints

### Get Current Campaign
```http
GET /api/v1/campaign
```

Response:
```json
{
    "campaign": "Production",
    "display_name": "Production Drilling",
    "thresholds": {
        "flow_imbalance_warning": 10.0,
        "flow_imbalance_critical": 20.0,
        "weight_well_control": 0.30
    }
}
```

### Switch Campaign
```http
POST /api/v1/campaign
Content-Type: application/json

{
    "campaign": "PlugAbandonment"
}
```

Response:
```json
{
    "success": true,
    "campaign": "PlugAbandonment",
    "message": "Campaign switched to Plug & Abandonment"
}
```

## Dashboard UI

The dashboard header includes a campaign selector:

- **Dropdown**: Switch between "Production Drilling" and "Plug & Abandonment"
- **Badge**: Visual indicator showing current campaign (PROD = green, P&A = yellow)
- **Real-time sync**: Campaign state is fetched on page load and updated on switch

### JavaScript API

```javascript
// Switch campaign
async function switchCampaign(campaign) {
    const response = await fetch('/api/v1/campaign', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ campaign: campaign })
    });
    // Updates badge and dropdown on success
}

// Fetch current campaign on page load
async function fetchCurrentCampaign() {
    const response = await fetch('/api/v1/campaign');
    const data = await response.json();
    // Sync UI state
}
```

## WITS Simulator Support

The WITS simulator (`wits_simulator.py`) supports both campaign modes:

```bash
# Production drilling (default)
python wits_simulator.py --stdout

# Plug & Abandonment
python wits_simulator.py --stdout --campaign pa
```

### P&A Simulation States

The simulator includes P&A-specific operational states:

| State | Description |
|-------|-------------|
| `CIRCULATING` | Initial circulation before P&A operations |
| `DISPLACING` | Displacing wellbore fluids before cement |
| `CEMENTING` | Pumping cement (reduced flow, higher SPP) |
| `SETTING_PLUG` | Waiting for cement to set |
| `PRESSURE_TESTING` | Testing barrier integrity |

### P&A Simulation Parameters

During P&A operations, the simulator adjusts parameters:

- **Cementing**: Flow rate ~300 L/min, SPP ~2000 kPa, flow_out < flow_in (cement stays in hole)
- **Pressure Testing**: Static well, casing_pressure applied, monitors for leaks
- **Random Events**: 1% chance per tick of minor pressure leak during testing

## Usage Examples

### Command Line

```bash
# Start simulator in P&A mode
python wits_simulator.py --stdout --campaign pa --interval 0.1 | \
    cargo run --release -- --stdin

# Switch campaign via API during operation
curl -X POST http://localhost:8080/api/v1/campaign \
    -H "Content-Type: application/json" \
    -d '{"campaign":"PlugAbandonment"}'
```

### Programmatic (Rust)

```rust
// Get current campaign from AppState
let state = app_state.read().await;
let campaign = state.campaign;
let thresholds = &state.campaign_thresholds;

// Switch campaign
{
    let mut state = app_state.write().await;
    state.set_campaign(Campaign::PlugAbandonment);
}

// Process packet with campaign context
let advisory = coordinator
    .process_packet(&packet, campaign)
    .await;
```

## Threshold Comparison

| Parameter | Production | P&A |
|-----------|------------|-----|
| Flow Imbalance Warning | 10.0 gpm | 5.0 gpm |
| Flow Imbalance Critical | 20.0 gpm | 10.0 gpm |
| MSE Weight | 25% | 15% |
| Hydraulic Weight | 25% | 35% |
| Well Control Weight | 30% | 40% |
| Formation Weight | 20% | 10% |
| Pressure Test Tolerance | N/A | 50 psi |
| Cement Pressure Hold | N/A | 500 psi |

## Advisory Output Differences

### Production Advisory Example
```
TYPE: OPTIMIZATION
PRIORITY: MEDIUM
RECOMMENDATION: Consider adjusting WOB to 28 klbs to improve MSE efficiency. Current deviation: 25%
EXPECTED BENEFIT: Potential 15% ROP improvement
```

### P&A Advisory Example
```
TYPE: BARRIER_VERIFICATION
PRIORITY: HIGH
RECOMMENDATION: Pressure test shows stable at 1000 psi for 15 minutes. Barrier integrity verified.
EXPECTED BENEFIT: Regulatory compliance, well integrity confirmed
```

## Files Modified

| File | Changes |
|------|---------|
| `src/types.rs` | Added `Campaign` enum, `CampaignThresholds` struct |
| `src/pipeline/processor.rs` | Added campaign fields to `AppState`, `set_campaign()` method |
| `src/pipeline/coordinator.rs` | Updated `process_packet()` to accept campaign |
| `src/llm/strategic_llm.rs` | Added P&A prompt, campaign-aware advisory generation |
| `src/api/handlers.rs` | Added `get_campaign`, `set_campaign` handlers |
| `src/api/routes.rs` | Added `/campaign` routes |
| `static/index.html` | Added campaign selector UI |
| `wits_simulator.py` | Added `--campaign` flag, P&A states and physics |

## Future Enhancements

1. **Workover Campaign**: Add support for workover operations
2. **Campaign History**: Track campaign switches with timestamps
3. **Custom Thresholds**: Allow per-well threshold overrides
4. **Campaign Templates**: Pre-defined threshold sets for specific well types
5. **Regulatory Compliance**: Auto-generate P&A documentation
