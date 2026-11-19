import { lazy, type ComponentType } from 'react';

export type LayoutType = 'fullpage' | 'terminal' | 'dashboard';

export interface Route {
  path: string;
  component: ComponentType<object>;
  layout: LayoutType;
}

// Eager-load landing and auth — they're the first pages users see
const LandingPage = lazy(() => import('@features/info/LandingPage'));
const AuthPage = lazy(() => import('@features/account/AuthPage'));

export const routes: Route[] = [
  // Full page — no shell chrome, page controls its own scroll
  { path: '', layout: 'fullpage', component: LandingPage },
  { path: 'landing', layout: 'fullpage', component: LandingPage },
  { path: 'auth', layout: 'fullpage', component: AuthPage },
  { path: 'forgot-password', layout: 'fullpage', component: lazy(() => import('@features/remaining-account/ForgotPasswordPage')) },
  { path: 'reset-password', layout: 'fullpage', component: lazy(() => import('@features/remaining-account/ResetPasswordPage')) },
  { path: 'verify-email', layout: 'fullpage', component: lazy(() => import('@features/remaining-account/EmailVerificationPage')) },
  { path: 'maintenance', layout: 'fullpage', component: lazy(() => import('@features/info/MaintenancePage')) },

  // Terminal — fixed height, no scroll (trading terminal uses every pixel)
  { path: 'trade', layout: 'terminal', component: lazy(() => import('@features/trading/TradingTerminal')) },

  // Dashboard — header + scrollable content + footer (everything else)
  { path: 'options', layout: 'dashboard', component: lazy(() => import('@features/options/OptionsPage')) },
  { path: 'delivery', layout: 'dashboard', component: lazy(() => import('@features/delivery/DeliveryPage')) },
  { path: 'margin', layout: 'dashboard', component: lazy(() => import('@features/margin/MarginPage')) },
  { path: 'portfolio-margin', layout: 'dashboard', component: lazy(() => import('@features/portfolio-margin/PortfolioMarginPage')) },
  { path: 'spread', layout: 'dashboard', component: lazy(() => import('@features/spread/SpreadPage')) },

  { path: 'dashboard', layout: 'dashboard', component: lazy(() => import('@features/dashboard/DashboardPage')) },
  { path: 'markets', layout: 'dashboard', component: lazy(() => import('@features/markets/MarketsPage')) },

  { path: 'earn', layout: 'dashboard', component: lazy(() => import('@features/earn/EarnPage')) },
  { path: 'bots', layout: 'dashboard', component: lazy(() => import('@features/bots/BotsPage')) },
  { path: 'copy', layout: 'dashboard', component: lazy(() => import('@features/copy-trading/CopyTradingPage')) },
  { path: 'algo', layout: 'dashboard', component: lazy(() => import('@features/algo/AlgoPage')) },
  { path: 'web3', layout: 'dashboard', component: lazy(() => import('@features/web3/Web3Page')) },
  { path: 'convert', layout: 'dashboard', component: lazy(() => import('@features/convert/ConvertPage')) },
  { path: 'buy-crypto', layout: 'dashboard', component: lazy(() => import('@features/fiat/FiatPage')) },
  { path: 'block-trade', layout: 'dashboard', component: lazy(() => import('@features/block-trade/BlockTradePage')) },

  { path: 'wallet', layout: 'dashboard', component: lazy(() => import('@features/wallet/WalletPage')) },
  { path: 'settings', layout: 'dashboard', component: lazy(() => import('@features/account/SettingsPage')) },
  { path: 'security', layout: 'dashboard', component: lazy(() => import('@features/account/SecurityPage')) },

  { path: 'admin', layout: 'dashboard', component: lazy(() => import('@features/admin/AdminPage')) },
  { path: 'sub-accounts', layout: 'dashboard', component: lazy(() => import('@features/sub-accounts/SubAccountsPage')) },
  { path: 'kyc', layout: 'dashboard', component: lazy(() => import('@features/compliance/KycPage')) },

  { path: 'history', layout: 'dashboard', component: lazy(() => import('@features/history/HistoryPage')) },
  { path: 'tax-report', layout: 'dashboard', component: lazy(() => import('@features/history/TaxReportPage')) },
  { path: 'pnl', layout: 'dashboard', component: lazy(() => import('@features/history/PnlPage')) },
  { path: 'statements', layout: 'dashboard', component: lazy(() => import('@features/history/StatementsPage')) },

  { path: 'fees', layout: 'dashboard', component: lazy(() => import('@features/info/FeesPage')) },
  { path: 'status', layout: 'dashboard', component: lazy(() => import('@features/info/StatusPage')) },
  { path: 'help', layout: 'dashboard', component: lazy(() => import('@features/info/HelpPage')) },
  { path: 'legal', layout: 'dashboard', component: lazy(() => import('@features/info/LegalPage')) },
  { path: 'proof-of-reserves', layout: 'dashboard', component: lazy(() => import('@features/info/ProofOfReservesPage')) },
  { path: 'insurance-fund', layout: 'dashboard', component: lazy(() => import('@features/info/InsuranceFundPage')) },
  { path: 'api-docs', layout: 'dashboard', component: lazy(() => import('@features/info/ApiDocsPage')) },
  { path: 'testnet', layout: 'dashboard', component: lazy(() => import('@features/info/TestnetPage')) },

  { path: 'social', layout: 'dashboard', component: lazy(() => import('@features/social/SocialFeedPage')) },
  { path: 'trader-profile', layout: 'dashboard', component: lazy(() => import('@features/social/TraderProfilePage')) },

  { path: 'activity', layout: 'dashboard', component: lazy(() => import('@features/remaining-account/ActivityLogPage')) },
  { path: 'address-book', layout: 'dashboard', component: lazy(() => import('@features/remaining-account/AddressBookPage')) },
  { path: 'notifications', layout: 'dashboard', component: lazy(() => import('@features/remaining-account/NotificationsPage')) },
  { path: 'achievements', layout: 'dashboard', component: lazy(() => import('@features/remaining-account/AchievementsPage')) },
  { path: 'rewards', layout: 'dashboard', component: lazy(() => import('@features/remaining-account/RewardsPage')) },
  { path: 'gift-cards', layout: 'dashboard', component: lazy(() => import('@features/remaining-account/GiftCardsPage')) },
  { path: 'complaints', layout: 'dashboard', component: lazy(() => import('@features/remaining-account/ComplaintsPage')) },
  { path: 'account-mode', layout: 'dashboard', component: lazy(() => import('@features/remaining-account/AccountModePage')) },

  { path: 'p2p', layout: 'dashboard', component: lazy(() => import('@features/p2p/P2PPage')) },
  { path: 'launchpad', layout: 'dashboard', component: lazy(() => import('@features/launchpad/LaunchpadPage')) },
  { path: 'launchpool', layout: 'dashboard', component: lazy(() => import('@features/launchpool/LaunchpoolPage')) },
  { path: 'leveraged-tokens', layout: 'dashboard', component: lazy(() => import('@features/leveraged-tokens/LeveragedTokensPage')) },
  { path: 'competitions', layout: 'dashboard', component: lazy(() => import('@features/competitions/CompetitionsPage')) },
  { path: 'referral', layout: 'dashboard', component: lazy(() => import('@features/referral/ReferralPage')) },
  { path: 'token', layout: 'dashboard', component: lazy(() => import('@features/token/TokenPage')) },
];
