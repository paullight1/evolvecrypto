import { get, post } from '@shared/api/client';
import type { PortfolioMarginStatus, PortfolioAnalysis } from './types';

export const getPortfolioMarginStatus = (): Promise<PortfolioMarginStatus> =>
  get<PortfolioMarginStatus>('/account/portfolio-margin');

export const enablePortfolioMargin = (): Promise<PortfolioMarginStatus> =>
  post<PortfolioMarginStatus>('/account/portfolio-margin/enable');

export const disablePortfolioMargin = (): Promise<PortfolioMarginStatus> =>
  post<PortfolioMarginStatus>('/account/portfolio-margin/disable');

export const getPortfolioAnalysis = (): Promise<PortfolioAnalysis> =>
  get<PortfolioAnalysis>('/account/portfolio-margin/analysis');
