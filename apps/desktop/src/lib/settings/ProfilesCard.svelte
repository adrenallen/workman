<script lang="ts">
  import ArrowRightLeftIcon from '@lucide/svelte/icons/arrow-right-left';
  import CheckIcon from '@lucide/svelte/icons/check';
  import ImportIcon from '@lucide/svelte/icons/import';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import TrashIcon from '@lucide/svelte/icons/trash-2';
  import { open, save } from '@tauri-apps/plugin-dialog';

  import { Button } from '$lib/components/ui/button';
  import { Separator } from '$lib/components/ui/separator';
  import ProfileSwitchDialog from '../ProfileSwitchDialog.svelte';
  import type {
    DaemonClient,
    Profile,
    ProfileSwitchImpact
  } from '../daemon';

  interface Props {
    client: DaemonClient;
    connected: boolean;
    onError: (message: string) => void;
    onSwitched: () => void;
  }

  let { client, connected, onError, onSwitched }: Props = $props();
  let profiles = $state<Profile[]>([]);
  let loading = $state(false);
  let busy = $state(false);
  let newName = $state('');
  let copyCurrent = $state(true);
  let renameId = $state<number | null>(null);
  let renameName = $state('');
  let pendingSwitch = $state<ProfileSwitchImpact | null>(null);
  let switchError = $state<string | null>(null);

  $effect(() => {
    if (connected) void refresh();
  });

  async function refresh(): Promise<void> {
    loading = true;
    try {
      profiles = await client.profiles();
    } catch (cause) {
      onError(message(cause));
    } finally {
      loading = false;
    }
  }

  async function create(): Promise<void> {
    const name = newName.trim();
    if (!name || busy) return;
    busy = true;
    try {
      await client.createProfile(name, copyCurrent);
      newName = '';
      copyCurrent = true;
      await refresh();
    } catch (cause) {
      onError(message(cause));
    } finally {
      busy = false;
    }
  }

  function beginRename(profile: Profile): void {
    renameId = profile.id;
    renameName = profile.name;
  }

  async function rename(): Promise<void> {
    if (renameId === null || !renameName.trim() || busy) return;
    busy = true;
    try {
      await client.renameProfile(renameId, renameName.trim());
      renameId = null;
      await refresh();
    } catch (cause) {
      onError(message(cause));
    } finally {
      busy = false;
    }
  }

  async function requestSwitch(profile: Profile): Promise<void> {
    if (profile.active || busy) return;
    busy = true;
    try {
      const impact = await client.profileSwitchImpact(profile.id);
      if (impact.impact.running_processes.length === 0) {
        await client.switchProfile(profile.id, false);
        onSwitched();
      } else {
        pendingSwitch = impact;
        switchError = null;
      }
    } catch (cause) {
      onError(message(cause));
    } finally {
      busy = false;
    }
  }

  async function confirmSwitch(): Promise<void> {
    if (!pendingSwitch || busy) return;
    busy = true;
    switchError = null;
    try {
      await client.switchProfile(pendingSwitch.profile.id, true);
      pendingSwitch = null;
      onSwitched();
    } catch (cause) {
      switchError = message(cause);
    } finally {
      busy = false;
    }
  }

  async function remove(profile: Profile): Promise<void> {
    if (profile.active || busy) return;
    if (!window.confirm(`Delete the ${profile.name} profile? Projects and their coordination data stay on disk.`)) return;
    busy = true;
    try {
      await client.deleteProfile(profile.id);
      await refresh();
    } catch (cause) {
      onError(message(cause));
    } finally {
      busy = false;
    }
  }

  async function exportProfile(profile: Profile): Promise<void> {
    const destination = await save({
      title: `Export ${profile.name}`,
      defaultPath: `${safeFileName(profile.name)}.workman-profile.json`,
      filters: [{ name: 'Workman profile', extensions: ['json'] }]
    });
    if (typeof destination !== 'string') return;
    busy = true;
    try {
      await client.exportProfile(profile.id, destination);
    } catch (cause) {
      onError(message(cause));
    } finally {
      busy = false;
    }
  }

  async function importProfile(): Promise<void> {
    const source = await open({
      directory: false,
      multiple: false,
      title: 'Import a Workman profile',
      filters: [{ name: 'Workman profile', extensions: ['json'] }]
    });
    if (typeof source !== 'string') return;
    busy = true;
    try {
      await client.importProfile(source);
      await refresh();
    } catch (cause) {
      onError(message(cause));
    } finally {
      busy = false;
    }
  }

  function safeFileName(name: string): string {
    return name.trim().replace(/[^a-z0-9._-]+/gi, '-').replace(/^-|-$/g, '') || 'profile';
  }

  function message(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }
</script>

<section class="overflow-hidden rounded-md border bg-card text-card-foreground" aria-labelledby="profiles-card-title">
  <header class="flex flex-wrap items-start justify-between gap-4 px-4 py-3">
    <div class="flex min-w-0 gap-3">
      <span class="profile-glyph" aria-hidden="true"><i></i><i></i><i></i></span>
      <div>
        <p class="font-mono text-xs font-semibold tracking-[0.08em] text-muted-foreground uppercase">Workspace sets</p>
        <h2 id="profiles-card-title" class="mt-1 text-lg font-semibold tracking-tight">Profiles</h2>
        <p class="mt-1 max-w-2xl text-sm leading-5 text-muted-foreground">
          Switch project sets, shell settings, agent presets, and custom agent marks without removing your main workspace.
        </p>
      </div>
    </div>
    <Button variant="outline" size="sm" disabled={!connected || busy} onclick={() => void importProfile()}>
      <ImportIcon size={14} />Import
    </Button>
  </header>

  <Separator />

  <form class="grid gap-3 bg-muted/25 px-4 py-3 sm:grid-cols-[minmax(0,1fr)_auto_auto] sm:items-end" onsubmit={(event) => { event.preventDefault(); void create(); }}>
    <label class="grid gap-1.5 text-xs font-medium text-muted-foreground">
      New profile
      <input class="h-9 rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none focus:border-ring" bind:value={newName} maxlength="80" placeholder="Demo recording" />
    </label>
    <label class="flex h-9 items-center gap-2 text-sm text-muted-foreground">
      <input type="checkbox" bind:checked={copyCurrent} />Copy current state
    </label>
    <Button size="sm" type="submit" disabled={!connected || busy || !newName.trim()}>
      <PlusIcon size={14} />Create
    </Button>
  </form>

  <Separator />

  {#if loading && profiles.length === 0}
    <div class="flex items-center gap-2 px-4 py-6 text-sm text-muted-foreground"><LoaderCircleIcon class="animate-spin" size={15} />Reading profiles…</div>
  {:else}
    <div class="profile-list" aria-live="polite">
      {#each profiles as profile (profile.id)}
        <article class:active={profile.active}>
          <span class="slot" aria-hidden="true"></span>
          <div class="min-w-0 flex-1">
            {#if renameId === profile.id}
              <form class="flex gap-2" onsubmit={(event) => { event.preventDefault(); void rename(); }}>
                <input class="h-8 min-w-0 flex-1 rounded border border-input bg-background px-2 text-sm" bind:value={renameName} maxlength="80" aria-label="Profile name" />
                <Button size="sm" type="submit" disabled={busy || !renameName.trim()}><CheckIcon size={13} />Save</Button>
                <Button size="sm" variant="ghost" type="button" disabled={busy} onclick={() => (renameId = null)}>Cancel</Button>
              </form>
            {:else}
              <div class="flex min-w-0 items-center gap-2">
                <strong class="truncate text-sm">{profile.name}</strong>
                {#if profile.active}<span class="active-label">Loaded</span>{/if}
              </div>
              <p class="mt-1 font-mono text-xs text-muted-foreground">{profile.project_count} project{profile.project_count === 1 ? '' : 's'} · {profile.agent_tool_count} agent preset{profile.agent_tool_count === 1 ? '' : 's'}</p>
            {/if}
          </div>
          {#if renameId !== profile.id}
            <div class="row-actions">
              {#if !profile.active}
                <Button size="sm" variant="outline" disabled={busy} onclick={() => void requestSwitch(profile)}><ArrowRightLeftIcon size={13} />Switch</Button>
              {/if}
              <Button size="sm" variant="ghost" disabled={busy} onclick={() => void exportProfile(profile)}>Export</Button>
              <Button size="icon-sm" variant="ghost" aria-label={`Rename ${profile.name}`} disabled={busy} onclick={() => beginRename(profile)}><PencilIcon size={13} /></Button>
              <Button size="icon-sm" variant="ghost" aria-label={`Delete ${profile.name}`} disabled={busy || profile.active} onclick={() => void remove(profile)}><TrashIcon size={13} /></Button>
            </div>
          {/if}
        </article>
      {/each}
    </div>
  {/if}

  <Separator />
  <footer class="px-4 py-3 text-xs leading-5 text-muted-foreground">
    Todos, scratchpads, process history, and worktree metadata follow their project. Profile archives omit daemon credentials, update keys, tokens, and process environments.
  </footer>
</section>

{#if pendingSwitch}
  <ProfileSwitchDialog
    profile={pendingSwitch.profile}
    processes={pendingSwitch.impact.running_processes}
    {busy}
    error={switchError}
    onConfirm={() => void confirmSwitch()}
    onClose={() => { if (!busy) pendingSwitch = null; }}
  />
{/if}

<style>
  .profile-glyph { position: relative; display: grid; width: 36px; height: 36px; flex: 0 0 auto; place-items: center; border: 1px solid var(--border); border-radius: 6px; background: var(--muted); }
  .profile-glyph i { position: absolute; width: 17px; height: 10px; border: 1px solid var(--muted-foreground); border-radius: 2px; background: var(--card); }
  .profile-glyph i:nth-child(1) { transform: translate(-4px, -5px); opacity: .45; }
  .profile-glyph i:nth-child(2) { transform: translate(0, 0); opacity: .7; }
  .profile-glyph i:nth-child(3) { transform: translate(4px, 5px); border-color: var(--signal); }
  .profile-list { display: grid; }
  .profile-list article { position: relative; display: flex; min-width: 0; align-items: center; gap: 12px; padding: 11px 14px 11px 18px; border-bottom: 1px solid var(--border); }
  .profile-list article:last-child { border-bottom: 0; }
  .profile-list article.active { background: color-mix(in srgb, var(--signal) 5%, transparent); }
  .slot { position: absolute; inset: 8px auto 8px 0; width: 3px; border-radius: 0 2px 2px 0; background: transparent; }
  article.active .slot { background: var(--signal); box-shadow: 0 0 12px color-mix(in srgb, var(--signal) 45%, transparent); }
  .active-label { border: 1px solid color-mix(in srgb, var(--signal) 45%, var(--border)); border-radius: 999px; padding: 1px 6px; color: var(--signal); font: 700 10px/1.4 'JetBrains Mono Variable', monospace; letter-spacing: .06em; text-transform: uppercase; }
  .row-actions { display: flex; flex: 0 0 auto; align-items: center; gap: 2px; }
  @media (max-width: 720px) { .profile-list article { align-items: flex-start; flex-wrap: wrap; } .row-actions { width: 100%; padding-left: 0; } }
  @media (prefers-reduced-motion: reduce) { :global(.animate-spin) { animation: none; } }
</style>
