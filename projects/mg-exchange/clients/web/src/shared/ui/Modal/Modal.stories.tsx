import { useState } from 'react';
import type { Meta, StoryObj } from '@storybook/react-vite';
import { Modal } from './Modal';
import { Button } from '../Button/Button';

const meta = {
  title: 'Shared/UI/Modal',
  component: Modal,
  parameters: {
    layout: 'centered',
  },
  // Required props — overridden per-story via render
  args: {
    open: false,
    onClose: () => {},
    children: null,
  },
} satisfies Meta<typeof Modal>;

export default meta;
type Story = StoryObj<typeof meta>;

export const WithTitle: Story = {
  name: 'Default (with title)',
  args: {
    open: true,
    onClose: () => {},
    title: 'Confirm Order',
    children: null,
  },
  render: () => {
    const [open, setOpen] = useState(false);
    return (
      <>
        <Button onClick={() => setOpen(true)}>Open Modal</Button>
        <Modal open={open} onClose={() => setOpen(false)} title="Confirm Order">
          <p>Are you sure you want to place this order?</p>
          <p style={{ color: 'var(--color-text-secondary)', fontSize: '14px', marginTop: '8px' }}>
            Buy 0.5 BTC at $67,450.00 — Total: $33,725.00
          </p>
          <div style={{ display: 'flex', gap: '8px', marginTop: '20px', justifyContent: 'flex-end' }}>
            <Button variant="ghost" onClick={() => setOpen(false)}>Cancel</Button>
            <Button variant="long" onClick={() => setOpen(false)}>Confirm Buy</Button>
          </div>
        </Modal>
      </>
    );
  },
};

export const WithoutTitle: Story = {
  name: 'Without title',
  args: {
    open: true,
    onClose: () => {},
    children: null,
  },
  render: () => {
    const [open, setOpen] = useState(false);
    return (
      <>
        <Button onClick={() => setOpen(true)}>Open Titleless Modal</Button>
        <Modal open={open} onClose={() => setOpen(false)}>
          <div style={{ textAlign: 'center', padding: '8px 0' }}>
            <p style={{ fontSize: '18px', fontWeight: 600, marginBottom: '8px' }}>Session Expired</p>
            <p style={{ color: 'var(--color-text-secondary)' }}>Your session has timed out. Please log in again.</p>
            <Button style={{ marginTop: '20px' }} onClick={() => setOpen(false)}>Log In</Button>
          </div>
        </Modal>
      </>
    );
  },
};

export const LongScrollableContent: Story = {
  name: 'Long scrollable content',
  args: {
    open: true,
    onClose: () => {},
    title: 'Trade History',
    children: null,
  },
  render: () => {
    const [open, setOpen] = useState(false);
    const items = Array.from({ length: 30 }, (_, i) => `Trade #${i + 1} — BTC/USDT @ $${(67000 + i * 10).toLocaleString()}`);
    return (
      <>
        <Button onClick={() => setOpen(true)}>Open Scrollable Modal</Button>
        <Modal open={open} onClose={() => setOpen(false)} title="Trade History">
          <ul style={{ margin: 0, padding: '0 0 0 16px', lineHeight: '2' }}>
            {items.map((item) => (
              <li key={item} style={{ fontSize: '14px', color: 'var(--color-text-secondary)' }}>
                {item}
              </li>
            ))}
          </ul>
          <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: '16px' }}>
            <Button variant="ghost" onClick={() => setOpen(false)}>Close</Button>
          </div>
        </Modal>
      </>
    );
  },
};

export const SizeSm: Story = {
  name: 'Size: sm',
  args: {
    open: true,
    onClose: () => {},
    title: 'Quick Action',
    size: 'sm',
    children: null,
  },
  render: () => {
    const [open, setOpen] = useState(false);
    return (
      <>
        <Button onClick={() => setOpen(true)}>Open Small Modal</Button>
        <Modal open={open} onClose={() => setOpen(false)} title="Cancel Order" size="sm">
          <p>Cancel order #BTC-000123?</p>
          <div style={{ display: 'flex', gap: '8px', marginTop: '16px', justifyContent: 'flex-end' }}>
            <Button variant="ghost" size="sm" onClick={() => setOpen(false)}>Keep</Button>
            <Button variant="danger" size="sm" onClick={() => setOpen(false)}>Cancel Order</Button>
          </div>
        </Modal>
      </>
    );
  },
};

export const SizeLg: Story = {
  name: 'Size: lg',
  args: {
    open: true,
    onClose: () => {},
    title: 'API Documentation',
    size: 'lg',
    children: null,
  },
  render: () => {
    const [open, setOpen] = useState(false);
    return (
      <>
        <Button onClick={() => setOpen(true)}>Open Large Modal</Button>
        <Modal open={open} onClose={() => setOpen(false)} title="API Key Details" size="lg">
          <p>This large modal is suitable for detailed forms, documentation or data tables.</p>
          <div style={{ marginTop: '16px', display: 'flex', justifyContent: 'flex-end' }}>
            <Button variant="ghost" onClick={() => setOpen(false)}>Close</Button>
          </div>
        </Modal>
      </>
    );
  },
};
