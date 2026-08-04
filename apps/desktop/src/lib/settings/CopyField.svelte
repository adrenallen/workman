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
    color: #78919c;
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 8px;
    font-weight: 650;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .field-label small {
    color: #526b76;
    font-size: 7px;
    font-weight: 500;
  }

  .value-row {
    min-width: 0;
    border: 1px solid #2a4652;
    border-radius: 3px;
    background: #08171e;
  }

  code {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    padding: 10px 11px;
    color: #c0d0d5;
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 9px;
    line-height: 1.45;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .multiline code {
    overflow-x: auto;
    padding-block: 12px;
    color: #9fded4;
    scrollbar-width: thin;
    text-overflow: clip;
  }

  .sensitive code {
    color: #e0b875;
    letter-spacing: 0.08em;
  }

  button {
    align-self: stretch;
    gap: 5px;
    border: 0;
    border-left: 1px solid #2a4652;
    padding: 0 11px;
    background: #102832;
    color: #9eb1b8;
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 8px;
    font-weight: 650;
    cursor: pointer;
  }

  button:hover { background: #16333e; color: #e0ebed; }
  button.copied { color: var(--signal); }
  button span { color: var(--signal); font-size: 12px; }
</style>
