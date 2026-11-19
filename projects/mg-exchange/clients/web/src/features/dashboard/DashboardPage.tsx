import { useState, useEffect } from 'react';
import { PortfolioSummary } from './components/PortfolioSummary';
import { QuickActions } from './components/QuickActions';
import { RecentOrdersList } from './components/RecentOrdersList';
import { OpenPositionsSummary } from './components/OpenPositionsSummary';
import { getWalletOverview, getRecentOrders, getOpenPositions } from './api';
import type { WalletOverview } from './types';
import type { OpenOrder, Position } from '@features/trading/types';
import styles from './DashboardPage.module.css';

export default function DashboardPage() {
  const [overview, setOverview] = useState<WalletOverview | null>(null);
  const [overviewLoading, setOverviewLoading] = useState(true);
  const [orders, setOrders] = useState<OpenOrder[]>([]);
  const [ordersLoading, setOrdersLoading] = useState(true);
  const [positions, setPositions] = useState<Position[]>([]);
  const [positionsLoading, setPositionsLoading] = useState(true);

  useEffect(() => {
    getWalletOverview()
      .then(setOverview)
      .catch(() => setOverview(null))
      .finally(() => setOverviewLoading(false));

    getRecentOrders(5)
      .then(setOrders)
      .catch(() => setOrders([]))
      .finally(() => setOrdersLoading(false));

    getOpenPositions()
      .then(setPositions)
      .catch(() => setPositions([]))
      .finally(() => setPositionsLoading(false));
  }, []);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Dashboard</h1>
      <div className={styles.grid}>
        <PortfolioSummary overview={overview} loading={overviewLoading} />
        <QuickActions />
        <div className={styles.gridFull}>
          <RecentOrdersList orders={orders} loading={ordersLoading} />
        </div>
        <div className={styles.gridFull}>
          <OpenPositionsSummary positions={positions} loading={positionsLoading} />
        </div>
      </div>
    </div>
  );
}
