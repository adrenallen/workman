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
    padding: 52px 32px;
    color: #738b98;
    text-align: center;
  }

  .route {
    display: grid;
    width: 48px;
    height: 48px;
    place-items: center;
    margin-bottom: 20px;
    border: 1px solid #31505d;
    background: #0d202a;
    color: var(--signal);
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 20px;
    box-shadow: 7px 7px 0 #0a1820, 8px 8px 0 #203943;
  }

  .eyebrow {
    color: var(--signal);
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 8px;
    font-weight: 700;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  h2 {
    margin: 9px 0 0;
    color: #e4ecef;
    font-size: clamp(22px, 3.2vw, 34px);
    font-weight: 540;
    letter-spacing: -0.035em;
  }

  p {
    max-width: 390px;
    margin: 12px 0 22px;
    color: #6f8793;
    font-size: 13px;
    line-height: 1.6;
  }

  button {
    display: inline-flex;
    align-items: center;
    gap: 9px;
    min-height: 38px;
    border: 1px solid #4d847b;
    border-radius: 3px;
    padding: 0 16px;
    background: #17372f;
    color: #e1f2ee;
    font-size: 11px;
    font-weight: 650;
    cursor: pointer;
  }

  button span {
    color: var(--signal);
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 15px;
  }

  button:hover:not(:disabled) {
    border-color: var(--signal);
    background: #1b4137;
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.45;
  }
</style>
