import { useState, useEffect } from 'react';
import { Select } from '@shared/ui/Select';
import { Input } from '@shared/ui/Input';
import { Button } from '@shared/ui/Button';
import { ConfirmDialog } from '@shared/ui/ConfirmDialog';
import { getBalances, getWithdrawalFee, submitWithdrawal } from '../../api';
import { toast } from '@shared/ui/Toast';
import styles from './WithdrawForm.module.css';

const ASSETS = ['BTC', 'ETH', 'USDT', 'USDC', 'BNB'];
const NETWORKS: Record<string, string[]> = {
  BTC: ['Bitcoin'],
  ETH: ['ERC20', 'Arbitrum', 'Optimism'],
  USDT: ['ERC20', 'TRC20', 'BEP20'],
  USDC: ['ERC20', 'Arbitrum', 'Solana'],
  BNB: ['BEP20'],
};

export function WithdrawForm() {
  const [asset, setAsset] = useState(ASSETS[0]);
  const [network, setNetwork] = useState(NETWORKS[ASSETS[0]][0]);
  const [address, setAddress] = useState('');
  const [amount, setAmount] = useState('');
  const [memo, setMemo] = useState('');
  const [fee, setFee] = useState('—');
  const [maxBalance, setMaxBalance] = useState('0');
  const [confirming, setConfirming] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [errors, setErrors] = useState<{ address?: string; amount?: string }>({});

  useEffect(() => {
    const nets = NETWORKS[asset];
    if (!nets.includes(network)) setNetwork(nets[0]);
  }, [asset, network]);

  useEffect(() => {
    getBalances()
      .then((bs) => {
        const bal = bs.find((b) => b.asset === asset);
        setMaxBalance(bal?.available ?? '0');
      })
      .catch(() => setMaxBalance('0'));
  }, [asset]);

  useEffect(() => {
    getWithdrawalFee(asset, network)
      .then((f) => setFee(f.fee))
      .catch(() => setFee('—'));
  }, [asset, network]);

  function validate(): boolean {
    const errs: { address?: string; amount?: string } = {};
    if (!address.trim()) errs.address = 'Address is required';
    const amt = parseFloat(amount);
    if (!amount || isNaN(amt) || amt <= 0) {
      errs.amount = 'Enter a valid amount';
    } else if (amt > parseFloat(maxBalance)) {
      errs.amount = 'Exceeds available balance';
    }
    setErrors(errs);
    return Object.keys(errs).length === 0;
  }

  function handleSubmitClick() {
    if (validate()) setConfirming(true);
  }

  async function handleConfirm() {
    setConfirming(false);
    setSubmitting(true);
    try {
      await submitWithdrawal({ asset, network, address, amount, memo: memo || undefined });
      toast.success('Withdrawal submitted successfully');
      setAddress('');
      setAmount('');
      setMemo('');
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Withdrawal failed';
      toast.error(msg);
    } finally {
      setSubmitting(false);
    }
  }

  const assetOptions = ASSETS.map((a) => ({ value: a, label: a }));
  const networkOptions = (NETWORKS[asset] ?? []).map((n) => ({ value: n, label: n }));

  return (
    <div className={styles.container}>
      <div className={styles.selectors}>
        <Select
          label="Asset"
          options={assetOptions}
          value={asset}
          onChange={(e) => setAsset(e.target.value)}
        />
        <Select
          label="Network"
          options={networkOptions}
          value={network}
          onChange={(e) => setNetwork(e.target.value)}
        />
      </div>

      <Input
        label="Withdrawal Address"
        placeholder="Enter destination address"
        value={address}
        onChange={(e) => setAddress(e.target.value)}
        error={errors.address}
      />

      <Input
        label="Amount"
        placeholder="0.00"
        value={amount}
        onChange={(e) => setAmount(e.target.value)}
        error={errors.amount}
        suffix={
          <button
            type="button"
            className={styles.maxBtn}
            onClick={() => setAmount(maxBalance)}
          >
            Max
          </button>
        }
      />

      <Input
        label="Memo / Tag (optional)"
        placeholder="Required for some networks"
        value={memo}
        onChange={(e) => setMemo(e.target.value)}
      />

      <div className={styles.feeRow}>
        <span className={styles.feeLabel}>Network fee</span>
        <span className={styles.feeValue}>{fee} {asset}</span>
      </div>

      <div className={styles.balanceRow}>
        <span className={styles.feeLabel}>Available</span>
        <span className={styles.feeValue}>{maxBalance} {asset}</span>
      </div>

      <Button
        variant="primary"
        fullWidth
        loading={submitting}
        onClick={handleSubmitClick}
      >
        Withdraw {asset}
      </Button>

      <ConfirmDialog
        open={confirming}
        onConfirm={() => { void handleConfirm(); }}
        onCancel={() => setConfirming(false)}
        title="Confirm Withdrawal"
        message={`Send ${amount} ${asset} to ${address}?`}
        confirmText="Confirm Withdrawal"
        variant="warning"
      />
    </div>
  );
}
