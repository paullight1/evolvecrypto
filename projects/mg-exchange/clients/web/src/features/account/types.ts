export interface RegisterRequest {
  email: string;
  password: string;
}

export interface RegisterResponse {
  user_id: string;
  email: string;
  message?: string;
}

export interface LoginRequest {
  email: string;
  password: string;
}

export interface LoginResponse {
  access_token: string;
  refresh_token: string;
  user_id: string;
  email: string;
}

export interface ApiKey {
  key: string;
  label: string;
  permissions: string[];
  created_at: number;
}

export interface Session {
  id: string;
  ip: string;
  device: string;
  last_active: number;
}

export interface UserSettings {
  theme: string;
  language: string;
  timezone: string;
}

export interface TwoFactorSetupResponse {
  secret: string;
  qr_url: string;
}

export interface ChangePasswordRequest {
  current_password: string;
  new_password: string;
}

export interface CreateApiKeyRequest {
  label: string;
  permissions: string[];
}

export interface CreateApiKeyResponse {
  key: string;
  secret: string;
  label: string;
  permissions: string[];
  created_at: number;
}
