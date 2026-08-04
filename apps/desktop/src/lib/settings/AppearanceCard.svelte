<script lang="ts">
  import { onMount } from 'svelte';

  type AppearancePreference = 'system' | 'dark' | 'light';

  const storageKey = 'gbuild.appearance';
  const choices: { id: AppearancePreference; label: string; description: string }[] = [
    { id: 'system', label: 'System', description: 'Follow this Mac when more palettes ship.' },
    { id: 'dark', label: 'Dark', description: 'The current deep-ink workbench palette.' },
    { id: 'light', label: 'Light', description: 'Preference saved; light palette is coming later.' }
  ];

  let preference = $state<AppearancePreference>('system');

  onMount(() => {
    const stored = localStorage.getItem(storageKey);
    if (stored === 'system' || stored === 'dark' || stored === 'light') preference = stored;
    apply(preference);
  });

  function select(next: AppearancePreference): void {
    preference = next;
    localStorage.setItem(storageKey, next);
    apply(next);
  }

  function apply(next: AppearancePreference): void {
    const root = document.documentElement;
    root.dataset.appearance = next;
    // Dark is the only shipped palette; the preference remains durable for future palettes.
    root.dataset.theme = 'dark';
    root.style.colorScheme = 'dark';
  }
</script>

<section class="card appearance-card" aria-labelledby="appearance-card-title">
  <header>
    <span class="eyebrow">Interface</span>
    <h2 id="appearance-card-title">Appearance</h2>
    <p>Choose how gbuild should follow the desktop palette.</p>
  </header>
  <div class="theme-options" role="radiogroup" aria-label="Appearance preference">
    {#each choices as choice}
      <button
        type="button"
        role="radio"
        aria-checked={preference === choice.id}
        class:active={preference === choice.id}
        onclick={() => select(choice.id)}
      >
        <span class={`swatch ${choice.id}`} aria-hidden="true"><i></i><i></i><i></i></span>
        <span class="choice-copy">
          <strong>{choice.label}</strong>
          <small>{choice.description}</small>
        </span>
        <span class="choice-state">{preference === choice.id ? 'Selected' : ''}</span>
      </button>
    {/each}
  </div>
  <footer>Theme preference is stored in this desktop webview. Dark remains the rendered palette in this release.</footer>
</section>

<style>
  .card {
    border: 1px solid #29444f;
    border-radius: 5px;
    background: rgb(10 28 36 / 91%);
  }

  header { padding: 17px 18px 13px; }
  .eyebrow, .choice-copy small, .choice-state, footer { font-family: 'JetBrains Mono Variable', monospace; }
  .eyebrow { color: #6f8994; font-size: 7px; font-weight: 650; letter-spacing: 0.1em; text-transform: uppercase; }
  h2 { margin: 3px 0 0; color: #e1ebed; font-size: 17px; }
  header p { margin: 4px 0 0; color: #6e8690; font-size: 10px; }

  .theme-options { border-top: 1px solid #243e49; }

  .theme-options button {
    display: grid;
    width: 100%;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 11px;
    border: 0;
    border-bottom: 1px solid #213943;
    padding: 11px 14px;
    background: transparent;
    color: #aebfc4;
    text-align: left;
    cursor: pointer;
  }

  .theme-options button:last-child { border-bottom: 0; }
  .theme-options button:hover { background: rgb(92 130 141 / 8%); }
  .theme-options button.active { background: linear-gradient(90deg, rgb(99 215 197 / 8%), transparent); box-shadow: inset 2px 0 var(--signal); }

  .swatch {
    display: flex;
    width: 34px;
    height: 25px;
    align-items: flex-end;
    gap: 2px;
    border: 1px solid #3a5360;
    padding: 4px;
    background: #0a1820;
  }

  .swatch i { width: 7px; background: #27414d; }
  .swatch i:nth-child(1) { height: 14px; }
  .swatch i:nth-child(2) { height: 9px; background: var(--signal); }
  .swatch i:nth-child(3) { height: 5px; }
  .swatch.light { background: #d8e2e3; }
  .swatch.light i { background: #8aa1a7; }
  .swatch.light i:nth-child(2) { background: #348b80; }
  .swatch.system { background: linear-gradient(135deg, #d8e2e3 0 49%, #0a1820 51%); }

  .choice-copy { min-width: 0; }
  .choice-copy strong, .choice-copy small { display: block; }
  .choice-copy strong { color: #b9c8cd; font-size: 10px; }
  .choice-copy small { margin-top: 3px; color: #5d7680; font-size: 7px; line-height: 1.35; }
  .choice-state { min-width: 45px; color: var(--signal); font-size: 7px; text-align: right; text-transform: uppercase; }

  footer {
    border-top: 1px solid #243e49;
    padding: 9px 14px;
    color: #526b75;
    font-size: 7px;
    line-height: 1.45;
  }
</style>
