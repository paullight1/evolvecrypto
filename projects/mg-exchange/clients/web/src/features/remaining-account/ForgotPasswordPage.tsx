import { useState } from 'react';
import { Input } from '@shared/ui/Input';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { navigate } from '@app/Router';
import { post } from '@shared/api/client';
import styles from './AccountPage.module.css';

export default function ForgotPasswordPage() {
  const [email, setEmail] = useState('');
  const [loading, setLoading] = useState(false);
  const [sent, setSent] = useState(false);

  async function handleSend() {
    if (!email.trim()) return;
    setLoading(true);
    try {
      await post('/auth/forgot-password', { email: email.trim() });
      setSent(true);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to send reset email');
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Forgot Password</h1>

      {sent ? (
        <div>
          <p style={{ color: 'var(--text-1)', marginBottom: 'var(--pad)' }}>
            If that email is registered, you will receive a password reset link shortly.
          </p>
          <Button variant="secondary" onClick={() => navigate('auth')}>Back to Login</Button>
        </div>
      ) : (
        <div className={styles.form}>
          <Input
            label="Email Address"
            type="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            placeholder="you@example.com"
          />
          <Button variant="primary" loading={loading} onClick={() => { void handleSend(); }}>
            Send Reset Link
          </Button>
          <Button variant="ghost" size="sm" onClick={() => navigate('auth')}>Back to Login</Button>
        </div>
      )}
    </div>
  );
}
