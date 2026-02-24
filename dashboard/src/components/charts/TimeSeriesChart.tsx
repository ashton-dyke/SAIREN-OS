import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from 'recharts';
import { colors } from '../../theme/colors';

interface Series {
  dataKey: string;
  color: string;
  name: string;
}

interface Props {
  title: string;
  data: Record<string, number | string>[];
  series: Series[];
  yUnit?: string;
}

export function TimeSeriesChart({ title, data, series, yUnit }: Props) {
  return (
    <div className="bg-bg-card border border-border rounded-lg p-3 space-y-2">
      <h3 className="text-text-secondary text-xs uppercase tracking-wider font-medium">
        {title}
      </h3>
      <ResponsiveContainer width="100%" height={180}>
        <LineChart data={data} margin={{ top: 5, right: 5, bottom: 5, left: 5 }}>
          <CartesianGrid stroke="#21283b" strokeDasharray="3 3" />
          <XAxis
            dataKey="idx"
            tick={{ fill: colors.textMuted, fontSize: 10 }}
            axisLine={{ stroke: colors.border }}
            tickLine={false}
          />
          <YAxis
            tick={{ fill: colors.textMuted, fontSize: 10 }}
            axisLine={{ stroke: colors.border }}
            tickLine={false}
            unit={yUnit}
            width={45}
          />
          <Tooltip
            contentStyle={{
              background: colors.bgSecondary,
              border: `1px solid ${colors.border}`,
              borderRadius: 4,
              fontSize: 11,
            }}
            labelStyle={{ color: colors.textSecondary }}
          />
          {series.map((s) => (
            <Line
              key={s.dataKey}
              type="monotone"
              dataKey={s.dataKey}
              stroke={s.color}
              name={s.name}
              dot={false}
              strokeWidth={1.5}
              isAnimationActive={false}
            />
          ))}
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
}
