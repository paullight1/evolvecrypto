import { useState, useEffect } from 'react';
import { Input } from '@shared/ui/Input';
import { Select } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { CopyableText } from '@shared/ui/CopyableText';
import { get, post } from '@shared/api/client';
import styles from './AccountPage.module.css';

interface GiftCard {
  id: string;
  code: string;
  amount: string;
  asset: string;
  status: 'active' | 'redeemed';
  created_at: number;
}

const AMOUNT_OPTIONS = [
  { value: '10', label: '$10' },
  { value: '25', label: '$25' },
  { value: '50', label: '$50' },
  { value: '100', label: '$100' },
];

export default function GiftCardsPage() {
  const [cards, setCards] = useState<GiftCard[]>([]);
  const [loading, setLoading] = useState(true);
  const [amount, setAmount] = useState('10');
  const [redeemCode, setRedeemCode] = useState('');
  const [purchasing, setPurchasing] = useState(false);
  const [redeeming, setRedeeming] = useState(false);

  function loadCards() {
    setLoading(true);
    get<GiftCard[]>('/gift-cards').then(setCards).catch(() => setCards([])).finally(() => setLoading(false));
  }

  useEffect(() => { loadCards(); }, []);

  async function handleCreate() {
    setPurchasing(true);
    try {
      await post('/gift-cards', { amount, asset: 'USDT' });
      toast.success('Gift card created');
      loadCards();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed');
    } finally {
      setPurchasing(false);
    }
  }

  async function handleRedeem() {
    if (!redeemCode.trim()) return;
    setRedeeming(true);
    try {
      await post('/gift-cards/redeem', { code: redeemCode.trim() });
      toast.success('Gift card redeemed');
      setRedeemCode('');
      loadCards();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed');
    } finally {
      setRedeeming(false);
    }
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Gift Cards</h1>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 'var(--pad)', marginBottom: 'var(--pad)' }}>
        <div className={styles.form}>
          <h2 className={styles.sectionTitle}>Create</h2>
          <Select label="Amount" options={AMOUNT_OPTIONS} value={amount} onChange={(e) => setAmount(e.target.value)} />
          <Button variant="primary" loading={purchasing} onClick={() => { void handleCreate(); }}>Purchase</Button>
        </div>
        <div className={styles.form}>
          <h2 className={styles.sectionTitle}>Redeem</h2>
          <Input label="Code" value={redeemCode} onChange={(e) => setRedeemCode(e.target.value)} placeholder="XXXX-XXXX-XXXX" />
          <Button variant="secondary" loading={redeeming} onClick={() => { void handleRedeem(); }}>Redeem</Button>
        </div>
      </div>

      <h2 className={styles.sectionTitle}>My Gift Cards</h2>
      {loading ? <p>Loading...</p> : (
        <div className={styles.giftCardGrid}>
          {cards.map((c) => (
            <div key={c.id} className={styles.giftCard} style={{ opacity: c.status === 'redeemed' ? 0.5 : 1 }}>
              <div className={styles.giftAmount}>{c.amount} {c.asset}</div>
              <CopyableText value={c.code} />
              <div style={{ fontSize: 'var(--text-xs)', color: 'var(--text-2)', marginTop: '4px' }}>{c.status}</div>
            </div>
          ))}
          {cards.length === 0 && <p style={{ color: 'var(--text-2)' }}>No gift cards</p>}
        </div>
      )}
    </div>
  );
}
