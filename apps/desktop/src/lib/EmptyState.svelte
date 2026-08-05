<script lang="ts">
  interface Props {
    eyebrow: string;
    title: string;
    body: string;
    actionLabel: string;
    icon?: string;
    disabled?: boolean;
    onAction: () => void;
  }

  let {
    eyebrow,
    title,
    body,
    actionLabel,
    icon = '→',
    disabled = false,
    onAction
  }: Props = $props();
</script>

<section class="empty-state">
  <span class="route" aria-hidden="true">{icon}</span>
  <span class="eyebrow">{eyebrow}</span>
  <h2>{title}</h2>
  <p>{body}</p>
  <button type="button" {disabled} onclick={onAction}>
    <span aria-hidden="true">+</span>
    {actionLabel}
  </button>
</section>

<style>
  .empty-state {
    display: grid;
    width: min(480px, calc(100% - 40px));
    place-items: center;
    align-content: center;
    margin: auto;
    padding: 28px 24px;
    color: #90969f;
    text-align: center;
  }

  .route {
    display: grid;
    width: 40px;
    height: 40px;
    place-items: center;
    margin-bottom: 13px;
    border: 1px solid #454a51;
    background: var(--popover);
    color: #a8aeb6;
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 17px;
  }

  .eyebrow {
    color: #9198a1;
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: var(--font-size-xs);
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  h2 {
    margin: 6px 0 0;
    color: var(--foreground);
    font-size: clamp(20px, 2.8vw, 28px);
    font-weight: 620;
    letter-spacing: -0.035em;
  }

  p {
    max-width: 390px;
    margin: 8px 0 15px;
    color: #949ba4;
    font-size: 12px;
    line-height: 1.5;
  }

  button {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    min-height: 32px;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    padding: 0 11px;
    background: var(--accent);
    color: var(--foreground);
    font-size: var(--font-size-sm);
    font-weight: 650;
    cursor: pointer;
  }

  button span {
    color: var(--text-soft);
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 15px;
  }

  button:hover:not(:disabled) {
    border-color: var(--muted-foreground);
    background: var(--border);
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.45;
  }
</style>
