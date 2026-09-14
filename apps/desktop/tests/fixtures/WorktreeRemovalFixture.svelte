<script lang="ts">
  import WorktreeRemoveDialog from '../../src/lib/WorktreeRemoveDialog.svelte';
  import type { Project } from '../../src/lib/daemon';
  import type { WorktreeEntry, WorktreeDeleteSafety } from '../../src/lib/worktrees';

  const scenario = new URLSearchParams(location.search).get('scenario') ?? 'clean';
  let open = $state(true);
  let result = $state('');
  const project = { id: 1, name: 'feature/local-work', path: '/tmp/removal-fixture' } as Project;
  const safety: WorktreeDeleteSafety = {
    dirty_files: scenario === 'dirty' ? 2 : 0,
    untracked_files: scenario === 'dirty' ? 1 : 0,
    dirty_paths: scenario === 'dirty' ? ['src/app.ts', 'notes.txt'] : [],
    at_risk_commits: scenario === 'commits' ? 1 : 0,
    at_risk_subjects: scenario === 'commits' ? ['Save local implementation'] : [],
    dependent_worktrees: scenario === 'dependent' ? ['/tmp/dependent-worktree'] : [],
    requires_force: scenario !== 'clean'
  };
  const entry = {
    project_id: 1, path: project.path, branch: scenario === 'commits' ? 'detached@12345678' : 'feature/local-work',
    kind: scenario === 'dependent' ? 'main' : 'adopted', status: scenario === 'dirty' ? 'dirty' : 'clean',
    delete_safety: safety
  } as WorktreeEntry;
</script>
<output>{result}</output>
{#if open}
  <WorktreeRemoveDialog {project} {entry}
    onClose={() => { open = false; result = 'cancelled'; }}
    onConfirm={(deleteFromDisk, forceDirty) => {
      result = JSON.stringify({ deleteFromDisk, forceDirty }); open = false;
    }} />
{/if}
