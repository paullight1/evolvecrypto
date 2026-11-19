import { Table, type ColumnDef } from '@shared/ui/Table';
import type { SpreadInstrument } from '../../types';

interface SpreadListProps {
  instruments: SpreadInstrument[];
  loading: boolean;
  onSelect: (instrument: SpreadInstrument) => void;
}

type SpreadRow = SpreadInstrument & Record<string, unknown>;

const COLUMNS: ColumnDef<SpreadRow>[] = [
  { key: 'name', header: 'Spread' },
  { key: 'near_leg', header: 'Near Leg' },
  { key: 'far_leg', header: 'Far Leg' },
  { key: 'spread_value', header: 'Value', align: 'right', sortable: true },
  { key: 'bid', header: 'Bid', align: 'right' },
  { key: 'ask', header: 'Ask', align: 'right' },
  { key: 'last', header: 'Last', align: 'right' },
  {
    key: 'volume_24h',
    header: 'Volume 24h',
    align: 'right',
    sortable: true,
    render: (r) => parseFloat(r.volume_24h as string).toLocaleString(),
  },
];

export function SpreadList({ instruments, loading, onSelect }: SpreadListProps) {
  return (
    <Table<SpreadRow>
      columns={COLUMNS}
      data={instruments as SpreadRow[]}
      loading={loading}
      emptyText="No spread instruments available"
      onRowClick={(row) => onSelect(row as SpreadInstrument)}
    />
  );
}
