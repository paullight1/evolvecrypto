import { useState, useEffect, useCallback } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { StatsGrid } from './components/StatsGrid';
import { KycReviewTable } from './components/KycReviewTable';
import { AmlAlertsTable } from './components/AmlAlertsTable';
import { WithdrawalsTable } from './components/WithdrawalsTable';
import { SystemHealthPanel } from './components/SystemHealthPanel';
import { Table } from '@shared/ui/Table';
import { Button } from '@shared/ui/Button';
import { Badge } from '@shared/ui/Badge';
import { toast } from '@shared/ui/Toast';
import { Spinner } from '@shared/ui/Spinner';
import {
  getAdminStats, getKycPending, getAmlAlerts, getPendingWithdrawals, getSystemHealth,
  listUsers, getAuditLog, getTreasury, getFeatureFlags, setFeatureFlag,
  haltTrading, resumeTrading,
  type AdminUser, type AuditEntry, type TreasuryData,
} from './api';
import type { AdminStats, KycEntry, AmlAlert, PendingWithdrawal, SystemHealth } from './types';
import styles from './AdminPage.module.css';

const TABS: TabDef[] = [
  { key: 'overview', label: 'Overview' },
  { key: 'kyc', label: 'KYC' },
  { key: 'aml', label: 'AML' },
  { key: 'withdrawals', label: 'Withdrawals' },
  { key: 'users', label: 'Users' },
  { key: 'audit', label: 'Audit Log' },
  { key: 'treasury', label: 'Treasury' },
  { key: 'system', label: 'System' },
];

export default function AdminPage() {
  const [tab, setTab] = useState('overview');
  const [stats, setStats] = useState<AdminStats | null>(null);
  const [statsLoading, setStatsLoading] = useState(false);
  const [kyc, setKyc] = useState<KycEntry[]>([]);
  const [kycLoading, setKycLoading] = useState(false);
  const [aml, setAml] = useState<AmlAlert[]>([]);
  const [amlLoading, setAmlLoading] = useState(false);
  const [withdrawals, setWithdrawals] = useState<PendingWithdrawal[]>([]);
  const [wdLoading, setWdLoading] = useState(false);
  const [health, setHealth] = useState<SystemHealth | null>(null);
  const [healthLoading, setHealthLoading] = useState(false);
  const [users, setUsers] = useState<AdminUser[]>([]);
  const [usersLoading, setUsersLoading] = useState(false);
  const [auditLog, setAuditLog] = useState<AuditEntry[]>([]);
  const [auditLoading, setAuditLoading] = useState(false);
  const [treasury, setTreasury] = useState<TreasuryData | null>(null);
  const [treasuryLoading, setTreasuryLoading] = useState(false);
  const [featureFlags, setFeatureFlags] = useState<Record<string, boolean>>({});

  const loadKyc = useCallback(() => {
    setKycLoading(true);
    getKycPending().then(setKyc).catch(() => setKyc([])).finally(() => setKycLoading(false));
  }, []);

  const loadWithdrawals = useCallback(() => {
    setWdLoading(true);
    getPendingWithdrawals().then(setWithdrawals).catch(() => setWithdrawals([])).finally(() => setWdLoading(false));
  }, []);

  useEffect(() => {
    if (tab === 'overview') {
      setStatsLoading(true);
      getAdminStats().then(setStats).catch(() => setStats(null)).finally(() => setStatsLoading(false));
    } else if (tab === 'kyc') {
      loadKyc();
    } else if (tab === 'aml') {
      setAmlLoading(true);
      getAmlAlerts().then(setAml).catch(() => setAml([])).finally(() => setAmlLoading(false));
    } else if (tab === 'withdrawals') {
      loadWithdrawals();
    } else if (tab === 'system') {
      setHealthLoading(true);
      Promise.all([
        getSystemHealth().then(setHealth).catch(() => setHealth(null)),
        getFeatureFlags().then(setFeatureFlags).catch(() => setFeatureFlags({})),
      ]).finally(() => setHealthLoading(false));
    } else if (tab === 'users') {
      setUsersLoading(true);
      listUsers({ limit: 100 }).then(setUsers).catch(() => setUsers([])).finally(() => setUsersLoading(false));
    } else if (tab === 'audit') {
      setAuditLoading(true);
      getAuditLog(200).then(setAuditLog).catch(() => setAuditLog([])).finally(() => setAuditLoading(false));
    } else if (tab === 'treasury') {
      setTreasuryLoading(true);
      getTreasury().then(setTreasury).catch(() => setTreasury(null)).finally(() => setTreasuryLoading(false));
    }
  }, [tab, loadKyc, loadWithdrawals]);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Admin Panel</h1>
      <div className={styles.controls}>
        <Button variant="danger" size="sm" onClick={() => { haltTrading().then(() => toast.success('Trading halted')).catch(() => toast.error('Failed')); }}>
          Halt Trading
        </Button>
        <Button variant="primary" size="sm" onClick={() => { resumeTrading().then(() => toast.success('Trading resumed')).catch(() => toast.error('Failed')); }}>
          Resume Trading
        </Button>
      </div>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />
      <div className={styles.content}>
        {tab === 'overview' && <StatsGrid stats={stats} loading={statsLoading} />}
        {tab === 'kyc' && <KycReviewTable entries={kyc} loading={kycLoading} onRefresh={loadKyc} />}
        {tab === 'aml' && <AmlAlertsTable alerts={aml} loading={amlLoading} />}
        {tab === 'withdrawals' && <WithdrawalsTable withdrawals={withdrawals} loading={wdLoading} onRefresh={loadWithdrawals} />}

        {tab === 'users' && (
          usersLoading ? <Spinner size="lg" /> : (
            <Table
              columns={[
                { key: 'email', header: 'Email', sortable: true },
                { key: 'kyc_level', header: 'KYC', width: '60px', render: (r) => <Badge variant="info">{`L${r.kyc_level}`}</Badge> },
                { key: 'status', header: 'Status', render: (r) => <Badge variant={r.status === 'active' ? 'success' : 'danger'}>{String(r.status)}</Badge> },
                { key: 'created_at', header: 'Joined', render: (r) => new Date(Number(r.created_at) * 1000).toLocaleDateString() },
              ]}
              data={users as (AdminUser & Record<string, unknown>)[]}
              emptyText="No users"
            />
          )
        )}

        {tab === 'audit' && (
          auditLoading ? <Spinner size="lg" /> : (
            <Table
              columns={[
                { key: 'action', header: 'Action' },
                { key: 'actor', header: 'Actor' },
                { key: 'target', header: 'Target' },
                { key: 'details', header: 'Details' },
                { key: 'timestamp', header: 'Time', render: (r) => new Date(Number(r.timestamp) * 1000).toLocaleString() },
              ]}
              data={auditLog as (AuditEntry & Record<string, unknown>)[]}
              emptyText="No audit entries"
            />
          )
        )}

        {tab === 'treasury' && (
          treasuryLoading ? <Spinner size="lg" /> : treasury ? (
            <>
              <div className={styles.treasuryTotal}>Total: {treasury.total_balance_usd} USD</div>
              <Table
                columns={[
                  { key: 'asset', header: 'Asset', sortable: true },
                  { key: 'balance', header: 'Balance', align: 'right' as const },
                  { key: 'usd_value', header: 'USD Value', align: 'right' as const },
                ]}
                data={treasury.assets as ({ asset: string; balance: string; usd_value: string } & Record<string, unknown>)[]}
                emptyText="No treasury data"
              />
            </>
          ) : <p>Failed to load treasury data</p>
        )}

        {tab === 'system' && (
          <>
            <SystemHealthPanel health={health} loading={healthLoading} />
            {Object.keys(featureFlags).length > 0 && (
              <div className={styles.featureFlags}>
                <h3 className={styles.sectionTitle}>Feature Flags</h3>
                {Object.entries(featureFlags).map(([flag, enabled]) => (
                  <div key={flag} className={styles.flagRow}>
                    <span>{flag}</span>
                    <Button
                      variant={enabled ? 'primary' : 'secondary'}
                      size="sm"
                      onClick={() => {
                        setFeatureFlag(flag, !enabled)
                          .then(() => { setFeatureFlags((prev) => ({ ...prev, [flag]: !enabled })); toast.success(`${flag}: ${!enabled ? 'enabled' : 'disabled'}`); })
                          .catch(() => toast.error('Failed'));
                      }}
                    >
                      {enabled ? 'ON' : 'OFF'}
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
