import { useState, useEffect } from 'react';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { Input } from '@shared/ui/Input';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { get, post, del } from '@shared/api/client';
import styles from './AccountPage.module.css';

interface Address {
  id: string;
  label: string;
  asset: string;
  network: string;
  address: string;
}

interface AddrRow extends Record<string, unknown> {
  id: string;
  label: string;
  asset: string;
  network: string;
  address: string;
}

export default function AddressBookPage() {
  const [addresses, setAddresses] = useState<Address[]>([]);
  const [loading, setLoading] = useState(true);
  const [label, setLabel] = useState('');
  const [asset, setAsset] = useState('');
  const [network, setNetwork] = useState('');
  const [address, setAddress] = useState('');
  const [adding, setAdding] = useState(false);

  function loadAddresses() {
    setLoading(true);
    get<Address[]>('/account/address-book')
      .then(setAddresses)
      .catch(() => setAddresses([]))
      .finally(() => setLoading(false));
  }

  useEffect(() => { loadAddresses(); }, []);

  async function handleAdd() {
    if (!label || !asset || !network || !address) return;
    setAdding(true);
    try {
      await post('/account/address-book', { label, asset, network, address });
      toast.success('Address saved');
      setLabel(''); setAsset(''); setNetwork(''); setAddress('');
      loadAddresses();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed');
    } finally {
      setAdding(false);
    }
  }

  async function handleRemove(id: string) {
    try {
      await del(`/account/address-book/${id}`);
      toast.success('Address removed');
      loadAddresses();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed');
    }
  }

  const COLUMNS: ColumnDef<AddrRow>[] = [
    { key: 'label', header: 'Label' },
    { key: 'asset', header: 'Asset', width: '80px' },
    { key: 'network', header: 'Network', width: '100px' },
    { key: 'address', header: 'Address', render: (r) => <span title={String(r.address)}>{String(r.address).slice(0, 20)}…</span> },
    { key: 'id', header: '',
      render: (r) => <Button variant="danger" size="sm" onClick={() => { void handleRemove(String(r.id)); }}>Remove</Button> },
  ];

  const rows: AddrRow[] = addresses.map((a) => ({
    id: a.id, label: a.label, asset: a.asset, network: a.network, address: a.address,
  }));

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Address Book</h1>

      <div className={styles.form} style={{ maxWidth: '100%' }}>
        <div className={styles.formRow}>
          <Input label="Label" value={label} onChange={(e) => setLabel(e.target.value)} placeholder="My BTC Wallet" />
          <Input label="Asset" value={asset} onChange={(e) => setAsset(e.target.value)} placeholder="BTC" />
          <Input label="Network" value={network} onChange={(e) => setNetwork(e.target.value)} placeholder="BTC" />
        </div>
        <div className={styles.formRow}>
          <Input label="Address" value={address} onChange={(e) => setAddress(e.target.value)} placeholder="bc1q..." />
          <Button variant="primary" loading={adding} onClick={() => { void handleAdd(); }}>Add</Button>
        </div>
      </div>

      <Table<AddrRow> columns={COLUMNS} data={rows} loading={loading} emptyText="No saved addresses" />
    </div>
  );
}
