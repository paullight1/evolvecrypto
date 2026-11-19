import { useState, useEffect } from 'react';
import { Card } from '@shared/ui/Card';
import { Input } from '@shared/ui/Input';
import { Button } from '@shared/ui/Button';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { toast } from '@shared/ui/Toast';
import { createSubAccount, getSubAccounts, getSubAccountBalances } from './api';
import type { SubAccount, SubAccountBalance } from './types';
import styles from './SubAccountsPage.module.css';

interface SubRow extends Record<string, unknown> {
  id: string;
  label: string;
  email: string;
  status: string;
  created_at: number;
}

const SUB_COLS: ColumnDef<SubRow>[] = [
  { key: 'label', header: 'Label' },
  { key: 'email', header: 'Email' },
  { key: 'status', header: 'Status', width: '90px' },
  { key: 'created_at', header: 'Created', render: (r) => new Date((r.created_at as number) * 1000).toLocaleDateString() },
];

interface BalRow extends Record<string, unknown> {
  asset: string;
  available: string;
  locked: string;
  total: string;
}

const BAL_COLS: ColumnDef<BalRow>[] = [
  { key: 'asset', header: 'Asset', width: '90px' },
  { key: 'available', header: 'Available', align: 'right' },
  { key: 'locked', header: 'Locked', align: 'right' },
  { key: 'total', header: 'Total', align: 'right' },
];

export default function SubAccountsPage() {
  const [subAccounts, setSubAccounts] = useState<SubAccount[]>([]);
  const [loading, setLoading] = useState(true);
  const [label, setLabel] = useState('');
  const [email, setEmail] = useState('');
  const [creating, setCreating] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [balances, setBalances] = useState<SubAccountBalance[]>([]);
  const [balLoading, setBalLoading] = useState(false);

  function loadAccounts() {
    setLoading(true);
    getSubAccounts().then(setSubAccounts).catch(() => setSubAccounts([])).finally(() => setLoading(false));
  }

  useEffect(() => { loadAccounts(); }, []);

  useEffect(() => {
    if (!selectedId) return;
    setBalLoading(true);
    getSubAccountBalances(selectedId).then(setBalances).catch(() => setBalances([])).finally(() => setBalLoading(false));
  }, [selectedId]);

  async function handleCreate() {
    if (!label.trim() || !email.trim()) return;
    setCreating(true);
    try {
      await createSubAccount(label.trim(), email.trim());
      toast.success('Sub-account created');
      setLabel('');
      setEmail('');
      loadAccounts();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to create');
    } finally {
      setCreating(false);
    }
  }

  const subRows: SubRow[] = subAccounts.map((s) => ({
    id: s.id, label: s.label, email: s.email, status: s.status, created_at: s.created_at,
  }));

  const balRows: BalRow[] = balances.map((b) => ({
    asset: b.asset, available: b.available, locked: b.locked, total: b.total,
  }));

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Sub-Accounts</h1>

      <Card padding="md">
        <h2 className={styles.sectionTitle}>Create Sub-Account</h2>
        <div className={styles.createForm}>
          <div className={styles.formRow}>
            <Input label="Label" value={label} onChange={(e) => setLabel(e.target.value)} placeholder="Trading Bot 1" />
            <Input label="Email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder="sub@example.com" />
            <Button variant="primary" loading={creating} onClick={() => { void handleCreate(); }}>Create</Button>
          </div>
        </div>
      </Card>

      <Card padding="md">
        <h2 className={styles.sectionTitle}>Sub-Accounts</h2>
        <Table<SubRow> columns={SUB_COLS} data={subRows} loading={loading} emptyText="No sub-accounts yet"
          onRowClick={(r) => setSelectedId(r.id)} />

        {selectedId && (
          <div className={styles.balanceSection}>
            <h3 className={styles.sectionTitle}>Balances for {subAccounts.find((s) => s.id === selectedId)?.label}</h3>
            <Table<BalRow> columns={BAL_COLS} data={balRows} loading={balLoading} emptyText="No balances" />
          </div>
        )}
      </Card>
    </div>
  );
}
