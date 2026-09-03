import {
  nativeHotkeyAccelerator,
  type HotkeyPreferences,
  type RecordingHotkeyAction
} from './hotkeys.ts';

export type NativeRecordingAction =
  | 'snap'
  | 'snapRegion'
  | 'snapFull'
  | 'toggleAnnotation'
  | 'undo'
  | 'clear'
  | 'togglePause'
  | 'toggleMute'
  | 'finish';

const actionMap: Record<NativeRecordingAction, RecordingHotkeyAction> = {
  snap: 'feedback-snap',
  snapRegion: 'feedback-snap-region',
  snapFull: 'feedback-snap-display',
  toggleAnnotation: 'feedback-toggle-annotation',
  undo: 'feedback-undo',
  clear: 'feedback-clear',
  togglePause: 'feedback-toggle-pause',
  toggleMute: 'feedback-toggle-mute',
  finish: 'feedback-finish'
};

export function recordingHotkeyBindings(
  preferences: HotkeyPreferences
): Partial<Record<NativeRecordingAction, string>> {
  return Object.fromEntries(
    Object.entries(actionMap).flatMap(([nativeAction, hotkeyAction]) => {
      const accelerator = nativeHotkeyAccelerator(preferences[hotkeyAction]);
      return accelerator ? [[nativeAction, accelerator]] : [];
    })
  );
}
