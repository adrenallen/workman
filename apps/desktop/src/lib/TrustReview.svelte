<script lang="ts">
  import type { TrustFieldChange, TrustReview } from './daemon';

  interface Props {
    review: TrustReview;
    busy: boolean;
    onApprove: () => void;
    onClose: () => void;
  }

  let { review, busy, onApprove, onClose }: Props = $props();

  function formatValue(value: unknown): string {
    if (typeof value === 'string') return value || '—';
    return JSON.stringify(value, null, 2);
  }

  function changeLabel(change: TrustFieldChange): string {
    return change.previous === null ? 'New' : 'Changed';
  }

  function closeOnEscape(event: KeyboardEvent): void {
    if (event.key === 'Escape' && !busy) onClose();
  }
</script>

<svelte:window onkeydown={closeOnEscape} />

<div
  class="backdrop"
  role="presentation"
  onclick={(event) => {
    if (event.target === event.currentTarget && !busy) onClose();
  }}
>
  <div
    class="review"
    role="dialog"
    aria-modal="true"
    aria-labelledby="trust-title"
    aria-describedby="trust-description"
  >
    <header>
      <div class="interlock" aria-hidden="true"><span></span><i></i></div>
      <div>
        <span class="eyebrow">Command interlock</span>
        <h2 id="trust-title">Review {review.process_name}</h2>
      </div>
      <button class="close" type="button" aria-label="Close trust review" disabled={busy} onclick={onClose}>×</button>
    </header>

    <p id="trust-description" class="description">
      This repository is asking gbuild to execute a command on your machine. Approval applies only
      to the exact fields below; another change locks it again.
    </p>

    <div class="change-summary">
      <strong>{review.changes.length.toString().padStart(2, '0')}</strong>
      <span>{review.changes.some((change) => change.previous !== null) ? 'fields changed since approval' : 'fields require first review'}</span>
    </div>

    <div class="changes">
      {#each review.changes as change (change.field)}
        <article>
          <div class="field-heading">
            <strong>{change.field.replaceAll('_', ' ')}</strong>
            <span class:changed={change.previous !== null}>{changeLabel(change)}</span>
          </div>
          {#if change.previous !== null}
            <div class="value previous">
              <small>Previous</small>
              <pre>{formatValue(change.previous)}</pre>
            </div>
          {/if}
          <div class="value current">
            <small>{change.previous === null ? 'Requested' : 'Now'}</small>
            <pre>{formatValue(change.current)}</pre>
          </div>
        </article>
      {/each}
    </div>

    <footer>
      <code title={review.expected_hash}>{review.expected_hash.slice(0, 19)}…</code>
      <div>
        <button class="cancel" type="button" disabled={busy} onclick={onClose}>Keep blocked</button>
        <button class="approve" type="button" disabled={busy} onclick={onApprove}>
          {busy ? 'Approving…' : 'Trust & allow'}
        </button>
      </div>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    z-index: 30;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 24px;
    background: rgb(3 10 15 / 78%);
    backdrop-filter: blur(7px);
  }

  .review {
    width: min(720px, 100%);
    max-height: min(780px, calc(100vh - 48px));
    overflow: hidden;
    border: 1px solid #7e633c;
    border-radius: 5px;
    background: #0d1b24;
    box-shadow: 0 28px 90px rgb(0 0 0 / 48%), inset 0 1px rgb(228 174 91 / 8%);
  }

  header {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 14px;
    padding: 20px 22px 17px;
    border-bottom: 1px solid #473b2d;
    background: linear-gradient(105deg, rgb(228 174 91 / 9%), transparent 66%);
  }

  .interlock {
    position: relative;
    display: grid;
    width: 36px;
    height: 36px;
    place-items: center;
    border: 1px solid #8a6b3c;
    transform: rotate(45deg);
  }

  .interlock span {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #e4ae5b;
    box-shadow: 0 0 0 5px rgb(228 174 91 / 10%);
  }

  .interlock i {
    position: absolute;
    right: 4px;
    bottom: 4px;
    width: 5px;
    height: 5px;
    background: #765c38;
  }

  .eyebrow,
  .field-heading span,
  .value small,
  footer button,
  footer code,
  .change-summary span {
    font-family: 'JetBrains Mono Variable', monospace;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .eyebrow {
    color: #e4ae5b;
    font-size: 8px;
  }

  h2 {
    margin: 5px 0 0;
    color: #edf2f3;
    font-size: 22px;
    font-weight: 510;
    letter-spacing: -0.025em;
  }

  .close {
    width: 32px;
    height: 32px;
    border: 1px solid transparent;
    background: transparent;
    color: #78909b;
    font-size: 22px;
    cursor: pointer;
  }

  .close:hover:not(:disabled) {
    border-color: #4a5e68;
    color: #d4dfe3;
  }

  .description {
    margin: 0;
    padding: 17px 22px;
    color: #8da1aa;
    font-size: 12px;
    line-height: 1.6;
  }

  .change-summary {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 0 22px 12px;
  }

  .change-summary strong {
    color: #e4ae5b;
    font-size: 23px;
    font-weight: 430;
  }

  .change-summary span {
    color: #6f858e;
    font-size: 7px;
  }

  .changes {
    max-height: min(430px, 50vh);
    overflow-y: auto;
    padding: 0 22px 5px;
    scrollbar-color: #3f4d4e transparent;
    scrollbar-width: thin;
  }

  article {
    margin-bottom: 9px;
    border: 1px solid #293e48;
    border-left: 2px solid #8a6b3c;
    background: #0a171f;
  }

  .field-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 10px;
    border-bottom: 1px solid #263943;
  }

  .field-heading strong {
    color: #b8c7cd;
    font-size: 10px;
    font-weight: 600;
    text-transform: capitalize;
  }

  .field-heading span {
    color: #e4ae5b;
    font-size: 7px;
  }

  .field-heading span.changed {
    color: #ef9a75;
  }

  .value {
    display: grid;
    grid-template-columns: 68px minmax(0, 1fr);
    gap: 8px;
    padding: 8px 10px;
  }

  .value + .value {
    border-top: 1px solid #20343e;
  }

  .value small {
    padding-top: 2px;
    color: #607984;
    font-size: 7px;
  }

  .value pre {
    min-width: 0;
    margin: 0;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
    color: #b8c7cd;
    font-family: 'JetBrains Mono Variable', monospace;
    font-size: 10px;
    line-height: 1.5;
  }

  .value.previous pre {
    color: #8b7c78;
    text-decoration-color: rgb(239 125 117 / 55%);
    text-decoration-line: line-through;
  }

  .value.current pre {
    color: #d4dfd9;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 15px;
    padding: 15px 22px 18px;
    border-top: 1px solid #293d46;
    background: #0b1921;
  }

  footer code {
    overflow: hidden;
    color: #566f79;
    font-size: 7px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  footer div {
    display: flex;
    gap: 7px;
  }

  footer button {
    border-radius: 2px;
    padding: 9px 12px;
    font-size: 8px;
    font-weight: 650;
    cursor: pointer;
  }

  .cancel {
    border: 1px solid #354b56;
    background: transparent;
    color: #899da6;
  }

  .approve {
    border: 1px solid #e4ae5b;
    background: #e4ae5b;
    color: #1c1811;
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.48;
  }

  @media (max-width: 620px) {
    .backdrop { padding: 10px; }
    .value { grid-template-columns: 1fr; }
    footer { align-items: stretch; flex-direction: column; }
    footer div { justify-content: flex-end; }
  }
</style>
