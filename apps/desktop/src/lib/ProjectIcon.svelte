<script lang="ts">
  import FolderGit2Icon from '@lucide/svelte/icons/folder-git-2';
  import FolderIcon from '@lucide/svelte/icons/folder';
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';

  import {
    normalizeProjectIcon,
    projectIconComponent,
    projectIconColorValue,
  } from './projectAppearance';

  interface Props {
    icon?: string | null;
    image?: string | null;
    color?: string | null;
    fallback?: 'project' | 'repository' | 'worktree';
    size?: number;
    label?: string | null;
    class?: string;
  }

  let {
    icon = null,
    image = null,
    color = null,
    fallback = 'project',
    size = 15,
    label = null,
    class: className = ''
  }: Props = $props();

  const fallbackIcons = {
    project: FolderIcon,
    repository: FolderGit2Icon,
    worktree: GitBranchIcon
  };

  let normalizedIcon = $derived(normalizeProjectIcon(icon));
  let Icon = $derived(projectIconComponent(normalizedIcon) ?? fallbackIcons[fallback]);
  let iconColor = $derived(normalizedIcon ? projectIconColorValue(color) : undefined);
</script>

<span
  class={`project-icon ${image ? 'image' : normalizedIcon ? 'custom' : 'automatic'} ${className}`}
  style:color={iconColor}
  aria-hidden={label ? undefined : 'true'}
  aria-label={label ?? undefined}
>
  {#if image}
    <img src={image} alt="" width={size} height={size} />
  {:else}
    <Icon {size} strokeWidth={1.8} />
  {/if}
</span>

<style>
  .project-icon { display: inline-grid; width: 1em; height: 1em; flex: none; place-items: center; }
  .project-icon img { display: block; max-width: none; border-radius: 3px; object-fit: contain; image-rendering: auto; }
</style>
