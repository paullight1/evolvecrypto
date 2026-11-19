import { useState, useEffect } from 'react';
import { Card } from '@shared/ui/Card';
import { Input } from '@shared/ui/Input';
import { Button } from '@shared/ui/Button';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { toast } from '@shared/ui/Toast';
import { get, post } from '@shared/api/client';
import styles from './InfoPage.module.css';

interface PorSummary {
  total_assets: string;
  total_liabilities: string;
  reserve_ratio: string;
  last_audit: number;
}

interface AssetReserve {
  asset: string;
  exchange_balance: string;
  user_balance: string;
  ratio: string;
}

interface AssetRow extends Record<string, unknown> {
  asset: string;
  exchange_balance: string;
  user_balance: string;
  ratio: string;
}

const COLS: ColumnDef<AssetRow>[] = [
  { key: 'asset', header: 'Asset', width: '90px' },
  { key: 'exchange_balance', header: 'Exchange Balance', align: 'right' },
  { key: 'user_balance', header: 'User Balance', align: 'right' },
  { key: 'ratio', header: 'Reserve Ratio', align: 'right' },
];

export default function ProofOfReservesPage() {
  const [summary, setSummary] = useState<PorSummary | null>(null);
  const [assets, setAssets] = useState<AssetReserve[]>([]);
  const [loading, setLoading] = useState(true);
  const [leaf, setLeaf] = useState('');
  const [verifying, setVerifying] = useState(false);

  useEffect(() => {
    Promise.all([
      get<PorSummary>('/proof-of-reserves/summary'),
      get<AssetReserve[]>('/proof-of-reserves/assets'),
    ])
      .then(([s, a]) => { setSummary(s); setAssets(a); })
      .catch(() => { setSummary(null); setAssets([]); })
      .finally(() => setLoading(false));
  }, []);

  async function handleVerify() {
    if (!leaf.trim()) return;
    setVerifying(true);
    try {
      const result = await post<{ verified: boolean; message: string }>('/proof-of-reserves/verify', { leaf });
      if ((result as { verified: boolean }).verified) {
        toast.success('Proof verified successfully');
      } else {
        toast.error((result as { message: string }).message ?? 'Verification failed');
      }
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Verification error');
    } finally {
      setVerifying(false);
    }
  }

  const rows: AssetRow[] = assets.map((a) => ({ asset: a.asset, exchange_balance: a.exchange_balance, user_balance: a.user_balance, ratio: a.ratio }));

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Proof of Reserves</h1>

      {!loading && summary && (
        <div className={styles.porStats}>
          <div className={styles.porStat}>
            <div className={styles.porStatLabel}>Reserve Ratio</div>
            <div className={styles.porStatValue}>{summary.reserve_ratio}</div>
          </div>
          <div className={styles.porStat}>
            <div className={styles.porStatLabel}>Total Assets</div>
            <div className={styles.porStatValue}>{summary.total_assets}</div>
          </div>
          <div className={styles.porStat}>
            <div className={styles.porStatLabel}>Last Audit</div>
            <div className={styles.porStatValue}>{new Date(summary.last_audit * 1000).toLocaleDateString()}</div>
          </div>
        </div>
      )}

      <Table<AssetRow> columns={COLS} data={rows} loading={loading} emptyText="No data" />

      <Card padding="md" className={styles.merkleCard}>
        <h2 style={{ fontSize: 'var(--text-sm)', fontWeight: 'var(--font-bold)', marginBottom: 'var(--pad-sm)' }}>
          Merkle Proof Verification
        </h2>
        <div className={styles.merkleForm}>
          <Input
            label="Your Merkle Leaf (account hash)"
            value={leaf}
            onChange={(e) => setLeaf(e.target.value)}
            placeholder="0x..."
          />
          <Button variant="primary" loading={verifying} onClick={() => { void handleVerify(); }}>
            Verify Inclusion
          </Button>
        </div>
      </Card>
    </div>
  );
}
