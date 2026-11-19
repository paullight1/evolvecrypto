import { useState, useEffect } from 'react';
import { Card } from '@shared/ui/Card';
import { NumberInput } from '@shared/ui/NumberInput';
import { Button } from '@shared/ui/Button';
import { Select, type SelectOption } from '@shared/ui/Select';
import { toast } from '@shared/ui/Toast';
import { stakingStake, stakingUnstake, stakingClaimRewards, getStakingBalance } from '../../api';
import type { StakingBalance } from '../../types';
import styles from './StakingPanel.module.css';

const STAKING_ASSETS: SelectOption[] = [
  { value: 'BTC', label: 'BTC' },
  { value: 'ETH', label: 'ETH' },
  { value: 'SOL', label: 'SOL' },
  { value: 'BNB', label: 'BNB' },
];

export function StakingPanel() {
  const [asset, setAsset] = useState('ETH');
  const [stakeAmount, setStakeAmount] = useState('');
  const [unstakeAmount, setUnstakeAmount] = useState('');
  const [balance, setBalance] = useState<StakingBalance | null>(null);
  const [balanceLoading, setBalanceLoading] = useState(false);
  const [staking, setStaking] = useState(false);
  const [unstaking, setUnstaking] = useState(false);
  const [claiming, setClaiming] = useState(false);

  function loadBalance() {
    setBalanceLoading(true);
    getStakingBalance(asset)
      .then(setBalance)
      .catch(() => setBalance(null))
      .finally(() => setBalanceLoading(false));
  }

  useEffect(() => {
    loadBalance();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asset]);

  async function handleStake() {
    const num = parseFloat(stakeAmount);
    if (!isFinite(num) || num <= 0) {
      toast.error('Enter a valid stake amount');
      return;
    }
    setStaking(true);
    try {
      await stakingStake({ asset, amount: stakeAmount });
      toast.success(`Staked ${stakeAmount} ${asset}`);
      setStakeAmount('');
      loadBalance();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Stake failed');
    } finally {
      setStaking(false);
    }
  }

  async function handleUnstake() {
    const num = parseFloat(unstakeAmount);
    if (!isFinite(num) || num <= 0) {
      toast.error('Enter a valid unstake amount');
      return;
    }
    setUnstaking(true);
    try {
      await stakingUnstake({ asset, amount: unstakeAmount });
      toast.success(`Unstaked ${unstakeAmount} ${asset}`);
      setUnstakeAmount('');
      loadBalance();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Unstake failed');
    } finally {
      setUnstaking(false);
    }
  }

  async function handleClaim() {
    setClaiming(true);
    try {
      await stakingClaimRewards(asset);
      toast.success(`Rewards claimed for ${asset}`);
      loadBalance();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Claim failed');
    } finally {
      setClaiming(false);
    }
  }

  return (
    <div className={styles.container}>
      <Select
        label="Asset"
        options={STAKING_ASSETS}
        value={asset}
        onChange={(e) => setAsset(e.target.value)}
      />

      {balance && !balanceLoading && (
        <Card className={styles.balanceCard}>
          <div className={styles.balanceGrid}>
            <div className={styles.balanceStat}>
              <span className={styles.statLabel}>Staked</span>
              <span className={styles.statValue}>
                {balance.staked} {asset}
              </span>
            </div>
            <div className={styles.balanceStat}>
              <span className={styles.statLabel}>Rewards Earned</span>
              <span className={styles.statValueGreen}>
                {balance.rewards_earned} {asset}
              </span>
            </div>
            <div className={styles.balanceStat}>
              <span className={styles.statLabel}>Current APY</span>
              <span className={styles.statValueGreen}>{balance.apy}</span>
            </div>
          </div>

          <Button
            variant="secondary"
            size="sm"
            loading={claiming}
            onClick={() => { void handleClaim(); }}
            disabled={parseFloat(balance.rewards_earned) <= 0}
          >
            Claim Rewards
          </Button>
        </Card>
      )}

      <div className={styles.forms}>
        <Card>
          <h3 className={styles.formTitle}>Stake</h3>
          <NumberInput
            label={`Amount (${asset})`}
            min={0}
            placeholder="0.00"
            value={stakeAmount}
            onChange={(v) => setStakeAmount(v)}
          />
          <Button
            variant="long"
            fullWidth
            loading={staking}
            onClick={() => { void handleStake(); }}
          >
            Stake {asset}
          </Button>
        </Card>

        <Card>
          <h3 className={styles.formTitle}>Unstake</h3>
          <NumberInput
            label={`Amount (${asset})`}
            min={0}
            placeholder="0.00"
            value={unstakeAmount}
            onChange={(v) => setUnstakeAmount(v)}
          />
          <Button
            variant="short"
            fullWidth
            loading={unstaking}
            onClick={() => { void handleUnstake(); }}
          >
            Unstake {asset}
          </Button>
        </Card>
      </div>
    </div>
  );
}
