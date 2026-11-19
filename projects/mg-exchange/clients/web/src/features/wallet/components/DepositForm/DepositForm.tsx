import { useState, useEffect } from 'react';
import { Select } from '@shared/ui/Select';
import { Spinner } from '@shared/ui/Spinner';
import { CopyableText } from '@shared/ui/CopyableText';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { EmptyState } from '@shared/ui/EmptyState';
import { getDepositAddress, getDeposits } from '../../api';
import type { DepositAddress, DepositRecord } from '../../types';
import styles from './DepositForm.module.css';

const ASSETS = ['BTC', 'ETH', 'USDT', 'USDC', 'BNB'];
const NETWORKS: Record<string, string[]> = {
  BTC: ['Bitcoin'],
  ETH: ['ERC20', 'Arbitrum', 'Optimism'],
  USDT: ['ERC20', 'TRC20', 'BEP20'],
  USDC: ['ERC20', 'Arbitrum', 'Solana'],
  BNB: ['BEP20'],
};

interface TxRow extends Record<string, unknown> {
  asset: string;
  amount: string;
  status: string;
  created_at: number;
  tx_hash: string;
}

const TX_COLUMNS: ColumnDef<TxRow>[] = [
  { key: 'asset', header: 'Asset', width: '80px' },
  { key: 'amount', header: 'Amount', align: 'right' },
  { key: 'status', header: 'Status' },
  {
    key: 'created_at',
    header: 'Date',
    render: (row) => new Date((row.created_at as number) * 1000).toLocaleDateString(),
  },
  { key: 'tx_hash', header: 'TX Hash', render: (row) => row.tx_hash ? String(row.tx_hash).slice(0, 12) + '...' : '—' },
];

export function DepositForm() {
  const [asset, setAsset] = useState(ASSETS[0]);
  const [network, setNetwork] = useState(NETWORKS[ASSETS[0]][0]);
  const [addressInfo, setAddressInfo] = useState<DepositAddress | null>(null);
  const [loading, setLoading] = useState(false);
  const [history, setHistory] = useState<DepositRecord[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);

  useEffect(() => {
    const nets = NETWORKS[asset];
    if (!nets.includes(network)) {
      setNetwork(nets[0]);
    }
  }, [asset, network]);

  useEffect(() => {
    setLoading(true);
    setAddressInfo(null);
    getDepositAddress(asset, network)
      .then(setAddressInfo)
      .catch(() => setAddressInfo(null))
      .finally(() => setLoading(false));
  }, [asset, network]);

  useEffect(() => {
    setHistoryLoading(true);
    getDeposits(20)
      .then(setHistory)
      .catch(() => setHistory([]))
      .finally(() => setHistoryLoading(false));
  }, []);

  const assetOptions = ASSETS.map((a) => ({ value: a, label: a }));
  const networkOptions = (NETWORKS[asset] ?? []).map((n) => ({ value: n, label: n }));
  const txRows: TxRow[] = history.map((t) => ({
    asset: t.asset,
    amount: t.amount,
    status: t.status,
    created_at: t.created_at,
    tx_hash: t.tx_hash ?? '',
  }));

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

      <div className={styles.addressCard}>
        {loading ? (
          <div className={styles.center}><Spinner /></div>
        ) : addressInfo ? (
          <>
            <div className={styles.addressRow}>
              <div className={styles.qrPlaceholder} aria-label="QR code">
                <img
                  src={`https://api.qrserver.com/v1/create-qr-code/?size=120x120&data=${encodeURIComponent(addressInfo.address)}`}
                  alt="Deposit address QR code"
                  width={120}
                  height={120}
                  style={{ borderRadius: 'var(--radius-sm)' }}
                />
              </div>
              <div className={styles.addressDetails}>
                <p className={styles.label}>Deposit Address</p>
                <CopyableText value={addressInfo.address} />
                {addressInfo.memo && (
                  <>
                    <p className={styles.label}>Memo / Tag</p>
                    <CopyableText value={addressInfo.memo} />
                  </>
                )}
              </div>
            </div>
          </>
        ) : (
          <p className={styles.empty}>Could not load deposit address.</p>
        )}
      </div>

      <div className={styles.history}>
        <h3 className={styles.historyTitle}>Recent Deposits</h3>
        <Table<TxRow>
          columns={TX_COLUMNS}
          data={txRows}
          loading={historyLoading}
          emptyNode={
            <EmptyState
              title="No deposits yet"
              description="Select an asset and network above to get your deposit address"
            />
          }
        />
      </div>
    </div>
  );
}
