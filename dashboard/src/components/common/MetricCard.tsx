interface MetricCardProps {
  label: string;
  value: string | number;
  unit?: string;
  alert?: boolean;
}

export function MetricCard({ label, value, unit, alert }: MetricCardProps) {
  return (
    <div
      className={`rounded-lg px-3 py-2 border ${
        alert
          ? 'border-accent-red bg-accent-red/10'
          : 'border-border bg-bg-card'
      }`}
    >
      <div className="text-text-muted text-[10px] uppercase tracking-wider mb-0.5">
        {label}
      </div>
      <div className="text-lg font-bold tabular-nums">
        {typeof value === 'number' ? value.toFixed(1) : value}
        {unit && (
          <span className="text-text-secondary text-xs ml-1">{unit}</span>
        )}
      </div>
    </div>
  );
}
