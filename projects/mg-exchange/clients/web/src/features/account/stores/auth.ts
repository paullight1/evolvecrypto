import { create } from 'zustand';

interface AuthState {
  isLoggedIn: boolean;
  userId: string | null;
  email: string | null;
  login: (accessToken: string, refreshToken: string, userId: string, email: string) => void;
  logout: () => void;
  hydrate: () => void;
}

export const useAuthStore = create<AuthState>((set) => ({
  isLoggedIn: false,
  userId: null,
  email: null,

  login: (accessToken, refreshToken, userId, email) => {
    sessionStorage.setItem('access_token', accessToken);
    sessionStorage.setItem('refresh_token', refreshToken);
    set({ isLoggedIn: true, userId, email });
  },

  logout: () => {
    sessionStorage.removeItem('access_token');
    sessionStorage.removeItem('refresh_token');
    set({ isLoggedIn: false, userId: null, email: null });
    window.location.href = '/auth';
  },

  hydrate: () => {
    const token = sessionStorage.getItem('access_token');
    if (token) {
      // Token exists — treat as logged in, but we don't have userId/email persisted
      // A real app would decode the JWT or fetch /me; for now mark logged in
      set({ isLoggedIn: true });
    }
  },
}));
