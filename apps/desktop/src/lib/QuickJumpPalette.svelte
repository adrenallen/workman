<script lang="ts">
  import BotIcon from '@lucide/svelte/icons/bot';
  import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
  import FolderIcon from '@lucide/svelte/icons/folder';
  import GitBranchPlusIcon from '@lucide/svelte/icons/git-branch-plus';
  import NotebookTextIcon from '@lucide/svelte/icons/notebook-text';
  import PlayIcon from '@lucide/svelte/icons/play';
  import SearchIcon from '@lucide/svelte/icons/search';
  import SettingsIcon from '@lucide/svelte/icons/settings';
  import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
  import type { AgentTool } from './agentTools';
  import type { Project } from './daemon';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Input } from '$lib/components/ui/input';
  import { ScrollArea } from '$lib/components/ui/scroll-area';
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
  let searchInput = $state<HTMLInputElement | null>(null);
  let entries = $derived(buildEntries());
  let rankedEntries = $derived(rankEntries(entries, query, recentKeys));
  let activeEntry = $derived(rankedEntries[selectedIndex] ?? null);

  $effect(() => {
    if (selectedIndex >= rankedEntries.length) selectedIndex = Math.max(0, rankedEntries.length - 1);
  });

  $effect(() => {
    if (searchInput) queueMicrotask(() => searchInput?.focus());
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
      if (project.repository_id !== null && project.parent_project_id === null) {
        next.push({
          key: `action:new-worktree:${project.id}`,
          kind: 'action',
          label: `New worktree in ${name}`,
          detail: 'Create a branch workspace and jump to it',
          projectName: name,
          searchText: `new create worktree branch fork ${name} ${project.name} ${project.path}`,
          target: { type: 'new-worktree', projectId: project.id },
          creation: true
        });
      }
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
          detail: 'Add a command to awm.yml',
          projectName: name,
          searchText: `add new command awm yml ${name} ${project.name} ${project.path}`,
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
          detail: 'Create an Unnamed scratchpad and start writing',
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

  function entryIcon(entry: PaletteEntry) {
    if (entry.kind === 'action') {
      if (entry.target.type === 'new-terminal') return SquareTerminalIcon;
      if (entry.target.type === 'new-worktree') return GitBranchPlusIcon;
      if (entry.target.type === 'add-command') return PlayIcon;
      if (entry.target.type === 'new-todo') return CircleCheckIcon;
      if (entry.target.type === 'new-scratchpad') return NotebookTextIcon;
      if (entry.target.type === 'spawn-agent') return BotIcon;
      return SettingsIcon;
    }
    switch (entry.kind) {
      case 'project': return FolderIcon;
      case 'todo': return CircleCheckIcon;
      case 'agent': return BotIcon;
      case 'terminal': return SquareTerminalIcon;
      case 'command': return PlayIcon;
      case 'scratchpad': return NotebookTextIcon;
    }
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

<Dialog.Root open onOpenChange={(open) => { if (!open) onClose(); }}>
  <Dialog.Content
    class="quick-jump w-[min(680px,calc(100vw-36px))] max-w-none gap-0 overflow-hidden rounded-lg border border-border bg-popover p-0 shadow-2xl"
    showCloseButton={false}
    aria-labelledby="quick-jump-title"
  >
    <header>
      <div class="palette-title">
        <span class="jump-mark" aria-hidden="true"><SearchIcon size={14} strokeWidth={1.8} /></span>
        <div><strong id="quick-jump-title">Quick jump</strong><small>Every project, one search</small></div>
      </div>
      <kbd>⌘ K</kbd>
    </header>

    <label class="search-field">
      <SearchIcon size={15} strokeWidth={1.8} aria-hidden="true" />
      <Input
        bind:ref={searchInput}
        bind:value={query}
        class="h-9 border-0 bg-transparent px-0 text-sm shadow-none focus-visible:ring-0"
        role="combobox"
        aria-label="Search projects and project items"
        aria-expanded="true"
        aria-controls="quick-jump-results"
        aria-activedescendant={activeEntry ? `quick-jump-option-${selectedIndex}` : undefined}
        autocomplete="off"
        spellcheck="false"
        placeholder="Jump or create a project, worktree, agent, terminal, command, todo, or scratchpad"
        oninput={() => (selectedIndex = 0)}
        onkeydown={handleKeydown}
      />
      {#if loading}<span class="indexing" aria-label="Refreshing index">indexing</span>{/if}
    </label>

    <div class="result-summary" aria-live="polite">
      <span>{query ? `${rankedEntries.length} matches` : 'Recent and available'}</span>
      <small>{projects.length} project{projects.length === 1 ? '' : 's'}</small>
    </div>

    <ScrollArea id="quick-jump-results" class="results" role="listbox" aria-label="Quick jump results">
      {#each rankedEntries as entry, index (entry.key)}
        {@const Icon = entryIcon(entry)}
        <button
          id={`quick-jump-option-${index}`}
          class="result-row"
          class:active={index === selectedIndex}
          type="button"
          role="option"
          aria-selected={index === selectedIndex}
          onmouseenter={() => (selectedIndex = index)}
          onclick={() => choose(entry)}
        >
          <span class={`kind-glyph ${entry.kind}`} aria-hidden="true"><Icon size={14} strokeWidth={1.8} /></span>
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
    </ScrollArea>

    <footer>
      <span><kbd>↑</kbd><kbd>↓</kbd> move</span>
      <span><kbd>↵</kbd> open</span>
      <span><kbd>esc</kbd> close</span>
      <small>Fuzzy subsequence search</small>
    </footer>
  </Dialog.Content>
</Dialog.Root>

<style>
  .quick-jump {
    display: grid;
    max-height: min(610px, calc(100vh - 110px));
    grid-template-rows: auto auto auto minmax(0, 1fr) auto;
    overflow: hidden;
    border: 1px solid var(--border);
    background: var(--popover);
    color: var(--text);
  }

  header { display: flex; min-height: 45px; align-items: center; justify-content: space-between; border-bottom: 1px solid var(--border); padding: 6px 9px; background: var(--popover); }
  .palette-title { display: flex; min-width: 0; align-items: center; gap: 8px; }
  .palette-title div, .palette-title strong, .palette-title small { display: block; }
  .palette-title strong { color: var(--foreground); font-size: var(--font-size-sm); font-weight: 700; }
  .palette-title small { margin-top: 1px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .jump-mark { display: grid; width: 25px; height: 25px; place-items: center; border: 1px solid var(--border-strong); background: var(--popover); color: var(--foreground); font: 13px 'JetBrains Mono Variable', monospace; }
  kbd { display: inline-grid; min-width: 21px; min-height: 19px; place-items: center; border: 1px solid var(--border-strong); border-bottom-color: #5c626b; border-radius: 3px; padding: 1px 5px; background: var(--accent); color: #afb5bd; font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }

  .search-field { display: grid; min-height: 47px; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 7px; border-bottom: 1px solid var(--border); padding: 7px 10px; background: #121416; color: var(--muted-foreground); }
  .search-field > :global(svg) { color: var(--muted-foreground); }
  .search-field :global(input) { width: 100%; border: 0; outline: 0; padding: 4px 0; background: transparent; color: var(--foreground); font-size: 12px; }
  .search-field :global(input::placeholder) { color: var(--muted-foreground); }
  .indexing { color: var(--text-soft); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; letter-spacing: 0.04em; text-transform: uppercase; }

  .result-summary { display: flex; min-height: 26px; align-items: center; justify-content: space-between; border-bottom: 1px solid var(--accent); padding: 4px 10px; color: var(--text-soft); font-size: var(--font-size-xs); font-weight: 680; text-transform: uppercase; }
  .result-summary small { color: var(--muted-foreground); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .results { min-height: 90px; padding: 4px; }
  .result-row { display: grid; width: 100%; min-height: 43px; grid-template-columns: 27px minmax(0, 1fr) auto; align-items: center; gap: 7px; border: 1px solid transparent; border-radius: 3px; padding: 4px 6px; background: transparent; color: var(--text-soft); text-align: left; cursor: pointer; }
  .result-row:hover, .result-row.active { border-color: #484e56; background: #24272c; }
  .result-row.active { box-shadow: inset 2px 0 var(--text-soft); }
  .kind-glyph { display: grid; width: 24px; height: 24px; place-items: center; border: 1px solid var(--border-strong); background: var(--popover); color: var(--text-soft); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .kind-glyph.project { color: var(--text-soft); }
  .kind-glyph.todo, .kind-glyph.scratchpad { color: #b6aa91; }
  .kind-glyph.agent, .kind-glyph.terminal, .kind-glyph.command { color: #9db5aa; }
  .result-copy { min-width: 0; }
  .result-copy strong, .result-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .result-copy strong { color: var(--foreground); font-size: var(--font-size-sm); font-weight: 650; }
  .result-copy small { margin-top: 2px; color: var(--muted-foreground); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .result-path { display: flex; max-width: 235px; align-items: center; justify-content: flex-end; gap: 5px; overflow: hidden; }
  .result-path b { overflow: hidden; color: var(--text-soft); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; font-weight: 500; text-overflow: ellipsis; white-space: nowrap; }
  .result-path em, .result-path i { flex: none; border: 1px solid #393e45; border-radius: 3px; padding: 1px 4px; color: var(--muted-foreground); background: var(--popover); font: normal var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .result-path i { border-color: #55504a; color: #b2a890; }
  .no-results { display: grid; min-height: 112px; place-content: center; gap: 4px; color: var(--muted-foreground); text-align: center; }
  .no-results strong { color: var(--foreground); font-size: var(--font-size-sm); }
  .no-results span { font-size: var(--font-size-xs); }

  footer { display: flex; min-height: 34px; align-items: center; gap: 13px; border-top: 1px solid var(--border); padding: 5px 9px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  footer span { display: flex; align-items: center; gap: 3px; }
  footer kbd { min-width: 18px; min-height: 17px; padding: 0 4px; }
  footer small { margin-left: auto; color: var(--muted-foreground); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }

  @media (max-width: 620px) {
    .quick-jump { width: calc(100vw - 16px); max-height: calc(100vh - 56px); }
    .result-path { max-width: 115px; }
    .result-path em, footer small { display: none; }
  }

  @media (prefers-reduced-motion: no-preference) {
    .quick-jump { animation: palette-enter 100ms ease-out; }
    @keyframes palette-enter { from { opacity: 0; transform: translateY(-4px); } }
  }
</style>
