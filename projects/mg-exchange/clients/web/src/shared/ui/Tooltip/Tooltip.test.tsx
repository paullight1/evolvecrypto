import { describe, it, expect } from 'vitest';
import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Tooltip } from './Tooltip';

describe('Tooltip', () => {
  it('does not show tooltip initially', () => {
    render(
      <Tooltip content="Tip text">
        <button type="button">Hover me</button>
      </Tooltip>,
    );
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  });

  it('shows tooltip after hover delay', async () => {
    render(
      <Tooltip content="Tip text">
        <button type="button">Hover me</button>
      </Tooltip>,
    );
    await act(async () => {
      await userEvent.hover(screen.getByRole('button'));
      // Wait for the 200ms delay
      await new Promise((r) => setTimeout(r, 250));
    });
    expect(screen.getByRole('tooltip')).toBeInTheDocument();
    expect(screen.getByRole('tooltip')).toHaveTextContent('Tip text');
  });

  it('hides tooltip on mouse leave', async () => {
    render(
      <Tooltip content="Tip text">
        <button type="button">Hover me</button>
      </Tooltip>,
    );
    await act(async () => {
      await userEvent.hover(screen.getByRole('button'));
      await new Promise((r) => setTimeout(r, 250));
    });
    expect(screen.getByRole('tooltip')).toBeInTheDocument();
    await userEvent.unhover(screen.getByRole('button'));
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  });
});
