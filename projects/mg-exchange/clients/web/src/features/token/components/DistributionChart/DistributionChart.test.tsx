import { render, screen, act } from '@testing-library/react';
import { describe, it, expect } from 'vitest';

import { DistributionChart } from './DistributionChart';

describe('DistributionChart', () => {
  it('renders the SVG donut chart', async () => {
    await act(async () => {
      render(<DistributionChart />);
    });

    const svg = document.querySelector('svg');
    expect(svg).toBeInTheDocument();
  });

  it('renders circle segments for each distribution category', async () => {
    await act(async () => {
      render(<DistributionChart />);
    });

    const circles = document.querySelectorAll('circle');
    expect(circles.length).toBe(6); // 6 distribution categories
  });

  it('renders the EXC center label', async () => {
    await act(async () => {
      render(<DistributionChart />);
    });

    expect(screen.getByText('EXC')).toBeInTheDocument();
  });

  it('renders all distribution category labels in the legend', async () => {
    await act(async () => {
      render(<DistributionChart />);
    });

    expect(screen.getByText('Public Sale')).toBeInTheDocument();
    expect(screen.getByText('Team & Advisors')).toBeInTheDocument();
    expect(screen.getByText('Ecosystem Fund')).toBeInTheDocument();
    expect(screen.getByText('Insurance Pool')).toBeInTheDocument();
    expect(screen.getByText('Marketing')).toBeInTheDocument();
    expect(screen.getByText('Reserve')).toBeInTheDocument();
  });

  it('renders percentage values for each category', async () => {
    await act(async () => {
      render(<DistributionChart />);
    });

    expect(screen.getByText('30%')).toBeInTheDocument();
    expect(screen.getByText('20%')).toBeInTheDocument();
    expect(screen.getByText('25%')).toBeInTheDocument();
  });
});
