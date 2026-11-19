import { useState, useEffect } from 'react';
import { TokenList } from './components/TokenList';
import { PurchaseForm } from './components/PurchaseForm';
import { RedeemForm } from './components/RedeemForm';
import { getLeveragedTokens } from './api';
import type { LeveragedToken } from './types';
import styles from './LeveragedTokensPage.module.css';

export default function LeveragedTokensPage() {
  const [tokens, setTokens] = useState<LeveragedToken[]>([]);
  const [loading, setLoading] = useState(false);
  const [purchaseTarget, setPurchaseTarget] = useState<LeveragedToken | null>(null);
  const [redeemTarget, setRedeemTarget] = useState<LeveragedToken | null>(null);

  function load() {
    setLoading(true);
    getLeveragedTokens()
      .then(setTokens)
      .catch(() => setTokens([]))
      .finally(() => setLoading(false));
  }

  useEffect(() => { load(); }, []);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Leveraged Tokens</h1>
      <div className={styles.content}>
        <TokenList
          tokens={tokens}
          loading={loading}
          onPurchase={setPurchaseTarget}
          onRedeem={setRedeemTarget}
        />
      </div>

      <PurchaseForm
        token={purchaseTarget}
        onClose={() => setPurchaseTarget(null)}
        onSuccess={load}
      />
      <RedeemForm
        token={redeemTarget}
        onClose={() => setRedeemTarget(null)}
        onSuccess={load}
      />
    </div>
  );
}
