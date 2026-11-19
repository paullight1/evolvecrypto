import { useState, useEffect } from 'react';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { Button } from '@shared/ui/Button';
import { Badge } from '@shared/ui/Badge';
import { ConfirmDialog } from '@shared/ui/ConfirmDialog';
import { getSessions, revokeSession } from '../../api';
import { toast } from '@shared/ui/Toast';
import type { Session } from '../../types';
import styles from './SessionList.module.css';

interface SessionRow extends Record<string, unknown> {
  id: string;
  ip: string;
  device: string;
  last_active: number;
  isCurrent: boolean;
}

export function SessionList() {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [loading, setLoading] = useState(false);
  const [revokeTarget, setRevokeTarget] = useState<string | null>(null);

  function loadSessions() {
    setLoading(true);
    getSessions()
      .then(setSessions)
      .catch(() => setSessions([]))
      .finally(() => setLoading(false));
  }

  useEffect(() => { loadSessions(); }, []);

  async function handleRevoke() {
    if (!revokeTarget) return;
    try {
      await revokeSession(revokeTarget);
      toast.success('Session revoked');
      loadSessions();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to revoke session');
    } finally {
      setRevokeTarget(null);
    }
  }

  const columns: ColumnDef<SessionRow>[] = [
    { key: 'device', header: 'Device' },
    { key: 'ip', header: 'IP Address' },
    {
      key: 'last_active',
      header: 'Last Active',
      render: (row) => new Date((row.last_active as number) * 1000).toLocaleString(),
    },
    {
      key: 'isCurrent',
      header: 'Status',
      render: (row) =>
        row.isCurrent ? (
          <Badge variant="success">Current</Badge>
        ) : (
          <Badge variant="neutral">Active</Badge>
        ),
    },
    {
      key: '_action',
      header: '',
      align: 'right',
      render: (row) =>
        row.isCurrent ? null : (
          <Button variant="danger" size="sm" onClick={() => setRevokeTarget(String(row.id))}>
            Revoke
          </Button>
        ),
    },
  ];

  const rows: SessionRow[] = sessions.map((s, i) => ({
    id: s.id,
    ip: s.ip,
    device: s.device,
    last_active: s.last_active,
    isCurrent: i === 0,
  }));

  return (
    <div className={styles.container}>
      <Table<SessionRow>
        columns={columns}
        data={rows}
        loading={loading}
        emptyText="No active sessions"
      />

      <ConfirmDialog
        open={revokeTarget !== null}
        onConfirm={() => { void handleRevoke(); }}
        onCancel={() => setRevokeTarget(null)}
        title="Revoke Session"
        message="This will sign out the device associated with this session."
        confirmText="Revoke Session"
        variant="danger"
      />
    </div>
  );
}
