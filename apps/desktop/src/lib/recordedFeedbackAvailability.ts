import { invoke } from '@tauri-apps/api/core';
import { derived, get, writable } from 'svelte/store';

import { defaultRecordedFeedbackAgentPrompt } from './recordedFeedbackPrompt.ts';

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
  agentPrompt: string;
}

const maxAgentPromptLength = 32_000;
const defaultPreferences: RecordedFeedbackPreferences = {
  showInSidebar: true,
  agentPrompt: defaultRecordedFeedbackAgentPrompt
};

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
  updateRecordedFeedbackPreferences({ showInSidebar });
}

export function setRecordedFeedbackAgentPrompt(agentPrompt: string): void {
  if (agentPrompt.length > maxAgentPromptLength) {
    throw new Error(`The feedback prompt cannot exceed ${maxAgentPromptLength.toLocaleString()} characters.`);
  }
  updateRecordedFeedbackPreferences({ agentPrompt });
}

function updateRecordedFeedbackPreferences(patch: Partial<RecordedFeedbackPreferences>): void {
  const preferences = { ...get(recordedFeedbackPreferences), ...patch };
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
    if (typeof stored?.showInSidebar === 'boolean') {
      return {
        showInSidebar: stored.showInSidebar,
        agentPrompt: typeof stored.agentPrompt === 'string' && stored.agentPrompt.length <= maxAgentPromptLength
          ? stored.agentPrompt
          : defaultRecordedFeedbackAgentPrompt
      };
    }
  } catch {
    // Defaults keep the section visible when storage is unavailable or malformed.
  }
  return defaultPreferences;
}
