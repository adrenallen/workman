<script lang="ts">
  import type { AgentTool } from './agentTools';
  import type { Project } from './daemon';
  import {
    fuzzySubsequenceScore,
    navigationTargetKey,
    type AppNavigationTarget,
    type NavigationProjectSnapshot
  } from './navigation';
  import { projectTreeSelection, type ProjectTreeItemKind } from './projectTree';

  interface Props {
    projects: Project[];
    index: Record<number, NavigationProjectSnapshot>;
    currentProjectId: number | null;
    agentTools: AgentTool[];
    recentKeys: string[];
    loading: boolean;
    onChoose: (target: AppNavigationTarget) => void;
    onClose: () => void;
  }

  type PaletteKind = 'action' | 'project' | ProjectTreeItemKind;

  interface PaletteEntry {
    key: string;
    kind: PaletteKind;
    label: string;
    detail: string;
    projectName: string | null;
    searchText: string;
    target: AppNavigationTarget;
    creation: boolean;
  }

  interface RankedEntry extends PaletteEntry {
    score: number;
    recentRank: number | null;
  }

  let {
    projects,
    index,
    currentProjectId,
    agentTools,
    recentKeys,
    loading,
    onChoose,
    onClose
  }: Props = $props();

  let query = $state('');
  let selectedIndex = $state(0);
  let entries = $derived(buildEntries());
  let rankedEntries = $derived(rankEntries(entries, query, recentKeys));
  let activeEntry = $derived(rankedEntries[selectedIndex] ?? null);

  $effect(() => {
    if (selectedIndex >= rankedEntries.length) selectedIndex = Math.max(0, rankedEntries.length - 1);
  });

  function buildEntries(): PaletteEntry[] {
    const next: PaletteEntry[] = [];
    const currentProject = projects.find((project) => project.id === currentProjectId) ?? null;

    next.push({
      key: 'action:settings',
      kind: 'action',
      label: 'Open Settings',
      detail: 'Application and daemon settings',
      projectName: currentProject ? projectLabel(currentProject) : null,
      searchText: 'open settings preferences configuration',
      target: { type: 'settings', projectId: currentProject?.id },
      creation: false
    });

    for (const project of projects) {
      const name = projectLabel(project);
      next.push(
        {
          key: `action:new-terminal:${project.id}`,
          kind: 'action',
          label: `New terminal in ${name}`,
          detail: 'Start a shell and jump to it',
          projectName: name,
          searchText: `new terminal shell create spawn ${name} ${project.name} ${project.path}`,
          target: { type: 'new-terminal', projectId: project.id },
          creation: true
        },
        {
          key: `action:add-command:${project.id}`,
          kind: 'action',
          label: `Add command in ${name}`,
          detail: 'Add a command to gbuild.yml',
          projectName: name,
          searchText: `add new command gbuild yml ${name} ${project.name} ${project.path}`,
          target: { type: 'add-command', projectId: project.id },
          creation: true
        },
        {
          key: `action:new-todo:${project.id}`,
          kind: 'action',
          label: `New todo in ${name}`,
          detail: 'Open the todo creation form',
          projectName: name,
          searchText: `new add create todo task ${name} ${project.name} ${project.path}`,
          target: { type: 'new-todo', projectId: project.id },
          creation: true
        },
        {
          key: `action:new-scratchpad:${project.id}`,
          kind: 'action',
          label: `New scratchpad in ${name}`,
          detail: 'Open the scratchpad creation form',
          projectName: name,
          searchText: `new add create scratchpad note ${name} ${project.name} ${project.path}`,
          target: { type: 'new-scratchpad', projectId: project.id },
          creation: true
        }
      );

      for (const tool of agentTools) {
        next.push({
          key: `action:spawn-agent:${project.id}:${tool.id}`,
          kind: 'action',
          label: `Spawn ${tool.name} in ${name}`,
          detail: tool.command,
          projectName: name,
          searchText: `spawn new create agent ${tool.name} ${tool.command} ${name} ${project.name} ${project.path}`,
          target: {
            type: 'spawn-agent',
            projectId: project.id,
            agentToolId: tool.id,
            agentToolName: tool.name
          },
          creation: true
        });
      }
    }

    for (const project of projects) {
      const name = projectLabel(project);
      const snapshot = index[project.id];
      const target: AppNavigationTarget = { type: 'project', projectId: project.id };
      next.push({
        key: navigationTargetKey(target),
        kind: 'project',
        label: name,
        detail: project.path,
        projectName: name,
        searchText: `${name} ${project.name} ${project.path} project`,
        target,
        creation: false
      });

      for (const process of snapshot?.processes ?? []) {
        const label = process.kind === 'terminal' ? workingDirLabel(process.working_dir) : process.name;
        const selection = projectTreeSelection(process.kind, process.id, project.id, label);
        const processTarget: AppNavigationTarget = { type: 'item', selection };
        next.push({
          key: navigationTargetKey(processTarget),
          kind: process.kind,
          label,
          detail: process.command ?? process.working_dir,
          projectName: name,
          searchText: `${label} ${process.name} ${process.command ?? ''} ${process.working_dir} ${name} ${process.kind}`,
          target: processTarget,
          creation: false
        });
      }

      for (const todo of snapshot?.coordination?.todos ?? []) {
        const selection = projectTreeSelection('todo', todo.id, project.id, todo.title);
        const todoTarget: AppNavigationTarget = { type: 'item', selection };
        next.push({
          key: navigationTargetKey(todoTarget),
          kind: 'todo',
          label: todo.title,
          detail: `${todo.status.replace('_', ' ')} · ${todo.priority}`,
          projectName: name,
          searchText: `${todo.title} ${todo.tags.join(' ')} ${todo.status} ${todo.priority} ${name} todo`,
          target: todoTarget,
          creation: false
        });
      }

      for (const scratchpad of snapshot?.coordination?.scratchpads ?? []) {
        const selection = projectTreeSelection(
          'scratchpad',
          scratchpad.id,
          project.id,
          scratchpad.name
        );
        const scratchpadTarget: AppNavigationTarget = { type: 'item', selection };
        next.push({
          key: navigationTargetKey(scratchpadTarget),
          kind: 'scratchpad',
          label: scratchpad.name,
          detail: `revision ${scratchpad.revision}`,
          projectName: name,
          searchText: `${scratchpad.name} ${scratchpad.tags.join(' ')} ${name} scratchpad note`,
          target: scratchpadTarget,
          creation: false
        });
      }
    }

    return next;
  }

  function rankEntries(
    candidates: PaletteEntry[],
    value: string,
    recents: string[]
  ): RankedEntry[] {
    const needle = value.trim();
    const creationIntent = /^(new|spawn|add)(?:\s|$)/i.test(needle);
    const recentRank = new Map(recents.map((key, rank) => [key, rank]));
    const ranked: RankedEntry[] = [];

    for (const candidate of candidates) {
      const projectId = targetProjectId(candidate.target);
      if (!needle && candidate.creation && projectId !== currentProjectId) continue;
      const labelScore = fuzzySubsequenceScore(needle, candidate.label);
      const searchScore = fuzzySubsequenceScore(needle, candidate.searchText);
      const fuzzyScore = Math.max(labelScore ?? Number.NEGATIVE_INFINITY, searchScore ?? Number.NEGATIVE_INFINITY);
      if (needle && !Number.isFinite(fuzzyScore)) continue;
      const rank = recentRank.get(candidate.key) ?? null;
      const actionBonus = candidate.kind === 'action' ? (needle ? 4 : 120) : 0;
      const creationBonus = candidate.creation && creationIntent ? 180 : 0;
      const recentBonus = rank === null ? 0 : needle ? Math.max(2, 18 - rank) : 500 - rank * 12;
      const currentBonus = candidate.projectName && candidate.target.type !== 'project' && projectId === currentProjectId ? (needle ? 8 : 36) : 0;
      ranked.push({
        ...candidate,
        score: (needle ? fuzzyScore : 0) + actionBonus + creationBonus + recentBonus + currentBonus,
        recentRank: rank
      });
    }

    return ranked
      .sort((left, right) =>
        right.score - left.score ||
        left.kind.localeCompare(right.kind) ||
        left.label.localeCompare(right.label)
      )
      .slice(0, 80);
  }

  function targetProjectId(target: AppNavigationTarget): number | null {
    if (target.type === 'item') return target.selection.projectId;
    if ('projectId' in target && typeof target.projectId === 'number') return target.projectId;
    return null;
  }

  function projectLabel(project: Project): string {
    return project.display_name ?? project.name;
  }

  function workingDirLabel(path: string): string {
    const parts = path.split('/').filter(Boolean);
    return parts[0] === 'Users' && parts.length > 2 ? `~/${parts.slice(2).join('/')}` : path;
  }

  function kindLabel(kind: PaletteKind): string {
    switch (kind) {
      case 'action': return 'Action';
      case 'project': return 'Project';
      case 'todo': return 'Todo';
      case 'agent': return 'Agent';
      case 'terminal': return 'Terminal';
      case 'command': return 'Command';
      case 'scratchpad': return 'Scratchpad';
    }
  }

  function kindGlyph(kind: PaletteKind): string {
    switch (kind) {
      case 'action': return '→';
      case 'project': return '◆';
      case 'todo': return '○';
      case 'agent': return '◎';
      case 'terminal': return '>_';
      case 'command': return '▶';
      case 'scratchpad': return '≡';
    }
  }

  function focusSearch(node: HTMLInputElement): void {
    queueMicrotask(() => node.focus());
  }

  function choose(entry: PaletteEntry | null): void {
    if (!entry) return;
    onChoose(entry.target);
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (rankedEntries.length === 0) return;
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      selectedIndex = (selectedIndex + delta + rankedEntries.length) % rankedEntries.length;
      queueMicrotask(() => document.getElementById(`quick-jump-option-${selectedIndex}`)?.scrollIntoView({ block: 'nearest' }));
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      choose(activeEntry);
    }
  }
</script>

<div
  class="palette-backdrop"
  role="presentation"
  onpointerdown={(event) => { if (event.target === event.currentTarget) onClose(); }}
>
  <div class="quick-jump" role="dialog" aria-modal="true" aria-labelledby="quick-jump-title" tabindex="-1">
    <header>
      <div class="palette-title">
        <span class="jump-mark" aria-hidden="true">⌕</span>
        <div><strong id="quick-jump-title">Quick jump</strong><small>Every project, one search</small></div>
      </div>
      <kbd>⌘ K</kbd>
    </header>

    <label class="search-field">
      <span aria-hidden="true">›</span>
      <input
        bind:value={query}
        use:focusSearch
        role="combobox"
        aria-label="Search projects and project items"
        aria-expanded="true"
        aria-controls="quick-jump-results"
        aria-activedescendant={activeEntry ? `quick-jump-option-${selectedIndex}` : undefined}
        autocomplete="off"
        spellcheck="false"
        placeholder="Jump to a project, agent, terminal, command, todo, or scratchpad"
        oninput={() => (selectedIndex = 0)}
        onkeydown={handleKeydown}
      />
      {#if loading}<span class="indexing" aria-label="Refreshing index">indexing</span>{/if}
    </label>

    <div class="result-summary" aria-live="polite">
      <span>{query ? `${rankedEntries.length} matches` : 'Recent and available'}</span>
      <small>{projects.length} project{projects.length === 1 ? '' : 's'}</small>
    </div>

    <div id="quick-jump-results" class="results" role="listbox" aria-label="Quick jump results">
      {#each rankedEntries as entry, index (entry.key)}
        <button
          id={`quick-jump-option-${index}`}
          class:active={index === selectedIndex}
          type="button"
          role="option"
          aria-selected={index === selectedIndex}
          onmouseenter={() => (selectedIndex = index)}
          onclick={() => choose(entry)}
        >
          <span class={`kind-glyph ${entry.kind}`} aria-hidden="true">{kindGlyph(entry.kind)}</span>
          <span class="result-copy"><strong>{entry.label}</strong><small>{entry.detail}</small></span>
          <span class="result-path">
            {#if entry.recentRank !== null}<i>recent</i>{/if}
            {#if entry.projectName}<b>{entry.projectName}</b>{/if}
            <em>{kindLabel(entry.kind)}</em>
          </span>
        </button>
      {:else}
        <div class="no-results"><strong>No jump found</strong><span>Try fewer letters — matching follows characters in order.</span></div>
      {/each}
    </div>

    <footer>
      <span><kbd>↑</kbd><kbd>↓</kbd> move</span>
      <span><kbd>↵</kbd> open</span>
      <span><kbd>esc</kbd> close</span>
      <small>Fuzzy subsequence search</small>
    </footer>
  </div>
</div>

<style>
  .palette-backdrop {
    position: fixed;
    z-index: 1000;
    inset: 0;
    display: grid;
    place-items: start center;
    padding: min(13vh, 92px) 18px 18px;
    background: rgb(5 7 9 / 68%);
  }

  .quick-jump {
    display: grid;
    width: min(680px, calc(100vw - 36px));
    max-height: min(610px, calc(100vh - 110px));
    grid-template-rows: auto auto auto minmax(0, 1fr) auto;
    overflow: hidden;
    border: 1px solid #565c65;
    border-radius: 5px;
    background: #17191c;
    box-shadow: 0 18px 48px rgb(0 0 0 / 44%);
    color: var(--text);
  }

  header { display: flex; min-height: 45px; align-items: center; justify-content: space-between; border-bottom: 1px solid var(--border); padding: 6px 9px; background: #1b1e22; }
  .palette-title { display: flex; min-width: 0; align-items: center; gap: 8px; }
  .palette-title div, .palette-title strong, .palette-title small { display: block; }
  .palette-title strong { color: #eef0f2; font-size: 11px; font-weight: 700; }
  .palette-title small { margin-top: 1px; color: #858c95; font-size: 8px; }
  .jump-mark { display: grid; width: 25px; height: 25px; place-items: center; border: 1px solid #474c54; background: #202328; color: #c6cbd1; font: 13px 'JetBrains Mono Variable', monospace; }
  kbd { display: inline-grid; min-width: 21px; min-height: 19px; place-items: center; border: 1px solid #42474f; border-bottom-color: #5c626b; border-radius: 3px; padding: 1px 5px; background: #23262b; color: #afb5bd; font: 8px 'JetBrains Mono Variable', monospace; }

  .search-field { display: grid; min-height: 47px; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 7px; border-bottom: 1px solid var(--border); padding: 7px 10px; background: #121416; color: #8f969f; }
  .search-field > span:first-child { color: #b7bdc5; font: 15px/1 'JetBrains Mono Variable', monospace; }
  .search-field input { width: 100%; border: 0; outline: 0; padding: 4px 0; background: transparent; color: #f0f1f3; font-size: 12px; }
  .search-field input::placeholder { color: #69717a; }
  .indexing { color: #a1a7ae; font: 7px 'JetBrains Mono Variable', monospace; letter-spacing: 0.04em; text-transform: uppercase; }

  .result-summary { display: flex; min-height: 26px; align-items: center; justify-content: space-between; border-bottom: 1px solid #292d32; padding: 4px 10px; color: #9aa1aa; font-size: 8px; font-weight: 680; text-transform: uppercase; }
  .result-summary small { color: #707780; font: 7px 'JetBrains Mono Variable', monospace; }
  .results { min-height: 90px; overflow-y: auto; padding: 4px; scrollbar-color: #474c54 transparent; scrollbar-width: thin; }
  .results button { display: grid; width: 100%; min-height: 43px; grid-template-columns: 27px minmax(0, 1fr) auto; align-items: center; gap: 7px; border: 1px solid transparent; border-radius: 3px; padding: 4px 6px; background: transparent; color: var(--text-soft); text-align: left; cursor: pointer; }
  .results button:hover, .results button.active { border-color: #484e56; background: #24272c; }
  .results button.active { box-shadow: inset 2px 0 #969da6; }
  .kind-glyph { display: grid; width: 24px; height: 24px; place-items: center; border: 1px solid #3c4148; background: #1c1f23; color: #aab0b8; font: 8px 'JetBrains Mono Variable', monospace; }
  .kind-glyph.project { color: #c1c6cc; }
  .kind-glyph.todo, .kind-glyph.scratchpad { color: #b6aa91; }
  .kind-glyph.agent, .kind-glyph.terminal, .kind-glyph.command { color: #9db5aa; }
  .result-copy { min-width: 0; }
  .result-copy strong, .result-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .result-copy strong { color: #e1e4e7; font-size: 10px; font-weight: 650; }
  .result-copy small { margin-top: 2px; color: #7f8790; font: 7px 'JetBrains Mono Variable', monospace; }
  .result-path { display: flex; max-width: 235px; align-items: center; justify-content: flex-end; gap: 5px; overflow: hidden; }
  .result-path b { overflow: hidden; color: #aeb4bc; font: 8px 'JetBrains Mono Variable', monospace; font-weight: 500; text-overflow: ellipsis; white-space: nowrap; }
  .result-path em, .result-path i { flex: none; border: 1px solid #393e45; border-radius: 3px; padding: 1px 4px; color: #858d96; background: #1c1f23; font: normal 7px 'JetBrains Mono Variable', monospace; }
  .result-path i { border-color: #55504a; color: #b2a890; }
  .no-results { display: grid; min-height: 112px; place-content: center; gap: 4px; color: #858c95; text-align: center; }
  .no-results strong { color: #c6cbd1; font-size: 10px; }
  .no-results span { font-size: 8px; }

  footer { display: flex; min-height: 34px; align-items: center; gap: 13px; border-top: 1px solid var(--border); padding: 5px 9px; color: #858c95; font-size: 8px; }
  footer span { display: flex; align-items: center; gap: 3px; }
  footer kbd { min-width: 18px; min-height: 17px; padding: 0 4px; }
  footer small { margin-left: auto; color: #666e78; font: 7px 'JetBrains Mono Variable', monospace; }

  @media (max-width: 620px) {
    .palette-backdrop { padding: 48px 8px 8px; }
    .quick-jump { width: calc(100vw - 16px); max-height: calc(100vh - 56px); }
    .result-path { max-width: 115px; }
    .result-path em, footer small { display: none; }
  }

  @media (prefers-reduced-motion: no-preference) {
    .quick-jump { animation: palette-enter 100ms ease-out; }
    @keyframes palette-enter { from { opacity: 0; transform: translateY(-4px); } }
  }
</style>
