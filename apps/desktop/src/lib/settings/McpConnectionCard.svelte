<script lang="ts">
  import type { McpConnectionInfo } from '../settings';
  import CopyField from './CopyField.svelte';

  interface Props {
    connection: McpConnectionInfo;
  }

  let { connection }: Props = $props();
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
      <h2 id="mcp-card-title">Connect Claude Code</h2>
      <p>One authenticated local endpoint gives agents access to this gbuild daemon.</p>
    </div>
    <span class="availability"><i aria-hidden="true"></i>Local only</span>
  </header>

  <div class="handshake" aria-label="Connection path">
    <span>Claude Code</span><i aria-hidden="true"></i><span>127.0.0.1</span><i aria-hidden="true"></i><span>gbuild MCP</span>
  </div>

  <div class="connection-fields">
    <CopyField label="Endpoint" value={connection.endpoint} />
    <CopyField
      label="Bearer token"
      value={connection.token}
      displayValue={maskedToken}
      sensitive
    />
    <div class="command-field">
      <CopyField
        label="Ready-to-paste command"
        value={connection.claude_command}
        multiline
        sensitive
      />
      <p>Paste this command into a shell once. It registers the endpoint and authorization header together.</p>
    </div>
  </div>
</section>

<style>
  .card {
    border: 1px solid #2a4652;
    border-radius: 5px;
    background: linear-gradient(145deg, rgb(15 37 47 / 96%), rgb(8 24 31 / 96%));
    box-shadow: 0 18px 45px rgb(0 0 0 / 13%);
  }

  .mcp-card { overflow: hidden; }

  .card-heading {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 13px;
    padding: 18px 20px;
  }

  .card-icon {
    display: grid;
    width: 38px;
    height: 38px;
    place-items: center;
    border: 1px solid #3e716f;
    background: rgb(99 215 197 / 9%);
    color: var(--signal);
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 15px;
    font-weight: 720;
  }

  .eyebrow,
  .availability,
  .handshake {
    font-family: 'JetBrains Mono Variable', monospace;
    text-transform: uppercase;
  }

  .eyebrow {
    color: var(--signal);
    font-size: 7px;
    font-weight: 650;
    letter-spacing: 0.1em;
  }

  h2 {
    margin: 3px 0 0;
    color: #e4edef;
    font-size: 18px;
    font-weight: 630;
  }

  .card-heading p,
  .command-field p {
    margin: 4px 0 0;
    color: #78909a;
    font-size: 10px;
    line-height: 1.5;
  }

  .availability {
    display: flex;
    align-items: center;
    gap: 7px;
    color: #76909a;
    font-size: 7px;
    letter-spacing: 0.06em;
  }

  .availability i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--signal);
    box-shadow: 0 0 9px rgb(99 215 197 / 55%);
  }

  .handshake {
    display: grid;
    grid-template-columns: auto minmax(24px, 1fr) auto minmax(24px, 1fr) auto;
    align-items: center;
    gap: 9px;
    border-block: 1px solid #27434e;
    padding: 9px 20px;
    background: rgb(7 22 29 / 56%);
    color: #76919a;
    font-size: 7px;
    letter-spacing: 0.08em;
  }

  .handshake i {
    height: 1px;
    background: linear-gradient(90deg, #31545d, var(--signal), #31545d);
  }

  .connection-fields {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 14px;
    padding: 18px 20px 20px;
  }

  .command-field { grid-column: 1 / -1; }

  @media (max-width: 720px) {
    .card-heading { grid-template-columns: auto minmax(0, 1fr); }
    .availability { grid-column: 2; }
    .connection-fields { grid-template-columns: 1fr; }
    .command-field { grid-column: auto; }
  }
</style>
