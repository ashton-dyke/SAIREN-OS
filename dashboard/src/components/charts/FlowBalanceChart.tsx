import type { LiveData } from '../../api/types';
import { TimeSeriesChart } from './TimeSeriesChart';
import { colors } from '../../theme/colors';

interface Props {
  history: LiveData[];
}

export function FlowBalanceChart({ history }: Props) {
  const data = history.map((d, i) => ({
    idx: i,
    flow_in: d.drilling.flow_in,
    flow_out: d.drilling.flow_out,
    balance: d.drilling.flow_balance,
  }));

  return (
    <TimeSeriesChart
      title="Flow Balance"
      data={data}
      series={[
        { dataKey: 'flow_in', color: colors.blue, name: 'Flow In' },
        { dataKey: 'flow_out', color: colors.green, name: 'Flow Out' },
        { dataKey: 'balance', color: colors.yellow, name: 'Balance' },
      ]}
      yUnit=" gpm"
    />
  );
}
