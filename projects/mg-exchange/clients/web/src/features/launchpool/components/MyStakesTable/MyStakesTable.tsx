import { useState } from 'react';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { unstakeFromPool, claimRewards } from '../../api';
import type { LaunchpoolStake } from '../../types';
import styles from './MyStakesTable.module.css';

interface MyStakesTableProps {
  stakes: LaunchpoolStake[];
  loading: boolean;
  onUpdated: () => void;
}

export function MyStakesTable({ stakes, loading, onUpdated }: MyStakesTableProps) {
  const [acting, setActing] = useState<string | null>(null);

  async function handleUnstake(stake: LaunchpoolStake) {
    setActing(`unstake-${stake.id}`);
    try {
      await unstakeFromPool({ pool_id: stake.pool_id, amount: stake.staked_amount });
      toast.success('Unstaked successfully');
      onUpdated();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Unstake failed');
    } finally {
      setActing(null);
    }
  }

  async function handleClaim(stake: LaunchpoolStake) {
    setActing(`claim-${stake.id}`);
    try {
      await claimRewards(stake.pool_id);
      toast.success('Rewards claimed');
      onUpdated();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Claim failed');
    } finally {
      setActing(null);
    }
  }

  const columns: ColumnDef<Record<string, unknown>>[] = [
    { key: 'stake_token', header: 'Staked Asset' },
    { key: 'reward_token', header: 'Reward Token' },
    { key: 'staked_amount', header: 'Staked Amount', align: 'right' },
    { key: 'rewards_earned', header: 'Rewards Earned', align: 'right' },
    {
      key: 'staked_at',
      header: 'Date',
      render: (row) => new Date((row.staked_at as number) * 1000).toLocaleDateString(),
    },
    {
      key: 'actions',
      header: 'Actions',
      render: (row) => {
        const stake = row as unknown as LaunchpoolStake;
        return (
          <div className={styles.actions}>
            <Button
              size="sm"
              variant="ghost"
              loading={acting === `claim-${stake.id}`}
              onClick={() => { void handleClaim(stake); }}
            >
              Claim
            </Button>
            <Button
              size="sm"
              variant="danger"
              loading={acting === `unstake-${stake.id}`}
              onClick={() => { void handleUnstake(stake); }}
            >
              Unstake
            </Button>
          </div>
        );
      },
    },
  ];

  return (
    <Table
      columns={columns}
      data={stakes as unknown as Record<string, unknown>[]}
      loading={loading}
      emptyText="No active stakes"
    />
  );
}
