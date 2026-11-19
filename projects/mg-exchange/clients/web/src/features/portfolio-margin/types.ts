export interface PortfolioMarginStatus {
  enabled: boolean;
  margin_level: string;
  initial_margin: string;
  maintenance_margin: string;
  available_balance: string;
  total_equity: string;
}

export interface ScenarioAnalysis {
  scenario: string;
  pnl: string;
  margin_required: string;
  margin_ratio: string;
}

export interface PortfolioAnalysis {
  scenarios: ScenarioAnalysis[];
  net_delta: string;
  net_gamma: string;
  stress_loss: string;
}
