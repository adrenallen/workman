<script lang="ts">
  import { tick } from 'svelte';
  import { focusPanel } from '../../src/lib/keyboardNavigation';
  import RecentWorkspaceSwitcher from '../../src/lib/RecentWorkspaceSwitcher.svelte';
  import { emptyWorkspaceViewHistory, recordWorkspaceView, recentWorkspaceViews, sameWorkspaceView } from '../../src/lib/workspaceViewHistory';
  import ScratchpadDetailView from '../../src/lib/ScratchpadDetailView.svelte';
  import type { ScratchpadRead } from '../../src/lib/coordination';

  let switcher: RecentWorkspaceSwitcher | undefined;
  let failSaves = $state(false);
  let shown = $state(true);
  const scratchpad = { projectId: 1, pane: { type: 'selection' as const, selection: { key: 'scratchpad:1001', kind: 'scratchpad' as const, id: 1001, projectId: 1, label: 'Editor regression fixture' } } };
  const other = { projectId: 1, pane: { type: 'overview' as const } };
  let history = $state(recordWorkspaceView(recordWorkspaceView(emptyWorkspaceViewHistory, other), scratchpad));
  async function switchPane(next = !shown) {
    shown = next;
    history = recordWorkspaceView(history, shown ? scratchpad : other);
    await tick();
    if (shown) focusPanel('main');
  }
  let read = $state<ScratchpadRead>({
    scratchpad: {
      id: 1001, project_id: 1, name: 'Editor regression fixture', revision: 1,
      tags: [], archived: false, created_by: 'Fixture', updated_by: 'Fixture',
      content: Array.from({ length: 50 }, (_, i) => [
        `## Section ${i + 1}`,
        ...Array.from({ length: 5 }, (_, j) => `Paragraph ${i + 1}.${j + 1}: Click here to edit **bold text** and select ordinary text without losing your place.`),
        '```javascript',
        `const section = ${i + 1};`,
        '// **literal** [text](https://example.com) inside code',
        'console.log(section);',
        '```',
        'Text after the code block with `inline code`.'
      ].join('\n\n')).join('\n\n')
    },
    total_lines: 1200, comments: [], comment_total_count: 0,
    unresolved_comment_count: 0, comments_revision: 0
  });
  const quote = 'Paragraph 28.1: Click here to edit **bold text** and select ordinary text without losing your place.';
  const start = read.scratchpad.content.indexOf(quote);
  read.comments = [{
    id: 1, scratchpad_id: 1001, actor: 'Reviewer', actor_kind: 'user', body: 'A comment on this paragraph.',
    quote, anchor_start: start, anchor_end: start + quote.length,
    anchor_prefix: null, anchor_suffix: null, anchor_revision: 1, resolved: false,
    created_at: Date.now(), updated_at: Date.now(), anchor_state: 'anchored',
    current_start: start, current_end: start + quote.length,
    current_start_line: null, current_end_line: null, can_edit: true, can_resolve: true, can_delete: true
  }];
  read.comment_total_count = 1;
  read.unresolved_comment_count = 1;
  async function save(content: string, expectedRevision: number): Promise<ScratchpadRead> {
    if (failSaves) throw new Error('Fixture save unavailable');
    if (expectedRevision !== read.scratchpad.revision) throw new Error('Revision conflict');
    const title = /^# ([^\n]+)\n*/.exec(content);
    read = { ...read, scratchpad: { ...read.scratchpad,
      name: title?.[1] ?? read.scratchpad.name,
      content: title ? content.slice(title[0].length) : content,
      revision: expectedRevision + 1
    }};
    return read;
  }
</script>

<div class="fixture">
  <div class="fixture-controls">
    <button type="button" onclick={() => void switcher?.triggerFromNativeMenu()}>Native shortcut</button>
    <button type="button" onclick={() => void switchPane()}>Switch pane</button>
    <button type="button" onclick={() => { read = { ...read, scratchpad: { ...read.scratchpad, content: read.scratchpad.content + '\n\nAgent addition.', revision: read.scratchpad.revision + 1 } }; }}>Agent edit</button>
    <button type="button" onclick={() => (failSaves = !failSaves)}>{failSaves ? 'Resume saves' : 'Pause saves'}</button>
  </div>
  <div class="pane" data-app-panel="main">
  {#if shown}
    <ScratchpadDetailView {read} loading={false} onRefresh={() => {}} onSave={save} />
  {:else}<p>Another pane</p>{/if}
  </div>
  <RecentWorkspaceSwitcher bind:this={switcher} getItems={() => recentWorkspaceViews(history, () => true).map(view => ({ view, current: sameWorkspaceView(view, history.current), label: view.pane.type === 'selection' ? 'Editor regression fixture' : 'Overview', project: 'Fixture', kind: view.pane.type === 'selection' ? 'scratchpad' : 'overview' }))} onChoose={view => void switchPane(view.pane.type === 'selection')} blocked={() => false} />
</div>

<style>
  :global(html), :global(body), :global(#app) { height: 100%; margin: 0; }
  .fixture-controls { display: flex; justify-content: center; gap: 24px; }
  .pane { min-height: 0; display: flex; flex-direction: column; }
  .fixture { display: grid; height: 100%; grid-template-rows: 32px minmax(0, 1fr); }
</style>
