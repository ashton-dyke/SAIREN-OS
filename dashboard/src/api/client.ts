// Typed fetch wrapper for v2 API endpoints.

import type {
  ApiEnvelope,
  LiveData,
  HourlyReport,
  DailyReport,
  CriticalReport,
  FeedbackOutcome,
  CategoryStats,
  ThresholdSuggestion,
  LookaheadStatus,
} from './types';

const BASE = '/api/v2';

async function fetchJson<T>(url: string): Promise<T> {
  const resp = await fetch(url);
  if (!resp.ok) {
    throw new Error(`API ${resp.status}: ${resp.statusText}`);
  }
  const envelope: ApiEnvelope<T> = await resp.json();
  return envelope.data;
}

export async function fetchLiveData(): Promise<LiveData> {
  return fetchJson<LiveData>(`${BASE}/live`);
}

export async function fetchHourlyReports(limit = 24): Promise<HourlyReport[]> {
  return fetchJson<HourlyReport[]>(`${BASE}/reports/hourly?limit=${limit}`);
}

export async function fetchDailyReports(limit = 7): Promise<DailyReport[]> {
  return fetchJson<DailyReport[]>(`${BASE}/reports/daily?limit=${limit}`);
}

export async function fetchCriticalReports(limit = 50): Promise<CriticalReport[]> {
  return fetchJson<CriticalReport[]>(`${BASE}/reports/critical?limit=${limit}`);
}

export async function submitFeedback(
  timestamp: number,
  outcome: FeedbackOutcome,
  submittedBy = 'operator',
  notes = '',
): Promise<void> {
  const resp = await fetch(`${BASE}/advisory/feedback/${timestamp}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ outcome, submitted_by: submittedBy, notes }),
  });
  if (!resp.ok) {
    throw new Error(`API ${resp.status}: ${resp.statusText}`);
  }
}

export async function fetchFeedbackStats(): Promise<CategoryStats[]> {
  return fetchJson<CategoryStats[]>(`${BASE}/advisory/feedback/stats`);
}

export async function fetchConfigSuggestions(): Promise<ThresholdSuggestion[]> {
  return fetchJson<ThresholdSuggestion[]>(`${BASE}/config/suggestions`);
}

export async function fetchLookaheadStatus(): Promise<LookaheadStatus> {
  return fetchJson<LookaheadStatus>(`${BASE}/lookahead/status`);
}
