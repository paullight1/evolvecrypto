import { useState, useEffect } from 'react';
import { Card } from '@shared/ui/Card';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { get, post } from '@shared/api/client';
import styles from './AccountPage.module.css';

interface AccountMode {
  mode: 'standard' | 'unified';
  description: string;
}

export default function AccountModePage() {
  const [current, setCurrent] = useState<AccountMode | null>(null);
  const [loading, setLoading] = useState(true);
  const [switching, setSwitching] = useState(false);

  useEffect(() => {
    get<AccountMode>('/account/mode')
      .then(setCurrent)
      .catch(() => setCurrent(null))
      .finally(() => setLoading(false));
  }, []);

  async function handleSwitch(mode: 'standard' | 'unified') {
    setSwitching(true);
    try {
      const updated = await post<AccountMode>('/account/mode', { mode });
      setCurrent(updated as AccountMode);
      toast.success('Account mode updated');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to switch mode');
    } finally {
      setSwitching(false);
    }
  }

  const modes = [
    {
      key: 'standard' as const,
      label: 'Standard Mode',
      desc: 'Separate margin wallets for each trading type. Classic portfolio management.',
    },
    {
      key: 'unified' as const,
      label: 'Unified Margin Mode',
      desc: 'Single margin pool across all products. Higher capital efficiency, cross-product netting.',
    },
  ];

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Account Mode</h1>
      <p className={styles.subtitle}>
        Switch between Standard and Unified Margin modes. Changing mode may affect open positions.
      </p>

      {loading ? <p>Loading...</p> : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--pad-sm)' }}>
          {modes.map((m) => (
            <Card key={m.key} padding="md">
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
                <div>
                  <div style={{ fontSize: 'var(--text-md)', fontWeight: 'var(--font-bold)', color: 'var(--text-0)', marginBottom: '4px' }}>
                    {m.label}
                  </div>
                  <div style={{ fontSize: 'var(--text-sm)', color: 'var(--text-2)' }}>{m.desc}</div>
                </div>
                {current?.mode === m.key ? (
                  <span style={{ fontSize: 'var(--text-xs)', color: 'var(--color-buy)', fontWeight: 'var(--font-bold)' }}>
                    Active
                  </span>
                ) : (
                  <Button
                    variant="secondary"
                    size="sm"
                    loading={switching}
                    onClick={() => { void handleSwitch(m.key); }}
                  >
                    Switch
                  </Button>
                )}
              </div>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
