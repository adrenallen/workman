<script lang="ts">
  import TrustReview from '../../src/lib/TrustReview.svelte';
  import WorktreeDialog from '../../src/lib/WorktreeDialog.svelte';
  import type { Project } from '../../src/lib/daemon';
  import type { WorktreeRepository } from '../../src/lib/worktrees';
  let dialog = $state<'trust' | 'branch' | null>('trust');
  let result = $state('');
  const project = { id: 1, name: 'Fixture', path: '/tmp/fixture', branch: 'main' } as Project;
  const repository = { id: 1, name: 'Fixture', root_path: '/tmp/fixture', managed_root: '/tmp/worktrees', preferences: {}, herd: { parked: false, available: false, tld: null, error: null } } as WorktreeRepository;
</script>
<button onclick={() => { dialog = 'trust'; }}>Review command</button>
<button onclick={() => { dialog = 'branch'; }}>Choose branch</button>
<output>{result}</output>
{#if dialog === 'trust'}
<TrustReview busy={false} review={{
  process_id: 1, process_name: 'Build and test', trusted: false, expected_hash: 'fixture',
  fields: { command: 'npm run build && npm test', working_dir: '/Users/demo/projects/workman/apps/desktop', env: { NODE_ENV: 'development' }, auto_start: false, auto_restart: false, restart_when_changed: [] },
  changes: [{ field: 'command', previous: 'npm run build', current: 'npm run build && npm test' }]
}} onClose={() => { dialog = null; }} onApprove={() => { result = 'Approved'; dialog = null; }} />
{:else if dialog === 'branch'}
<WorktreeDialog mode="create" sourceProject={project} {repository}
  refOptions={[{ name: 'main', source: 'current' }, { name: 'origin/main', source: 'default' }, { name: 'origin/dev', source: 'remote' }]}
  defaultRef="origin/main" onLoadBranches={() => {}}
  onValidateRef={async ref => ({ repository_id: 1, requested_ref: ref, resolved_ref: ref, commit: '0123456789abcdef' })}
  onSubmit={submission => { result = JSON.stringify(submission); dialog = null; }}
  onOpenProject={() => {}} onClearConflict={() => {}} onClose={() => { dialog = null; }} />
{/if}
