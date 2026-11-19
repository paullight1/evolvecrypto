import { describe, it, expect, beforeEach } from 'vitest';
import { useThemeStore } from './theme';

// The store module calls getInitialTheme() and applyTheme() at module load time.
// jsdom provides localStorage and window.matchMedia (polyfilled in setup) so the
// module initialises fine. We reset store state before every test.

describe('useThemeStore', () => {
  beforeEach(() => {
    localStorage.clear();
    // Reset the document class list so applyTheme side-effects are clean
    document.documentElement.classList.remove('light');
    // Reset the store to a known dark state so tests are independent
    useThemeStore.setState({ theme: 'dark' });
  });

  it('setTheme("light") updates theme to light', () => {
    useThemeStore.getState().setTheme('light');
    expect(useThemeStore.getState().theme).toBe('light');
  });

  it('setTheme("dark") updates theme to dark', () => {
    useThemeStore.getState().setTheme('light');
    useThemeStore.getState().setTheme('dark');
    expect(useThemeStore.getState().theme).toBe('dark');
  });

  it('toggleTheme switches from dark to light', () => {
    useThemeStore.setState({ theme: 'dark' });
    useThemeStore.getState().toggleTheme();
    expect(useThemeStore.getState().theme).toBe('light');
  });

  it('toggleTheme switches from light back to dark', () => {
    useThemeStore.setState({ theme: 'light' });
    useThemeStore.getState().toggleTheme();
    expect(useThemeStore.getState().theme).toBe('dark');
  });

  it('setTheme("light") persists to localStorage', () => {
    useThemeStore.getState().setTheme('light');
    expect(localStorage.getItem('theme')).toBe('light');
  });

  it('setTheme("dark") persists to localStorage', () => {
    useThemeStore.getState().setTheme('light');
    useThemeStore.getState().setTheme('dark');
    expect(localStorage.getItem('theme')).toBe('dark');
  });

  it('setTheme("light") adds "light" class to documentElement', () => {
    useThemeStore.getState().setTheme('light');
    expect(document.documentElement.classList.contains('light')).toBe(true);
  });

  it('setTheme("dark") removes "light" class from documentElement', () => {
    document.documentElement.classList.add('light');
    useThemeStore.getState().setTheme('dark');
    expect(document.documentElement.classList.contains('light')).toBe(false);
  });
});
