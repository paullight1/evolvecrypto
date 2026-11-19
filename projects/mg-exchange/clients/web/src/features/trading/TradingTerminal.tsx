import { useState } from 'react';
import { useMarketStore } from './stores/market';
import { TickerBar } from './components/TickerBar';
import { Chart } from './components/Chart';
import { OrderBook } from './components/OrderBook';
import { RecentTrades } from './components/RecentTrades';
import { OrderEntry } from './components/OrderEntry';
import { BottomPanel } from './components/BottomPanel';
import { PairList } from './components/PairList';
import styles from './TradingTerminal.module.css';

export default function TradingTerminal() {
  const { symbol } = useMarketStore();
  const [pairListOpen, setPairListOpen] = useState(false);

  return (
    <div className={styles.terminal}>
      {/* Row 1: Ticker bar — spans all 3 columns */}
      <div className={styles.ticker}>
        <TickerBar onPairListOpen={() => setPairListOpen(true)} />
      </div>

      {/* Row 2, Col 1: Left column — OrderBook + RecentTrades */}
      <div className={styles.leftColumn}>
        <div className={styles.leftOrderBook}>
          <OrderBook symbol={symbol} />
        </div>
        <div className={styles.leftRecentTrades}>
          <RecentTrades symbol={symbol} />
        </div>
      </div>

      {/* Row 2, Col 2: Chart */}
      <div className={styles.chart}>
        <Chart symbol={symbol} />
      </div>

      {/* Row 2, Col 3: Order Entry */}
      <div className={styles.orderEntry}>
        <OrderEntry />
      </div>

      {/* Row 3: Bottom panel — spans all 3 columns */}
      <div className={styles.bottomPanel}>
        <BottomPanel />
      </div>

      {/* Pair list overlay */}
      <PairList
        isOpen={pairListOpen}
        onClose={() => setPairListOpen(false)}
      />
    </div>
  );
}
