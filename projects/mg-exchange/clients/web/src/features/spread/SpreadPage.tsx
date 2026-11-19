import { useState, useEffect } from 'react';
import { SpreadList } from './components/SpreadList';
import { SpreadOrderForm } from './components/SpreadOrderForm';
import { getSpreadInstruments } from './api';
import type { SpreadInstrument } from './types';
import styles from './SpreadPage.module.css';

export default function SpreadPage() {
  const [instruments, setInstruments] = useState<SpreadInstrument[]>([]);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<SpreadInstrument | null>(null);

  useEffect(() => {
    setLoading(true);
    getSpreadInstruments()
      .then(setInstruments)
      .catch(() => setInstruments([]))
      .finally(() => setLoading(false));
  }, []);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Spread Trading</h1>

      <div className={styles.layout}>
        <div className={styles.listPanel}>
          <h2 className={styles.sectionTitle}>Instruments</h2>
          <SpreadList instruments={instruments} loading={loading} onSelect={setSelected} />
        </div>

        <div className={styles.formPanel}>
          <h2 className={styles.sectionTitle}>
            {selected ? `Order: ${selected.name}` : 'Place Order'}
          </h2>
          <SpreadOrderForm instrument={selected} />
        </div>
      </div>
    </div>
  );
}
