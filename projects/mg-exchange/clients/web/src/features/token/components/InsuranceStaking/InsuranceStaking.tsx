import { useState, type FormEvent } from 'react';
import { Card } from '@shared/ui/Card';
import { Input } from '@shared/ui/Input';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { stakeInsurance, unstakeInsurance } from '../../api';
import type { InsuranceStakeInfo } from '../../types';
import styles from './InsuranceStaking.module.css';

interface InsuranceStakingProps {
  info: InsuranceStakeInfo | null;
  onUpdated: () => void;
}

export function InsuranceStaking({ info, onUpdated }: InsuranceStakingProps) {
  const [stakeAmount, setStakeAmount] = useState('');
  const [unstakeAmount, setUnstakeAmount] = useState('');
  const [staking, setStaking] = useState(false);
  const [unstaking, setUnstaking] = useState(false);

  async function handleStake(e: FormEvent) {
    e.preventDefault();
    setStaking(true);
    try {
      await stakeInsurance(stakeAmount);
      toast.success(`Staked ${stakeAmount} EXC`);
      setStakeAmount('');
      onUpdated();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Stake failed');
    } finally {
      setStaking(false);
    }
  }

  async function handleUnstake(e: FormEvent) {
    e.preventDefault();
    setUnstaking(true);
    try {
      await unstakeInsurance(unstakeAmount);
      toast.success(`Unstaked ${unstakeAmount} EXC`);
      setUnstakeAmount('');
      onUpdated();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Unstake failed');
    } finally {
      setUnstaking(false);
    }
  }

  return (
    <div className={styles.wrap}>
      <Card className={styles.poolStats}>
        <h3 className={styles.poolTitle}>Insurance Pool Stats</h3>
        <div className={styles.statsGrid}>
          <div className={styles.stat}>
            <span className={styles.statLabel}>Pool Total Staked</span>
            <span className={styles.statValue}>{info ? parseFloat(info.pool_total_staked).toLocaleString() : '—'} EXC</span>
          </div>
          <div className={styles.stat}>
            <span className={styles.statLabel}>Your Staked</span>
            <span className={styles.statValue}>{info?.user_staked ?? '0'} EXC</span>
          </div>
          <div className={styles.stat}>
            <span className={styles.statLabel}>Your Rewards</span>
            <span className={styles.statValue}>{info?.user_rewards_earned ?? '0'} EXC</span>
          </div>
          <div className={styles.stat}>
            <span className={styles.statLabel}>APY</span>
            <span className={styles.statValue}>{info?.apy ?? '—'}</span>
          </div>
        </div>
        {info && (
          <p className={styles.lockNote}>Lock period: {info.lock_days} days</p>
        )}
      </Card>

      <div className={styles.forms}>
        <Card>
          <form onSubmit={(e) => { void handleStake(e); }} className={styles.form}>
            <h4 className={styles.formTitle}>Stake EXC</h4>
            <Input label="Amount (EXC)" type="number" value={stakeAmount} onChange={(e) => setStakeAmount(e.target.value)} placeholder="0.00" required />
            <Button type="submit" variant="primary" fullWidth loading={staking}>Stake</Button>
          </form>
        </Card>
        <Card>
          <form onSubmit={(e) => { void handleUnstake(e); }} className={styles.form}>
            <h4 className={styles.formTitle}>Unstake EXC</h4>
            <Input label="Amount (EXC)" type="number" value={unstakeAmount} onChange={(e) => setUnstakeAmount(e.target.value)} placeholder="0.00" required />
            <Button type="submit" variant="ghost" fullWidth loading={unstaking}>Unstake</Button>
          </form>
        </Card>
      </div>
    </div>
  );
}
