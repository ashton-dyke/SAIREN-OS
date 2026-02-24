import type { LiveData } from '../../api/types';
import { TimeSeriesChart } from './TimeSeriesChart';
import { colors } from '../../theme/colors';

interface Props {
  history: LiveData[];
}

export function MSEChart({ history }: Props) {
  const data = history.map((d, i) => ({
    idx: i,
    mse: d.drilling.mse,
    baseline: d.drilling.mse_baseline,
  }));

  return (
    <TimeSeriesChart
      title="MSE vs Baseline"
      data={data}
      series={[
        { dataKey: 'mse', color: colors.orange, name: 'MSE' },
        { dataKey: 'baseline', color: colors.textMuted, name: 'Baseline' },
      ]}
      yUnit=" psi"
    />
  );
}
