import { useState, useEffect } from 'react';
import { Button } from '@shared/ui/Button';
import { navigate } from '@app/Router';
import { post } from '@shared/api/client';
import styles from './AccountPage.module.css';

export default function EmailVerificationPage() {
  const [status, setStatus] = useState<'verifying' | 'success' | 'error'>('verifying');
  const [message, setMessage] = useState('');

  useEffect(() => {
    const token = new URLSearchParams(window.location.search).get('token') ?? '';
    if (!token) {
      setStatus('error');
      setMessage('No verification token found.');
      return;
    }

    post<{ message: string }>('/auth/verify-email', { token })
      .then((res) => {
        setStatus('success');
        setMessage((res as { message: string }).message ?? 'Email verified successfully.');
      })
      .catch((e: unknown) => {
        setStatus('error');
        setMessage(e instanceof Error ? e.message : 'Verification failed.');
      });
  }, []);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Email Verification</h1>

      {status === 'verifying' && <p style={{ color: 'var(--text-2)' }}>Verifying your email...</p>}

      {status === 'success' && (
        <>
          <p style={{ color: 'var(--color-buy)', marginBottom: 'var(--pad)' }}>{message}</p>
          <Button variant="primary" onClick={() => navigate('auth')}>Login</Button>
        </>
      )}

      {status === 'error' && (
        <>
          <p style={{ color: 'var(--color-sell)', marginBottom: 'var(--pad)' }}>{message}</p>
          <Button variant="secondary" onClick={() => navigate('auth')}>Back to Login</Button>
        </>
      )}
    </div>
  );
}
