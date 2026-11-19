import { useState, useEffect } from 'react';
import { Modal } from '@shared/ui/Modal';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { getLeaderboard } from '../../api';
import type { Competition, CompetitionEntry } from '../../types';
import styles from './LeaderboardTable.module.css';

interface LeaderboardTableProps {
  competition: Competition | null;
  onClose: () => void;
}

export function LeaderboardTable({ competition, onClose }: LeaderboardTableProps) {
  const [entries, setEntries] = useState<CompetitionEntry[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!competition) return;
    setLoading(true);
    getLeaderboard(competition.id)
      .then(setEntries)
      .catch(() => setEntries([]))
      .finally(() => setLoading(false));
  }, [competition]);

  const columns: ColumnDef<Record<string, unknown>>[] = [
    {
      key: 'rank',
      header: '#',
      render: (row) => {
        const rank = row.rank as number;
        const medal = rank === 1 ? '🥇' : rank === 2 ? '🥈' : rank === 3 ? '🥉' : `${rank}`;
        return <span className={styles.rank}>{medal}</span>;
      },
    },
    { key: 'nickname', header: 'Trader' },
    { key: 'score', header: 'Score', align: 'right', sortable: true },
    { key: 'prize', header: 'Prize', align: 'right' },
  ];

  return (
    <Modal
      open={competition !== null}
      onClose={onClose}
      title={competition ? `${competition.name} — Leaderboard` : 'Leaderboard'}
    >
      <Table
        columns={columns}
        data={entries as unknown as Record<string, unknown>[]}
        loading={loading}
        emptyText="No entries yet"
      />
    </Modal>
  );
}
