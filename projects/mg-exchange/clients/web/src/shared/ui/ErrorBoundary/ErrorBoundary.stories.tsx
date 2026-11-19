import { useState } from 'react';
import type { Meta, StoryObj } from '@storybook/react-vite';
import { ErrorBoundary } from './ErrorBoundary';

const meta = {
  title: 'Shared/ErrorBoundary',
  component: ErrorBoundary,
  parameters: {
    layout: 'centered',
  },
  decorators: [
    (Story) => (
      <div style={{ padding: '32px', background: 'var(--bg-1)', minWidth: '400px' }}>
        <Story />
      </div>
    ),
  ],
  args: {
    children: null,
  },
} satisfies Meta<typeof ErrorBoundary>;

export default meta;
type Story = StoryObj<typeof meta>;

function ThrowOnMount({ message }: { message: string }): never {
  throw new Error(message);
}

function BombButton(): React.ReactElement {
  const [shouldThrow, setShouldThrow] = useState(false);

  if (shouldThrow) {
    throw new Error('WebSocket connection lost unexpectedly');
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '12px', alignItems: 'center' }}>
      <p style={{ color: 'var(--text-1)', fontFamily: 'var(--font-ui)', fontSize: 'var(--text-sm)', margin: 0 }}>
        Component rendered successfully. Click below to simulate a crash.
      </p>
      <button
        style={{
          padding: '8px 16px',
          background: 'var(--short)',
          color: '#fff',
          border: 'none',
          borderRadius: 'var(--radius-sm)',
          cursor: 'pointer',
          fontFamily: 'var(--font-ui)',
          fontSize: 'var(--text-sm)',
        }}
        onClick={() => setShouldThrow(true)}
      >
        Simulate crash
      </button>
    </div>
  );
}

export const NormalState: Story = {
  render: () => (
    <ErrorBoundary>
      <div style={{
        padding: '20px',
        background: 'var(--bg-2)',
        borderRadius: 'var(--radius)',
        color: 'var(--text-1)',
        fontFamily: 'var(--font-ui)',
        fontSize: 'var(--text-sm)',
      }}>
        Order book loaded and rendering normally.
      </div>
    </ErrorBoundary>
  ),
};

export const ErrorState: Story = {
  render: () => (
    <ErrorBoundary>
      <ThrowOnMount message="Failed to parse market data: unexpected token at position 42" />
    </ErrorBoundary>
  ),
};

export const WithCustomFallback: Story = {
  render: () => (
    <ErrorBoundary
      fallback={
        <div style={{
          padding: '16px',
          background: 'var(--short-bg)',
          border: '1px solid var(--short)',
          borderRadius: 'var(--radius)',
          color: 'var(--short)',
          fontFamily: 'var(--font-ui)',
          fontSize: 'var(--text-sm)',
          textAlign: 'center',
        }}>
          Chart unavailable — refresh to try again
        </div>
      }
    >
      <ThrowOnMount message="Canvas context unavailable" />
    </ErrorBoundary>
  ),
};

export const Interactive: Story = {
  render: () => (
    <ErrorBoundary>
      <BombButton />
    </ErrorBoundary>
  ),
};
