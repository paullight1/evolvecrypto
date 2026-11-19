import { useState } from 'react';
import { Card } from '@shared/ui/Card';
import { Select } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { post } from '@shared/api/client';
import styles from './InfoPage.module.css';

const ASSET_OPTIONS = [
  { value: 'USDT', label: 'USDT (10,000)' },
  { value: 'BTC', label: 'BTC (1.0)' },
  { value: 'ETH', label: 'ETH (10.0)' },
];

export default function TestnetPage() {
  const [asset, setAsset] = useState('USDT');
  const [requesting, setRequesting] = useState(false);

  async function handleRequest() {
    setRequesting(true);
    try {
      await post('/testnet/faucet', { asset });
      toast.success(`Testnet ${asset} sent to your account`);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Faucet request failed');
    } finally {
      setRequesting(false);
    }
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Testnet Faucet</h1>
      <p className={styles.subtitle}>Request testnet tokens to practice trading without real funds.</p>

      <Card padding="md" className={styles.faucetCard}>
        <div className={styles.faucetForm}>
          <Select
            label="Asset"
            options={ASSET_OPTIONS}
            value={asset}
            onChange={(e) => setAsset(e.target.value)}
          />
          <Button variant="primary" loading={requesting} onClick={() => { void handleRequest(); }}>
            Request Tokens
          </Button>
        </div>
      </Card>
    </div>
  );
}
