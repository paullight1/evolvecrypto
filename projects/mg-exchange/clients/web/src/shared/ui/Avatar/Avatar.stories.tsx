import type { Meta, StoryObj } from '@storybook/react-vite';
import { Avatar, type AvatarSize } from './Avatar';

const meta = {
  title: 'Shared/UI/Avatar',
  component: Avatar,
  parameters: {
    layout: 'centered',
  },
  args: {
    size: 'md',
    online: false,
  },
} satisfies Meta<typeof Avatar>;

export default meta;
type Story = StoryObj<typeof meta>;

export const WithImage: Story = {
  args: {
    src: 'https://i.pravatar.cc/150?img=3',
    alt: 'User avatar',
    name: 'Alice Chen',
  },
};

export const WithInitials: Story = {
  args: {
    name: 'Bob Nakamura',
  },
};

export const NoNameFallback: Story = {
  name: 'No name fallback (?)',
  args: {
    name: undefined,
    src: undefined,
  },
};

export const WithOnline: Story = {
  args: {
    name: 'Carol Smith',
    online: true,
  },
};

export const ImageError: Story = {
  name: 'Image error → initials fallback',
  args: {
    src: 'https://broken.example.invalid/image.png',
    name: 'David Park',
  },
};

export const AllSizes: Story = {
  name: 'All Sizes',
  render: () => {
    const sizes: AvatarSize[] = ['xs', 'sm', 'md', 'lg'];
    return (
      <div style={{ display: 'flex', gap: '16px', alignItems: 'flex-end' }}>
        {sizes.map((size) => (
          <div key={size} style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: '8px' }}>
            <Avatar size={size} name="Eve Torres" />
            <span style={{ fontSize: '11px', color: 'var(--text-2)' }}>{size}</span>
          </div>
        ))}
      </div>
    );
  },
};

export const InitialColors: Story = {
  name: 'Deterministic colors from name',
  render: () => {
    const names = ['Alice', 'Bob', 'Carol', 'David', 'Eve'];
    return (
      <div style={{ display: 'flex', gap: '12px', alignItems: 'center' }}>
        {names.map((name) => (
          <Avatar key={name} name={name} />
        ))}
      </div>
    );
  },
};
