// TypeScript interfaces matching Rust v2 response types.

export interface ApiEnvelope<T> {
  data: T;
  meta: { timestamp: string; version: string };
}

export interface ApiError {
  error: { code: string; message: string };
  meta: { timestamp: string; version: string };
}

// /api/v2/live
export interface LiveData {
  health: HealthV2;
  status: StatusV2;
  drilling: DrillingV2;
  verification: VerificationV2;
  baseline_summary: BaselineSummaryV2;
  ml_latest: MLSummaryV2 | null;
  shift: ShiftSummaryV2;
}

export interface HealthV2 {
  overall_score: number;
  severity: string;
  diagnosis: string;
  recommendation: string;
  confidence: number;
  timestamp: string;
  components: {
    pipeline: boolean;
    baseline: string;
    ml: boolean;
    fleet: boolean;
    storage: boolean;
  };
}

export interface StatusV2 {
  system_status: string;
  rig_state: string;
  operation: string;
  operation_code: string;
  uptime_secs: number;
  total_analyses: number;
  packets_processed: number;
  campaign: string;
  campaign_code: string;
}

export interface DrillingV2 {
  bit_depth: number;
  rop: number;
  wob: number;
  rpm: number;
  torque: number;
  spp: number;
  hook_load: number;
  flow_in: number;
  flow_out: number;
  flow_balance: number;
  pit_volume: number;
  mud_weight: number;
  ecd: number;
  ecd_margin: number;
  gas_units: number;
  mse: number;
  mse_efficiency: number;
  mse_baseline: number;
  d_exponent: number;
  dxc: number;
  formation_type: string;
  formation_change: boolean;
  trend: string;
  votes: SpecialistVotesV2 | null;
}

export interface SpecialistVotesV2 {
  mse: string;
  hydraulic: string;
  well_control: string;
  formation: string;
}

export interface VerificationV2 {
  has_verification: boolean;
  status: string | null;
  suspected_fault: string | null;
  reasoning: string | null;
  final_severity: string | null;
  verified_count: number;
  rejected_count: number;
}

export interface BaselineSummaryV2 {
  overall_status: string;
  locked_count: number;
  learning_count: number;
  total_metrics: number;
}

export interface MLSummaryV2 {
  has_data: boolean;
  timestamp: number | null;
  confidence: string | null;
  composite_score: number | null;
  best_wob: number | null;
  best_rpm: number | null;
  best_flow: number | null;
}

export interface ShiftSummaryV2 {
  duration_hours: number;
  packets_processed: number;
  tickets_created: number;
  tickets_verified: number;
  tickets_rejected: number;
  peak_severity: string;
  avg_mse_efficiency: number | null;
  acknowledgments: number;
}

// Reports
export interface HourlyReport {
  health_score: number;
  severity: string;
  diagnosis: string;
  action: string;
  raw: string;
}

export interface DailyReport {
  health_score: number;
  severity: string;
  diagnosis: string;
  action: string;
  details: {
    trend: string;
    top_drivers: string;
    confidence: string;
    next_check: string;
  } | null;
  raw: string;
}

export interface CriticalReport {
  report_id: string;
  timestamp: number;
  timestamp_formatted: string;
  efficiency_score: number;
  risk_level: string;
  recommendation: string;
  expected_benefit: string;
  reasoning: string;
  trigger_parameter: string;
  trigger_value: number;
  threshold_value: number;
  drilling_params: {
    bit_depth: number;
    rop: number;
    wob: number;
    rpm: number;
    torque: number;
    flow_in: number;
    flow_out: number;
    flow_balance: number;
    spp: number;
    mud_weight: number;
    ecd: number;
    pit_volume: number;
    mse: number;
    mse_efficiency: number;
  };
  votes_summary: string[];
  digital_signature: string;
  signature_timestamp: string;
}
