import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { Badge } from './Badge';

const VARIANTS = ['success', 'danger', 'warning', 'info', 'neutral', 'long', 'short'] as const;

describe('Badge', () => {
  VARIANTS.forEach((variant) => {
    it(`renders ${variant} variant`, () => {
      render(<Badge variant={variant}>Label</Badge>);
      expect(screen.getByText('Label')).toBeInTheDocument();
    });
  });

  it('renders sm size', () => {
    render(<Badge variant="success" size="sm">Small</Badge>);
    expect(screen.getByText('Small')).toBeInTheDocument();
  });
});
