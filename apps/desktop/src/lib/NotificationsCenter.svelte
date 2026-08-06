<script lang="ts">
  import BellIcon from '@lucide/svelte/icons/bell';
  import CheckIcon from '@lucide/svelte/icons/check';
  import CircleHelpIcon from '@lucide/svelte/icons/circle-help';
  import CircleCheckIcon from '@lucide/svelte/icons/circle-check';

  import IconButton from '$lib/components/ds/IconButton.svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Popover from '$lib/components/ui/popover';
  import { ScrollArea } from '$lib/components/ui/scroll-area';
  import * as Tabs from '$lib/components/ui/tabs';
  import type { Notification } from '$lib/daemon';

  interface Props {
    notifications: Notification[];
    busy?: boolean;
    onRefresh: () => void;
    onOpen: (notification: Notification) => void;
    onMarkRead: (notification: Notification) => void;
    onMarkAll: () => void;
  }

  let {
    notifications,
    busy = false,
    onRefresh,
    onOpen,
    onMarkRead,
    onMarkAll
  }: Props = $props();

  let open = $state(false);
  let tab = $state<'unread' | 'history'>('unread');
  let unread = $derived(notifications.filter((notification) => notification.read_at === null));
  let history = $derived(notifications.filter((notification) => notification.read_at !== null));
  let visible = $derived(tab === 'unread' ? unread : history);

  function changeOpen(next: boolean): void {
    open = next;
    if (next) {
      if (unread.length > 0) tab = 'unread';
      onRefresh();
    }
  }

  function choose(notification: Notification): void {
    open = false;
    onOpen(notification);
  }

  function showHistory(): void {
    tab = 'history';
  }

  function notificationTypeLabel(notification: Notification): string {
    switch (notification.type) {
      case 'process_crashed': return 'Process crashed';
      case 'timer_fired': return 'Timer fired';
      case 'needs_input': return 'Agent needs input';
      default: return 'Agent finished';
    }
  }

  function timeLabel(timestamp: number): string {
    const elapsed = Math.max(0, Date.now() - timestamp);
    const minutes = Math.floor(elapsed / 60_000);
    if (minutes < 1) return 'just now';
    if (minutes < 60) return `${minutes}m ago`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours}h ago`;
    return new Intl.DateTimeFormat(undefined, {
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit'
    }).format(new Date(timestamp));
  }
</script>

<Popover.Root {open} onOpenChange={changeOpen}>
  <Popover.Trigger>
    {#snippet child({ props })}
      <IconButton
        {...props}
        class="relative size-7 shrink-0 rounded border border-border bg-card"
        label={unread.length === 0
          ? 'Notifications · no unread items'
          : `Notifications · ${unread.length} unread item${unread.length === 1 ? '' : 's'}`}
        aria-expanded={open}
      >
        {#snippet icon()}
          <BellIcon size={15} strokeWidth={1.8} />
          {#if unread.length > 0}
            <span
              class="unread-badge"
              aria-label={`${unread.length} unread notification${unread.length === 1 ? '' : 's'}`}
            >{unread.length > 99 ? '99+' : unread.length}</span>
          {/if}
        {/snippet}
      </IconButton>
    {/snippet}
  </Popover.Trigger>

  <Popover.Content
    side="bottom"
    align="start"
    sideOffset={6}
    class="w-[360px] gap-0 overflow-hidden rounded-md border border-border bg-popover p-0 shadow-xl ring-0"
  >
    <section class="notification-center" aria-label="Notifications center">
      <header>
        <div>
          <span>Activity</span>
          <h2>Notifications</h2>
        </div>
        <Button
          variant="ghost"
          size="sm"
          disabled={busy || unread.length === 0}
          onclick={onMarkAll}
        >
          <CheckIcon size={14} aria-hidden="true" />Mark all read
        </Button>
      </header>

      <Tabs.Root value={tab} onValueChange={(value) => (tab = value as typeof tab)}>
        <Tabs.List variant="line" class="notification-tabs">
          <Tabs.Trigger value="unread">Unread <span>{unread.length}</span></Tabs.Trigger>
          <Tabs.Trigger value="history">History <span>{history.length}</span></Tabs.Trigger>
        </Tabs.List>
      </Tabs.Root>

      <ScrollArea class="notification-scroll">
        <div class="notification-list" aria-live="polite">
          {#each visible as notification (notification.id)}
            <article class:read={notification.read_at !== null}>
              <button type="button" class="notification-open" onclick={() => choose(notification)}>
                <span class="type-icon" aria-hidden="true">
                  {#if notification.type === 'needs_input'}
                    <CircleHelpIcon size={16} strokeWidth={1.8} />
                  {:else}
                    <CircleCheckIcon size={16} strokeWidth={1.8} />
                  {/if}
                </span>
                <span class="notification-copy">
                  <span class="notification-meta">
                    <strong>{notificationTypeLabel(notification)}</strong>
                    <time datetime={new Date(notification.created_at).toISOString()}>{timeLabel(notification.created_at)}</time>
                  </span>
                  <span class="notification-body">{notification.body}</span>
                </span>
              </button>
              {#if notification.read_at === null}
                <IconButton
                  class="mark-one size-7"
                  label={`Mark “${notification.body}” read`}
                  disabled={busy}
                  onclick={() => onMarkRead(notification)}
                >
                  {#snippet icon()}<CheckIcon size={14} />{/snippet}
                </IconButton>
              {/if}
            </article>
          {:else}
            <div class="notification-empty">
              <CircleCheckIcon size={22} strokeWidth={1.6} aria-hidden="true" />
              <strong>{tab === 'unread' ? 'You’re caught up' : 'No read notifications'}</strong>
              <p>{tab === 'unread'
                ? 'Finished agents and other events that need attention appear here.'
                : 'Notifications move here after you mark them read.'}</p>
              {#if tab === 'unread'}
                <Button size="sm" variant="outline" onclick={showHistory}>View history</Button>
              {:else}
                <Button size="sm" variant="outline" onclick={() => (open = false)}>Close</Button>
              {/if}
            </div>
          {/each}
        </div>
      </ScrollArea>
    </section>
  </Popover.Content>
</Popover.Root>

<style>
  .unread-badge { position: absolute; top: -4px; right: -5px; display: inline-flex; min-width: 16px; height: 16px; align-items: center; justify-content: center; border: 2px solid var(--card); border-radius: 999px; padding: 0 3px; background: var(--foreground); color: var(--background); font: 700 10px/1 'JetBrains Mono Variable', monospace; pointer-events: none; }
  .notification-center { display: grid; min-height: 220px; grid-template-rows: auto auto minmax(0, 1fr); color: var(--foreground); }
  .notification-center > header { display: flex; min-height: 52px; align-items: center; justify-content: space-between; gap: var(--space-3); border-bottom: 1px solid var(--border); padding: var(--space-2) var(--space-3); }
  .notification-center > header span, .notification-center > header h2 { display: block; }
  .notification-center > header span { color: var(--muted-foreground); font: 700 var(--font-size-xs) 'JetBrains Mono Variable', monospace; letter-spacing: 0.04em; text-transform: uppercase; }
  .notification-center > header h2 { margin: 2px 0 0; font-size: var(--font-size-base); font-weight: 680; }
  :global(.notification-tabs) { width: 100%; justify-content: flex-start; gap: var(--space-3); border-bottom: 1px solid var(--border); padding: 0 var(--space-3); }
  :global(.notification-tabs button) { min-height: 34px; flex: none; gap: var(--space-1); border-radius: 0; padding-inline: 0; font-size: var(--font-size-sm); }
  :global(.notification-tabs button span) { color: var(--muted-foreground); font: 600 var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  :global(.notification-scroll) { max-height: min(360px, calc(100vh - 150px)); }
  .notification-list { display: grid; padding: var(--space-1); }
  .notification-list article { display: flex; min-height: 58px; align-items: center; border-bottom: 1px solid var(--border); }
  .notification-list article:last-child { border-bottom: 0; }
  .notification-list article.read { opacity: 0.72; }
  .notification-open { display: flex; min-width: 0; flex: 1; align-items: start; gap: var(--space-2); border: 0; border-radius: var(--radius); padding: var(--space-2); background: transparent; color: inherit; text-align: left; cursor: pointer; }
  .notification-open:hover { background: var(--accent); }
  .notification-open:focus-visible { outline: 1px solid var(--ring); outline-offset: -1px; }
  .type-icon { display: grid; width: 22px; height: 22px; flex: none; place-items: center; color: var(--muted-foreground); }
  .notification-copy { min-width: 0; flex: 1; }
  .notification-meta { display: flex; align-items: baseline; justify-content: space-between; gap: var(--space-2); }
  .notification-meta strong { overflow: hidden; font-size: var(--font-size-sm); font-weight: 650; text-overflow: ellipsis; white-space: nowrap; }
  .notification-meta time { flex: none; color: var(--muted-foreground); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; }
  .notification-body { display: -webkit-box; overflow: hidden; margin-top: 2px; color: var(--muted-foreground); font-size: var(--font-size-xs); line-height: 1.35; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; }
  :global(.mark-one) { margin-right: var(--space-1); flex: none; }
  .notification-empty { display: grid; min-height: 190px; place-items: center; align-content: center; gap: var(--space-1); padding: var(--space-4); color: var(--muted-foreground); text-align: center; }
  .notification-empty strong { margin-top: var(--space-1); color: var(--foreground); font-size: var(--font-size-sm); }
  .notification-empty p { max-width: 260px; margin: 0 0 var(--space-2); font-size: var(--font-size-xs); line-height: 1.45; }
</style>
