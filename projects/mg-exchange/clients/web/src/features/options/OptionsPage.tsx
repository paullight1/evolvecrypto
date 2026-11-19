import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { Select } from '@shared/ui/Select';
import { OptionsChain } from './components/OptionsChain/OptionsChain';
import { OptionOrderEntry } from './components/OptionOrderEntry/OptionOrderEntry';
import { OptionPositions } from './components/OptionPositions/OptionPositions';
import { PortfolioGreeks } from './components/PortfolioGreeks/PortfolioGreeks';
import { getOptionInstruments, getOptionTickers, getOptionPositions, getOptionGreeks } from './api';
import type { OptionInstrument, OptionTicker, OptionPosition, OptionGreeks } from './types';
import styles from './OptionsPage.module.css';

const TABS: TabDef[] = [
  { key: 'chain', label: 'Chain' },
  { key: 'positions', label: 'Positions' },
  { key: 'greeks', label: 'Greeks' },
];

const UNDERLYING_OPTIONS = [
  { value: 'BTC', label: 'BTC' },
  { value: 'ETH', label: 'ETH' },
];

export default function OptionsPage() {
  const [tab, setTab] = useState('chain');
  const [underlying, setUnderlying] = useState('BTC');
  const [expiry, setExpiry] = useState('');
  const [selectedSymbol, setSelectedSymbol] = useState('');
  const [instruments, setInstruments] = useState<OptionInstrument[]>([]);
  const [tickers, setTickers] = useState<OptionTicker[]>([]);
  const [positions, setPositions] = useState<OptionPosition[]>([]);
  const [greeks, setGreeks] = useState<OptionGreeks | null>(null);
  const [chainLoading, setChainLoading] = useState(false);
  const [positionsLoading, setPositionsLoading] = useState(false);
  const [greeksLoading, setGreeksLoading] = useState(false);

  useEffect(() => {
    setChainLoading(true);
    Promise.all([getOptionInstruments(underlying), getOptionTickers(underlying)])
      .then(([instr, tckrs]) => {
        setInstruments(instr);
        setTickers(tckrs);
        const expiries = Array.from(new Set(instr.map((i) => i.expiry))).sort();
        if (expiries.length > 0 && !expiries.includes(expiry)) {
          setExpiry(expiries[0]);
        }
      })
      .catch(() => {
        setInstruments([]);
        setTickers([]);
      })
      .finally(() => setChainLoading(false));
  }, [underlying]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (tab !== 'positions') return;
    setPositionsLoading(true);
    getOptionPositions()
      .then(setPositions)
      .catch(() => setPositions([]))
      .finally(() => setPositionsLoading(false));
  }, [tab]);

  useEffect(() => {
    if (tab !== 'greeks') return;
    setGreeksLoading(true);
    getOptionGreeks()
      .then(setGreeks)
      .catch(() => setGreeks(null))
      .finally(() => setGreeksLoading(false));
  }, [tab]);

  const expiries = Array.from(new Set(instruments.map((i) => i.expiry))).sort();
  const expiryOptions = expiries.map((e) => ({ value: e, label: e }));

  return (
    <div className={styles.page}>
      <div className={styles.header}>
        <h1 className={styles.title}>Options</h1>
        <div className={styles.controls}>
          <Select
            label="Underlying"
            options={UNDERLYING_OPTIONS}
            value={underlying}
            onChange={(e) => setUnderlying(e.target.value)}
            size="sm"
          />
          {expiries.length > 0 && (
            <Select
              label="Expiry"
              options={expiryOptions}
              value={expiry}
              onChange={(e) => setExpiry(e.target.value)}
              size="sm"
            />
          )}
        </div>
      </div>

      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'chain' && (
          <div className={styles.chainLayout}>
            <div className={styles.chainTable}>
              <OptionsChain
                instruments={instruments}
                tickers={tickers}
                loading={chainLoading}
                expiry={expiry}
                onSelectSymbol={(sym, side) => {
                  setSelectedSymbol(sym);
                  void side;
                }}
              />
            </div>
            <div className={styles.orderEntry}>
              <OptionOrderEntry symbol={selectedSymbol} />
            </div>
          </div>
        )}
        {tab === 'positions' && (
          <OptionPositions positions={positions} loading={positionsLoading} />
        )}
        {tab === 'greeks' && (
          <PortfolioGreeks greeks={greeks} loading={greeksLoading} />
        )}
      </div>
    </div>
  );
}
