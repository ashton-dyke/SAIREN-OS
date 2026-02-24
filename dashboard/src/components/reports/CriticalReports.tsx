import { useEffect, useState } from 'react';
import { fetchCriticalReports } from '../../api/client';
import type { CriticalReport } from '../../api/types';
import { SeverityBadge } from '../common/SeverityBadge';
import { severityColor } from '../../theme/colors';

export function CriticalReports() {
  const [reports, setReports] = useState<CriticalReport[]>([]);
  const [selected, setSelected] = useState<CriticalReport | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;
    const load = async () => {
      try {
        const data = await fetchCriticalReports(50);
        if (active) {
          setReports(data);
          if (data.length > 0 && !selected) setSelected(data[0]);
        }
      } catch {
        // ignore
      } finally {
        if (active) setLoading(false);
      }
    };
    load();
    const id = setInterval(load, 30_000);
    return () => {
      active = false;
      clearInterval(id);
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  if (loading) {
    return (
      <div className="flex items-center justify-center h-48 text-text-secondary text-sm">
        Loading critical reports...
      </div>
    );
  }

  if (reports.length === 0) {
    return (
      <div className="flex items-center justify-center h-48 text-text-secondary text-sm">
        No critical reports. System operating normally.
      </div>
    );
  }

  return (
    <div className="flex h-full overflow-hidden">
      {/* Left: report list */}
      <div className="w-80 shrink-0 border-r border-border overflow-y-auto">
        {reports.map((r) => (
          <button
            key={r.report_id}
            onClick={() => setSelected(r)}
            className={`w-full text-left px-3 py-2 border-b border-border hover:bg-bg-hover transition-colors ${
              selected?.report_id === r.report_id ? 'bg-bg-hover' : ''
            }`}
          >
            <div className="flex items-center justify-between mb-1">
              <span className="text-xs font-mono text-text-muted">
                {r.report_id}
              </span>
              <SeverityBadge severity={r.risk_level} />
            </div>
            <p className="text-xs text-text-primary line-clamp-2 leading-snug">
              {r.recommendation}
            </p>
            <p className="text-[10px] text-text-muted mt-1">
              {r.timestamp_formatted}
            </p>
          </button>
        ))}
      </div>

      {/* Right: detail pane */}
      <div className="flex-1 overflow-y-auto p-4">
        {selected ? (
          <ReportDetail report={selected} />
        ) : (
          <div className="text-text-secondary text-sm">Select a report</div>
        )}
      </div>
    </div>
  );
}

function ReportDetail({ report }: { report: CriticalReport }) {
  const color = severityColor(report.risk_level);
  const p = report.drilling_params;

  return (
    <div className="space-y-4 max-w-2xl">
      <div>
        <div className="flex items-center gap-3 mb-2">
          <SeverityBadge severity={report.risk_level} />
          <span className="text-text-muted text-xs font-mono">
            {report.report_id}
          </span>
          <span className="text-text-muted text-xs">
            Efficiency: {report.efficiency_score}%
          </span>
        </div>
        <h2
          className="text-sm font-medium leading-snug"
          style={{ color }}
        >
          {report.recommendation}
        </h2>
      </div>

      <div>
        <h3 className="text-text-secondary text-xs uppercase tracking-wider mb-1">
          Reasoning
        </h3>
        <p className="text-sm text-text-primary leading-relaxed">
          {report.reasoning}
        </p>
      </div>

      <div>
        <h3 className="text-text-secondary text-xs uppercase tracking-wider mb-1">
          Expected Benefit
        </h3>
        <p className="text-sm text-text-primary">{report.expected_benefit}</p>
      </div>

      <div>
        <h3 className="text-text-secondary text-xs uppercase tracking-wider mb-1">
          Trigger
        </h3>
        <p className="text-sm">
          {report.trigger_parameter}: {report.trigger_value.toFixed(1)} (threshold:{' '}
          {report.threshold_value.toFixed(1)})
        </p>
      </div>

      <div>
        <h3 className="text-text-secondary text-xs uppercase tracking-wider mb-2">
          Drilling Parameters at Time of Alert
        </h3>
        <div className="grid grid-cols-3 gap-2 text-xs">
          <Param label="Bit Depth" value={p.bit_depth} unit="ft" />
          <Param label="ROP" value={p.rop} unit="ft/hr" />
          <Param label="WOB" value={p.wob} unit="klbs" />
          <Param label="RPM" value={p.rpm} />
          <Param label="Torque" value={p.torque} unit="kft-lbs" />
          <Param label="SPP" value={p.spp} unit="psi" />
          <Param label="Flow In" value={p.flow_in} unit="gpm" />
          <Param label="Flow Out" value={p.flow_out} unit="gpm" />
          <Param label="Balance" value={p.flow_balance} unit="gpm" />
          <Param label="Mud Wt" value={p.mud_weight} unit="ppg" />
          <Param label="ECD" value={p.ecd} unit="ppg" />
          <Param label="Pit Vol" value={p.pit_volume} unit="bbl" />
          <Param label="MSE" value={p.mse} unit="psi" />
          <Param label="MSE Eff" value={p.mse_efficiency} unit="%" />
        </div>
      </div>

      {report.votes_summary.length > 0 && (
        <div>
          <h3 className="text-text-secondary text-xs uppercase tracking-wider mb-1">
            Specialist Votes
          </h3>
          <ul className="text-xs space-y-0.5">
            {report.votes_summary.map((v, i) => (
              <li key={i} className="text-text-primary">
                {v}
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="text-[10px] text-text-muted border-t border-border pt-2 space-y-0.5">
        <div>Signature: {report.digital_signature}</div>
        <div>Signed: {report.signature_timestamp}</div>
      </div>
    </div>
  );
}

function Param({
  label,
  value,
  unit,
}: {
  label: string;
  value: number;
  unit?: string;
}) {
  return (
    <div className="bg-bg-secondary rounded px-2 py-1">
      <div className="text-text-muted text-[10px]">{label}</div>
      <div className="text-text-primary font-medium tabular-nums">
        {value.toFixed(1)}
        {unit && <span className="text-text-muted ml-0.5">{unit}</span>}
      </div>
    </div>
  );
}
