import { fetchFormationContext } from '../../api/client';
import { usePolling } from '../../hooks/usePolling';
import type { FormationContext, CfcDetection, BitWearStatus } from '../../api/types';

export function FormationCard() {
  const { data } = usePolling(fetchFormationContext, 10_000);

  if (!data || !data.current) {
    return (
      <div className="bg-bg-card rounded-lg border border-border p-3">
        <h3 className="text-text-secondary text-xs uppercase tracking-wider font-medium mb-2">
          Formation Context
        </h3>
        <p className="text-text-muted text-xs">No formation prognosis loaded</p>
      </div>
    );
  }

  return (
    <div className="bg-bg-card rounded-lg border border-border p-3 space-y-3">
      <h3 className="text-text-secondary text-xs uppercase tracking-wider font-medium">
        Formation Context
      </h3>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
        {/* Current Formation */}
        <CurrentFormationSection current={data.current} cfcDetection={data.cfc_detection} />

        {/* Next Boundary */}
        {data.next_boundary ? (
          <NextBoundarySection boundary={data.next_boundary} />
        ) : (
          <div className="text-xs text-text-muted">At final formation</div>
        )}
      </div>

      {/* Bit wear indicator */}
      {data.bit_wear && (
        <BitWearIndicator wear={data.bit_wear} />
      )}

      {/* Proactive damping recipe */}
      {data.proactive_damping && (
        <div className="text-xs text-accent-green border border-accent-green/30 rounded px-2 py-1">
          Proven recipe: WOB {(data.proactive_damping.recommended_wob_change_pct ?? 0) > 0 ? '+' : ''}
          {(data.proactive_damping.recommended_wob_change_pct ?? 0).toFixed(0)}%, RPM{' '}
          {(data.proactive_damping.recommended_rpm_change_pct ?? 0) > 0 ? '+' : ''}
          {(data.proactive_damping.recommended_rpm_change_pct ?? 0).toFixed(0)}%
          {' '}(reduced CV by {(data.proactive_damping.historical_cv_reduction_pct ?? 0).toFixed(0)}%)
        </div>
      )}

      {/* Connection gas trending warning */}
      {data.connection_gas_trending_up && (
        <div className="text-xs text-accent-yellow border border-accent-yellow/30 rounded px-2 py-1">
          <span className="mr-1">&#9888;</span>
          Connection gas trending up ({data.connection_gas.length} events)
        </div>
      )}

      {/* Upcoming formations + TD */}
      {(data.upcoming.length > 0 || data.target_depth_ft) && (
        <UpcomingStrip upcoming={data.upcoming} targetDepth={data.target_depth_ft} />
      )}
    </div>
  );
}

function CurrentFormationSection({
  current,
  cfcDetection,
}: {
  current: NonNullable<FormationContext['current']>;
  cfcDetection: FormationContext['cfc_detection'];
}) {
  const pct = current.formation_thickness_ft > 0
    ? (current.depth_in_formation_ft / current.formation_thickness_ft) * 100
    : 0;
  const clampedPct = Math.min(Math.max(pct, 0), 100);

  return (
    <div className="space-y-1.5">
      <div className="text-sm text-text-primary font-medium">
        {current.name}{' '}
        <span className="text-text-muted font-normal">({current.lithology})</span>
      </div>
      {cfcDetection && (
        <CfcAnnotation detection={cfcDetection} />
      )}
      <div className="grid grid-cols-2 gap-x-3 gap-y-0.5 text-xs">
        <div className="flex justify-between">
          <span className="text-text-secondary">Hardness</span>
          <span className="font-mono">{current.hardness.toFixed(0)}/10</span>
        </div>
        <div className="flex justify-between">
          <span className="text-text-secondary">PP</span>
          <span className="font-mono">{current.pore_pressure_ppg.toFixed(1)} ppg</span>
        </div>
        <div className="flex justify-between">
          <span className="text-text-secondary">FG</span>
          <span className="font-mono">{current.fracture_gradient_ppg.toFixed(1)} ppg</span>
        </div>
        <div className="flex justify-between">
          <span className="text-text-secondary">MW</span>
          <span className="font-mono">{current.mud_weight_ppg.toFixed(1)} ppg</span>
        </div>
      </div>

      {/* Progress bar */}
      <div className="space-y-0.5">
        <div className="w-full h-1.5 bg-bg-hover rounded-full overflow-hidden">
          <div
            className="h-full bg-accent-blue rounded-full transition-all duration-500"
            style={{ width: `${clampedPct}%` }}
          />
        </div>
        <div className="text-[10px] text-text-muted tabular-nums">
          {current.depth_in_formation_ft.toFixed(0)} / {current.formation_thickness_ft.toFixed(0)} ft
          {' '}({clampedPct.toFixed(0)}%)
        </div>
      </div>
    </div>
  );
}

function NextBoundarySection({ boundary }: { boundary: NonNullable<FormationContext['next_boundary']> }) {
  const isClose = boundary.distance_ft < 200;
  const isNear = boundary.distance_ft < 500;

  const borderColor = isClose
    ? 'border-accent-red/50'
    : isNear
      ? 'border-accent-yellow/50'
      : 'border-border';
  const bgColor = isClose
    ? 'bg-accent-red/5'
    : isNear
      ? 'bg-accent-yellow/5'
      : '';
  const distColor = isClose
    ? 'text-accent-red'
    : isNear
      ? 'text-accent-yellow'
      : 'text-text-primary';

  return (
    <div className={`space-y-1.5 rounded p-2 border ${borderColor} ${bgColor}`}>
      <div className="flex items-center justify-between">
        <span className="text-xs text-text-secondary">Next Boundary</span>
        <span className={`text-xs font-bold tabular-nums ${distColor}`}>
          {boundary.distance_ft.toFixed(0)} ft
        </span>
      </div>
      <div className="text-sm text-text-primary font-medium">
        {boundary.formation_name}
        <span className="text-text-muted font-normal text-xs ml-1">
          {boundary.lithology}, H: {boundary.hardness.toFixed(0)}/10
        </span>
      </div>

      {boundary.parameter_changes.length > 0 && (
        <div>
          <div className="text-[10px] text-text-muted uppercase tracking-wider mb-0.5">
            Parameter Changes
          </div>
          <ul className="text-xs text-text-primary space-y-0.5">
            {boundary.parameter_changes.map((c, i) => (
              <li key={i}>{c}</li>
            ))}
          </ul>
        </div>
      )}

      {boundary.hazards.length > 0 && (
        <div>
          <ul className="text-xs space-y-0.5">
            {boundary.hazards.map((h, i) => (
              <li key={i} className="text-accent-yellow">
                <span className="mr-1">&#9888;</span>{h}
              </li>
            ))}
          </ul>
        </div>
      )}

      {boundary.offset_notes && (
        <div className="text-[10px] text-text-secondary italic">
          {boundary.offset_notes}
        </div>
      )}
    </div>
  );
}

function CfcAnnotation({ detection }: { detection: CfcDetection }) {
  const offset = Math.abs(detection.depth_offset_from_prognosis_ft);
  const color = offset < 20
    ? 'text-accent-green'
    : offset < 50
      ? 'text-accent-yellow'
      : 'text-accent-red';

  return (
    <div className={`text-[10px] ${color}`}>
      CfC boundary at {detection.last_transition_depth_ft.toFixed(0)} ft
      {' '}({detection.depth_offset_from_prognosis_ft > 0 ? '+' : ''}
      {detection.depth_offset_from_prognosis_ft.toFixed(1)} ft from prognosis)
    </div>
  );
}

function BitWearIndicator({ wear }: { wear: BitWearStatus }) {
  const pct = Math.min(Math.max((wear.wear_index ?? 0) * 100, 0), 100);
  const color = pct > 80
    ? 'bg-accent-red'
    : pct > 50
      ? 'bg-accent-yellow'
      : pct > 30
        ? 'bg-accent-blue'
        : 'bg-accent-green';

  return (
    <div className="text-xs space-y-0.5">
      <div className="flex items-center justify-between">
        <span className="text-text-secondary">Bit Wear</span>
        <span className="font-mono text-text-primary">
          {pct.toFixed(0)}%
          {wear.advisory && (
            <span className="text-accent-yellow ml-1">- {wear.advisory}</span>
          )}
        </span>
      </div>
      <div className="w-full h-1 bg-bg-hover rounded-full overflow-hidden">
        <div className={`h-full ${color} rounded-full transition-all duration-500`} style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

function UpcomingStrip({
  upcoming,
  targetDepth,
}: {
  upcoming: FormationContext['upcoming'];
  targetDepth: number | null;
}) {
  return (
    <div className="border-t border-border pt-2 text-xs text-text-secondary">
      <span className="text-text-muted mr-1">Upcoming:</span>
      {upcoming.map((f, i) => (
        <span key={f.name}>
          {i > 0 && <span className="text-text-muted mx-1">&middot;</span>}
          {f.name}{' '}
          <span className="text-text-muted">({f.depth_top_ft.toFixed(0)})</span>
        </span>
      ))}
      {targetDepth != null && (
        <>
          {upcoming.length > 0 && <span className="text-text-muted mx-1">&middot;</span>}
          <span>
            TD: <span className="text-text-muted">{targetDepth.toFixed(0)} ft</span>
          </span>
        </>
      )}
    </div>
  );
}
