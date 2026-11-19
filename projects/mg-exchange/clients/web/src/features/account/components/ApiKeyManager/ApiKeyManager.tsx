import { useState, useEffect } from 'react';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { Button } from '@shared/ui/Button';
import { Input } from '@shared/ui/Input';
import { Modal } from '@shared/ui/Modal';
import { ConfirmDialog } from '@shared/ui/ConfirmDialog';
import { CopyableText } from '@shared/ui/CopyableText';
import { Checkbox } from '@shared/ui/Checkbox';
import { getApiKeys, createApiKey, revokeApiKey } from '../../api';
import { toast } from '@shared/ui/Toast';
import type { ApiKey } from '../../types';
import styles from './ApiKeyManager.module.css';

const PERMISSION_OPTIONS = ['read', 'trade', 'withdraw'];

interface KeyRow extends Record<string, unknown> {
  key: string;
  label: string;
  permissions: string;
  created_at: number;
}

export function ApiKeyManager() {
  const [keys, setKeys] = useState<ApiKey[]>([]);
  const [loading, setLoading] = useState(false);
  const [showCreate, setShowCreate] = useState(false);
  const [newLabel, setNewLabel] = useState('');
  const [newPerms, setNewPerms] = useState<string[]>(['read']);
  const [creating, setCreating] = useState(false);
  const [secret, setSecret] = useState('');
  const [revokeTarget, setRevokeTarget] = useState<string | null>(null);

  function loadKeys() {
    setLoading(true);
    getApiKeys()
      .then(setKeys)
      .catch(() => setKeys([]))
      .finally(() => setLoading(false));
  }

  useEffect(() => { loadKeys(); }, []);

  async function handleCreate() {
    if (!newLabel.trim()) return;
    setCreating(true);
    try {
      const res = await createApiKey({ label: newLabel, permissions: newPerms });
      setSecret(res.secret);
      setNewLabel('');
      setNewPerms(['read']);
      loadKeys();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to create API key');
    } finally {
      setCreating(false);
    }
  }

  async function handleRevoke() {
    if (!revokeTarget) return;
    try {
      await revokeApiKey(revokeTarget);
      toast.success('API key revoked');
      loadKeys();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to revoke key');
    } finally {
      setRevokeTarget(null);
    }
  }

  function togglePerm(perm: string) {
    setNewPerms((prev) =>
      prev.includes(perm) ? prev.filter((p) => p !== perm) : [...prev, perm],
    );
  }

  const columns: ColumnDef<KeyRow>[] = [
    { key: 'label', header: 'Label' },
    { key: 'key', header: 'Key', render: (row) => <CopyableText value={String(row.key)} displayValue={`${String(row.key).slice(0, 8)}...`} /> },
    { key: 'permissions', header: 'Permissions' },
    {
      key: 'created_at',
      header: 'Created',
      render: (row) => new Date((row.created_at as number) * 1000).toLocaleDateString(),
    },
    {
      key: '_action',
      header: '',
      align: 'right',
      render: (row) => (
        <Button variant="danger" size="sm" onClick={() => setRevokeTarget(String(row.key))}>
          Revoke
        </Button>
      ),
    },
  ];

  const rows: KeyRow[] = keys.map((k) => ({
    key: k.key,
    label: k.label,
    permissions: k.permissions.join(', '),
    created_at: k.created_at,
  }));

  return (
    <div className={styles.container}>
      <div className={styles.toolbar}>
        <h3 className={styles.title}>API Keys</h3>
        <Button variant="secondary" size="sm" onClick={() => setShowCreate(true)}>
          + New Key
        </Button>
      </div>

      <Table<KeyRow>
        columns={columns}
        data={rows}
        loading={loading}
        emptyText="No API keys"
      />

      <Modal open={showCreate} onClose={() => setShowCreate(false)} title="Create API Key">
        {secret ? (
          <div className={styles.secretPanel}>
            <p className={styles.secretWarning}>
              Copy your secret now — it will not be shown again.
            </p>
            <CopyableText value={secret} />
            <Button variant="primary" fullWidth onClick={() => { setSecret(''); setShowCreate(false); }}>
              Done
            </Button>
          </div>
        ) : (
          <div className={styles.createForm}>
            <Input
              label="Label"
              placeholder="e.g. Trading Bot"
              value={newLabel}
              onChange={(e) => setNewLabel(e.target.value)}
            />
            <div>
              <p className={styles.permLabel}>Permissions</p>
              <div className={styles.permOptions}>
                {PERMISSION_OPTIONS.map((perm) => (
                  <Checkbox
                    key={perm}
                    checked={newPerms.includes(perm)}
                    onChange={() => togglePerm(perm)}
                    label={perm}
                  />
                ))}
              </div>
            </div>
            <Button
              variant="primary"
              fullWidth
              loading={creating}
              onClick={() => { void handleCreate(); }}
            >
              Create Key
            </Button>
          </div>
        )}
      </Modal>

      <ConfirmDialog
        open={revokeTarget !== null}
        onConfirm={() => { void handleRevoke(); }}
        onCancel={() => setRevokeTarget(null)}
        title="Revoke API Key"
        message="This key will be permanently revoked and can no longer be used."
        confirmText="Revoke"
        variant="danger"
      />
    </div>
  );
}
