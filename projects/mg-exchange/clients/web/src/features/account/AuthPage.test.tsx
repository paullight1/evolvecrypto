import { render, screen, fireEvent, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { useAuthStore } from './stores/auth';

// Mock navigate to prevent hash changes during tests
vi.mock('@app/Router', () => ({
  navigate: vi.fn(),
}));

// Mock the API
vi.mock('./api', () => ({
  login: vi.fn(),
  register: vi.fn(),
}));

import AuthPage from './AuthPage';
import { login, register } from './api';
import { navigate } from '@app/Router';

const mockLogin = vi.mocked(login);
const mockRegister = vi.mocked(register);
const mockNavigate = vi.mocked(navigate);

function resetAuthStore() {
  useAuthStore.setState({ isLoggedIn: false, userId: null, email: null });
}

describe('AuthPage', () => {
  beforeEach(() => {
    resetAuthStore();
    vi.clearAllMocks();
  });

  it('renders login form by default', () => {
    render(<AuthPage />);
    expect(screen.getByLabelText('Email')).toBeInTheDocument();
    expect(screen.getByLabelText('Password')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sign In' })).toBeInTheDocument();
  });

  it('switches to register tab', async () => {
    render(<AuthPage />);
    const registerTab = screen.getByRole('tab', { name: 'Register' });
    await userEvent.click(registerTab);
    expect(screen.getByRole('button', { name: 'Create Account' })).toBeInTheDocument();
  });

  it('shows validation error when email is empty', async () => {
    render(<AuthPage />);
    await userEvent.click(screen.getByRole('button', { name: 'Sign In' }));
    expect(screen.getByText('Email is required')).toBeInTheDocument();
  });

  it('shows validation error for invalid email', async () => {
    render(<AuthPage />);
    await userEvent.type(screen.getByLabelText('Email'), 'notanemail');
    await userEvent.click(screen.getByRole('button', { name: 'Sign In' }));
    expect(screen.getByText('Enter a valid email address')).toBeInTheDocument();
  });

  it('shows validation error when password is empty', async () => {
    render(<AuthPage />);
    await userEvent.type(screen.getByLabelText('Email'), 'user@example.com');
    await userEvent.click(screen.getByRole('button', { name: 'Sign In' }));
    expect(screen.getByText('Password is required')).toBeInTheDocument();
  });

  it('calls login API and navigates on success', async () => {
    mockLogin.mockResolvedValueOnce({
      access_token: 'tok_access',
      refresh_token: 'tok_refresh',
      user_id: 'uid_123',
      email: 'user@example.com',
    });

    render(<AuthPage />);
    await userEvent.type(screen.getByLabelText('Email'), 'user@example.com');
    await userEvent.type(screen.getByLabelText('Password'), 'password123');

    await act(async () => {
      fireEvent.submit(screen.getByRole('button', { name: 'Sign In' }).closest('form')!);
    });

    expect(mockLogin).toHaveBeenCalledWith({
      email: 'user@example.com',
      password: 'password123',
    });
    expect(mockNavigate).toHaveBeenCalledWith('');
    expect(useAuthStore.getState().isLoggedIn).toBe(true);
  });

  it('calls register API on register tab submit', async () => {
    mockRegister.mockResolvedValueOnce({
      user_id: 'uid_new',
      email: 'new@example.com',
    });

    render(<AuthPage />);
    // Switch to register tab
    const registerTab = screen.getByRole('tab', { name: 'Register' });
    fireEvent.click(registerTab);

    await userEvent.type(screen.getByLabelText('Email'), 'new@example.com');
    await userEvent.type(screen.getByLabelText('Password'), 'strongpassword');
    await userEvent.type(screen.getByLabelText('Confirm Password'), 'strongpassword');

    // Check terms checkbox to enable submit
    const checkbox = screen.getByRole('checkbox');
    fireEvent.click(checkbox);

    await act(async () => {
      fireEvent.submit(
        screen.getByRole('button', { name: 'Create Account' }).closest('form')!,
      );
    });

    expect(mockRegister).toHaveBeenCalledWith({
      email: 'new@example.com',
      password: 'strongpassword',
    });
  });

  it('shows show/hide password toggle', () => {
    render(<AuthPage />);
    const eyeBtn = screen.getByRole('button', { name: 'Show password' });
    expect(eyeBtn).toBeInTheDocument();
  });

  it('toggles password visibility', async () => {
    render(<AuthPage />);
    const passwordInput = screen.getByLabelText('Password') as HTMLInputElement;
    expect(passwordInput.type).toBe('password');

    const eyeBtn = screen.getByRole('button', { name: 'Show password' });
    await userEvent.click(eyeBtn);
    expect(passwordInput.type).toBe('text');
  });

  it('renders social login buttons on login tab', () => {
    render(<AuthPage />);
    expect(screen.getByRole('button', { name: /google/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /apple/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /telegram/i })).toBeInTheDocument();
  });

  it('does not render social login on register tab', async () => {
    render(<AuthPage />);
    const registerTab = screen.getByRole('tab', { name: 'Register' });
    await userEvent.click(registerTab);
    expect(screen.queryByRole('button', { name: /google/i })).not.toBeInTheDocument();
  });

  it('renders forgot password link on login tab', () => {
    render(<AuthPage />);
    expect(screen.getByRole('button', { name: 'Forgot password?' })).toBeInTheDocument();
  });

  it('renders terms checkbox on register tab', async () => {
    render(<AuthPage />);
    const registerTab = screen.getByRole('tab', { name: 'Register' });
    await userEvent.click(registerTab);
    expect(screen.getByRole('checkbox')).toBeInTheDocument();
  });
});
