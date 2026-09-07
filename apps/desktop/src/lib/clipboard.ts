import { invoke, isTauri } from '@tauri-apps/api/core';

export async function writeClipboardText(text: string): Promise<void> {
  if (isTauri()) await invoke('terminal_write_clipboard_text', { text });
  else await navigator.clipboard.writeText(text);
}
