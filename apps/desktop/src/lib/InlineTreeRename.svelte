<script lang="ts">
  import { onMount } from 'svelte';

  interface Props {
    value: string;
    label: string;
    depth?: number;
    onSubmit: (name: string) => void;
    onCancel: () => void;
  }

  let { value, label, depth = 0, onSubmit, onCancel }: Props = $props();
  let input: HTMLInputElement;
  let draft = $state('');
  let settled = false;

  onMount(() => {
    draft = value;
    requestAnimationFrame(() => {
      input.focus();
      input.select();
    });
  });

  function submit(): void {
    if (settled) return;
    const name = draft.trim();
    if (name) {
      settled = true;
      onSubmit(name);
    }
  }

  function cancel(): void {
    if (settled) return;
    settled = true;
    onCancel();
  }
</script>

<form
  class="inline-rename"
  style={`--rename-indent: ${7 + depth * 14}px`}
  aria-label={`Rename ${label}`}
  onsubmit={(event) => {
    event.preventDefault();
    submit();
  }}
>
  <span class="signal" aria-hidden="true"></span>
  <input
    bind:this={input}
    bind:value={draft}
    aria-label={`New name for ${label}`}
    onkeydown={(event) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        cancel();
      }
    }}
    onblur={() => {
      if (draft.trim() && draft.trim() !== value) submit();
      else cancel();
    }}
  />
  <span class="hint" aria-hidden="true">↵</span>
</form>

<style>
  .inline-rename {
    display: grid;
    grid-template-columns: 2px minmax(0, 1fr) auto;
    align-items: center;
    min-height: 30px;
    margin: 1px 4px;
    padding: 2px 7px 2px var(--rename-indent, 7px);
    border: 1px solid #46636b;
    border-radius: 3px;
    background: #172328;
  }

  .signal {
    align-self: stretch;
    width: 2px;
    border-radius: 2px;
    background: #55b6c9;
  }

  input {
    min-width: 0;
    height: 22px;
    padding: 0 6px;
    border: 0;
    outline: 0;
    background: transparent;
    color: #f1f3f5;
    font: 560 var(--font-size-sm)/1.2 'Archivo Variable', sans-serif;
  }

  .hint {
    color: #82909a;
    font: var(--font-size-sm)/1 'JetBrains Mono Variable', monospace;
  }
</style>
