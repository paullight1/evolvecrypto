import { useState, useEffect } from 'react';
import { Card } from '@shared/ui/Card';
import { Badge } from '@shared/ui/Badge';
import { Button } from '@shared/ui/Button';
import { Steps } from '@shared/ui/Steps';
import { toast } from '@shared/ui/Toast';
import { getKycStatus, startKyc } from './api';
import type { KycStatus } from './types';
import styles from './KycPage.module.css';

type BadgeVariant = 'neutral' | 'warning' | 'success' | 'danger';

function kycVariant(status: string): BadgeVariant {
  if (status === 'approved') return 'success';
  if (status === 'pending') return 'warning';
  if (status === 'rejected') return 'danger';
  return 'neutral';
}

export default function KycPage() {
  const [kycStatus, setKycStatus] = useState<KycStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [starting, setStarting] = useState(false);

  useEffect(() => {
    getKycStatus()
      .then(setKycStatus)
      .catch(() => setKycStatus(null))
      .finally(() => setLoading(false));
  }, []);

  async function handleStart(level: number) {
    setStarting(true);
    try {
      await startKyc(level);
      toast.success('KYC application submitted');
      const updated = await getKycStatus();
      setKycStatus(updated);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to start KYC');
    } finally {
      setStarting(false);
    }
  }

  if (loading) return <div className={styles.page}><p>Loading...</p></div>;

  const status = kycStatus?.status ?? 'none';

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>KYC Verification</h1>

      <Steps
        currentStep={status === 'none' ? 0 : status === 'pending' ? 1 : status === 'approved' ? 3 : 0}
        steps={[
          { key: 'submit', label: 'Submit ID', description: 'Upload government-issued ID' },
          { key: 'review', label: 'Under Review', description: 'Verification takes 24–48 hours' },
          { key: 'verified', label: 'Verified', description: 'Full access to all features' },
        ]}
      />

      <Card padding="md" className={styles.statusCard}>
        <div className={styles.statusRow}>
          <span className={styles.statusLabel}>Status</span>
          <Badge variant={kycVariant(status)} size="sm">{status.toUpperCase()}</Badge>
        </div>
        {kycStatus && kycStatus.level > 0 && (
          <div className={styles.statusRow}>
            <span className={styles.statusLabel}>Level</span>
            <span>{kycStatus.level}</span>
          </div>
        )}
        {kycStatus?.submitted_at && (
          <div className={styles.statusRow}>
            <span className={styles.statusLabel}>Submitted</span>
            <span>{new Date(kycStatus.submitted_at * 1000).toLocaleDateString()}</span>
          </div>
        )}
        {status === 'rejected' && kycStatus?.rejection_reason && (
          <div className={styles.reason}>Reason: {kycStatus.rejection_reason}</div>
        )}
      </Card>

      {(status === 'none' || status === 'rejected') && (
        <Card padding="md">
          <h2 style={{ marginBottom: 'var(--pad-sm)', fontSize: 'var(--text-sm)' }}>Identity Verification</h2>
          <p style={{ color: 'var(--text-2)', fontSize: 'var(--text-xs)', marginBottom: 'var(--pad-sm)' }}>
            Upload a government-issued ID (passport, driver&apos;s license, or national ID card).
            Accepted formats: JPG, PNG, PDF. Max 5MB per file.
          </p>
          <input
            type="file"
            accept="image/*,.pdf"
            style={{ fontSize: 'var(--text-sm)', color: 'var(--text-1)' }}
            onChange={(e) => {
              const file = e.target.files?.[0];
              if (file && file.size > 5 * 1024 * 1024) {
                e.target.value = '';
                alert('File too large. Maximum size is 5MB.');
              }
            }}
          />
          <div className={styles.actions} style={{ marginTop: 'var(--pad)' }}>
            <Button variant="primary" loading={starting} onClick={() => { void handleStart(1); }}>
              Start KYC Level 1
            </Button>
            <Button variant="secondary" loading={starting} onClick={() => { void handleStart(2); }}>
              Start KYC Level 2
            </Button>
          </div>
        </Card>
      )}

      {status === 'pending' && (
        <Card padding="md">
          <p style={{ color: 'var(--text-2)', fontSize: 'var(--text-sm)' }}>
            Your application is under review. This typically takes 1-3 business days.
          </p>
        </Card>
      )}

      {status === 'approved' && (
        <Card padding="md">
          <p style={{ color: 'var(--color-buy)', fontSize: 'var(--text-sm)' }}>
            Verification complete. You have full access to all platform features.
          </p>
        </Card>
      )}
    </div>
  );
}
