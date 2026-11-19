import { useState, useEffect, Suspense, type ComponentType } from 'react';
import { routes, type LayoutType } from './routes';
import { FullPageLayout } from './layouts/FullPageLayout';
import { TerminalLayout } from './layouts/TerminalLayout';
import { DashboardLayout } from './layouts/DashboardLayout';
import { Spinner } from '@shared/ui/Spinner';

// ── History API Router ─────────────────────────────────────────────────
// Clean URLs: /trade, /auth, /markets — no hash fragments.
// Browser back/forward handled via popstate event.
// In-app navigation via navigate() which calls pushState + dispatches popstate.

function getPath(): string {
  return window.location.pathname.replace(/^\//, '');
}

export function navigate(path: string): void {
  const url = path ? `/${path}` : '/';
  if (window.location.pathname !== url) {
    window.history.pushState(null, '', url);
    window.dispatchEvent(new PopStateEvent('popstate'));
  }
}

const LAYOUTS: Record<LayoutType, React.ComponentType<{ children: React.ReactNode }>> = {
  fullpage: FullPageLayout,
  terminal: TerminalLayout,
  dashboard: DashboardLayout,
};

function PageFallback() {
  return (
    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '100%' }}>
      <Spinner size="lg" />
    </div>
  );
}

export function Router() {
  const [currentPath, setCurrentPath] = useState(getPath);

  useEffect(() => {
    function onPopState() {
      setCurrentPath(getPath());
    }
    window.addEventListener('popstate', onPopState);
    return () => window.removeEventListener('popstate', onPopState);
  }, []);

  const matched = routes.find((r) => r.path === currentPath) ?? routes[0];
  const Page = matched.component as ComponentType<object>;
  const Layout = LAYOUTS[matched.layout];

  return (
    <Layout key={matched.layout}>
      <Suspense fallback={<PageFallback />}>
        <Page key={matched.path} />
      </Suspense>
    </Layout>
  );
}
