import type { ReactNode } from 'react';

interface Props {
  children: ReactNode;
}

/**
 * Full-page layout: no header, no footer, no wrapper constraints.
 * The page component (LandingPage, AuthPage, etc.) controls its own
 * height, scroll, and background entirely.
 */
export function FullPageLayout({ children }: Props) {
  return <>{children}</>;
}
