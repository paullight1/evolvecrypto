import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, act } from '@testing-library/react';
import { Countdown } from './Countdown';

describe('Countdown', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders countdown text for a future target', () => {
    const target = Date.now() + 5 * 60 * 1000; // 5 minutes
    render(<Countdown target={target} format="ms" />);
    expect(screen.getByText('05:00')).toBeInTheDocument();
  });

  it('shows completedText when target is in the past', () => {
    const target = Date.now() - 1000;
    render(<Countdown target={target} completedText="Ended" />);
    expect(screen.getByText('Ended')).toBeInTheDocument();
  });

  it('uses default completedText "Ended" when not provided', () => {
    const target = Date.now() - 1;
    render(<Countdown target={target} />);
    expect(screen.getByText('Ended')).toBeInTheDocument();
  });

  it('calls onComplete exactly once when countdown reaches zero', () => {
    const onComplete = vi.fn();
    const target = Date.now() + 2000; // 2 seconds
    render(<Countdown target={target} format="ms" onComplete={onComplete} />);

    act(() => {
      vi.advanceTimersByTime(3000);
    });

    expect(onComplete).toHaveBeenCalledOnce();
  });

  it('does not call onComplete for an already-expired target', () => {
    const onComplete = vi.fn();
    const target = Date.now() - 1000;
    render(<Countdown target={target} onComplete={onComplete} />);
    expect(onComplete).not.toHaveBeenCalled();
  });

  it('counts down over time', () => {
    const target = Date.now() + 65 * 1000; // 1m 05s
    render(<Countdown target={target} format="ms" />);

    expect(screen.getByText('01:05')).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(5000);
    });

    expect(screen.getByText('01:00')).toBeInTheDocument();
  });

  it('formats ms correctly', () => {
    const target = Date.now() + 90 * 1000; // 1m 30s
    render(<Countdown target={target} format="ms" />);
    expect(screen.getByText('01:30')).toBeInTheDocument();
  });

  it('formats hms correctly', () => {
    const target = Date.now() + (2 * 3600 + 3 * 60 + 4) * 1000;
    render(<Countdown target={target} format="hms" />);
    expect(screen.getByText('02:03:04')).toBeInTheDocument();
  });

  it('formats dhms correctly', () => {
    const target = Date.now() + (1 * 86400 + 2 * 3600 + 3 * 60 + 4) * 1000;
    render(<Countdown target={target} format="dhms" />);
    expect(screen.getByText('1d 02h 03m 04s')).toBeInTheDocument();
  });

  it('auto selects dhms when days > 0', () => {
    const target = Date.now() + (2 * 86400 + 1 * 3600) * 1000;
    render(<Countdown target={target} format="auto" />);
    // Should contain "d" indicating dhms format
    expect(screen.getByText(/\dd /)).toBeInTheDocument();
  });

  it('auto selects hms when hours > 0 and days === 0', () => {
    const target = Date.now() + (3 * 3600 + 30 * 60) * 1000;
    render(<Countdown target={target} format="auto" />);
    // hms format: HH:MM:SS — three colon-separated segments
    expect(screen.getByText(/^\d{2}:\d{2}:\d{2}$/)).toBeInTheDocument();
  });

  it('auto selects ms when less than one hour remains', () => {
    const target = Date.now() + 45 * 60 * 1000;
    render(<Countdown target={target} format="auto" />);
    // ms format: MM:SS — two colon-separated segments
    expect(screen.getByText(/^\d{2}:\d{2}$/)).toBeInTheDocument();
  });

  it('renders prefix text', () => {
    const target = Date.now() + 60 * 1000;
    render(<Countdown target={target} prefix="Ends in" format="ms" />);
    expect(screen.getByText('Ends in')).toBeInTheDocument();
  });

  it('accepts a Date object as target', () => {
    const target = new Date(Date.now() + 90 * 1000);
    render(<Countdown target={target} format="ms" />);
    expect(screen.getByText('01:30')).toBeInTheDocument();
  });

  it('applies custom className', () => {
    const { container } = render(
      <Countdown target={Date.now() - 1} className="my-class" />,
    );
    expect(container.firstChild).toHaveClass('my-class');
  });

  it('cleans up interval on unmount without errors', () => {
    const target = Date.now() + 60 * 1000;
    const { unmount } = render(<Countdown target={target} format="ms" />);
    unmount();
    // Advancing time after unmount should not throw
    act(() => {
      vi.advanceTimersByTime(5000);
    });
  });
});
