import { useEffect, useState } from 'react';
import { fetchHourlyReports } from '../../api/client';
import type { HourlyReport } from '../../api/types';
import { ReportCard } from './ReportCard';

export function HourlyView() {
  const [reports, setReports] = useState<HourlyReport[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;
    const load = async () => {
      try {
        const data = await fetchHourlyReports(24);
        if (active) setReports(data);
      } catch {
        // ignore
      } finally {
        if (active) setLoading(false);
      }
    };
    load();
    const id = setInterval(load, 60_000);
    return () => {
      active = false;
      clearInterval(id);
    };
  }, []);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-48 text-text-secondary text-sm">
        Loading hourly reports...
      </div>
    );
  }

  if (reports.length === 0) {
    return (
      <div className="flex items-center justify-center h-48 text-text-secondary text-sm">
        No hourly reports available yet. Reports are generated after the first hour of operation.
      </div>
    );
  }

  return (
    <div className="p-4 space-y-3 overflow-y-auto h-full">
      <h2 className="text-text-secondary text-xs uppercase tracking-wider font-medium">
        Hourly Reports (last 24)
      </h2>
      <div className="grid gap-3 grid-cols-1 md:grid-cols-2 xl:grid-cols-3">
        {reports.map((r, i) => (
          <ReportCard
            key={i}
            score={r.health_score}
            severity={r.severity}
            diagnosis={r.diagnosis}
            action={r.action}
          />
        ))}
      </div>
    </div>
  );
}
