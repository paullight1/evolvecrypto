import { useState, type FormEvent } from 'react';
import { Input } from '@shared/ui/Input';
import { Checkbox } from '@shared/ui/Checkbox';
import { Divider } from '@shared/ui/Divider';
import { toast } from '@shared/ui/Toast';
import { ApiError } from '@shared/api/client';
import { login, register, socialAuth } from './api';
import { useAuthStore } from './stores/auth';
import { navigate } from '@app/Router';
import styles from './AuthPage.module.css';

type Tab = 'login' | 'register';

function PasswordStrengthBar({ password }: { password: string }) {
  const strength = getPasswordStrength(password);
  return (
    <div className={styles.strengthBarWrapper} aria-hidden="true">
      <div className={styles.strengthBar}>
        <div
          className={`${styles.strengthFill} ${styles[`strength${strength.level}`]}`}
          style={{ width: `${strength.pct}%` }}
        />
      </div>
      {password.length > 0 && (
        <span className={`${styles.strengthLabel} ${styles[`strengthLabel${strength.level}`]}`}>
          {strength.label}
        </span>
      )}
    </div>
  );
}

function getPasswordStrength(password: string): { level: 'Weak' | 'Fair' | 'Good' | 'Strong'; pct: number; label: string } {
  if (password.length === 0) return { level: 'Weak', pct: 0, label: '' };
  let score = 0;
  if (password.length >= 8) score++;
  if (password.length >= 12) score++;
  if (/[A-Z]/.test(password)) score++;
  if (/[0-9]/.test(password)) score++;
  if (/[^A-Za-z0-9]/.test(password)) score++;
  if (score <= 1) return { level: 'Weak', pct: 25, label: 'Weak' };
  if (score === 2) return { level: 'Fair', pct: 50, label: 'Fair' };
  if (score === 3) return { level: 'Good', pct: 75, label: 'Good' };
  return { level: 'Strong', pct: 100, label: 'Strong' };
}

export default function AuthPage() {
  const [tab, setTab] = useState<Tab>('login');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [showPassword, setShowPassword] = useState(false);
  const [agreeTerms, setAgreeTerms] = useState(false);
  const [emailError, setEmailError] = useState('');
  const [passwordError, setPasswordError] = useState('');
  const [confirmError, setConfirmError] = useState('');
  const [loading, setLoading] = useState(false);
  const doLogin = useAuthStore((s) => s.login);

  function resetErrors() {
    setEmailError('');
    setPasswordError('');
    setConfirmError('');
  }

  function switchTab(next: Tab) {
    setTab(next);
    resetErrors();
    setPassword('');
    setConfirmPassword('');
    setAgreeTerms(false);
    setShowPassword(false);
  }

  function validate(): boolean {
    let valid = true;
    resetErrors();

    if (!email.trim()) {
      setEmailError('Email is required');
      valid = false;
    } else if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) {
      setEmailError('Enter a valid email address');
      valid = false;
    }

    if (!password) {
      setPasswordError('Password is required');
      valid = false;
    } else if (tab === 'register' && password.length < 8) {
      setPasswordError('Password must be at least 8 characters');
      valid = false;
    }

    if (tab === 'register') {
      if (confirmPassword !== password) {
        setConfirmError('Passwords do not match');
        valid = false;
      }
    }

    return valid;
  }

  async function handleSocialLogin(provider: string) {
    setLoading(true);
    try {
      const token = `${provider}-${Date.now()}`;
      const res = await socialAuth(provider, token);
      doLogin(res.access_token, res.refresh_token, res.user_id, res.email);
      toast.success(`Signed in with ${provider}`);
      navigate('');
    } catch (err) {
      const message = err instanceof ApiError ? err.message : `${provider} sign-in failed`;
      toast.error(message);
    } finally {
      setLoading(false);
    }
  }

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!validate()) return;

    setLoading(true);
    try {
      if (tab === 'login') {
        const res = await login({ email, password });
        doLogin(res.access_token, res.refresh_token, res.user_id, res.email);
        toast.success('Welcome back!');
        navigate('');
      } else {
        await register({ email, password });
        toast.success('Account created — please log in');
        switchTab('login');
      }
    } catch (err) {
      const message = err instanceof ApiError ? err.message : 'Something went wrong';
      toast.error(message);
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className={styles.page}>
      {/* Left branding panel */}
      <div className={styles.brandPanel} aria-hidden="true">
        <div className={styles.brandContent}>
          <div className={styles.brandLogo}>Exchange</div>
          <h2 className={styles.brandHeadline}>Welcome to Exchange</h2>
          <ul className={styles.brandFeatures}>
            <li className={styles.brandFeatureItem}>
              <span className={styles.brandFeatureIcon}>⚡</span>
              <div>
                <div className={styles.brandFeatureTitle}>Sub-millisecond matching</div>
                <div className={styles.brandFeatureDesc}>Orders execute in under 500µs</div>
              </div>
            </li>
            <li className={styles.brandFeatureItem}>
              <span className={styles.brandFeatureIcon}>🛡️</span>
              <div>
                <div className={styles.brandFeatureTitle}>Proof of Reserves</div>
                <div className={styles.brandFeatureDesc}>100% cryptographically verified</div>
              </div>
            </li>
            <li className={styles.brandFeatureItem}>
              <span className={styles.brandFeatureIcon}>📂</span>
              <div>
                <div className={styles.brandFeatureTitle}>Open-source engine</div>
                <div className={styles.brandFeatureDesc}>Audit the code that handles your funds</div>
              </div>
            </li>
          </ul>
        </div>
        <div className={styles.brandGradient} />
      </div>

      {/* Right auth panel */}
      <div className={styles.authPanel}>
        <div className={styles.authCard}>
          {/* Logo (visible on mobile only) */}
          <div className={styles.mobileLogoRow}>
            <button
              className={styles.mobileLogo}
              onClick={() => navigate('')}
              type="button"
            >
              Exchange
            </button>
          </div>

          {/* Tabs */}
          <div className={styles.tabs} role="tablist">
            <button
              className={`${styles.tab} ${tab === 'login' ? styles.activeTab : ''}`}
              onClick={() => switchTab('login')}
              type="button"
              role="tab"
              aria-selected={tab === 'login'}
            >
              Log In
            </button>
            <button
              className={`${styles.tab} ${tab === 'register' ? styles.activeTab : ''}`}
              onClick={() => switchTab('register')}
              type="button"
              role="tab"
              aria-selected={tab === 'register'}
            >
              Register
            </button>
          </div>

          {/* Social login (login tab only) */}
          {tab === 'login' && (
            <>
              <div className={styles.socialRow}>
                <button className={styles.socialBtn} type="button" aria-label="Continue with Google" onClick={() => handleSocialLogin('google')} disabled={loading}>
                  <svg className={styles.socialIcon} width="18" height="18" viewBox="0 0 24 24" aria-hidden="true">
                    <path d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z" fill="#4285F4"/>
                    <path d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z" fill="#34A853"/>
                    <path d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l3.66-2.84z" fill="#FBBC05"/>
                    <path d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z" fill="#EA4335"/>
                  </svg>
                  Google
                </button>
                <button className={styles.socialBtn} type="button" aria-label="Continue with Apple" onClick={() => handleSocialLogin('apple')} disabled={loading}>
                  <svg className={styles.socialIcon} width="18" height="18" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                    <path d="M17.05 20.28c-.98.95-2.05.8-3.08.35-1.09-.46-2.09-.48-3.24 0-1.44.62-2.2.44-3.06-.35C2.79 15.25 3.51 7.7 9.05 7.4c1.4.07 2.38.74 3.19.8 1.22-.25 2.38-.96 3.67-.84 1.57.16 2.75.73 3.52 1.84-3.25 1.95-2.49 5.9.5 7.08-.57 1.56-1.32 3.12-2.88 3.99zM12.03 7.25c-.16-2.5 1.95-4.63 4.31-4.85.32 2.76-2.47 4.96-4.31 4.85z"/>
                  </svg>
                  Apple
                </button>
                <button className={styles.socialBtn} type="button" aria-label="Continue with Telegram" onClick={() => handleSocialLogin('telegram')} disabled={loading}>
                  <svg className={styles.socialIcon} width="18" height="18" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                    <path d="M11.944 0A12 12 0 0 0 0 12a12 12 0 0 0 12 12 12 12 0 0 0 12-12A12 12 0 0 0 12 0zm4.962 7.224c.1-.002.321.023.465.14a.506.506 0 0 1 .171.325c.016.093.036.306.02.472-.18 1.898-.962 6.502-1.36 8.627-.168.9-.499 1.201-.82 1.23-.696.065-1.225-.46-1.9-.902-1.056-.693-1.653-1.124-2.678-1.8-1.185-.78-.417-1.21.258-1.91.177-.184 3.247-2.977 3.307-3.23.007-.032.014-.15-.056-.212s-.174-.041-.249-.024c-.106.024-1.793 1.14-5.061 3.345-.48.33-.913.49-1.302.48-.428-.008-1.252-.241-1.865-.44-.752-.245-1.349-.374-1.297-.789.027-.216.325-.437.893-.663 3.498-1.524 5.83-2.529 6.998-3.014 3.332-1.386 4.025-1.627 4.476-1.635z"/>
                  </svg>
                  Telegram
                </button>
              </div>
              <Divider label="or continue with email" spacing="lg" />
            </>
          )}

          {/* Form */}
          <form className={styles.form} onSubmit={handleSubmit} noValidate>
            <Input
              label="Email"
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              error={emailError}
              placeholder="trader@example.com"
              autoComplete="email"
            />

            <Input
              label="Password"
              type={showPassword ? 'text' : 'password'}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              error={passwordError}
              placeholder={tab === 'register' ? 'At least 8 characters' : '••••••••'}
              autoComplete={tab === 'login' ? 'current-password' : 'new-password'}
              suffix={
                <button
                  className={styles.eyeBtn}
                  type="button"
                  onClick={() => setShowPassword((v) => !v)}
                  aria-label={showPassword ? 'Hide password' : 'Show password'}
                >
                  {showPassword ? '🙈' : '👁'}
                </button>
              }
            />

            {tab === 'register' && (
              <>
                <PasswordStrengthBar password={password} />

                <Input
                  label="Confirm Password"
                  type={showPassword ? 'text' : 'password'}
                  value={confirmPassword}
                  onChange={(e) => setConfirmPassword(e.target.value)}
                  error={confirmError}
                  placeholder="Repeat your password"
                  autoComplete="new-password"
                />

                <div className={styles.termsLabel}>
                  <Checkbox
                    checked={agreeTerms}
                    onChange={(v) => setAgreeTerms(v)}
                  />
                  <span className={styles.termsText}>
                    I agree to the{' '}
                    <button
                      className={styles.termsLink}
                      type="button"
                      onClick={() => navigate('legal')}
                    >
                      Terms of Service
                    </button>
                    {' '}and{' '}
                    <button
                      className={styles.termsLink}
                      type="button"
                      onClick={() => navigate('legal')}
                    >
                      Privacy Policy
                    </button>
                  </span>
                </div>
              </>
            )}

            {tab === 'login' && (
              <div className={styles.forgotRow}>
                <button
                  className={styles.forgotLink}
                  type="button"
                  onClick={() => navigate('forgot-password')}
                >
                  Forgot password?
                </button>
              </div>
            )}

            <button
              className={styles.submitBtn}
              type="submit"
              disabled={loading || (tab === 'register' && !agreeTerms)}
              aria-busy={loading}
            >
              {loading ? 'Please wait…' : tab === 'login' ? 'Sign In' : 'Create Account'}
            </button>
          </form>

          {/* Switch link */}
          <p className={styles.switchText}>
            {tab === 'login' ? (
              <>
                No account?{' '}
                <button
                  className={styles.switchLink}
                  onClick={() => switchTab('register')}
                  type="button"
                >
                  Register
                </button>
              </>
            ) : (
              <>
                Already have an account?{' '}
                <button
                  className={styles.switchLink}
                  onClick={() => switchTab('login')}
                  type="button"
                >
                  Log In
                </button>
              </>
            )}
          </p>
        </div>
      </div>
    </div>
  );
}
