import { useEffect, useState } from 'react';
import { fetchDailyReports } from '../../api/client';
import type { DailyReport } from '../../api/types';
import { ReportCard } from './ReportCard';

export function DailyView() {
  const [reports, setReports] = useState<DailyReport[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;
    const load = async () => {
      try {
        const data = await fetchDailyReports(7);
        if (active) setReports(data);
      } catch {
        // ignore
      } finally {
        if (active) setLoading(false);
      }
    };
    load();
    const id = setInterval(load, 300_000); // 5 min
    return () => {
      active = false;
      clearInterval(id);
    };
  }, []);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-48 text-text-secondary text-sm">
        Loading daily reports...
      </div>
    );
  }

  if (reports.length === 0) {
    return (
      <div className="flex items-center justify-center h-48 text-text-secondary text-sm">
        No daily reports available yet. Reports are generated after the first 24 hours.
      </div>
    );
  }

  return (
    <div className="p-4 space-y-3 overflow-y-auto h-full">
      <h2 className="text-text-secondary text-xs uppercase tracking-wider font-medium">
        Daily Reports (last 7)
      </h2>
      <div className="grid gap-3 grid-cols-1 md:grid-cols-2">
        {reports.map((r, i) => (
          <div key={i} className="space-y-2">
            <ReportCard
              score={r.health_score}
              severity={r.severity}
              diagnosis={r.diagnosis}
              action={r.action}
            />
            {r.details && (
              <div className="bg-bg-card border border-border rounded-lg p-3 text-xs space-y-1 ml-2">
                <div>
                  <span className="text-text-secondary">Trend: </span>
                  <span>{r.details.trend}</span>
                </div>
                <div>
                  <span className="text-text-secondary">Top drivers: </span>
                  <span>{r.details.top_drivers}</span>
                </div>
                <div>
                  <span className="text-text-secondary">Confidence: </span>
                  <span>{r.details.confidence}</span>
                </div>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
