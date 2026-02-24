import { SeverityBadge } from '../common/SeverityBadge';

interface Props {
  score: number;
  severity: string;
  diagnosis: string;
  action: string;
}

export function ReportCard({ score, severity, diagnosis, action }: Props) {
  return (
    <div className="bg-bg-card border border-border rounded-lg p-4 space-y-2">
      <div className="flex items-center justify-between">
        <SeverityBadge severity={severity} />
        <span className="text-text-secondary text-xs tabular-nums">
          Score: {score.toFixed(0)}
        </span>
      </div>
      <p className="text-sm text-text-primary leading-snug">{diagnosis}</p>
      <p className="text-xs text-text-secondary">{action}</p>
    </div>
  );
}
