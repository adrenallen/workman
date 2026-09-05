<script lang="ts">
  import MicIcon from '@lucide/svelte/icons/mic';
  import SquareIcon from '@lucide/svelte/icons/square';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onDestroy, onMount, tick } from 'svelte';
  import { Button } from './components/ui/button';
  import { insertDictation } from './voiceInput';

  let { textarea, disabled = false, onText, onBusyChange = () => undefined }: {
    textarea: HTMLTextAreaElement | null;
    disabled?: boolean;
    onText: (text: string) => void;
    onBusyChange?: (busy: boolean) => void;
  } = $props();

  type Phase = 'idle' | 'checking' | 'setup' | 'installing' | 'starting' | 'recording' | 'transcribing';
  interface Preflight { supported: boolean; microphone_available: boolean; model_installed: boolean; model_size_bytes: number; }
  let phase = $state<Phase>('idle');
  let modelBytes = $state(0);
  let notice = $state('');
  let sessionId: string | null = null;
  let original = '';
  let selection = { start: 0, end: 0 };
  let target: HTMLTextAreaElement | null = null;
  let removeErrorListener: (() => void) | null = null;
  let stopTimer: ReturnType<typeof setTimeout> | null = null;
  let destroyed = false;
  let requestVersion = 0;

  function setPhase(next: Phase): void {
    phase = next;
    onBusyChange(next !== 'idle' && next !== 'setup');
  }

  function cleanup(): void {
    if (stopTimer !== null) clearTimeout(stopTimer);
    stopTimer = null;
    removeErrorListener?.();
    removeErrorListener = null;
  }

  async function cancel(): Promise<void> {
    requestVersion += 1;
    const id = sessionId;
    sessionId = null;
    cleanup();
    setPhase('idle');
    if (id) await invoke('dictation_cancel', { sessionId: id }).catch(() => undefined);
  }

  async function start(install = false): Promise<void> {
    if (disabled || !textarea || (phase !== 'idle' && phase !== 'setup')) return;
    const request = ++requestVersion;
    target = textarea;
    original = target.value;
    selection = { start: target.selectionStart, end: target.selectionEnd };
    notice = '';
    setPhase(install ? 'installing' : 'checking');
    try {
      const preflight = await invoke<Preflight>(install ? 'dictation_install_model' : 'dictation_preflight');
      if (destroyed || request !== requestVersion) return;
      if (!preflight.supported) throw new Error('Voice input is available on macOS and Windows.');
      if (!preflight.microphone_available) throw new Error('No microphone is available. Check your microphone connection and permissions, then try again.');
      if (!preflight.model_installed) {
        modelBytes = preflight.model_size_bytes;
        setPhase('setup');
        return;
      }
      const id = crypto.randomUUID();
      sessionId = id;
      setPhase('starting');
      const unlisten = await listen<{ session_id: string; message: string }>('dictation://error', ({ payload }) => {
        if (payload.session_id !== sessionId) return;
        notice = payload.message;
        void cancel();
      });
      if (destroyed || sessionId !== id) { unlisten(); return; }
      removeErrorListener = unlisten;
      await invoke('dictation_start', { sessionId: id });
      if (destroyed || sessionId !== id) {
        await invoke('dictation_cancel', { sessionId: id }).catch(() => undefined);
        return;
      }
      setPhase('recording');
      stopTimer = setTimeout(() => void finish(), 5 * 60 * 1000);
    } catch (cause) {
      if (request !== requestVersion) return;
      if (!destroyed) notice = String(cause);
      await cancel();
    }
  }

  async function finish(): Promise<void> {
    const id = sessionId;
    if (!id || phase !== 'recording') return;
    cleanup();
    setPhase('transcribing');
    try {
      const transcript = await invoke<string>('dictation_finish', { sessionId: id });
      if (destroyed || sessionId !== id || !target?.isConnected) return;
      if (transcript.trim()) {
        const inserted = insertDictation(target.value, original, selection, transcript);
        onText(inserted.text);
        setPhase('idle');
        await tick();
        target.focus();
        target.setSelectionRange(inserted.caret, inserted.caret);
      } else {
        notice = 'No speech detected. Try again closer to the microphone.';
      }
    } catch (cause) {
      if (!destroyed && sessionId === id) notice = `Could not transcribe voice input: ${String(cause)}`;
    } finally {
      if (sessionId === id) await cancel();
    }
  }

  onMount(() => {
    const stopForPageHide = () => { void cancel(); };
    window.addEventListener('pagehide', stopForPageHide);
    return () => window.removeEventListener('pagehide', stopForPageHide);
  });

  onDestroy(() => {
    destroyed = true;
    void cancel();
  });
</script>

{#if isTauri()}
  <div class="voice-input">
    {#if phase === 'setup'}
      <small>Voice input uses the same local Whisper model as feedback. Audio stays on this computer.</small>
      <Button type="button" variant="outline" size="sm" {disabled} onclick={() => void start(true)}>Download voice model ({Math.ceil(modelBytes / 1_000_000)} MB)</Button>
      <Button type="button" variant="ghost" size="sm" onclick={() => setPhase('idle')}>Cancel</Button>
    {:else if phase === 'recording'}
      <span class="recording" role="status"><MicIcon size={14} />Listening…</span>
      <Button type="button" variant="outline" size="sm" onclick={() => void finish()}><SquareIcon size={12} />Stop and insert</Button>
      <Button type="button" variant="ghost" size="sm" onclick={() => void cancel()}>Cancel</Button>
    {:else}
      <Button type="button" variant="ghost" size="sm" disabled={disabled || phase !== 'idle'} title="Dictate instructions with local voice input" onclick={() => void start()}>
        <MicIcon size={14} />{phase === 'transcribing' ? 'Transcribing…' : phase === 'installing' ? 'Downloading voice model…' : phase === 'checking' || phase === 'starting' ? 'Preparing microphone…' : 'Voice input'}
      </Button>
    {/if}
    {#if notice}<span class="notice" role="status">{notice}</span>{/if}
  </div>
{/if}

<style>
  .voice-input { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  small, .notice { color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.4; }
  .notice { flex-basis: 100%; }
  .recording { display: inline-flex; align-items: center; gap: 5px; color: var(--destructive); font-size: var(--font-size-xs); }
</style>
