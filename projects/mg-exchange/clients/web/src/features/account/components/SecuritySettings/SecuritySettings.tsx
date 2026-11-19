import { useState } from 'react';
import { Input } from '@shared/ui/Input';
import { Button } from '@shared/ui/Button';
import { Modal } from '@shared/ui/Modal';
import { setup2FA, enable2FA, changePassword } from '../../api';
import { toast } from '@shared/ui/Toast';
import styles from './SecuritySettings.module.css';

export function SecuritySettings() {
  const [show2FASetup, setShow2FASetup] = useState(false);
  const [qrUrl, setQrUrl] = useState('');
  const [tfaCode, setTfaCode] = useState('');
  const [tfaLoading, setTfaLoading] = useState(false);

  const [currentPassword, setCurrentPassword] = useState('');
  const [newPassword, setNewPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [pwLoading, setPwLoading] = useState(false);
  const [pwErrors, setPwErrors] = useState<{ current?: string; new?: string; confirm?: string }>({});

  async function handleSetup2FA() {
    setTfaLoading(true);
    try {
      const res = await setup2FA();
      setQrUrl(res.qr_url);
      setShow2FASetup(true);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to setup 2FA');
    } finally {
      setTfaLoading(false);
    }
  }

  async function handleEnable2FA() {
    if (!tfaCode.trim()) return;
    setTfaLoading(true);
    try {
      await enable2FA(tfaCode);
      toast.success('Two-factor authentication enabled');
      setShow2FASetup(false);
      setTfaCode('');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Invalid code');
    } finally {
      setTfaLoading(false);
    }
  }

  function validatePassword(): boolean {
    const errs: typeof pwErrors = {};
    if (!currentPassword) errs.current = 'Current password required';
    if (!newPassword || newPassword.length < 8) errs.new = 'Min 8 characters';
    if (newPassword !== confirmPassword) errs.confirm = 'Passwords do not match';
    setPwErrors(errs);
    return Object.keys(errs).length === 0;
  }

  async function handleChangePassword() {
    if (!validatePassword()) return;
    setPwLoading(true);
    try {
      await changePassword({ current_password: currentPassword, new_password: newPassword });
      toast.success('Password changed successfully');
      setCurrentPassword('');
      setNewPassword('');
      setConfirmPassword('');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Password change failed');
    } finally {
      setPwLoading(false);
    }
  }

  return (
    <div className={styles.container}>
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Two-Factor Authentication</h3>
        <p className={styles.description}>
          Protect your account with an authenticator app.
        </p>
        <Button variant="secondary" loading={tfaLoading} onClick={() => { void handleSetup2FA(); }}>
          Setup 2FA
        </Button>
      </section>

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Change Password</h3>
        <div className={styles.form}>
          <Input
            label="Current Password"
            type="password"
            value={currentPassword}
            onChange={(e) => setCurrentPassword(e.target.value)}
            error={pwErrors.current}
          />
          <Input
            label="New Password"
            type="password"
            value={newPassword}
            onChange={(e) => setNewPassword(e.target.value)}
            error={pwErrors.new}
          />
          <Input
            label="Confirm New Password"
            type="password"
            value={confirmPassword}
            onChange={(e) => setConfirmPassword(e.target.value)}
            error={pwErrors.confirm}
          />
          <Button
            variant="primary"
            loading={pwLoading}
            onClick={() => { void handleChangePassword(); }}
          >
            Change Password
          </Button>
        </div>
      </section>

      <Modal open={show2FASetup} onClose={() => setShow2FASetup(false)} title="Setup 2FA">
        <div className={styles.tfaModal}>
          {qrUrl && (
            <div className={styles.qrArea}>
              <p className={styles.description}>Scan with your authenticator app:</p>
              <div className={styles.qrPlaceholder} aria-label="QR code">
                <span className={styles.qrLabel}>QR</span>
              </div>
            </div>
          )}
          <Input
            label="Verification Code"
            placeholder="Enter 6-digit code"
            value={tfaCode}
            onChange={(e) => setTfaCode(e.target.value)}
            maxLength={6}
          />
          <Button
            variant="primary"
            fullWidth
            loading={tfaLoading}
            onClick={() => { void handleEnable2FA(); }}
          >
            Enable 2FA
          </Button>
        </div>
      </Modal>
    </div>
  );
}
