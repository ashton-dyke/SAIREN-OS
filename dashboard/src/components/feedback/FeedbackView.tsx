import { fetchFeedbackStats, fetchConfigSuggestions } from '../../api/client';
import { usePolling } from '../../hooks/usePolling';
import type { CategoryStats, ThresholdSuggestion } from '../../api/types';

export function FeedbackView() {
  const { data: stats } = usePolling(fetchFeedbackStats, 30_000);
  const { data: suggestions } = usePolling(fetchConfigSuggestions, 30_000);

  return (
    <div className="p-4 space-y-6 overflow-y-auto h-full">
      <h1 className="text-sm font-bold text-text-primary uppercase tracking-wider">
        Operator Feedback Analytics
      </h1>

      {/* Category Stats Grid */}
      <div>
        <h2 className="text-text-secondary text-xs uppercase tracking-wider font-medium mb-2">
          Category Stats
        </h2>
        {stats && stats.length > 0 ? (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
            {stats.map((cat) => (
              <CategoryCard key={cat.category} stats={cat} />
            ))}
          </div>
        ) : (
          <div className="text-text-secondary text-sm">
            No feedback data yet. Submit feedback on critical reports to populate stats.
          </div>
        )}
      </div>

      {/* Threshold Suggestions */}
      <div>
        <h2 className="text-text-secondary text-xs uppercase tracking-wider font-medium mb-2">
          Threshold Suggestions
        </h2>
        {suggestions && suggestions.length > 0 ? (
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            {suggestions.map((s, i) => (
              <SuggestionCard key={i} suggestion={s} />
            ))}
          </div>
        ) : (
          <div className="text-text-secondary text-sm">
            No threshold suggestions available. More feedback data is needed.
          </div>
        )}
      </div>
    </div>
  );
}

function CategoryCard({ stats }: { stats: CategoryStats }) {
  const rate = stats.confirmation_rate * 100;
  const barColor =
    rate >= 80 ? 'bg-accent-green' : rate >= 50 ? 'bg-accent-yellow' : 'bg-accent-red';

  return (
    <div className="bg-bg-card rounded-lg p-3 border border-border space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-xs font-medium text-text-primary">{stats.category}</span>
        <span className="text-xs text-text-muted">{stats.total} total</span>
      </div>

      {/* Confirmation rate bar */}
      <div className="space-y-1">
        <div className="flex items-center justify-between text-[10px] text-text-secondary">
          <span>Confirmation Rate</span>
          <span>{rate.toFixed(0)}%</span>
        </div>
        <div className="h-1.5 bg-bg-secondary rounded-full overflow-hidden">
          <div
            className={`h-full rounded-full ${barColor}`}
            style={{ width: `${rate}%` }}
          />
        </div>
      </div>

      {/* Counts */}
      <div className="flex gap-3 text-[10px]">
        <span className="text-accent-green">{stats.confirmed} confirmed</span>
        <span className="text-accent-red">{stats.false_positives} false pos</span>
        <span className="text-accent-yellow">{stats.unclear} unclear</span>
      </div>
    </div>
  );
}

function SuggestionCard({ suggestion }: { suggestion: ThresholdSuggestion }) {
  const confidence = suggestion.confidence * 100;

  return (
    <div className="bg-bg-card rounded-lg p-3 border border-border space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-xs font-medium text-text-primary">{suggestion.category}</span>
        <span className="text-[10px] text-text-muted">{suggestion.threshold_key}</span>
      </div>

      <div className="flex items-center gap-2 text-sm">
        <span className="text-text-secondary tabular-nums">
          {suggestion.current_value.toFixed(1)}
        </span>
        <span className="text-text-muted">&rarr;</span>
        <span className="text-accent-blue font-medium tabular-nums">
          {suggestion.suggested_value.toFixed(1)}
        </span>
      </div>

      <p className="text-[10px] text-text-secondary leading-snug">
        {suggestion.rationale}
      </p>

      <div className="flex items-center justify-between">
        <div className="flex items-center gap-1 text-[10px] text-text-muted">
          <span>Confidence</span>
          <span>{confidence.toFixed(0)}%</span>
        </div>
        <div className="h-1 w-16 bg-bg-secondary rounded-full overflow-hidden">
          <div
            className="h-full rounded-full bg-accent-blue"
            style={{ width: `${confidence}%` }}
          />
        </div>
      </div>
    </div>
  );
}
