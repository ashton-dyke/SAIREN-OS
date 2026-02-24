import type { HealthV2 } from '../../api/types';
import { severityColor } from '../../theme/colors';

interface AlertBannerProps {
  health: HealthV2;
}

export function AlertBanner({ health }: AlertBannerProps) {
  const sev = health.severity.toLowerCase();
  if (sev === 'healthy' || sev === 'low') return null;

  const color = severityColor(health.severity);
  const isCritical = sev === 'critical';

  return (
    <div
      className={`border-l-4 px-4 py-3 ${isCritical ? 'py-4' : ''}`}
      style={{
        borderColor: color,
        backgroundColor: `${color}15`,
      }}
    >
      <div className="flex items-start gap-3">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <span
              className="text-xs font-bold uppercase tracking-wider"
              style={{ color }}
            >
              {health.severity}
            </span>
            <span className="text-text-secondary text-xs">
              Score: {health.overall_score.toFixed(0)}
            </span>
          </div>
          <p className="text-sm text-text-primary leading-snug">
            {health.diagnosis}
          </p>
          {isCritical && (
            <p className="text-sm mt-1 font-medium" style={{ color }}>
              {health.recommendation}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
