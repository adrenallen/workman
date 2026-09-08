<script lang="ts">
  import PlusIcon from '@lucide/svelte/icons/plus';
  import FolderOpenIcon from '@lucide/svelte/icons/folder-open';
  import { Button } from './components/ui/button';
  import workmanLogo from '../../../../assets/branding/workman-logo-wide-transparent.png';

  let { connected, busy, onAddProject, onProfiles }: {
    connected: boolean;
    busy: boolean;
    onAddProject: () => void;
    onProfiles: () => void;
  } = $props();
</script>

<section class="welcome" aria-label="Welcome to Workman">
  <div class="welcome-content">
    <img src={workmanLogo} alt="Workman" class="welcome-logo" />
    <h1>Let’s get to work.</h1>
    <p>Bring your agents, terminals, and notes together.<br />Add a project folder to get started.</p>
    <Button class="welcome-action" disabled={!connected || busy} onclick={onAddProject}>
      <PlusIcon size={18} />Add a project
    </Button>
    <span class="local-copy"><FolderOpenIcon size={14} aria-hidden="true" /> Your files stay right where they are.</span>
    <Button variant="ghost" disabled={!connected} onclick={onProfiles}>Switch profiles</Button>
  </div>
</section>

<style>
  .welcome { display: grid; width: 100%; min-height: 0; overflow: auto; padding: 40px 24px; place-items: center; }
  .welcome-content { display: flex; width: min(100%, 680px); flex-direction: column; align-items: center; text-align: center; }
  .welcome-logo { display: block; width: 100%; max-width: 620px; height: auto; margin: -36px 0 -20px; }
  h1 { max-width: 30ch; margin: 0; color: var(--foreground); font-size: var(--font-size-welcome-title); font-weight: 650; letter-spacing: -.035em; line-height: 1.25; }
  p { margin: 16px 0 28px; color: var(--text-soft); font-size: var(--font-size-base); line-height: 1.75; }
  .welcome :global(.welcome-action) { height: 44px; padding-inline: 24px; gap: 10px; border: 1px solid var(--brand-blue); background: var(--brand-surface); color: var(--foreground); font-size: var(--font-size-base); box-shadow: 0 3px 0 var(--brand-violet); }
  .welcome :global(.welcome-action:hover:not(:disabled)) { background: var(--brand-selection); transform: translateY(-1px); }
  .local-copy { display: flex; align-items: center; gap: 7px; margin: 22px 0 14px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  @media (prefers-reduced-motion: reduce) { .welcome :global(.welcome-action) { transition: none; transform: none; } }
</style>
