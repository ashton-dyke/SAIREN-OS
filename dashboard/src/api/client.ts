// Typed fetch wrapper for v2 API endpoints.

import type {
  ApiEnvelope,
  LiveData,
  HourlyReport,
  DailyReport,
  CriticalReport,
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
