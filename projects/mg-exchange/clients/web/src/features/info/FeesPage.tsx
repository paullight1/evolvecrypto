import { useState, useEffect } from 'react';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { Badge } from '@shared/ui/Badge';
import { Spinner } from '@shared/ui/Spinner';
import { getFeeSchedule, getUserFeeInfo, type VipTier, type UserFeeInfo } from './api';
import styles from './InfoPage.module.css';

interface FeeRow extends Record<string, unknown> {
  level: number;
  tier: string;
  volume: string;
  balance: string;
  maker: string;
  taker: string;
  isCurrent: boolean;
}

function formatPct(raw: string): string {
  const n = parseFloat(raw);
  if (!isFinite(n)) return raw;
  const pct = n * 100;
  if (pct < 0) return `${pct.toFixed(3)}% (rebate)`;
  return `${pct.toFixed(3)}%`;
}

function formatVolume(raw: string): string {
  const n = parseFloat(raw);
  if (!isFinite(n) || n === 0) return '$0';
  if (n >= 1_000_000_000) return `$${(n / 1_000_000_000).toFixed(0)}B`;
  if (n >= 1_000_000) return `$${(n / 1_000_000).toFixed(0)}M`;
  if (n >= 1_000) return `$${(n / 1_000).toFixed(0)}K`;
  return `$${n.toFixed(0)}`;
}

const TABS: TabDef[] = [
  { key: 'trading', label: 'Trading Fees' },
  { key: 'withdrawal', label: 'Withdrawal Fees' },
];

const FEE_COLS: ColumnDef<FeeRow>[] = [
  {
    key: 'tier',
    header: 'Tier',
    render: (row) => (
      <span>
        {row.tier}
        {row.isCurrent && <Badge variant="success" className={styles.currentBadge}>Your Tier</Badge>}
      </span>
    ),
  },
  { key: 'volume', header: '30d Volume (or)', align: 'right' },
  { key: 'balance', header: 'Balance (USD)', align: 'right' },
  { key: 'maker', header: 'Maker Fee', align: 'right' },
  { key: 'taker', header: 'Taker Fee', align: 'right' },
];

export default function FeesPage() {
  const [tab, setTab] = useState('trading');
  const [tiers, setTiers] = useState<VipTier[]>([]);
  const [withdrawalFees, setWithdrawalFees] = useState<Record<string, string>>({});
  const [userInfo, setUserInfo] = useState<UserFeeInfo | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    Promise.all([
      getFeeSchedule().catch(() => ({ vip_tiers: [], withdrawal_fees: {} })),
      getUserFeeInfo().catch(() => null),
    ]).then(([schedule, user]) => {
      setTiers(schedule.vip_tiers);
      setWithdrawalFees(schedule.withdrawal_fees ?? {});
      setUserInfo(user);
    }).finally(() => setLoading(false));
  }, []);

  const feeRows: FeeRow[] = tiers.map((t) => ({
    level: t.level,
    tier: t.name,
    volume: formatVolume(t.min_volume_30d),
    balance: parseFloat(t.min_balance) > 0 ? formatVolume(t.min_balance) : '—',
    maker: formatPct(t.maker_fee),
    taker: formatPct(t.taker_fee),
    isCurrent: userInfo?.tier_level === t.level,
  }));

  const withdrawalRows = Object.entries(withdrawalFees).map(([asset, fee]) => ({
    asset,
    fee,
  }));

  if (loading) {
    return (
      <div className={styles.page}>
        <h1 className={styles.title}>Fee Schedule</h1>
        <Spinner size="lg" />
      </div>
    );
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Fee Schedule</h1>
      <p className={styles.subtitle}>
        Fees are based on your 30-day rolling trading volume or USD balance — whichever qualifies you for a higher tier.
      </p>

      {userInfo && (
        <div className={styles.userFeeBox}>
          <span>Your tier: <strong>{userInfo.tier_name}</strong></span>
          <span>30d volume: <strong>{formatVolume(userInfo.volume_30d)}</strong></span>
          <span>Maker: <strong>{formatPct(userInfo.maker_fee)}</strong></span>
          <span>Taker: <strong>{formatPct(userInfo.taker_fee)}</strong></span>
        </div>
      )}

      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} />

      {tab === 'trading' && (
        <Table<FeeRow> columns={FEE_COLS} data={feeRows} emptyText="No fee tiers configured" />
      )}

      {tab === 'withdrawal' && (
        <Table
          columns={[
            { key: 'asset', header: 'Asset', sortable: true },
            { key: 'fee', header: 'Withdrawal Fee', align: 'right' },
          ]}
          data={withdrawalRows}
          emptyText="No withdrawal fees configured"
        />
      )}
    </div>
  );
}
