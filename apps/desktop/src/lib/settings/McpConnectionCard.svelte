<script lang="ts">
  import type { McpClientId, McpConnectionInfo } from '../settings';
  import CopyField from './CopyField.svelte';

  interface Props {
    connection: McpConnectionInfo;
  }

  let { connection }: Props = $props();
  let selected = $state<McpClientId>('claude');
  let activeSetup = $derived(
    connection.setups.find((setup) => setup.client === selected) ?? connection.setups[0]
  );
  let maskedToken = $derived(maskToken(connection.token));

  function maskToken(token: string): string {
    if (token.length <= 12) return '••••••••••••';
    return `${token.slice(0, 6)}${'•'.repeat(18)}${token.slice(-6)}`;
  }
</script>

<section class="card mcp-card" aria-labelledby="mcp-card-title">
  <header class="card-heading">
    <div class="card-icon" aria-hidden="true">M</div>
    <div>
      <span class="eyebrow">Model context protocol</span>
      <h2 id="mcp-card-title">Connect your agent</h2>
      <p>Choose a client and copy the setup it understands. Every recipe targets this local daemon.</p>
    </div>
    <span class="availability" title="MCP transport available · Streamable HTTP"><i aria-hidden="true"></i>Streamable HTTP</span>
  </header>

  {#if activeSetup}
    <div class="handshake" aria-label="Connection path">
      <strong>{activeSetup.label}</strong><i aria-hidden="true"></i><span>127.0.0.1</span><i aria-hidden="true"></i><span>workman MCP</span>
    </div>

    <div class="client-switch" aria-label="MCP client">
      {#each connection.setups as setup (setup.client)}
        <button
          type="button"
          class:active={selected === setup.client}
          aria-pressed={selected === setup.client}
          onclick={() => (selected = setup.client)}
        >
          {setup.label}
        </button>
      {/each}
    </div>

    <div class="connection-overview">
      <CopyField label="Endpoint" value={connection.endpoint} />
      <CopyField
        label="Bearer token"
        value={connection.token}
        displayValue={maskedToken}
        sensitive
      />
    </div>

    <div class="recipe">
      <header>
        <div>
          <span class="recipe-kicker">Setup for</span>
          <h3>{activeSetup.label}</h3>
        </div>
        <p>{activeSetup.description}</p>
      </header>
      <div class="recipe-fields">
        {#each activeSetup.fields as field (`${activeSetup.client}-${field.label}`)}
          <div
            class:wide={field.format !== 'text' ||
              (activeSetup.client === 'generic' && field.label === 'Header value')}
            class="recipe-field"
          >
            <CopyField
              label={field.label}
              value={field.value}
              multiline={field.format !== 'text' || field.value.includes('\n')}
              sensitive={field.sensitive}
            />
          </div>
        {/each}
      </div>
    </div>
  {:else}
    <p class="missing">No MCP client recipes were returned by the daemon.</p>
  {/if}
</section>

<style>
  .card {
    overflow: hidden;
    border: 1px solid var(--border, var(--border));
    border-radius: 5px;
    background: var(--surface, var(--card));
  }

  .card-heading {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 14px;
  }

  .card-icon {
    display: grid;
    width: 38px;
    height: 38px;
    place-items: center;
    border: 1px solid var(--border-strong, var(--border-strong));
    background: var(--accent);
    color: var(--text-soft, var(--text-soft));
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 15px;
    font-weight: 720;
  }

  .eyebrow,
  .availability,
  .handshake,
  .client-switch,
  .recipe-kicker {
    font-family: 'JetBrains Mono Variable', monospace;
  }

  .eyebrow {
    color: var(--muted, var(--muted-foreground));
    font-size: var(--font-size-xs);
    font-weight: 650;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  h2 {
    margin: 3px 0 0;
    color: var(--text, var(--foreground));
    font-size: 18px;
    font-weight: 630;
  }

  .card-heading p {
    margin: 4px 0 0;
    color: var(--muted, var(--muted-foreground));
    font-size: var(--font-size-sm);
    line-height: 1.5;
  }

  .availability {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--muted, var(--muted-foreground));
    font-size: var(--font-size-xs);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .availability i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #8a929d;
  }

  .handshake {
    display: grid;
    grid-template-columns: auto minmax(24px, 1fr) auto minmax(24px, 1fr) auto;
    align-items: center;
    gap: 9px;
    border-block: 1px solid var(--border, var(--border));
    padding: 7px 14px;
    background: var(--surface, var(--card));
    color: var(--muted, var(--muted-foreground));
    font-size: var(--font-size-xs);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .handshake strong { color: var(--text-soft, var(--text-soft)); font-weight: 650; }
  .handshake i { height: 1px; background: var(--border-strong, var(--border-strong)); }

  .client-switch {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: 3px;
    border-bottom: 1px solid var(--border, var(--border));
    padding: 6px 14px;
    background: var(--surface, var(--card));
  }

  .client-switch button {
    min-height: 32px;
    overflow: hidden;
    border: 1px solid transparent;
    border-radius: 2px;
    padding: 7px 8px;
    background: transparent;
    color: var(--muted, var(--muted-foreground));
    font: inherit;
    font-size: var(--font-size-xs);
    font-weight: 620;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }

  .client-switch button:hover { background: #22252a; color: var(--text-soft, var(--text-soft)); }
  .client-switch button:focus-visible { outline: 2px solid #8a929d; outline-offset: 1px; }
  .client-switch button.active { border-color: var(--border-strong, var(--border-strong)); background: #292c31; color: var(--text, var(--foreground)); }

  .connection-overview {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 10px;
    padding: 12px 14px;
  }

  .recipe { border-top: 1px solid var(--border, var(--border)); background: var(--surface, var(--card)); }
  .recipe > header { display: flex; align-items: end; justify-content: space-between; gap: 10px; padding: 10px 14px 8px; }
  .recipe-kicker { color: var(--muted, var(--muted-foreground)); font-size: var(--font-size-xs); letter-spacing: 0.08em; text-transform: uppercase; }
  h3 { margin: 2px 0 0; color: var(--text-soft, var(--text-soft)); font-size: 13px; }
  .recipe > header p { max-width: 520px; margin: 0; color: var(--muted, var(--muted-foreground)); font-size: var(--font-size-sm); line-height: 1.45; text-align: right; }
  .recipe-fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; padding: 0 14px 14px; }
  .recipe-field.wide { grid-column: 1 / -1; }
  .missing { margin: 0; padding: 14px; color: var(--muted, var(--muted-foreground)); font-size: var(--font-size-sm); }

  @media (max-width: 760px) {
    .card-heading { grid-template-columns: auto minmax(0, 1fr); }
    .availability { grid-column: 2; }
    .client-switch { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .connection-overview, .recipe-fields { grid-template-columns: 1fr; }
    .recipe-field.wide { grid-column: auto; }
    .recipe > header { align-items: flex-start; flex-direction: column; gap: 5px; }
    .recipe > header p { text-align: left; }
  }
</style>
