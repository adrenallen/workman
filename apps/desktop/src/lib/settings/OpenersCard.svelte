<script lang="ts">
  import { onMount } from 'svelte';

  import {
    editorForSelection,
    ensureOpenersLoaded,
    openerSettings,
    setOpenersConfig,
    templateError,
    type OpenersConfig
  } from '../openers';

  let advanced = $state<'editor' | 'terminal' | 'browser' | null>(null);

  let selectedEditor = $derived(
    editorForSelection($openerSettings.config, $openerSettings.editors)
  );
  let editorError = $derived(
    $openerSettings.config.editor.selection === 'custom'
      ? templateError($openerSettings.config.editor.customTemplate)
      : null
  );
  let terminalError = $derived(
    $openerSettings.config.terminal.selection === 'custom'
      ? templateError($openerSettings.config.terminal.customTemplate)
      : null
  );
  let browserError = $derived(
    $openerSettings.config.browser.selection === 'custom'
      ? templateError($openerSettings.config.browser.customTemplate)
      : null
  );
  let sidebarCustomError = $derived(
    $openerSettings.config.sidebar.customEnabled
      ? templateError($openerSettings.config.sidebar.customTemplate)
      : null
  );

  onMount(() => {
    void ensureOpenersLoaded();
  });

  function change(update: (config: OpenersConfig) => void): void {
    const next = structuredClone($openerSettings.config);
    update(next);
    setOpenersConfig(next);
  }

  function toggleAdvanced(section: 'editor' | 'terminal' | 'browser'): void {
    advanced = advanced === section ? null : section;
  }

  function value(event: Event): string {
    return (event.currentTarget as HTMLInputElement | HTMLSelectElement).value;
  }

  function checked(event: Event): boolean {
    return (event.currentTarget as HTMLInputElement).checked;
  }
</script>

<section class="openers-card" aria-labelledby="openers-title">
  <header class="card-header">
    <div>
      <span class="eyebrow">Project tools</span>
      <h2 id="openers-title">Openers</h2>
      <p>Choose the apps Workman hands projects to. Sidebar actions use the same choices.</p>
    </div>
    <span
      class="detection"
      class:warning={$openerSettings.error !== null}
      title={$openerSettings.error ? 'App detection unavailable' : 'Detected local project opener applications'}
    >
      {#if !$openerSettings.loaded}
        Detecting apps…
      {:else if $openerSettings.error}
        Detection unavailable
      {:else}
        {$openerSettings.editors.length} editor{$openerSettings.editors.length === 1 ? '' : 's'} found
      {/if}
    </span>
  </header>

  <div class="tool-list">
    <div class="tool-row" class:expanded={advanced === 'editor'}>
      <span class="tool-mark" aria-hidden="true">&lt;&gt;</span>
      <label class="tool-copy" for="default-editor">
        <strong>Default editor</strong>
        <small>Used when opening projects. Can be overridden per-project.</small>
      </label>
      <div class="tool-controls">
        <select
          id="default-editor"
          value={$openerSettings.config.editor.selection}
          disabled={!$openerSettings.loaded}
          onchange={(event) => change((config) => {
            config.editor.selection = value(event) as OpenersConfig['editor']['selection'];
            if (config.editor.selection === 'custom') advanced = 'editor';
          })}
        >
          {#each $openerSettings.editors as editor (editor.id)}
            <option value={`detected:${editor.id}`}>{editor.label}</option>
          {/each}
          <option value="custom">Custom command…</option>
        </select>
        <button
          class="gear"
          class:active={advanced === 'editor'}
          type="button"
          aria-label="Configure editor command"
          aria-expanded={advanced === 'editor'}
          onclick={() => toggleAdvanced('editor')}
        >⚙</button>
      </div>
      {#if advanced === 'editor'}
        <div class="advanced">
          {#if selectedEditor}
            <span class="resolved" title={`Editor detected · ${selectedEditor.bundle_path}`}><i aria-hidden="true"></i>{selectedEditor.bundle_path}</span>
          {:else}
            <label for="editor-template">Command template</label>
            <input
              id="editor-template"
              class:invalid={editorError !== null}
              value={$openerSettings.config.editor.customTemplate}
              placeholder={'code {path}'}
              spellcheck="false"
              oninput={(event) => change((config) => { config.editor.customTemplate = value(event); })}
            />
            <small class:error={editorError !== null}>{editorError ?? 'Arguments are parsed directly; no shell is used.'}</small>
          {/if}
        </div>
      {/if}
    </div>

    <div class="tool-row" class:expanded={advanced === 'terminal'}>
      <span class="tool-mark" aria-hidden="true">&gt;_</span>
      <label class="tool-copy" for="default-terminal">
        <strong>Default terminal</strong>
        <small>Used when opening projects from the sidebar.</small>
      </label>
      <div class="tool-controls">
        <select
          id="default-terminal"
          value={$openerSettings.config.terminal.selection}
          onchange={(event) => change((config) => {
            config.terminal.selection = value(event) as OpenersConfig['terminal']['selection'];
            if (config.terminal.selection === 'custom') advanced = 'terminal';
          })}
        >
          <option value="system">Terminal (system)</option>
          <option value="custom">Custom command…</option>
        </select>
        <button
          class="gear"
          class:active={advanced === 'terminal'}
          type="button"
          aria-label="Configure terminal command"
          aria-expanded={advanced === 'terminal'}
          onclick={() => toggleAdvanced('terminal')}
        >⚙</button>
      </div>
      {#if advanced === 'terminal'}
        <div class="advanced">
          {#if $openerSettings.config.terminal.selection === 'system'}
            <span class="resolved" title="Terminal opener detected · macOS Terminal"><i aria-hidden="true"></i>macOS Terminal · system application</span>
          {:else}
            <label for="terminal-template">Command template</label>
            <input
              id="terminal-template"
              class:invalid={terminalError !== null}
              value={$openerSettings.config.terminal.customTemplate}
              placeholder={'open -a iTerm {path}'}
              spellcheck="false"
              oninput={(event) => change((config) => { config.terminal.customTemplate = value(event); })}
            />
            <small class:error={terminalError !== null}>{terminalError ?? 'Use {path} for the project directory.'}</small>
          {/if}
        </div>
      {/if}
    </div>

    <div class="tool-row" class:expanded={advanced === 'browser'}>
      <span class="tool-mark" aria-hidden="true">◎</span>
      <label class="tool-copy" for="default-browser">
        <strong>Default browser</strong>
        <small>Used when opening local service URLs from the process sidebar.</small>
      </label>
      <div class="tool-controls">
        <select
          id="default-browser"
          value={$openerSettings.config.browser.selection}
          onchange={(event) => change((config) => {
            config.browser.selection = value(event) as OpenersConfig['browser']['selection'];
            if (config.browser.selection === 'custom') advanced = 'browser';
          })}
        >
          <option value="system">System default</option>
          <option value="custom">Custom command…</option>
        </select>
        <button
          class="gear"
          class:active={advanced === 'browser'}
          type="button"
          aria-label="Configure browser command"
          aria-expanded={advanced === 'browser'}
          onclick={() => toggleAdvanced('browser')}
        >⚙</button>
      </div>
      {#if advanced === 'browser'}
        <div class="advanced">
          {#if $openerSettings.config.browser.selection === 'system'}
            <span class="resolved" title="Browser opener detected · macOS default browser"><i aria-hidden="true"></i>Use the macOS default browser</span>
          {:else}
            <label for="browser-template">Command template</label>
            <input
              id="browser-template"
              class:invalid={browserError !== null}
              value={$openerSettings.config.browser.customTemplate}
              placeholder={'open -a Safari {path}'}
              spellcheck="false"
              oninput={(event) => change((config) => { config.browser.customTemplate = value(event); })}
            />
            <small class:error={browserError !== null}>{browserError ?? 'Use {path} for the URL or local path.'}</small>
          {/if}
        </div>
      {/if}
    </div>
  </div>

  <div class="sidebar-actions">
    <div class="section-copy">
      <span class="eyebrow">Sidebar actions</span>
      <strong>Show on project hover</strong>
      <small>These choices also control the project context menu.</small>
    </div>
    <div class="switches">
      <label>
        <input
          type="checkbox"
          checked={$openerSettings.config.sidebar.editorEnabled}
          onchange={(event) => change((config) => { config.sidebar.editorEnabled = checked(event); })}
        />
        <span aria-hidden="true"></span>Editor
      </label>
      <label>
        <input
          type="checkbox"
          checked={$openerSettings.config.sidebar.finderEnabled}
          onchange={(event) => change((config) => { config.sidebar.finderEnabled = checked(event); })}
        />
        <span aria-hidden="true"></span>Finder
      </label>
      <label>
        <input
          type="checkbox"
          checked={$openerSettings.config.sidebar.customEnabled}
          onchange={(event) => change((config) => { config.sidebar.customEnabled = checked(event); })}
        />
        <span aria-hidden="true"></span>Custom
      </label>
    </div>
    <div class="custom-slot" class:enabled={$openerSettings.config.sidebar.customEnabled}>
      <label for="custom-label">Button label</label>
      <input
        id="custom-label"
        value={$openerSettings.config.sidebar.customLabel}
        disabled={!$openerSettings.config.sidebar.customEnabled}
        maxlength="32"
        placeholder="Open in…"
        oninput={(event) => change((config) => { config.sidebar.customLabel = value(event); })}
      />
      <label for="custom-template">Command template</label>
      <input
        id="custom-template"
        class:invalid={sidebarCustomError !== null}
        value={$openerSettings.config.sidebar.customTemplate}
        disabled={!$openerSettings.config.sidebar.customEnabled}
        placeholder={'my-app --project {path}'}
        spellcheck="false"
        oninput={(event) => change((config) => { config.sidebar.customTemplate = value(event); })}
      />
      {#if $openerSettings.config.sidebar.customEnabled}
        <small class="slot-help" class:error={sidebarCustomError !== null}>{sidebarCustomError ?? 'The project path is substituted as one argv value.'}</small>
      {/if}
    </div>
  </div>

  <footer>Preferences are saved in this desktop app. Custom commands execute as argv, never as shell text.</footer>
</section>

<style>
  .openers-card {
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
  }

  .card-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 14px 11px;
    border-bottom: 1px solid var(--border);
  }

  .eyebrow,
  .detection,
  .tool-copy small,
  .advanced,
  .section-copy small,
  .custom-slot label,
  .slot-help,
  footer {
    font-family: 'JetBrains Mono Variable', monospace;
  }

  .eyebrow { color: var(--muted-foreground); font-size: var(--font-size-xs); font-weight: 680; letter-spacing: .1em; text-transform: uppercase; }
  h2 { margin: 3px 0 0; color: var(--text); font-size: 15px; }
  .card-header p { margin: 4px 0 0; color: var(--muted); font-size: var(--font-size-sm); line-height: 1.45; }
  .detection { flex: none; margin-top: 2px; color: #7fb6a7; font-size: var(--font-size-xs); }
  .detection::before { display: inline-block; width: 5px; height: 5px; margin-right: 5px; border-radius: 50%; background: var(--signal); content: ''; }
  .detection.warning { color: var(--warning); }
  .detection.warning::before { background: var(--warning); }

  .tool-list { padding: 0 14px; }
  .tool-row {
    display: grid;
    grid-template-columns: 31px minmax(180px, 1fr) minmax(180px, 240px);
    align-items: center;
    column-gap: 10px;
    min-height: 57px;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 72%, transparent);
  }
  .tool-row.expanded { padding-top: 5px; }
  .tool-mark { display: grid; width: 27px; height: 27px; place-items: center; border: 1px solid #434950; border-radius: 3px; background: var(--popover); color: #a8b0b8; font: 650 var(--font-size-xs)/1 'JetBrains Mono Variable', monospace; }
  .tool-copy { min-width: 0; }
  .tool-copy strong, .tool-copy small { display: block; }
  .tool-copy strong { color: var(--text); font-size: var(--font-size-sm); font-weight: 640; }
  .tool-copy small { margin-top: 3px; color: var(--muted); font-size: var(--font-size-xs); line-height: 1.35; }
  .tool-controls { display: grid; grid-template-columns: minmax(0, 1fr) 28px; gap: 5px; }

  select,
  input {
    min-width: 0;
    height: 28px;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    padding: 0 8px;
    background: var(--popover);
    color: var(--text-soft);
    font: var(--font-size-xs) 'JetBrains Mono Variable', monospace;
  }
  select:focus-visible, input:focus-visible, button:focus-visible { border-color: var(--muted-foreground); outline: 1px solid var(--muted-foreground); outline-offset: 1px; }
  input.invalid { border-color: color-mix(in srgb, var(--fault) 72%, var(--border)); }
  input:disabled, select:disabled { opacity: .5; }
  .gear { width: 28px; height: 28px; border: 1px solid var(--border-strong); border-radius: 3px; background: var(--border); color: #9098a1; font-size: var(--font-size-sm); cursor: pointer; }
  .gear:hover, .gear.active { border-color: #68717b; background: #2d3238; color: #e0e3e6; }

  .advanced { display: grid; grid-column: 2 / -1; grid-template-columns: 100px minmax(0, 1fr); align-items: center; gap: 5px 8px; padding: 0 0 9px; }
  .advanced label { color: #929aa3; font-size: var(--font-size-xs); }
  .advanced small { grid-column: 2; color: #747c85; font-size: var(--font-size-xs); }
  .advanced small.error, .slot-help.error { color: #d88e8e; }
  .resolved { grid-column: 1 / -1; overflow: hidden; padding: 6px 8px; border: 1px solid #30353b; border-radius: 3px; color: #818a94; font-size: var(--font-size-xs); text-overflow: ellipsis; white-space: nowrap; }
  .resolved i { display: inline-block; width: 5px; height: 5px; margin-right: 6px; border-radius: 50%; background: var(--signal); }

  .sidebar-actions { display: grid; grid-template-columns: minmax(180px, .8fr) minmax(240px, 1fr); gap: 10px 18px; padding: 12px 14px; background: color-mix(in srgb, var(--surface-raised) 38%, var(--surface)); }
  .section-copy { grid-row: span 2; }
  .section-copy strong, .section-copy small { display: block; }
  .section-copy strong { margin-top: 4px; color: var(--text); font-size: var(--font-size-sm); }
  .section-copy small { margin-top: 4px; color: var(--muted); font-size: var(--font-size-xs); line-height: 1.4; }
  .switches { display: flex; flex-wrap: wrap; gap: 6px 14px; }
  .switches label { display: flex; align-items: center; gap: 6px; color: var(--text-soft); font-size: var(--font-size-xs); cursor: pointer; }
  .switches input { position: absolute; width: 1px; height: 1px; opacity: 0; }
  .switches label > span { position: relative; width: 23px; height: 13px; border: 1px solid #4a5058; border-radius: 8px; background: var(--popover); }
  .switches label > span::after { position: absolute; top: 2px; left: 2px; width: 7px; height: 7px; border-radius: 50%; background: #7c848d; content: ''; transition: transform 100ms ease-out, background 100ms ease-out; }
  .switches input:checked + span { border-color: #4f746a; background: #20362f; }
  .switches input:checked + span::after { background: var(--signal); transform: translateX(10px); }
  .switches input:focus-visible + span { outline: 1px solid var(--muted-foreground); outline-offset: 2px; }

  .custom-slot { display: grid; grid-template-columns: 70px minmax(100px, .45fr) 100px minmax(140px, 1fr); align-items: center; gap: 5px 7px; opacity: .48; }
  .custom-slot.enabled { opacity: 1; }
  .custom-slot label { color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .slot-help { grid-column: 4; color: #747c85; font-size: var(--font-size-xs); }

  footer { padding: 7px 14px; border-top: 1px solid var(--border); color: var(--muted-foreground); font-size: var(--font-size-xs); }

  @media (max-width: 820px) {
    .tool-row { grid-template-columns: 31px minmax(0, 1fr); padding: 9px 0; }
    .tool-controls { grid-column: 2; width: 100%; margin-top: 7px; }
    .advanced { grid-column: 2; margin-top: 7px; }
    .sidebar-actions { grid-template-columns: 1fr; }
    .section-copy { grid-row: auto; }
    .custom-slot { grid-template-columns: 70px minmax(0, 1fr); }
    .slot-help { grid-column: 2; }
  }

  @media (prefers-reduced-motion: reduce) {
    .switches label > span::after { transition: none; }
  }
</style>
