import { useState, useEffect } from 'react';
import { Button } from '@shared/ui/Button';
import { Toggle } from '@shared/ui/Toggle';
import { toast } from '@shared/ui/Toast';
import { get, put } from '@shared/api/client';
import styles from './AccountPage.module.css';

interface NotifPrefs {
  email_trades: boolean;
  email_deposits: boolean;
  email_withdrawals: boolean;
  email_security: boolean;
  email_promotions: boolean;
  push_price_alerts: boolean;
  push_liquidation: boolean;
  push_funding: boolean;
}

const PREFS_LABELS: { key: keyof NotifPrefs; label: string; desc: string }[] = [
  { key: 'email_trades', label: 'Trade Confirmations', desc: 'Email on order fill' },
  { key: 'email_deposits', label: 'Deposit Alerts', desc: 'Email when funds arrive' },
  { key: 'email_withdrawals', label: 'Withdrawal Alerts', desc: 'Email on withdrawal status change' },
  { key: 'email_security', label: 'Security Alerts', desc: 'Email on login or 2FA changes' },
  { key: 'email_promotions', label: 'Promotions', desc: 'News, campaigns, and offers' },
  { key: 'push_price_alerts', label: 'Price Alerts', desc: 'Push notification on price alert' },
  { key: 'push_liquidation', label: 'Liquidation Warnings', desc: 'Push when margin ratio is low' },
  { key: 'push_funding', label: 'Funding Rate', desc: 'Push on funding payment' },
];

const DEFAULT_PREFS: NotifPrefs = {
  email_trades: true, email_deposits: true, email_withdrawals: true,
  email_security: true, email_promotions: false, push_price_alerts: true,
  push_liquidation: true, push_funding: false,
};

export default function NotificationsPage() {
  const [prefs, setPrefs] = useState<NotifPrefs>(DEFAULT_PREFS);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    get<NotifPrefs>('/account/notifications')
      .then(setPrefs)
      .catch(() => setPrefs(DEFAULT_PREFS))
      .finally(() => setLoading(false));
  }, []);

  async function handleSave() {
    setSaving(true);
    try {
      await put('/account/notifications', prefs);
      toast.success('Preferences saved');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to save');
    } finally {
      setSaving(false);
    }
  }

  function toggle(key: keyof NotifPrefs) {
    setPrefs((p) => ({ ...p, [key]: !p[key] }));
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Notification Preferences</h1>
      {loading ? <p>Loading...</p> : (
        <>
          <div className={styles.toggleGrid}>
            {PREFS_LABELS.map(({ key, label, desc }) => (
              <div key={key} className={styles.toggleItem}>
                <Toggle
                  checked={prefs[key]}
                  onChange={() => toggle(key)}
                  label={label}
                  description={desc}
                />
              </div>
            ))}
          </div>
          <Button variant="primary" loading={saving} onClick={() => { void handleSave(); }} style={{ marginTop: 'var(--pad)' }}>
            Save Preferences
          </Button>
        </>
      )}
    </div>
  );
}
