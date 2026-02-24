import { useEffect, useRef, useState } from 'react';

/** Generic polling hook: calls `fetcher` every `intervalMs` and returns the latest value. */
export function usePolling<T>(
  fetcher: () => Promise<T>,
  intervalMs: number,
): { data: T | null; error: string | null; connected: boolean } {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [connected, setConnected] = useState(true);
  const savedFetcher = useRef(fetcher);
  savedFetcher.current = fetcher;

  useEffect(() => {
    let active = true;

    const poll = async () => {
      try {
        const result = await savedFetcher.current();
        if (active) {
          setData(result);
          setError(null);
          setConnected(true);
        }
      } catch (e) {
        if (active) {
          setError(e instanceof Error ? e.message : String(e));
          setConnected(false);
        }
      }
    };

    // Immediate first poll
    poll();
    const id = setInterval(poll, intervalMs);
    return () => {
      active = false;
      clearInterval(id);
    };
  }, [intervalMs]);

  return { data, error, connected };
}
