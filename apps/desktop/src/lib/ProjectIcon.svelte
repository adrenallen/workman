<script lang="ts">
  import BotIcon from '@lucide/svelte/icons/bot';
  import BoxesIcon from '@lucide/svelte/icons/boxes';
  import Code2Icon from '@lucide/svelte/icons/code-2';
  import DatabaseIcon from '@lucide/svelte/icons/database';
  import FolderGit2Icon from '@lucide/svelte/icons/folder-git-2';
  import FolderIcon from '@lucide/svelte/icons/folder';
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';
  import Globe2Icon from '@lucide/svelte/icons/globe-2';
  import RocketIcon from '@lucide/svelte/icons/rocket';
  import TerminalIcon from '@lucide/svelte/icons/terminal';
  import WorkflowIcon from '@lucide/svelte/icons/workflow';

  import {
    normalizeProjectIcon,
    projectIconColorValue,
    type ProjectIconName
  } from './projectAppearance';

  interface Props {
    icon?: string | null;
    color?: string | null;
    fallback?: 'project' | 'repository' | 'worktree';
    size?: number;
    label?: string | null;
    class?: string;
  }

  let {
    icon = null,
    color = null,
    fallback = 'project',
    size = 15,
    label = null,
    class: className = ''
  }: Props = $props();

  const customIcons = {
    bot: BotIcon,
    boxes: BoxesIcon,
    'code-2': Code2Icon,
    database: DatabaseIcon,
    'globe-2': Globe2Icon,
    rocket: RocketIcon,
    terminal: TerminalIcon,
    workflow: WorkflowIcon
  } satisfies Record<ProjectIconName, typeof FolderIcon>;
  const fallbackIcons = {
    project: FolderIcon,
    repository: FolderGit2Icon,
    worktree: GitBranchIcon
  };

  let normalizedIcon = $derived(normalizeProjectIcon(icon));
  let Icon = $derived(normalizedIcon ? customIcons[normalizedIcon] : fallbackIcons[fallback]);
  let iconColor = $derived(normalizedIcon ? projectIconColorValue(color) : undefined);
</script>

<span
  class={`project-icon ${normalizedIcon ? 'custom' : 'automatic'} ${className}`}
  style:color={iconColor}
  aria-hidden={label ? undefined : 'true'}
  aria-label={label ?? undefined}
>
  <Icon {size} strokeWidth={1.8} />
</span>

<style>
  .project-icon { display: inline-grid; width: 1em; height: 1em; flex: none; place-items: center; }
</style>
