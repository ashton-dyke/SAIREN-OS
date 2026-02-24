import type { DrillingV2 } from '../../api/types';
import { MetricCard } from '../common/MetricCard';

interface Props {
  drilling: DrillingV2;
}

export function WellControl({ drilling }: Props) {
  const flowAlert = Math.abs(drilling.flow_balance) > 10;
  const ecdAlert = drilling.ecd_margin < 0.5;

  return (
    <div className="space-y-2">
      <h3 className="text-text-secondary text-xs uppercase tracking-wider font-medium">
        Well Control
      </h3>
      <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 gap-2">
        <MetricCard label="Flow In" value={drilling.flow_in} unit="gpm" />
        <MetricCard label="Flow Out" value={drilling.flow_out} unit="gpm" />
        <MetricCard
          label="Balance"
          value={drilling.flow_balance}
          unit="gpm"
          alert={flowAlert}
        />
        <MetricCard label="Pit Vol" value={drilling.pit_volume} unit="bbl" />
        <MetricCard label="Mud Wt" value={drilling.mud_weight} unit="ppg" />
        <MetricCard
          label="ECD"
          value={drilling.ecd}
          unit="ppg"
          alert={ecdAlert}
        />
        <MetricCard
          label="ECD Margin"
          value={drilling.ecd_margin}
          unit="ppg"
          alert={ecdAlert}
        />
        <MetricCard label="Gas" value={drilling.gas_units} unit="units" />
      </div>
    </div>
  );
}
