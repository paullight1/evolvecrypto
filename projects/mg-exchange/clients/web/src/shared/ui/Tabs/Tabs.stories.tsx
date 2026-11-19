import { useState } from 'react';
import type { Meta, StoryObj } from '@storybook/react-vite';
import { Tabs, type TabDef } from './Tabs';

const meta = {
  title: 'Shared/UI/Tabs',
  component: Tabs,
  parameters: {
    layout: 'padded',
  },
  // Required props — overridden per-story via render
  args: {
    tabs: [],
    activeKey: '',
    onChange: () => {},
  },
} satisfies Meta<typeof Tabs>;

export default meta;
type Story = StoryObj<typeof meta>;

const BASIC_TABS: TabDef[] = [
  { key: 'positions', label: 'Positions' },
  { key: 'orders',    label: 'Open Orders' },
  { key: 'history',   label: 'History' },
];

const BADGE_TABS: TabDef[] = [
  { key: 'positions', label: 'Positions', badge: 3 },
  { key: 'orders',    label: 'Open Orders', badge: 12 },
  { key: 'history',   label: 'History', badge: 0 },
];

const MARKET_TABS: TabDef[] = [
  { key: 'chart',     label: 'Chart' },
  { key: 'orderbook', label: 'Order Book' },
  { key: 'trades',    label: 'Recent Trades' },
  { key: 'funding',   label: 'Funding' },
];

export const Default: Story = {
  name: 'Default (3 tabs)',
  args: {
    tabs: BASIC_TABS,
    activeKey: 'positions',
    onChange: () => {},
  },
  render: () => {
    const [active, setActive] = useState('positions');
    return (
      <div>
        <Tabs tabs={BASIC_TABS} activeKey={active} onChange={setActive} />
        <div style={{ padding: '16px', color: 'var(--color-text-secondary)', fontSize: '14px' }}>
          Active tab: <strong>{active}</strong>
        </div>
      </div>
    );
  },
};

export const WithBadges: Story = {
  name: 'With badge counts',
  args: {
    tabs: BADGE_TABS,
    activeKey: 'positions',
    onChange: () => {},
  },
  render: () => {
    const [active, setActive] = useState('positions');
    return (
      <div>
        <Tabs tabs={BADGE_TABS} activeKey={active} onChange={setActive} />
        <div style={{ padding: '16px', color: 'var(--color-text-secondary)', fontSize: '14px' }}>
          Badge of 0 is hidden; badges above 0 render as counts.
        </div>
      </div>
    );
  },
};

export const FourTabs: Story = {
  name: 'Four tabs',
  args: {
    tabs: MARKET_TABS,
    activeKey: 'chart',
    onChange: () => {},
  },
  render: () => {
    const [active, setActive] = useState('chart');
    return (
      <Tabs tabs={MARKET_TABS} activeKey={active} onChange={setActive} />
    );
  },
};
