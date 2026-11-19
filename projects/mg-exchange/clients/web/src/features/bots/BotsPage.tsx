import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { Card } from '@shared/ui/Card';
import { Badge } from '@shared/ui/Badge';
import { GridBotForm } from './components/GridBotForm';
import { DcaBotForm } from './components/DcaBotForm';
import { BotList } from './components/BotList';
import { getMyBots } from './api';
import type { Bot } from './types';
import styles from './BotsPage.module.css';

const TABS: TabDef[] = [
  { key: 'marketplace', label: 'Marketplace' },
  { key: 'my-bots', label: 'My Bots' },
];

type BotTypeKey = 'grid' | 'dca';

interface BotTypeOption {
  key: BotTypeKey;
  label: string;
  description: string;
  badge: string;
}

const BOT_TYPES: BotTypeOption[] = [
  {
    key: 'grid',
    label: 'Grid Bot',
    description: 'Automatically buys low and sells high within a price range using a grid of orders.',
    badge: 'Popular',
  },
  {
    key: 'dca',
    label: 'DCA Bot',
    description: 'Dollar-cost averaging — invest a fixed amount at regular intervals to reduce timing risk.',
    badge: 'Safe',
  },
];

export default function BotsPage() {
  const [tab, setTab] = useState('marketplace');
  const [selectedType, setSelectedType] = useState<BotTypeKey | null>(null);
  const [bots, setBots] = useState<Bot[]>([]);
  const [botsLoading, setBotsLoading] = useState(false);

  function loadBots() {
    setBotsLoading(true);
    getMyBots()
      .then(setBots)
      .catch(() => setBots([]))
      .finally(() => setBotsLoading(false));
  }

  useEffect(() => {
    if (tab === 'my-bots') loadBots();
  }, [tab]);

  function handleBotCreated() {
    setSelectedType(null);
    setTab('my-bots');
    loadBots();
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Trading Bots</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'marketplace' && (
          <>
            {selectedType === null ? (
              <div className={styles.typeGrid}>
                {BOT_TYPES.map((bt) => (
                  <Card
                    key={bt.key}
                    className={styles.typeCard}
                    padding="md"
                  >
                    <div className={styles.typeHeader}>
                      <span className={styles.typeLabel}>{bt.label}</span>
                      <Badge variant="info" size="sm">{bt.badge}</Badge>
                    </div>
                    <p className={styles.typeDesc}>{bt.description}</p>
                    <button
                      type="button"
                      className={styles.typeButton}
                      onClick={() => setSelectedType(bt.key)}
                    >
                      Create {bt.label}
                    </button>
                  </Card>
                ))}
              </div>
            ) : (
              <div className={styles.formWrapper}>
                <button
                  type="button"
                  className={styles.backButton}
                  onClick={() => setSelectedType(null)}
                >
                  ← Back to Marketplace
                </button>
                <h2 className={styles.formTitle}>
                  {selectedType === 'grid' ? 'Create Grid Bot' : 'Create DCA Bot'}
                </h2>
                {selectedType === 'grid' && (
                  <GridBotForm onCreated={handleBotCreated} />
                )}
                {selectedType === 'dca' && (
                  <DcaBotForm onCreated={handleBotCreated} />
                )}
              </div>
            )}
          </>
        )}

        {tab === 'my-bots' && (
          botsLoading ? (
            <div className={styles.loadingGrid}>
              {Array.from({ length: 3 }, (_, i) => (
                <div key={i} className={styles.skeletonCard} />
              ))}
            </div>
          ) : (
            <BotList bots={bots} onUpdated={loadBots} />
          )
        )}
      </div>
    </div>
  );
}
