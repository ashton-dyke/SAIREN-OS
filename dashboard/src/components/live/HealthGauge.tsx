import { severityColor } from '../../theme/colors';

interface HealthGaugeProps {
  score: number;
  severity: string;
}

export function HealthGauge({ score, severity }: HealthGaugeProps) {
  const color = severityColor(severity);
  const radius = 40;
  const circumference = 2 * Math.PI * radius;
  const pct = Math.max(0, Math.min(100, score));
  const offset = circumference - (pct / 100) * circumference;

  return (
    <div className="flex flex-col items-center">
      <svg width="100" height="100" viewBox="0 0 100 100">
        <circle
          cx="50" cy="50" r={radius}
          fill="none" stroke="#21283b" strokeWidth="8"
        />
        <circle
          cx="50" cy="50" r={radius}
          fill="none" stroke={color} strokeWidth="8"
          strokeLinecap="round"
          strokeDasharray={circumference}
          strokeDashoffset={offset}
          transform="rotate(-90 50 50)"
          className="transition-all duration-700"
        />
        <text
          x="50" y="46" textAnchor="middle"
          fill={color} fontSize="20" fontWeight="bold" fontFamily="monospace"
        >
          {Math.round(score)}
        </text>
        <text
          x="50" y="60" textAnchor="middle"
          fill="#8b949e" fontSize="9"
        >
          {severity}
        </text>
      </svg>
    </div>
  );
}
