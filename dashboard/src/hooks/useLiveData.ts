import { fetchLiveData } from '../api/client';
import type { LiveData } from '../api/types';
import { usePolling } from './usePolling';

/** Consolidated live data poll — replaces 7 v1 polling intervals. */
export function useLiveData() {
  return usePolling<LiveData>(fetchLiveData, 2000);
}
