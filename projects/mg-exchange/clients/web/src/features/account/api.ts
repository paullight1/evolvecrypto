import { get, post, del } from '@shared/api/client';
import type {
  LoginRequest,
  LoginResponse,
  RegisterRequest,
  RegisterResponse,
  ApiKey,
  Session,
  UserSettings,
  TwoFactorSetupResponse,
  ChangePasswordRequest,
  CreateApiKeyRequest,
  CreateApiKeyResponse,
} from './types';

export function login(payload: LoginRequest): Promise<LoginResponse> {
  return post<LoginResponse>('/auth/login', payload);
}

export function register(payload: RegisterRequest): Promise<RegisterResponse> {
  return post<RegisterResponse>('/auth/register', payload);
}

export function socialAuth(provider: string, token: string): Promise<LoginResponse> {
  return post<LoginResponse>(`/auth/social/${provider}`, { token });
}

export function refreshToken(token: string): Promise<{ access_token: string; refresh_token?: string }> {
  return post('/auth/refresh', { refresh_token: token });
}

// 2FA
export function setup2FA(): Promise<TwoFactorSetupResponse> {
  return post<TwoFactorSetupResponse>('/account/2fa/setup');
}

export function enable2FA(code: string): Promise<void> {
  return post<void>('/account/2fa/enable', { code });
}

export function disable2FA(code: string): Promise<void> {
  return post<void>('/account/2fa/disable', { code });
}

// API Keys
export function getApiKeys(): Promise<ApiKey[]> {
  return get<ApiKey[]>('/account/api-keys');
}

export function createApiKey(payload: CreateApiKeyRequest): Promise<CreateApiKeyResponse> {
  return post<CreateApiKeyResponse>('/account/api-keys', payload);
}

export function revokeApiKey(key: string): Promise<void> {
  return del<void>(`/account/api-keys/${key}`);
}

// Sessions
export function getSessions(): Promise<Session[]> {
  return get<Session[]>('/account/sessions');
}

export function revokeSession(id: string): Promise<void> {
  return del<void>(`/account/sessions/${id}`);
}

// Settings
export function getSettings(): Promise<UserSettings> {
  return get<UserSettings>('/account/settings');
}

export function updateSettings(settings: Partial<UserSettings>): Promise<UserSettings> {
  return post<UserSettings>('/account/settings', settings);
}

export function changePassword(payload: ChangePasswordRequest): Promise<void> {
  return post<void>('/account/password', payload);
}
