import type { Meta, StoryObj } from '@storybook/react-vite';
import { Accordion, type AccordionItem } from './Accordion';

const meta = {
  title: 'Shared/UI/Accordion',
  component: Accordion,
  parameters: {
    layout: 'padded',
  },
  args: {
    items: [],
    multiple: false,
  },
} satisfies Meta<typeof Accordion>;

export default meta;
type Story = StoryObj<typeof meta>;

const BASIC_ITEMS: AccordionItem[] = [
  {
    key: 'fees',
    title: 'What are the trading fees?',
    content: 'Maker fee: 0.01%. Taker fee: 0.06%. VIP tiers reduce fees progressively.',
  },
  {
    key: 'leverage',
    title: 'What leverage is available?',
    content: 'Up to 100× on major perpetual futures. Actual available leverage depends on position size and risk tier.',
  },
  {
    key: 'liquidation',
    title: 'How does liquidation work?',
    content: 'When your margin ratio falls below the maintenance margin, the engine liquidates your position at the bankruptcy price to prevent further losses.',
  },
];

const DISABLED_ITEMS: AccordionItem[] = [
  ...BASIC_ITEMS.slice(0, 2),
  {
    key: 'locked',
    title: 'Feature not available yet',
    content: 'Coming soon.',
    disabled: true,
  },
];

export const Default: Story = {
  name: 'Default (3 items, single mode)',
  args: {
    items: BASIC_ITEMS,
  },
};

export const MultipleOpen: Story = {
  name: 'Multiple open',
  args: {
    items: BASIC_ITEMS,
    multiple: true,
  },
};

export const DefaultOpen: Story = {
  name: 'Default open (first item)',
  args: {
    items: BASIC_ITEMS,
    defaultOpenKeys: ['fees'],
  },
};

export const Disabled: Story = {
  name: 'With disabled item',
  args: {
    items: DISABLED_ITEMS,
  },
};

export const WithComplexContent: Story = {
  name: 'Complex content',
  args: {
    items: [
      {
        key: 'table',
        title: 'Fee schedule',
        content: (
          <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '13px', color: 'var(--text-1)' }}>
            <thead>
              <tr>
                <th style={{ textAlign: 'left', padding: '4px 8px', color: 'var(--text-2)' }}>Tier</th>
                <th style={{ textAlign: 'right', padding: '4px 8px', color: 'var(--text-2)' }}>Maker</th>
                <th style={{ textAlign: 'right', padding: '4px 8px', color: 'var(--text-2)' }}>Taker</th>
              </tr>
            </thead>
            <tbody>
              {[['Regular', '0.01%', '0.06%'], ['VIP 1', '0.008%', '0.05%'], ['VIP 2', '0.006%', '0.04%']].map(([tier, maker, taker]) => (
                <tr key={tier}>
                  <td style={{ padding: '4px 8px' }}>{tier}</td>
                  <td style={{ padding: '4px 8px', textAlign: 'right', color: 'var(--long)' }}>{maker}</td>
                  <td style={{ padding: '4px 8px', textAlign: 'right', color: 'var(--short)' }}>{taker}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ),
      },
      {
        key: 'risk',
        title: 'Risk parameters',
        content: 'Initial margin: 1%. Maintenance margin: 0.5%. Liquidation fee: 0.5%.',
      },
    ],
    defaultOpenKeys: ['table'],
  },
};

export const SingleItem: Story = {
  name: 'Single item',
  args: {
    items: [BASIC_ITEMS[0]],
    defaultOpenKeys: ['fees'],
  },
};
