<script lang="ts">
  interface Props {
    label: string;
    value: string;
    displayValue?: string;
    multiline?: boolean;
    sensitive?: boolean;
  }

  let { label, value, displayValue, multiline = false, sensitive = false }: Props = $props();
  let copied = $state(false);

  async function copy(): Promise<void> {
    try {
      await navigator.clipboard.writeText(value);
    } catch {
      const field = document.createElement('textarea');
      field.value = value;
      field.setAttribute('readonly', '');
      field.style.position = 'fixed';
      field.style.opacity = '0';
      document.body.append(field);
      field.select();
      document.execCommand('copy');
      field.remove();
    }
    copied = true;
    setTimeout(() => (copied = false), 1600);
  }
</script>

<div class:multiline class:sensitive class="copy-field">
  <div class="field-label">
    <span>{label}</span>
    {#if sensitive}<small>Stored locally</small>{/if}
  </div>
  <div class="value-row">
    <code>{displayValue ?? value}</code>
    <button type="button" class:copied onclick={() => void copy()} aria-label={`Copy ${label}`}>
      <span aria-hidden="true">{copied ? '✓' : '⧉'}</span>
      {copied ? 'Copied' : 'Copy'}
    </button>
  </div>
</div>

<style>
  .copy-field {
    min-width: 0;
  }

  .field-label,
  .value-row,
  button {
    display: flex;
    align-items: center;
  }

  .field-label {
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 6px;
    color: var(--muted, #7d848e);
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 8px;
    font-weight: 650;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .field-label small {
    color: #737a83;
    font-size: 7px;
    font-weight: 500;
  }

  .value-row {
    min-width: 0;
    border: 1px solid var(--border, #30343a);
    border-radius: 3px;
    background: #121416;
  }

  code {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    padding: 10px 11px;
    color: var(--text-soft, #b3b8c0);
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 9px;
    line-height: 1.45;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .multiline code {
    overflow-x: auto;
    padding-block: 12px;
    color: #c8ccd1;
    scrollbar-width: thin;
    text-overflow: clip;
    white-space: pre;
  }

  .sensitive code {
    color: #d5d8dc;
  }

  button {
    align-self: stretch;
    gap: 5px;
    border: 0;
    border-left: 1px solid var(--border, #30343a);
    padding: 0 11px;
    background: #202328;
    color: var(--text-soft, #b3b8c0);
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 8px;
    font-weight: 650;
    cursor: pointer;
  }

  button:hover { background: #292d32; color: var(--text, #e5e7eb); }
  button.copied { color: var(--text, #e5e7eb); }
  button span { color: #a7adb5; font-size: 12px; }
</style>
