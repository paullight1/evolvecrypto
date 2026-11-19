import { useEffect } from 'react';
import { Router } from './Router';
import { ToastContainer } from '@shared/ui/Toast';
import { ErrorBoundary } from '@shared/ui/ErrorBoundary';
import { useAuthStore } from '@features/account/stores/auth';

/**
 * App root: wires up auth hydration, error boundary, router, and toasts.
 * Layout selection (fullpage / terminal / dashboard) is handled inside Router.
 */
export function App() {
  const hydrate = useAuthStore((s) => s.hydrate);

  useEffect(() => {
    hydrate();
  }, [hydrate]);

  return (
    <ErrorBoundary>
      <Router />
      <ToastContainer />
    </ErrorBoundary>
  );
}
