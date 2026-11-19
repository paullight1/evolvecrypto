import { useState, useEffect } from 'react';
import { Input } from '@shared/ui/Input';
import { Button } from '@shared/ui/Button';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { Badge } from '@shared/ui/Badge';
import { toast } from '@shared/ui/Toast';
import { get, post } from '@shared/api/client';
import styles from './AccountPage.module.css';

interface Complaint {
  id: string;
  subject: string;
  status: string;
  created_at: number;
}

interface ComplaintRow extends Record<string, unknown> {
  id: string;
  subject: string;
  status: string;
  created_at: number;
}

type BadgeVariant = 'neutral' | 'warning' | 'success' | 'danger';
function statusVariant(status: string): BadgeVariant {
  if (status === 'resolved') return 'success';
  if (status === 'open') return 'warning';
  return 'neutral';
}

const COLS: ColumnDef<ComplaintRow>[] = [
  { key: 'subject', header: 'Subject' },
  { key: 'status', header: 'Status', width: '100px',
    render: (r) => <Badge variant={statusVariant(String(r.status))} size="sm">{String(r.status)}</Badge> },
  { key: 'created_at', header: 'Date', render: (r) => new Date((r.created_at as number) * 1000).toLocaleDateString() },
];

export default function ComplaintsPage() {
  const [complaints, setComplaints] = useState<Complaint[]>([]);
  const [loading, setLoading] = useState(true);
  const [subject, setSubject] = useState('');
  const [description, setDescription] = useState('');
  const [submitting, setSubmitting] = useState(false);

  function loadComplaints() {
    get<Complaint[]>('/account/complaints')
      .then(setComplaints)
      .catch(() => setComplaints([]))
      .finally(() => setLoading(false));
  }

  useEffect(() => { loadComplaints(); }, []);

  async function handleSubmit() {
    if (!subject.trim() || !description.trim()) return;
    setSubmitting(true);
    try {
      await post('/account/complaints', { subject: subject.trim(), description: description.trim() });
      toast.success('Complaint submitted');
      setSubject('');
      setDescription('');
      loadComplaints();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed');
    } finally {
      setSubmitting(false);
    }
  }

  const rows: ComplaintRow[] = complaints.map((c) => ({
    id: c.id, subject: c.subject, status: c.status, created_at: c.created_at,
  }));

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Complaints</h1>

      <div className={styles.form} style={{ marginBottom: 'var(--pad)' }}>
        <Input label="Subject" value={subject} onChange={(e) => setSubject(e.target.value)} placeholder="Brief summary" />
        <Input label="Description" value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Detailed description of your issue" />
        <Button variant="primary" loading={submitting} onClick={() => { void handleSubmit(); }}>Submit</Button>
      </div>

      <h2 className={styles.sectionTitle}>My Complaints</h2>
      <Table<ComplaintRow> columns={COLS} data={rows} loading={loading} emptyText="No complaints" />
    </div>
  );
}
