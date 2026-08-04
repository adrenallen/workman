<script lang="ts">
  export type WorkspaceView = 'processes' | 'coordination';

  interface Props {
    active: WorkspaceView;
    onSelect: (view: WorkspaceView) => void;
  }

  let { active, onSelect }: Props = $props();
</script>

<nav class="workspace-tabs" aria-label="Workspace view">
  <button
    type="button"
    class:active={active === 'processes'}
    aria-current={active === 'processes' ? 'page' : undefined}
    onclick={() => onSelect('processes')}
  >
    <span aria-hidden="true">⌁</span>
    Processes
  </button>
  <button
    type="button"
    class:active={active === 'coordination'}
    aria-current={active === 'coordination' ? 'page' : undefined}
    onclick={() => onSelect('coordination')}
  >
    <span aria-hidden="true">◇</span>
    Coordination
  </button>
</nav>

<style>
  .workspace-tabs {
    display: flex;
    align-items: center;
    gap: 3px;
    margin: 8px clamp(24px, 4vw, 58px) 0;
    border-bottom: 1px solid #233a45;
  }

  button {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    border: 0;
    padding: 7px 10px 9px;
    background: transparent;
    color: #647d88;
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 8px;
    font-weight: 620;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
  }

  button::after {
    position: absolute;
    right: 8px;
    bottom: -1px;
    left: 8px;
    height: 1px;
    background: transparent;
    content: '';
  }

  button:hover { color: #a3b6bd; }
  button.active { color: #cbdadd; }
  button.active::after { background: var(--signal); box-shadow: 0 0 8px rgb(99 215 197 / 45%); }
  button span { color: #526e79; font-size: 11px; }
  button.active span { color: var(--signal); }
</style>
