import { Button } from '@shared/ui/Button';
import { navigate } from '@app/Router';
import { Card } from '@shared/ui/Card';
import styles from '../DashboardPage.module.css';

export function QuickActions() {
  return (
    <Card padding="md" className={styles.actionsCard}>
      <h2 className={styles.cardTitle}>Quick Actions</h2>
      <div className={styles.actionsGrid}>
        <Button variant="primary" size="sm" onClick={() => navigate('wallet')}>Deposit</Button>
        <Button variant="secondary" size="sm" onClick={() => navigate('wallet')}>Withdraw</Button>
        <Button variant="ghost" size="sm" onClick={() => navigate('')}>Trade</Button>
        <Button variant="ghost" size="sm" onClick={() => navigate('convert')}>Convert</Button>
      </div>
    </Card>
  );
}
