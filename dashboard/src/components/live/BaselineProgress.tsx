import type { BaselineSummaryV2 } from '../../api/types';

interface Props {
  baseline: BaselineSummaryV2;
}

export function BaselineProgress({ baseline }: Props) {
  const pct =
    baseline.total_metrics > 0
      ? Math.round((baseline.locked_count / baseline.total_metrics) * 100)
      : 0;

  return (
    <div className="space-y-2">
      <h3 className="text-text-secondary text-xs uppercase tracking-wider font-medium">
        Baseline Learning
      </h3>
      <div className="bg-bg-card rounded-lg p-3 border border-border space-y-2">
        <div className="flex justify-between text-xs">
          <span className="text-text-secondary">{baseline.overall_status}</span>
          <span className="text-text-primary font-medium">
            {baseline.locked_count}/{baseline.total_metrics} locked
          </span>
        </div>
        <div className="h-1.5 bg-bg-primary rounded-full overflow-hidden">
          <div
            className="h-full rounded-full transition-all duration-500"
            style={{
              width: `${pct}%`,
              backgroundColor: pct === 100 ? '#3fb950' : '#58a6ff',
            }}
          />
        </div>
      </div>
    </div>
  );
}
