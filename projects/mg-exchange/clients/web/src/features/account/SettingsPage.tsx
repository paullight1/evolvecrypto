import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { Select } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { ApiKeyManager } from './components/ApiKeyManager';
import { SessionList } from './components/SessionList';
import { getSettings, updateSettings } from './api';
import { toast } from '@shared/ui/Toast';
import type { UserSettings } from './types';
import styles from './SettingsPage.module.css';

const TABS: TabDef[] = [
  { key: 'general', label: 'General' },
  { key: 'api-keys', label: 'API Keys' },
  { key: 'sessions', label: 'Sessions' },
];

const THEME_OPTIONS = [
  { value: 'dark', label: 'Dark' },
  { value: 'light', label: 'Light' },
];

const LANGUAGE_OPTIONS = [
  { value: 'en', label: 'English' },
  { value: 'zh', label: 'Chinese' },
  { value: 'es', label: 'Spanish' },
];

const TIMEZONE_OPTIONS = [
  { value: 'UTC', label: 'UTC' },
  { value: 'America/New_York', label: 'New York (ET)' },
  { value: 'Europe/London', label: 'London (GMT)' },
  { value: 'Asia/Tokyo', label: 'Tokyo (JST)' },
  { value: 'Asia/Singapore', label: 'Singapore (SGT)' },
];

export default function SettingsPage() {
  const [tab, setTab] = useState('general');
  const [settings, setSettings] = useState<UserSettings>({ theme: 'dark', language: 'en', timezone: 'UTC' });
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    getSettings()
      .then(setSettings)
      .catch(() => { /* use defaults */ });
  }, []);

  async function handleSave() {
    setSaving(true);
    try {
      const updated = await updateSettings(settings);
      setSettings(updated);
      toast.success('Settings saved');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to save settings');
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Settings</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'general' && (
          <div className={styles.generalForm}>
            <Select
              label="Theme"
              options={THEME_OPTIONS}
              value={settings.theme}
              onChange={(e) => setSettings((s) => ({ ...s, theme: e.target.value }))}
            />
            <Select
              label="Language"
              options={LANGUAGE_OPTIONS}
              value={settings.language}
              onChange={(e) => setSettings((s) => ({ ...s, language: e.target.value }))}
            />
            <Select
              label="Timezone"
              options={TIMEZONE_OPTIONS}
              value={settings.timezone}
              onChange={(e) => setSettings((s) => ({ ...s, timezone: e.target.value }))}
            />
            <Button variant="primary" loading={saving} onClick={() => { void handleSave(); }}>
              Save Changes
            </Button>
          </div>
        )}
        {tab === 'api-keys' && <ApiKeyManager />}
        {tab === 'sessions' && <SessionList />}
      </div>
    </div>
  );
}
