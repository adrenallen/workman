import { invoke } from '@tauri-apps/api/core';
import { derived, writable } from 'svelte/store';

export const recordedFeedbackPreferencesStorageKey = 'workman.feedback.preferences.v1';

export interface RecordedFeedbackCapability {
  supported: boolean;
  platform: string;
}

export interface RecordedFeedbackCapabilityState extends RecordedFeedbackCapability {
  checked: boolean;
}

export interface RecordedFeedbackPreferences {
  showInSidebar: boolean;
}

const defaultPreferences: RecordedFeedbackPreferences = { showInSidebar: true };

export const recordedFeedbackPreferences = writable<RecordedFeedbackPreferences>(
  loadRecordedFeedbackPreferences()
);

export const recordedFeedbackCapability = writable<RecordedFeedbackCapabilityState>({
  checked: false,
  supported: false,
  platform: 'desktop'
});

export const recordedFeedbackSupported = derived(
  recordedFeedbackCapability,
  (capability) => capability.supported
);

export const showRecordedFeedbackSection = derived(
  [recordedFeedbackCapability, recordedFeedbackPreferences],
  ([capability, preferences]) => capability.supported && preferences.showInSidebar
);

let capabilityRequest: Promise<RecordedFeedbackCapabilityState> | null = null;

export function setRecordedFeedbackSidebarVisible(showInSidebar: boolean): void {
  const preferences = { showInSidebar };
  recordedFeedbackPreferences.set(preferences);
  try {
    localStorage.setItem(recordedFeedbackPreferencesStorageKey, JSON.stringify(preferences));
  } catch {
    // The preference still applies for this session when storage is unavailable.
  }
}

export function refreshRecordedFeedbackCapability(): Promise<RecordedFeedbackCapabilityState> {
  if (capabilityRequest) return capabilityRequest;
  capabilityRequest = invoke<RecordedFeedbackCapability>('feedback_capability')
    .then((capability) => {
      const state = { ...capability, checked: true };
      recordedFeedbackCapability.set(state);
      return state;
    })
    .catch(() => {
      const state = { checked: true, supported: false, platform: 'desktop' };
      recordedFeedbackCapability.set(state);
      return state;
    })
    .finally(() => {
      capabilityRequest = null;
    });
  return capabilityRequest;
}

export function platformDisplayName(platform: string): string {
  switch (platform) {
    case 'macos': return 'macOS';
    case 'windows': return 'Windows';
    case 'linux': return 'Linux';
    default: return 'this platform';
  }
}

function loadRecordedFeedbackPreferences(): RecordedFeedbackPreferences {
  try {
    const stored = JSON.parse(localStorage.getItem(recordedFeedbackPreferencesStorageKey) ?? 'null');
    if (typeof stored?.showInSidebar === 'boolean') return stored;
  } catch {
    // Defaults keep the section visible when storage is unavailable or malformed.
  }
  return defaultPreferences;
}
