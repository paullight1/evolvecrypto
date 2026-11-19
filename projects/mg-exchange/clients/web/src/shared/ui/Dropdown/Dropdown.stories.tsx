import { useState } from 'react';
import type { Meta, StoryObj } from '@storybook/react-vite';
import { Dropdown, DropdownItem, DropdownSeparator } from './Dropdown';
import { Button } from '../Button/Button';

const meta = {
  title: 'Shared/UI/Dropdown',
  component: Dropdown,
  parameters: {
    layout: 'centered',
  },
  args: {
    trigger: <Button variant="ghost" size="sm">Options</Button>,
    children: null,
  },
} satisfies Meta<typeof Dropdown>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  name: 'Default (3 items)',
  render: () => (
    <Dropdown trigger={<Button variant="ghost" size="sm">Options</Button>}>
      <DropdownItem>Edit</DropdownItem>
      <DropdownItem>Duplicate</DropdownItem>
      <DropdownItem>Archive</DropdownItem>
    </Dropdown>
  ),
};

export const WithIcons: Story = {
  name: 'With icons',
  render: () => (
    <Dropdown trigger={<Button variant="ghost" size="sm">Actions</Button>}>
      <DropdownItem icon={<span>✎</span>}>Edit Order</DropdownItem>
      <DropdownItem icon={<span>⎘</span>}>Duplicate</DropdownItem>
      <DropdownItem icon={<span>↗</span>}>Export</DropdownItem>
    </Dropdown>
  ),
};

export const WithSeparator: Story = {
  name: 'With separator',
  render: () => (
    <Dropdown trigger={<Button variant="ghost" size="sm">Manage</Button>}>
      <DropdownItem icon={<span>✎</span>}>Edit</DropdownItem>
      <DropdownItem icon={<span>⎘</span>}>Duplicate</DropdownItem>
      <DropdownSeparator />
      <DropdownItem destructive icon={<span>✕</span>}>Delete</DropdownItem>
    </Dropdown>
  ),
};

export const WithDestructive: Story = {
  name: 'With destructive item',
  render: () => (
    <Dropdown trigger={<Button variant="ghost" size="sm">Order actions</Button>}>
      <DropdownItem>Edit order</DropdownItem>
      <DropdownItem>Copy order ID</DropdownItem>
      <DropdownSeparator />
      <DropdownItem destructive>Cancel order</DropdownItem>
    </Dropdown>
  ),
};

export const BottomEnd: Story = {
  name: 'Placement: bottom-end',
  render: () => (
    <Dropdown
      placement="bottom-end"
      trigger={<Button variant="ghost" size="sm">Bottom End ▾</Button>}
    >
      <DropdownItem>Profile</DropdownItem>
      <DropdownItem>Settings</DropdownItem>
      <DropdownSeparator />
      <DropdownItem destructive>Sign out</DropdownItem>
    </Dropdown>
  ),
};

export const TopStart: Story = {
  name: 'Placement: top-start',
  render: () => (
    <div style={{ paddingTop: '120px' }}>
      <Dropdown
        placement="top-start"
        trigger={<Button variant="ghost" size="sm">Top Start ▴</Button>}
      >
        <DropdownItem>View chart</DropdownItem>
        <DropdownItem>Set alert</DropdownItem>
        <DropdownItem>Add to watchlist</DropdownItem>
      </Dropdown>
    </div>
  ),
};

export const Controlled: Story = {
  name: 'Controlled (external open/close)',
  render: () => {
    const [open, setOpen] = useState(false);
    return (
      <div style={{ display: 'flex', flexDirection: 'column', gap: '16px', alignItems: 'center' }}>
        <Button onClick={() => setOpen((o) => !o)}>
          {open ? 'Close dropdown' : 'Open dropdown'}
        </Button>
        <Dropdown
          open={open}
          onOpenChange={setOpen}
          trigger={<Button variant="ghost" size="sm">Trigger</Button>}
        >
          <DropdownItem onClick={() => setOpen(false)}>Option A</DropdownItem>
          <DropdownItem onClick={() => setOpen(false)}>Option B</DropdownItem>
          <DropdownItem onClick={() => setOpen(false)}>Option C</DropdownItem>
        </Dropdown>
      </div>
    );
  },
};

export const DisabledItems: Story = {
  name: 'Disabled items',
  render: () => (
    <Dropdown trigger={<Button variant="ghost" size="sm">With disabled</Button>}>
      <DropdownItem>Available action</DropdownItem>
      <DropdownItem disabled>Unavailable (disabled)</DropdownItem>
      <DropdownItem disabled destructive>Cannot delete (disabled)</DropdownItem>
      <DropdownItem>Another available</DropdownItem>
    </Dropdown>
  ),
};
