import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { Avatar } from './Avatar';

describe('Avatar', () => {
  it('renders an image when src is provided', () => {
    render(<Avatar src="https://example.com/photo.jpg" alt="Profile" name="Alice" />);
    const img = screen.getByRole('img');
    expect(img).toBeInTheDocument();
    expect(img).toHaveAttribute('src', 'https://example.com/photo.jpg');
  });

  it('shows initials when no src is provided', () => {
    render(<Avatar name="Bob Nakamura" />);
    expect(screen.getByText('B')).toBeInTheDocument();
  });

  it('shows "?" when no name and no src', () => {
    render(<Avatar />);
    expect(screen.getByText('?')).toBeInTheDocument();
  });

  it('uppercases the first letter of the name', () => {
    render(<Avatar name="carol" />);
    expect(screen.getByText('C')).toBeInTheDocument();
  });

  it('falls back to initials when image fails to load', () => {
    render(<Avatar src="https://broken.invalid/img.png" name="David Park" />);
    const img = screen.getByRole('img');
    fireEvent.error(img);
    expect(screen.getByText('D')).toBeInTheDocument();
  });

  it('renders online dot when online=true', () => {
    render(<Avatar name="Eve" online />);
    expect(screen.getByLabelText('online')).toBeInTheDocument();
  });

  it('does not render online dot when online=false', () => {
    render(<Avatar name="Eve" online={false} />);
    expect(screen.queryByLabelText('online')).not.toBeInTheDocument();
  });

  it('applies the xs size class', () => {
    const { container } = render(<Avatar name="A" size="xs" />);
    const root = container.firstChild as HTMLElement;
    expect(root.className).toMatch(/xs/);
  });

  it('applies the lg size class', () => {
    const { container } = render(<Avatar name="A" size="lg" />);
    const root = container.firstChild as HTMLElement;
    expect(root.className).toMatch(/lg/);
  });

  it('applies md size class by default', () => {
    const { container } = render(<Avatar name="A" />);
    const root = container.firstChild as HTMLElement;
    expect(root.className).toMatch(/md/);
  });

  it('forwards className to root element', () => {
    const { container } = render(<Avatar name="A" className="custom" />);
    const root = container.firstChild as HTMLElement;
    expect(root.className).toContain('custom');
  });
});
