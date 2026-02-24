import type { DrillingV2 } from '../../api/types';
import { MetricCard } from '../common/MetricCard';

interface Props {
  drilling: DrillingV2;
}

export function DrillingParams({ drilling }: Props) {
  return (
    <div className="space-y-2">
      <h3 className="text-text-secondary text-xs uppercase tracking-wider font-medium">
        Drilling Parameters
      </h3>
      <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 gap-2">
        <MetricCard label="Bit Depth" value={drilling.bit_depth} unit="ft" />
        <MetricCard label="ROP" value={drilling.rop} unit="ft/hr" />
        <MetricCard label="WOB" value={drilling.wob} unit="klbs" />
        <MetricCard label="RPM" value={drilling.rpm} />
        <MetricCard label="Torque" value={drilling.torque} unit="kft-lbs" />
        <MetricCard label="SPP" value={drilling.spp} unit="psi" />
        <MetricCard label="Hook Load" value={drilling.hook_load} unit="klbs" />
        <MetricCard
          label="MSE Eff"
          value={drilling.mse_efficiency}
          unit="%"
          alert={drilling.mse_efficiency < 50}
        />
      </div>
    </div>
  );
}
