import { fetchLookaheadStatus } from '../../api/client';
import { usePolling } from '../../hooks/usePolling';

export function LookAheadPanel() {
  const { data } = usePolling(fetchLookaheadStatus, 10_000);

  if (!data || !data.enabled || !data.next_formation) {
    return null;
  }

  const isUrgent = data.estimated_minutes != null && data.estimated_minutes < 10;

  return (
    <div
      className={`rounded-lg p-3 border space-y-2 ${
        isUrgent
          ? 'border-accent-red/50 bg-accent-red/5'
          : 'border-accent-yellow/50 bg-accent-yellow/5'
      }`}
    >
      <div className="flex items-center justify-between">
        <h3
          className={`text-xs uppercase tracking-wider font-medium ${
            isUrgent ? 'text-accent-red' : 'text-accent-yellow'
          }`}
        >
          Formation Lookahead
        </h3>
        {data.estimated_minutes != null && (
          <span
            className={`text-xs font-bold tabular-nums ${
              isUrgent ? 'text-accent-red' : 'text-accent-yellow'
            }`}
          >
            ~{data.estimated_minutes.toFixed(0)} min
          </span>
        )}
      </div>

      <div className="text-sm text-text-primary font-medium">
        Approaching: {data.next_formation}
      </div>

      {data.depth_remaining_ft != null && (
        <div className="text-xs text-text-secondary">
          {data.depth_remaining_ft.toFixed(0)} ft remaining
        </div>
      )}

      {data.parameter_changes.length > 0 && (
        <div>
          <div className="text-[10px] text-text-muted uppercase tracking-wider mb-0.5">
            Parameter Changes
          </div>
          <ul className="text-xs text-text-primary space-y-0.5">
            {data.parameter_changes.map((c, i) => (
              <li key={i}>{c}</li>
            ))}
          </ul>
        </div>
      )}

      {data.hazards.length > 0 && (
        <div>
          <div className="text-[10px] text-accent-red uppercase tracking-wider mb-0.5">
            Hazards
          </div>
          <ul className="text-xs text-text-primary space-y-0.5">
            {data.hazards.map((h, i) => (
              <li key={i}>{h}</li>
            ))}
          </ul>
        </div>
      )}

      {data.offset_notes && (
        <div className="text-[10px] text-text-secondary italic">
          {data.offset_notes}
        </div>
      )}
    </div>
  );
}
