import styles from './InfoPage.module.css';

export default function MaintenancePage() {
  return (
    <div className={styles.page}>
      <div className={styles.maintenanceCenter}>
        <div className={styles.maintenanceIcon}>🔧</div>
        <h1 className={styles.maintenanceTitle}>Under Maintenance</h1>
        <p className={styles.maintenanceMsg}>
          We are performing scheduled maintenance. The platform will be back shortly.
        </p>
        <p style={{ fontSize: 'var(--text-xs)', color: 'var(--text-2)' }}>
          Follow our announcements for updates.
        </p>
      </div>
    </div>
  );
}
