import type { SpecialistVotesV2 } from '../../api/types';
import { severityColor } from '../../theme/colors';

interface Props {
  votes: SpecialistVotesV2 | null;
}

function voteToSeverity(vote: string): string {
  // Rust Debug format: "Critical", "High", "Medium", "Low"
  return vote.replace(/"/g, '');
}

function VoteChip({ label, vote }: { label: string; vote: string }) {
  const sev = voteToSeverity(vote);
  const color = sev === '--' ? '#6e7681' : severityColor(sev);
  return (
    <div className="flex items-center gap-1.5">
      <span
        className="w-2 h-2 rounded-full shrink-0"
        style={{ backgroundColor: color }}
      />
      <span className="text-text-secondary text-xs">{label}</span>
      <span className="text-xs font-medium" style={{ color }}>
        {sev}
      </span>
    </div>
  );
}

export function SpecialistVotes({ votes }: Props) {
  if (!votes) return null;

  return (
    <div className="space-y-2">
      <h3 className="text-text-secondary text-xs uppercase tracking-wider font-medium">
        Specialist Votes
      </h3>
      <div className="grid grid-cols-2 gap-2 bg-bg-card rounded-lg p-3 border border-border">
        <VoteChip label="MSE" vote={votes.mse} />
        <VoteChip label="Hydraulic" vote={votes.hydraulic} />
        <VoteChip label="Well Control" vote={votes.well_control} />
        <VoteChip label="Formation" vote={votes.formation} />
      </div>
    </div>
  );
}
