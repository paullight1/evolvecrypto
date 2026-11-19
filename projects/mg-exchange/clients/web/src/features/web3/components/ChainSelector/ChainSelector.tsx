import { Select, type SelectOption } from '@shared/ui/Select';
import type { Chain } from '../../types';

interface ChainSelectorProps {
  chains: Chain[];
  value: string;
  onChange: (chainId: string) => void;
  label?: string;
}

export function ChainSelector({ chains, value, onChange, label = 'Chain' }: ChainSelectorProps) {
  const options: SelectOption[] = chains.map((c) => ({
    value: c.id,
    label: `${c.name} (${c.native_token})`,
  }));

  return (
    <Select
      label={label}
      options={options}
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}
