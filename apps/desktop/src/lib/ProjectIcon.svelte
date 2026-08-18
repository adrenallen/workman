<script lang="ts">
  import FolderGit2Icon from '@lucide/svelte/icons/folder-git-2';
  import FolderIcon from '@lucide/svelte/icons/folder';
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';

  import TooltipLabel from '$lib/components/ds/TooltipLabel.svelte';
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
    worktree?: boolean;
    worktreeTooltip?: boolean;
    class?: string;
  }

  let {
    icon = null,
    image = null,
    color = null,
    fallback = 'project',
    size = 15,
    label = null,
    worktree = false,
    worktreeTooltip = true,
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
  aria-hidden={label || worktree ? undefined : 'true'}
  aria-label={label ?? (worktree ? 'Worktree' : undefined)}
>
  {#if image}
    <img src={image} alt="" width={size} height={size} />
  {:else}
    <Icon {size} strokeWidth={1.8} />
  {/if}
  {#if worktree}
    {#if worktreeTooltip}
      <TooltipLabel label="Worktree">
        <span class="worktree-badge" data-project-worktree-badge aria-hidden="true">
          <GitBranchIcon size={7} strokeWidth={2.1} />
        </span>
      </TooltipLabel>
    {:else}
      <span class="worktree-badge" data-project-worktree-badge aria-hidden="true">
        <GitBranchIcon size={7} strokeWidth={2.1} />
      </span>
    {/if}
  {/if}
</span>

<style>
  .project-icon { position: relative; display: inline-grid; width: 1em; height: 1em; flex: none; overflow: visible; place-items: center; }
  .project-icon img { display: block; max-width: none; border-radius: 3px; object-fit: contain; image-rendering: auto; }
  .project-icon > :global(.tooltip-anchor) { position: absolute; z-index: 2; top: -4px; left: -4px; overflow: visible; }
  .project-icon > .worktree-badge { position: absolute; z-index: 2; top: -4px; left: -4px; overflow: visible; }
  .worktree-badge { display: grid; width: 11px; height: 11px; place-items: center; border: 1px solid var(--card); border-radius: 3px; color: var(--muted-foreground); background: var(--card); }
</style>
