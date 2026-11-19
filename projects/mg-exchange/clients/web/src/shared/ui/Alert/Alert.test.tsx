import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { Alert } from './Alert';

describe('Alert', () => {
  it('renders children content', () => {
    render(<Alert variant="info">Something went wrong.</Alert>);
    expect(screen.getByText('Something went wrong.')).toBeInTheDocument();
  });

  it('renders title when provided', () => {
    render(<Alert variant="warning" title="Heads up">Watch out.</Alert>);
    expect(screen.getByText('Heads up')).toBeInTheDocument();
  });

  it('does not render title element when omitted', () => {
    render(<Alert variant="info">Content</Alert>);
    expect(screen.queryByRole('heading')).not.toBeInTheDocument();
  });

  it('uses role="alert" for error variant', () => {
    render(<Alert variant="error">Error message.</Alert>);
    expect(screen.getByRole('alert')).toBeInTheDocument();
  });

  it('uses role="alert" for warning variant', () => {
    render(<Alert variant="warning">Warning message.</Alert>);
    expect(screen.getByRole('alert')).toBeInTheDocument();
  });

  it('uses role="status" for info variant', () => {
    render(<Alert variant="info">Info message.</Alert>);
    expect(screen.getByRole('status')).toBeInTheDocument();
  });

  it('uses role="status" for success variant', () => {
    render(<Alert variant="success">Success message.</Alert>);
    expect(screen.getByRole('status')).toBeInTheDocument();
  });

  it('does not render dismiss button when dismissible is false', () => {
    render(<Alert variant="info">Content</Alert>);
    expect(screen.queryByRole('button', { name: 'Dismiss' })).not.toBeInTheDocument();
  });

  it('renders dismiss button when dismissible is true', () => {
    render(<Alert variant="info" dismissible>Content</Alert>);
    expect(screen.getByRole('button', { name: 'Dismiss' })).toBeInTheDocument();
  });

  it('calls onDismiss when dismiss button is clicked', () => {
    const onDismiss = vi.fn();
    render(
      <Alert variant="info" dismissible onDismiss={onDismiss}>
        Content
      </Alert>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it('renders action when provided', () => {
    render(
      <Alert variant="warning" action={<button>Fix it</button>}>
        Content
      </Alert>,
    );
    expect(screen.getByRole('button', { name: 'Fix it' })).toBeInTheDocument();
  });

  it('renders error variant without throwing', () => {
    const { container } = render(<Alert variant="error">Content</Alert>);
    expect(container.firstChild).toBeInTheDocument();
  });

  it('applies custom className', () => {
    const { container } = render(
      <Alert variant="info" className="my-alert">
        Content
      </Alert>,
    );
    expect(container.firstChild).toHaveClass('my-alert');
  });
});
