import { severityColor } from '../../theme/colors';

export function SeverityBadge({ severity }: { severity: string }) {
  const color = severityColor(severity);
  return (
    <span
      className="inline-flex items-center px-2 py-0.5 rounded text-xs font-bold uppercase tracking-wider"
      style={{ color, borderColor: color, border: '1px solid' }}
    >
      {severity}
    </span>
  );
}
