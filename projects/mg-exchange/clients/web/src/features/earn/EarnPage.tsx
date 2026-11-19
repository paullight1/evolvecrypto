import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { NumberInput } from '@shared/ui/NumberInput';
import { ProductCard } from './components/ProductCard';
import { SubscribeModal } from './components/SubscribeModal';
import { AllocationTable } from './components/AllocationTable';
import { StakingPanel } from './components/StakingPanel';
import {
  getEarnProducts, getEarnAllocations,
  getDualProducts, getDualPositions, dualInvestSubscribe,
  getLendingRates, getLendingLoans, lendingBorrow,
  getLiquidityPositions,
  type DualProduct, type DualPosition,
  type LendingRate, type LoanPosition,
  type LiquidityPosition,
} from './api';
import { Table } from '@shared/ui/Table';
import { Button } from '@shared/ui/Button';
import { Spinner } from '@shared/ui/Spinner';
import { Badge } from '@shared/ui/Badge';
import { toast } from '@shared/ui/Toast';
import type { EarnProduct, EarnAllocation } from './types';
import styles from './EarnPage.module.css';

const TABS: TabDef[] = [
  { key: 'savings', label: 'Savings' },
  { key: 'staking', label: 'Staking' },
  { key: 'dual', label: 'Dual Investment' },
  { key: 'lending', label: 'Lending' },
  { key: 'liquidity', label: 'Liquidity' },
];

export default function EarnPage() {
  const [tab, setTab] = useState('savings');
  const [products, setProducts] = useState<EarnProduct[]>([]);
  const [productsLoading, setProductsLoading] = useState(false);
  const [allocations, setAllocations] = useState<EarnAllocation[]>([]);
  const [allocationsLoading, setAllocationsLoading] = useState(false);
  const [selectedProduct, setSelectedProduct] = useState<EarnProduct | null>(null);

  // Dual Investment
  const [dualProducts, setDualProducts] = useState<DualProduct[]>([]);
  const [dualPositions, setDualPositions] = useState<DualPosition[]>([]);
  const [dualLoading, setDualLoading] = useState(false);
  const [dualAmounts, setDualAmounts] = useState<Record<string, string>>({});

  // Lending
  const [lendingRates, setLendingRates] = useState<LendingRate[]>([]);
  const [loans, setLoans] = useState<LoanPosition[]>([]);
  const [lendingLoading, setLendingLoading] = useState(false);
  const [borrowAmounts, setBorrowAmounts] = useState<Record<string, string>>({});

  // Liquidity
  const [liqPositions, setLiqPositions] = useState<LiquidityPosition[]>([]);
  const [liqLoading, setLiqLoading] = useState(false);

  function loadProducts() {
    setProductsLoading(true);
    getEarnProducts()
      .then(setProducts)
      .catch(() => setProducts([]))
      .finally(() => setProductsLoading(false));
  }

  function loadAllocations() {
    setAllocationsLoading(true);
    getEarnAllocations()
      .then(setAllocations)
      .catch(() => setAllocations([]))
      .finally(() => setAllocationsLoading(false));
  }

  useEffect(() => {
    if (tab === 'savings') {
      loadProducts();
      loadAllocations();
    }
    if (tab === 'dual') {
      setDualLoading(true);
      Promise.all([getDualProducts(), getDualPositions()])
        .then(([prods, pos]) => { setDualProducts(prods); setDualPositions(pos); })
        .catch(() => { setDualProducts([]); setDualPositions([]); })
        .finally(() => setDualLoading(false));
    }
    if (tab === 'lending') {
      setLendingLoading(true);
      Promise.all([getLendingRates(), getLendingLoans()])
        .then(([rates, lns]) => { setLendingRates(rates); setLoans(lns); })
        .catch(() => { setLendingRates([]); setLoans([]); })
        .finally(() => setLendingLoading(false));
    }
    if (tab === 'liquidity') {
      setLiqLoading(true);
      getLiquidityPositions()
        .then(setLiqPositions)
        .catch(() => setLiqPositions([]))
        .finally(() => setLiqLoading(false));
    }
  }, [tab]);

  const savingsProducts = products.filter((p) => p.productType !== 'staking');

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Earn</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'savings' && (
          <>
            {productsLoading ? (
              <div className={styles.grid}>
                {Array.from({ length: 4 }, (_, i) => (
                  <div key={i} className={styles.skeletonCard} />
                ))}
              </div>
            ) : (
              <div className={styles.grid}>
                {savingsProducts.map((product) => (
                  <ProductCard
                    key={product.id}
                    product={product}
                    onSubscribe={setSelectedProduct}
                  />
                ))}
              </div>
            )}

            <h2 className={styles.sectionTitle}>My Allocations</h2>
            <AllocationTable
              allocations={allocations}
              loading={allocationsLoading}
              onRedeemed={loadAllocations}
            />
          </>
        )}

        {tab === 'staking' && <StakingPanel />}

        {tab === 'dual' && (
          dualLoading ? <Spinner size="lg" /> : (
            <>
              <h2 className={styles.sectionTitle}>Available Products</h2>
              <div className={styles.grid}>
                {dualProducts.map((p) => (
                  <div key={p.id} className={styles.card}>
                    <div className={styles.cardHeader}>
                      <span className={styles.cardTitle}>{p.underlying}</span>
                      <Badge variant={p.direction === 'buy_low' ? 'success' : 'warning'}>{p.direction === 'buy_low' ? 'Buy Low' : 'Sell High'}</Badge>
                    </div>
                    <div className={styles.cardBody}>
                      <div className={styles.stat}><span>APY</span><strong className={styles.apy}>{(parseFloat(p.apy) * 100).toFixed(1)}%</strong></div>
                      <div className={styles.stat}><span>Strike</span><span>{p.strike_price}</span></div>
                      <div className={styles.stat}><span>Duration</span><span>{p.duration_days}d</span></div>
                    </div>
                    <div style={{ display: 'flex', gap: 'var(--pad-xs)', alignItems: 'center' }}>
                      <NumberInput
                        label=""
                        value={dualAmounts[p.id] ?? ''}
                        onChange={(val) => setDualAmounts((prev) => ({ ...prev, [p.id]: val }))}
                        min={0}
                      />
                      <Button variant="primary" size="sm" onClick={() => {
                        const amt = dualAmounts[p.id];
                        if (!amt || parseFloat(amt) <= 0) { toast.error('Enter a valid amount'); return; }
                        dualInvestSubscribe({ product_id: p.id, amount: amt })
                          .then(() => { toast.success('Subscribed'); getDualPositions().then(setDualPositions); setDualAmounts((prev) => ({ ...prev, [p.id]: '' })); })
                          .catch(() => toast.error('Failed'));
                      }}>Subscribe</Button>
                    </div>
                  </div>
                ))}
                {dualProducts.length === 0 && <p className={styles.empty}>No dual investment products available</p>}
              </div>
              {dualPositions.length > 0 && (
                <>
                  <h2 className={styles.sectionTitle}>My Positions</h2>
                  <Table
                    columns={[
                      { key: 'product_id', header: 'Product' },
                      { key: 'amount', header: 'Amount', align: 'right' as const },
                      { key: 'apy', header: 'APY', align: 'right' as const, render: (r) => `${(parseFloat(String(r.apy)) * 100).toFixed(1)}%` },
                      { key: 'status', header: 'Status', render: (r) => <Badge variant="info">{String(r.status)}</Badge> },
                      { key: 'settlement_at', header: 'Settlement' },
                    ]}
                    data={dualPositions as (DualPosition & Record<string, unknown>)[]}
                    emptyText="No positions"
                  />
                </>
              )}
            </>
          )
        )}

        {tab === 'lending' && (
          lendingLoading ? <Spinner size="lg" /> : (
            <>
              <h2 className={styles.sectionTitle}>Interest Rates</h2>
              <Table
                columns={[
                  { key: 'asset', header: 'Asset', sortable: true },
                  { key: 'daily_rate', header: 'Daily Rate', align: 'right' as const },
                  { key: 'hourly_rate', header: 'Hourly Rate', align: 'right' as const },
                  { key: 'available', header: 'Available', align: 'right' as const },
                  {
                    key: 'actions', header: '', align: 'right' as const,
                    render: (r) => (
                      <div style={{ display: 'flex', gap: '4px', alignItems: 'center' }}>
                        <input
                          type="number"
                          className={styles.amountInput}
                          placeholder="Amt"
                          value={borrowAmounts[String(r.asset)] ?? ''}
                          onChange={(e) => setBorrowAmounts((prev) => ({ ...prev, [String(r.asset)]: e.target.value }))}
                          min="0"
                          step="any"
                          style={{ width: '70px' }}
                        />
                        <Button variant="primary" size="sm" onClick={() => {
                          const amt = borrowAmounts[String(r.asset)];
                          if (!amt || parseFloat(amt) <= 0) { toast.error('Enter amount'); return; }
                          lendingBorrow({ asset: String(r.asset), amount: amt, collateral_asset: 'USDT' })
                            .then(() => { toast.success('Borrowed'); getLendingLoans().then(setLoans); setBorrowAmounts((prev) => ({ ...prev, [String(r.asset)]: '' })); })
                            .catch(() => toast.error('Failed'));
                        }}>Borrow</Button>
                      </div>
                    ),
                  },
                ]}
                data={lendingRates as (LendingRate & Record<string, unknown>)[]}
                emptyText="No lending rates"
              />
              {loans.length > 0 && (
                <>
                  <h2 className={styles.sectionTitle}>My Loans</h2>
                  <Table
                    columns={[
                      { key: 'asset', header: 'Asset' },
                      { key: 'amount', header: 'Amount', align: 'right' as const },
                      { key: 'interest_rate', header: 'Rate', align: 'right' as const },
                      { key: 'collateral_asset', header: 'Collateral' },
                      { key: 'collateral_amount', header: 'Collateral Amt', align: 'right' as const },
                      { key: 'status', header: 'Status', render: (r) => <Badge variant="info">{String(r.status)}</Badge> },
                    ]}
                    data={loans as (LoanPosition & Record<string, unknown>)[]}
                    emptyText="No loans"
                  />
                </>
              )}
            </>
          )
        )}

        {tab === 'liquidity' && (
          liqLoading ? <Spinner size="lg" /> : (
            <>
              <h2 className={styles.sectionTitle}>My Liquidity Positions</h2>
              <Table
                columns={[
                  { key: 'pool', header: 'Pool' },
                  { key: 'amount', header: 'Amount', align: 'right' as const },
                  { key: 'share_pct', header: 'Share', align: 'right' as const },
                  { key: 'earned', header: 'Earned', align: 'right' as const },
                  { key: 'status', header: 'Status', render: (r) => <Badge variant="info">{String(r.status)}</Badge> },
                ]}
                data={liqPositions as (LiquidityPosition & Record<string, unknown>)[]}
                emptyText="No liquidity positions"
              />
            </>
          )
        )}
      </div>

      <SubscribeModal
        product={selectedProduct}
        onClose={() => setSelectedProduct(null)}
        onSuccess={() => {
          setSelectedProduct(null);
          loadAllocations();
        }}
      />
    </div>
  );
}
