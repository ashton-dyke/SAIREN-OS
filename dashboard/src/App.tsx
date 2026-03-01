import { useCallback, useRef } from 'react';
import { Routes, Route } from 'react-router-dom';
import { Header } from './components/layout/Header';
import { AlertBanner } from './components/layout/AlertBanner';
import { LiveView } from './components/live/LiveView';
import { HourlyView } from './components/reports/HourlyView';
import { DailyView } from './components/reports/DailyView';
import { CriticalReports } from './components/reports/CriticalReports';
import { FeedbackView } from './components/feedback/FeedbackView';
import { useLiveData } from './hooks/useLiveData';
import type { LiveData } from './api/types';

const HISTORY_MAX = 120;

export default function App() {
  const { data, connected } = useLiveData();
  const historyRef = useRef<LiveData[]>([]);

  // Maintain ring buffer of live data for charts
  const pushHistory = useCallback((d: LiveData) => {
    historyRef.current = [...historyRef.current.slice(-(HISTORY_MAX - 1)), d];
  }, []);

  if (data && (historyRef.current.length === 0 || historyRef.current[historyRef.current.length - 1] !== data)) {
    pushHistory(data);
  }

  return (
    <div className="flex flex-col h-screen">
      <Header status={data?.status ?? null} connected={connected} />
      {data?.health && <AlertBanner health={data.health} />}
      <main className="flex-1 overflow-hidden">
        <Routes>
          <Route
            path="/"
            element={
              data ? (
                <LiveView data={data} history={historyRef.current} />
              ) : (
                <Loading />
              )
            }
          />
          <Route path="/hourly" element={<HourlyView />} />
          <Route path="/daily" element={<DailyView />} />
          <Route path="/reports" element={<CriticalReports />} />
          <Route path="/feedback" element={<FeedbackView />} />
        </Routes>
      </main>
    </div>
  );
}

function Loading() {
  return (
    <div className="flex items-center justify-center h-full">
      <div className="text-text-secondary text-sm animate-pulse">
        Connecting to SAIREN-OS...
      </div>
    </div>
  );
}
