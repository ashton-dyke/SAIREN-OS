import type { LiveData } from '../../api/types';
import { HealthGauge } from './HealthGauge';
import { DrillingParams } from './DrillingParams';
import { WellControl } from './WellControl';
import { LookAheadPanel } from './LookAheadPanel';
import { SpecialistVotes } from './SpecialistVotes';
import { BaselineProgress } from './BaselineProgress';
import { FlowBalanceChart } from '../charts/FlowBalanceChart';
import { MSEChart } from '../charts/MSEChart';

interface Props {
  data: LiveData;
  history: LiveData[];
}

export function LiveView({ data, history }: Props) {
  const sev = data.health.severity.toLowerCase();
  const isCritical = sev === 'critical' || sev === 'high';

  return (
    <div className="p-4 space-y-4 overflow-y-auto h-full">
      {/* Top row: health gauge + status */}
      <div className="flex gap-4 items-start">
        <HealthGauge
          score={data.health.overall_score}
          severity={data.health.severity}
        />
        <div className="flex-1 min-w-0 space-y-1">
          <div className="flex items-center gap-2 text-xs text-text-secondary">
            <span>{data.status.rig_state}</span>
            <span className="text-text-muted">|</span>
            <span>{data.status.operation}</span>
            <span className="text-text-muted">|</span>
            <span>Uptime: {formatUptime(data.status.uptime_secs)}</span>
          </div>
          {isCritical && (
            <div className="bg-accent-red/10 border border-accent-red/30 rounded p-2 text-sm">
              <span className="text-accent-red font-bold">
                {data.health.recommendation}
              </span>
            </div>
          )}
          {data.shift.avg_mse_efficiency != null && (
            <div className="text-xs text-text-secondary">
              Shift avg MSE efficiency: {data.shift.avg_mse_efficiency.toFixed(1)}%
              {' | '}Tickets: {data.shift.tickets_created} created,{' '}
              {data.shift.tickets_verified} verified
            </div>
          )}
        </div>
      </div>

      {/* Drilling parameters */}
      <DrillingParams drilling={data.drilling} />

      {/* Well control */}
      <WellControl drilling={data.drilling} />

      {/* Formation lookahead */}
      <LookAheadPanel />

      {/* Charts row */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <FlowBalanceChart history={history} />
        <MSEChart history={history} />
      </div>

      {/* Bottom row: votes + baseline + ML */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <SpecialistVotes votes={data.drilling.votes} />
        <BaselineProgress baseline={data.baseline_summary} />
        {data.ml_latest && data.ml_latest.has_data && (
          <div className="space-y-2">
            <h3 className="text-text-secondary text-xs uppercase tracking-wider font-medium">
              ML Optimal
            </h3>
            <div className="bg-bg-card rounded-lg p-3 border border-border text-xs space-y-1">
              <div className="flex justify-between">
                <span className="text-text-secondary">Best WOB</span>
                <span>{data.ml_latest.best_wob?.toFixed(1) ?? '--'} klbs</span>
              </div>
              <div className="flex justify-between">
                <span className="text-text-secondary">Best RPM</span>
                <span>{data.ml_latest.best_rpm?.toFixed(0) ?? '--'}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-text-secondary">Best Flow</span>
                <span>{data.ml_latest.best_flow?.toFixed(0) ?? '--'} gpm</span>
              </div>
              <div className="flex justify-between">
                <span className="text-text-secondary">Confidence</span>
                <span>{data.ml_latest.confidence ?? '--'}</span>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function formatUptime(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return `${h}h ${m}m`;
}
