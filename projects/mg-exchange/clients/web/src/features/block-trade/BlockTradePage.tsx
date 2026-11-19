import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { Select, type SelectOption } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { RfqForm } from './components/RfqForm';
import { QuoteList } from './components/QuoteList';
import { BlockTradeHistory } from './components/BlockTradeHistory';
import { getRfqQuotes, getBlockTradeHistory } from './api';
import type { RfqQuote, BlockTrade } from './types';
import styles from './BlockTradePage.module.css';

const TABS: TabDef[] = [
  { key: 'rfq', label: 'New RFQ' },
  { key: 'quotes', label: 'Active Quotes' },
  { key: 'history', label: 'History' },
];

export default function BlockTradePage() {
  const [tab, setTab] = useState('rfq');
  const [rfqId, setRfqId] = useState('');
  const [rfqInput, setRfqInput] = useState('');
  const [quotes, setQuotes] = useState<RfqQuote[]>([]);
  const [quotesLoading, setQuotesLoading] = useState(false);
  const [trades, setTrades] = useState<BlockTrade[]>([]);
  const [tradesLoading, setTradesLoading] = useState(false);

  const rfqOptions: SelectOption[] = rfqId
    ? [{ value: rfqId, label: `RFQ ${rfqId.slice(0, 8)}...` }]
    : [];

  function loadQuotes(id: string) {
    setQuotesLoading(true);
    getRfqQuotes(id)
      .then(setQuotes)
      .catch(() => setQuotes([]))
      .finally(() => setQuotesLoading(false));
  }

  function loadTrades() {
    setTradesLoading(true);
    getBlockTradeHistory()
      .then(setTrades)
      .catch(() => setTrades([]))
      .finally(() => setTradesLoading(false));
  }

  useEffect(() => {
    if (tab === 'quotes' && rfqId) loadQuotes(rfqId);
    if (tab === 'history') loadTrades();
  }, [tab, rfqId]);

  function handleRfqCreated() {
    setTab('quotes');
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Block Trading</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'rfq' && <RfqForm onCreated={handleRfqCreated} />}

        {tab === 'quotes' && (
          <>
            <div className={styles.rfqRow}>
              <input
                className={styles.rfqInput}
                placeholder="Enter RFQ ID"
                value={rfqInput}
                onChange={(e) => setRfqInput(e.target.value)}
              />
              <Button
                variant="secondary"
                size="sm"
                onClick={() => { setRfqId(rfqInput); loadQuotes(rfqInput); }}
              >
                Load
              </Button>
              {rfqOptions.length > 0 && (
                <Select
                  options={rfqOptions}
                  value={rfqId}
                  onChange={(e) => setRfqId(e.target.value)}
                />
              )}
            </div>
            <QuoteList rfqId={rfqId} quotes={quotes} loading={quotesLoading} onExecuted={loadTrades} />
          </>
        )}

        {tab === 'history' && <BlockTradeHistory trades={trades} loading={tradesLoading} />}
      </div>
    </div>
  );
}
