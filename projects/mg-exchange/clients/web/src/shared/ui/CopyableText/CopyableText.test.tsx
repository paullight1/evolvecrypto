import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { CopyableText } from './CopyableText';

describe('CopyableText', () => {
  beforeEach(() => {
    Object.assign(navigator, {
      clipboard: {
        writeText: vi.fn().mockResolvedValue(undefined),
      },
    });
  });

  it('renders the value', () => {
    render(<CopyableText value="0xabc123" />);
    expect(screen.getByText('0xabc123')).toBeInTheDocument();
  });

  it('renders displayValue when provided', () => {
    render(<CopyableText value="0xabc123" displayValue="0xabc..." />);
    expect(screen.getByText('0xabc...')).toBeInTheDocument();
  });

  it('renders copy button', () => {
    render(<CopyableText value="abc" />);
    expect(screen.getByLabelText('Copy to clipboard')).toBeInTheDocument();
  });

  it('triggers clipboard copy on click', async () => {
    render(<CopyableText value="secret-key" />);
    await userEvent.click(screen.getByLabelText('Copy to clipboard'));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('secret-key');
  });

  it('shows Copied! feedback after click', async () => {
    render(<CopyableText value="abc" />);
    await userEvent.click(screen.getByLabelText('Copy to clipboard'));
    expect(screen.getByText('Copied!')).toBeInTheDocument();
  });
});
