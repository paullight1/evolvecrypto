import { useState } from 'react';
import { Input } from '@shared/ui/Input';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { navigate } from '@app/Router';
import { post } from '@shared/api/client';
import styles from './AccountPage.module.css';

export default function ResetPasswordPage() {
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [loading, setLoading] = useState(false);
  const [done, setDone] = useState(false);

  // Token would normally come from URL params; using hash param here
  const token = new URLSearchParams(window.location.search).get('token') ?? '';

  async function handleReset() {
    if (!password || password !== confirm) {
      toast.error('Passwords do not match');
      return;
    }
    if (password.length < 8) {
      toast.error('Password must be at least 8 characters');
      return;
    }
    setLoading(true);
    try {
      await post('/auth/reset-password', { token, new_password: password });
      setDone(true);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Reset failed');
    } finally {
      setLoading(false);
    }
  }

  if (done) {
    return (
      <div className={styles.page}>
        <h1 className={styles.title}>Password Reset</h1>
        <p style={{ color: 'var(--text-1)', marginBottom: 'var(--pad)' }}>Your password has been updated.</p>
        <Button variant="primary" onClick={() => navigate('auth')}>Login</Button>
      </div>
    );
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Reset Password</h1>
      <div className={styles.form}>
        <Input label="New Password" type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
        <Input label="Confirm Password" type="password" value={confirm} onChange={(e) => setConfirm(e.target.value)} />
        <Button variant="primary" loading={loading} onClick={() => { void handleReset(); }}>Reset Password</Button>
      </div>
    </div>
  );
}
