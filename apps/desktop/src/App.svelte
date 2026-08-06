<script lang="ts">
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import FolderGit2Icon from '@lucide/svelte/icons/folder-git-2';
  import FolderIcon from '@lucide/svelte/icons/folder';
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';
  import MoreHorizontalIcon from '@lucide/svelte/icons/more-horizontal';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import XIcon from '@lucide/svelte/icons/x';
  import { open } from '@tauri-apps/plugin-dialog';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount, tick } from 'svelte';

  import AddCommandDialog from './lib/AddCommandDialog.svelte';
  import AgentDoneToasts, { type AgentDoneNotice } from './lib/AgentDoneToasts.svelte';
  import IconButton from './lib/components/ds/IconButton.svelte';
  import StatusIndicator from './lib/components/ds/StatusIndicator.svelte';
  import TooltipLabel from './lib/components/ds/TooltipLabel.svelte';
  import { Button } from './lib/components/ui/button';
  import * as Dialog from './lib/components/ui/dialog';
  import ContextMenu from './lib/ContextMenu.svelte';
  import ClaimedTodoOverlay from './lib/ClaimedTodoOverlay.svelte';
  import EmptyState from './lib/EmptyState.svelte';
  import KeyboardShortcuts from './lib/KeyboardShortcuts.svelte';
  import NotificationsCenter from './lib/NotificationsCenter.svelte';
  import OptimisticProcessPanel from './lib/OptimisticProcessPanel.svelte';
  import ProcessStatusBar from './lib/ProcessStatusBar.svelte';
  import ProjectOpeners from './lib/ProjectOpeners.svelte';
  import ProjectTree from './lib/ProjectTree.svelte';
  import QuickJumpPalette from './lib/QuickJumpPalette.svelte';
  import ScratchpadDetailView from './lib/ScratchpadDetailView.svelte';
  import SettingsPanel from './lib/SettingsPanel.svelte';
  import { applyUpdate, checkForUpdates, type UpdateStatus } from './lib/settings';
  import TerminalView from './lib/TerminalView.svelte';
  import TodoBrowser from './lib/TodoBrowser.svelte';
  import TodoDetailView from './lib/TodoDetailView.svelte';
  import TrustReviewDialog from './lib/TrustReview.svelte';
  import WorktreeDialog from './lib/WorktreeDialog.svelte';
  import WorktreeImportDialog from './lib/WorktreeImportDialog.svelte';
  import WorktreeOperationRow from './lib/WorktreeOperationRow.svelte';
  import WorktreeProgressPanel from './lib/WorktreeProgressPanel.svelte';
  import WorktreeRemoveDialog from './lib/WorktreeRemoveDialog.svelte';
  import WorktreeRowMeta from './lib/WorktreeRowMeta.svelte';
  import type { AgentTool } from './lib/agentTools';
  import type { ClaimedTodo } from './lib/claimedTodos';
  import type {
    CoordinationSnapshot,
    NewTodoInput,
    ScratchpadRead,
    TodoDetail,
    TodoPriority
  } from './lib/coordination';
  import {
    contextMenuRequest,
    describeContextMenu,
    focusTerminalInput,
    keyboardContextMenuRequest,
    openWorkspacePath,
    type ContextActionId,
    type ContextMenuRequest,
    type ContextMenuTarget
  } from './lib/contextMenu';
  import {
    DaemonClient,
    isUnsupportedControlMethod,
    type ConnectionStatus,
    type Notification,
    type ProcessView,
    type Project,
    type TrustReview
  } from './lib/daemon';
  import {
    appNavigation,
    readRecentNavigationKeys,
    recordRecentNavigation,
    type AppNavigationRequest,
    type AppNavigationTarget,
    type NavigationProjectSnapshot
  } from './lib/navigation';
  import {
    NATIVE_MENU_EVENT,
    requestNativeUpdateCheck,
    type NativeMenuAction
  } from './lib/nativeMenu';
  import {
    openerSettings,
    openProjectCustom,
    openProjectEditor,
    openProjectFinder
  } from './lib/openers';
  import {
    focusAdjacentPanel,
    focusPanel,
    isTextEditingTarget,
    isTerminalInputTarget,
    moveListFocus,
    panelForTarget
  } from './lib/keyboardNavigation';
  import {
    clampPanelWidth,
    loadPanelPreference,
    savePanelPreference,
    startPanelResize
  } from './lib/panelPreferences';
  import {
    isProcessSelection,
    projectTreeSelection,
    type ProjectTreeSelection
  } from './lib/projectTree';
  import {
    moveTreeOrderBlock,
    reorderItem,
    siblingTarget,
    type ReorderDirection,
    type ReorderDrop
  } from './lib/reorder';
  import { openSettingsSection } from './lib/settingsSections';
  import {
    createOptimisticProcess,
    failOptimisticProcess,
    type OptimisticProcess
  } from './lib/optimisticProcesses';
  import {
    buildProjectRailGroups,
    projectBranchLabel,
    projectRepositoryTitle,
    type ProjectRailGroup,
    type WorktreeBranchOption,
    type WorktreeDialogSubmission,
    type WorktreeEntry,
    type WorktreeList,
    type WorktreeRepository
  } from './lib/worktrees';
  import {
    beginWorktreeOperation,
    dismissWorktreeOperation,
    failWorktreeOperation,
    worktreeOperations,
    type WorktreeOperation
  } from './lib/worktreeProgress';

  const client = new DaemonClient();
  const projectRailBounds = { min: 176, max: 340 };
  const treeRailBounds = { min: 220, max: 420 };
  const collapsedProjectRailWidth = 58;
  const collapsedTreeRailWidth = 54;
  const worktreeCollapseStorageKey = 'workman.worktree.repository-collapse.v1';

  let projects = $state<Project[]>([]);
  let processes = $state<ProcessView[]>([]);
  let optimisticProcesses = $state<OptimisticProcess[]>([]);
  let nextOptimisticProcessId = -1;
  let coordination = $state<CoordinationSnapshot | null>(null);
  let connection = $state<ConnectionStatus>({
    status: 'connecting',
    message: null,
    port: null,
    app_version: '',
    app_build_id: '',
    app_control_protocol_version: 0,
    daemon_version: null,
    daemon_build_id: null,
    daemon_control_protocol_version: null,
    version_compatible: false
  });
  let selection = $state<ProjectTreeSelection | null>(null);
  let todoDetail = $state<TodoDetail | null>(null);
  let scratchpadRead = $state<ScratchpadRead | null>(null);
  let detailLoading = $state(false);
  let detailBusy = $state(false);
  let busy = $state(false);
  let processBusyId = $state<number | null>(null);
  let loadedProjectId = $state<number | null>(null);
  let processRequest = 0;
  let coordinationRequest = 0;
  let error = $state<string | null>(null);
  let renameId = $state<number | null>(null);
  let renameValue = $state('');
  let settingsOpen = $state(false);
  let todoBrowserOpen = $state(false);
  let trustReview = $state<TrustReview | null>(null);
  let trustBusy = $state(false);
  let projectRailWidth = $state(238);
  let projectRailCollapsed = $state(false);
  let treeRailWidth = $state(280);
  let treeRailCollapsed = $state(false);

  let dialog = $state<'todo' | 'agent' | 'command' | null>(null);
  let todoTitle = $state('');
  let todoBody = $state('');
  let todoPriority = $state<TodoPriority>('medium');
  let todoTags = $state('');
  let scratchpadFocusRequest = $state(0);
  let agentTools = $state<AgentTool[]>([]);
  let agentToolsLoading = $state(false);
  let versionRestarting = $state(false);
  let startupUpdate = $state<UpdateStatus | null>(null);
  let startupUpdatePort = $state<number | null>(null);
  let quickJumpOpen = $state(false);
  let shortcutsOpen = $state(false);
  let quickJumpLoading = $state(false);
  let quickJumpRecentKeys = $state<string[]>([]);
  let navigationIndex = $state<Record<number, NavigationProjectSnapshot>>({});
  let navigationIndexRequest = 0;
  let projectReorderBusy = $state(false);
  let processReorderBusy = $state(false);
  let contextRequest = $state<ContextMenuRequest | null>(null);
  let treeRenameTarget = $state<ContextMenuTarget | null>(null);
  let worktreeLists = $state<Record<number, WorktreeList>>({});
  let worktreeRefreshingRepositoryId = $state<number | null>(null);
  let collapsedRepositories = $state<Record<number, boolean>>({});
  let worktreeDialog = $state<{
    mode: 'create' | 'fork' | 'adopt';
    sourceProject: Project;
    repository: WorktreeRepository;
    sourceEntry: WorktreeEntry | null;
  } | null>(null);
  let worktreeDialogBusy = $state(false);
  let worktreeDialogError = $state<string | null>(null);
  let branchOptions = $state<WorktreeBranchOption[]>([]);
  let originBranchesLoading = $state(false);
  let activeWorktreeOperationId = $state<string | null>(null);
  let agentDoneNotices = $state<AgentDoneNotice[]>([]);
  let agentDoneNoticeSequence = 0;
  let notifications = $state<Notification[]>([]);
  let notificationBusy = $state(false);
  let notificationRequest = 0;
  let notificationUnreadSignature: string | null = null;
  const reconciledWorktreeOperations = new Set<string>();
  const notifiedUnreadProcessIds = new Set<number>();
  const markReadPending = new Set<number>();
  let removeWorktreeDialog = $state<{
    project: Project;
    repository: WorktreeRepository;
    entry: WorktreeEntry;
  } | null>(null);
  let removeWorktreeBusy = $state(false);
  let removeWorktreeError = $state<string | null>(null);
  let importOffer = $state<{ repository: WorktreeRepository; entries: WorktreeEntry[] } | null>(null);
  let importBusyPath = $state<string | null>(null);
  let importError = $state<string | null>(null);
  const offeredImportRepositories = new Set<number>();

  let selectedProject = $derived(projects.find((project) => project.selected) ?? null);
  let projectRailGroups = $derived(buildProjectRailGroups(projects));
  let visibleProcesses = $derived([
    ...processes,
    ...optimisticProcesses.map((optimistic) => optimistic.process)
  ]);
  let selectedProcess = $derived(
    selection && isProcessSelection(selection)
      ? visibleProcesses.find((process) => process.id === selection?.id) ?? null
      : null
  );
  let selectedOptimisticProcess = $derived(
    selection && isProcessSelection(selection)
      ? optimisticProcesses.find((optimistic) => optimistic.process.id === selection?.id) ?? null
      : null
  );
  let activeWorktreeOperation = $derived(
    $worktreeOperations.find((operation) => operation.id === activeWorktreeOperationId) ?? null
  );
  let projectRailCount = $derived(
    projects.length
      + $worktreeOperations.filter((operation) =>
        operation.status === 'pending' || operation.status === 'running'
      ).length
  );
  let treeProcesses = $derived([
    ...visibleProcesses.filter((process) => process.kind === 'agent'),
    ...visibleProcesses.filter((process) => process.kind === 'terminal'),
    ...visibleProcesses.filter((process) => process.kind === 'command')
  ]);
  let frameItemLabel = $derived(
    settingsOpen
      ? 'Settings'
      : activeWorktreeOperation
        ? activeWorktreeOperation.label
        : todoBrowserOpen
          ? 'Todos'
          : (selection?.label ?? 'Project')
  );
  let windowTitle = $derived(
    selectedProject && selectedProcess
      ? `${projectLabel(selectedProject)}: ${selectedProcess.name}`
      : 'workman'
  );
  let contextMenuDescriptor = $derived(
    contextRequest ? describeContextMenu(contextRequest.target, $openerSettings) : null
  );
  let versionSkew = $derived(
    connection.status === 'connected' && !connection.version_compatible
  );
  let updateAvailable = $derived(startupUpdate?.check.available === true);
  let showVersionBanner = $derived(versionSkew || updateAvailable);

  $effect(() => {
    void getCurrentWindow().setTitle(windowTitle).catch(() => undefined);
  });

  $effect(() => {
    for (const operation of $worktreeOperations) {
      if (
        operation.status === 'completed'
        && operation.project
        && !reconciledWorktreeOperations.has(operation.id)
      ) {
        reconciledWorktreeOperations.add(operation.id);
        void reconcileCompletedWorktree(operation);
      }
    }
  });

  $effect(() => {
    const projectId = selectedProject?.id ?? null;
    const connected = connection.status === 'connected';
    if (!connected || projectId === null) {
      processes = [];
      optimisticProcesses = [];
      coordination = null;
      selection = null;
      todoDetail = null;
      scratchpadRead = null;
      settingsOpen = false;
      todoBrowserOpen = false;
      activeWorktreeOperationId = null;
      loadedProjectId = null;
      return;
    }
    if (loadedProjectId !== projectId) {
      loadedProjectId = projectId;
      processes = [];
      optimisticProcesses = [];
      coordination = null;
      selection = null;
      todoDetail = null;
      scratchpadRead = null;
      settingsOpen = false;
      todoBrowserOpen = false;
      activeWorktreeOperationId = null;
      void loadProject(projectId);
    }
  });

  onMount(() => {
    try {
      const saved = JSON.parse(localStorage.getItem(worktreeCollapseStorageKey) ?? '{}');
      if (saved && typeof saved === 'object') collapsedRepositories = saved;
    } catch {
      collapsedRepositories = {};
    }
    const projectPreference = loadPanelPreference(
      'project-rail',
      { collapsed: false, width: projectRailWidth },
      projectRailBounds.min,
      projectRailBounds.max
    );
    projectRailWidth = projectPreference.width;
    projectRailCollapsed = projectPreference.collapsed;
    const treePreference = loadPanelPreference(
      'section-rail',
      { collapsed: false, width: treeRailWidth },
      treeRailBounds.min,
      treeRailBounds.max
    );
    treeRailWidth = treePreference.width;
    treeRailCollapsed = treePreference.collapsed;

    let active = true;
    const stopStatuses = client.onProcessStatuses((next) => {
      if (!active) return;
      cacheProcessStatuses(next);
      if (selectedProject) {
        applyProcesses(next.filter((process) => process.project_id === selectedProject?.id));
      }
      reconcileAgentDoneNotices(next);
    });
    let lastNavigationRequest = 0;
    const stopNavigation = appNavigation.subscribe(({ request }) => {
      if (!active || !request || request.id === lastNavigationRequest) return;
      lastNavigationRequest = request.id;
      void resolveNavigationRequest(request).finally(() => appNavigation.acknowledge(request.id));
    });
    let stopNativeMenu = (): void => {};
    void listen<NativeMenuAction>(NATIVE_MENU_EVENT, ({ payload }) => {
      if (active) handleNativeMenuAction(payload);
    }).then((stop) => {
      if (active) stopNativeMenu = stop;
      else stop();
    }).catch(reportError);
    const projectTimer = setInterval(() => {
      if (active && connection.status === 'connected' && !busy) void refreshProjects();
    }, 5000);
    const coordinationTimer = setInterval(() => {
      if (
        active &&
        connection.status === 'connected' &&
        connection.version_compatible &&
        selectedProject
      ) {
        void refreshCoordination(selectedProject.id, false);
      }
    }, 2500);

    void client
      .start(
        (status) => { if (active) applyConnectionStatus(status); },
        (message) => { if (active) error = message; }
      )
      .then((status) => { if (active) applyConnectionStatus(status); })
      .catch(reportError);

    return () => {
      active = false;
      clearInterval(projectTimer);
      clearInterval(coordinationTimer);
      stopStatuses();
      stopNavigation();
      stopNativeMenu();
      client.close();
    };
  });

  function applyConnectionStatus(status: ConnectionStatus): void {
    const reconnected = connection.status !== 'connected' && status.status === 'connected';
    connection = status;
    if (status.status === 'connected') {
      console.info(
        `workman daemon: ${status.daemon_version ?? 'legacy'} ` +
          `(build ${status.daemon_build_id ?? 'unknown'}, protocol ${status.daemon_control_protocol_version ?? 'unknown'})`
      );
      if (status.version_compatible) versionRestarting = false;
      if (status.version_compatible && status.port !== startupUpdatePort) {
        startupUpdatePort = status.port;
        void startupUpdateCheck();
      }
    } else {
      startupUpdatePort = null;
    }
    if (reconnected) {
      void client.subscribeProcessStatuses().catch(reportError);
      void refreshProjects();
      if (status.version_compatible) void refreshNotifications();
    }
  }

  async function startupUpdateCheck(): Promise<void> {
    try {
      startupUpdate = await checkForUpdates(client, false);
    } catch (cause) {
      console.warn('workman startup update check failed', cause);
    }
  }

  function handleNativeMenuAction(action: NativeMenuAction): void {
    switch (action) {
      case 'settings':
        appNavigation.navigate({ type: 'settings' }, 'api');
        return;
      case 'about':
        openSettingsSection('about', selectedProject?.id);
        return;
      case 'check_updates':
        requestNativeUpdateCheck();
        openSettingsSection('about', selectedProject?.id);
        return;
      case 'toggle_project_rail':
        toggleProjectRail();
        return;
      case 'toggle_section_rail':
        toggleTreeRail();
        return;
    }
  }

  function handleShortcut(event: KeyboardEvent): void {
    const target = event.target as HTMLElement | null;
    if (contextRequest) {
      if (event.key === 'Escape') closeContextMenu();
      return;
    }
    if (isTerminalInputTarget(target)) return;
    if (event.metaKey && !event.altKey && !event.ctrlKey && event.key === '/') {
      event.preventDefault();
      if (shortcutsOpen) closeShortcuts();
      else openShortcuts();
      return;
    }
    if (event.metaKey && !event.altKey && !event.ctrlKey && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      if (shortcutsOpen) closeShortcuts();
      if (quickJumpOpen) closeQuickJump();
      else openQuickJump();
      return;
    }
    if (quickJumpOpen || shortcutsOpen) return;
    if (isTextEditingTarget(target)) return;
    if (
      event.metaKey && !event.ctrlKey && !event.shiftKey
      && (event.key === 'ArrowLeft' || event.key === 'ArrowRight')
    ) {
      event.preventDefault();
      focusAdjacentPanel(panelForTarget(target), event.key === 'ArrowLeft' ? -1 : 1);
      return;
    }
    if (
      event.metaKey && !event.altKey && !event.ctrlKey && !event.shiftKey
      && (event.key === 'ArrowUp' || event.key === 'ArrowDown')
    ) {
      event.preventDefault();
      cycleProcess(event.key === 'ArrowUp' ? -1 : 1, panelForTarget(target));
      return;
    }
    if (panelForTarget(target) === 'projects') handleProjectListKeys(event);
    if (event.defaultPrevented) return;
    if (event.metaKey && !event.altKey && event.key.toLowerCase() === 'b') {
      event.preventDefault();
      if (event.shiftKey) toggleTreeRail();
      else toggleProjectRail();
      return;
    }
    if (event.key === 'Escape') {
      if (quickJumpOpen) closeQuickJump();
      else if (dialog) dialog = null;
      else if (settingsOpen) settingsOpen = false;
      else clearSelection();
    }
  }

  function openQuickJump(): void {
    shortcutsOpen = false;
    quickJumpRecentKeys = readRecentNavigationKeys();
    quickJumpOpen = true;
    void refreshQuickJumpIndex(true);
  }

  function closeQuickJump(): void {
    quickJumpOpen = false;
  }

  function openShortcuts(): void {
    quickJumpOpen = false;
    shortcutsOpen = true;
  }

  function closeShortcuts(): void {
    shortcutsOpen = false;
  }

  function handleProjectListKeys(event: KeyboardEvent): void {
    if (
      event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey
      || (event.key !== 'ArrowDown' && event.key !== 'ArrowUp')
    ) return;
    const container = (event.target as HTMLElement | null)?.closest<HTMLElement>('.project-list');
    if (!container) return;
    if (
      moveListFocus(
        container,
        event.target,
        '.project-select:not(:disabled)',
        event.key === 'ArrowDown' ? 1 : -1
      )
    ) event.preventDefault();
  }

  function unfocusSelectedProcess(): void {
    clearSelection();
    void tick().then(() => focusPanel('tree'));
  }

  function cycleProcess(direction: -1 | 1, returnPanel: ReturnType<typeof panelForTarget>): void {
    if (treeProcesses.length === 0) return;
    const current = selectedProcess
      ? treeProcesses.findIndex((process) => process.id === selectedProcess?.id)
      : -1;
    const next = current < 0
      ? direction > 0 ? 0 : treeProcesses.length - 1
      : (current + direction + treeProcesses.length) % treeProcesses.length;
    const process = treeProcesses[next];
    if (!process) return;
    selectProcessById(process.id);
    if (returnPanel === 'projects' || returnPanel === 'tree') {
      void tick().then(() => focusPanel(returnPanel));
    }
  }

  function chooseQuickJumpTarget(target: AppNavigationTarget): void {
    closeQuickJump();
    appNavigation.navigate(target, 'palette');
  }

  function cacheProcessStatuses(next: ProcessView[]): void {
    const grouped = new Map<number, ProcessView[]>();
    for (const process of next) {
      const group = grouped.get(process.project_id) ?? [];
      group.push(process);
      grouped.set(process.project_id, group);
    }
    let changed = false;
    const updated = { ...navigationIndex };
    for (const [projectId, projectProcesses] of grouped) {
      updated[projectId] = {
        processes: projectProcesses,
        coordination: updated[projectId]?.coordination ?? null
      };
      changed = true;
    }
    if (changed) navigationIndex = updated;
  }

  function clearAgentUnreadLocally(processId: number): void {
    const clear = (process: ProcessView): ProcessView => process.id === processId
      ? { ...process, agent_state: { ...process.agent_state, unread: false } }
      : process;
    processes = processes.map(clear);
    const updated = { ...navigationIndex };
    for (const [projectId, snapshot] of Object.entries(updated)) {
      updated[Number(projectId)] = { ...snapshot, processes: snapshot.processes.map(clear) };
    }
    navigationIndex = updated;
    notifiedUnreadProcessIds.delete(processId);
    agentDoneNotices = agentDoneNotices.filter((notice) => notice.processId !== processId);
    const readAt = Date.now();
    notifications = notifications.map((notification) =>
      notification.process_id === processId && notification.read_at === null
        ? { ...notification, read_at: readAt }
        : notification
    );
  }

  async function refreshNotifications(): Promise<void> {
    if (connection.status !== 'connected' || !connection.version_compatible) return;
    const request = ++notificationRequest;
    try {
      const next = await client.notifications();
      if (request === notificationRequest) notifications = next;
    } catch (cause) {
      if (request === notificationRequest) reportError(cause);
    }
  }

  async function markCenterNotificationRead(notification: Notification): Promise<void> {
    if (notificationBusy || notification.read_at !== null) return;
    const previous = notifications;
    notificationBusy = true;
    notifications = notifications.map((candidate) => candidate.id === notification.id
      ? { ...candidate, read_at: Date.now() }
      : candidate);
    if (notification.process_id !== null) clearAgentUnreadLocally(notification.process_id);
    try {
      await client.markNotificationRead(notification.id);
    } catch (cause) {
      notifications = previous;
      reportError(cause);
      if (notification.project_id !== null) await refreshProcesses(notification.project_id);
    } finally {
      notificationBusy = false;
    }
  }

  async function markAllNotificationsRead(): Promise<void> {
    if (notificationBusy || notifications.every((notification) => notification.read_at !== null)) return;
    const previous = notifications;
    notificationBusy = true;
    const readAt = Date.now();
    const processIds = new Set(
      notifications
        .filter((notification) => notification.read_at === null && notification.process_id !== null)
        .map((notification) => notification.process_id!)
    );
    notifications = notifications.map((notification) => notification.read_at === null
      ? { ...notification, read_at: readAt }
      : notification);
    for (const processId of processIds) clearAgentUnreadLocally(processId);
    try {
      await client.markAllNotificationsRead();
    } catch (cause) {
      notifications = previous;
      reportError(cause);
      if (selectedProject) await refreshProcesses(selectedProject.id);
    } finally {
      notificationBusy = false;
    }
  }

  function openNotification(notification: Notification): void {
    void markCenterNotificationRead(notification);
    const process = notification.process_id === null
      ? null
      : Object.values(navigationIndex)
          .flatMap((snapshot) => snapshot.processes)
          .find((candidate) => candidate.id === notification.process_id)
        ?? processes.find((candidate) => candidate.id === notification.process_id)
        ?? null;
    const projectId = notification.project_id ?? process?.project_id ?? null;
    if (notification.process_id !== null && projectId !== null) {
      appNavigation.navigate({
        type: 'item',
        selection: projectTreeSelection(
          process?.kind ?? (notification.type === 'agent_done' ? 'agent' : 'command'),
          notification.process_id,
          projectId,
          process?.name ?? notification.body
        )
      }, 'api');
    } else if (notification.todo_id !== null && projectId !== null) {
      appNavigation.navigate({
        type: 'item',
        selection: projectTreeSelection('todo', notification.todo_id, projectId, notification.body)
      }, 'api');
    } else if (projectId !== null) {
      appNavigation.navigate({ type: 'project', projectId }, 'api');
    }
  }

  async function markAgentRead(processId: number, projectId: number): Promise<void> {
    if (markReadPending.has(processId)) return;
    markReadPending.add(processId);
    clearAgentUnreadLocally(processId);
    try {
      await client.markProcessRead(processId);
    } catch (cause) {
      reportError(cause);
      await refreshProcesses(projectId);
    } finally {
      markReadPending.delete(processId);
    }
  }

  function reconcileAgentDoneNotices(next: ProcessView[]): void {
    const unreadIds = new Set(
      next
        .filter((process) =>
          process.kind === 'agent'
          && process.agent_state.unread
          && !markReadPending.has(process.id)
        )
        .map((process) => process.id)
    );
    for (const processId of notifiedUnreadProcessIds) {
      if (!unreadIds.has(processId)) notifiedUnreadProcessIds.delete(processId);
    }
    agentDoneNotices = agentDoneNotices.filter((notice) => unreadIds.has(notice.processId));

    const signature = [...unreadIds].sort((left, right) => left - right).join(',');
    if (signature !== notificationUnreadSignature) {
      notificationUnreadSignature = signature;
      void refreshNotifications();
    }

    for (const process of next) {
      if (process.kind !== 'agent' || !process.agent_state.unread) continue;
      if (markReadPending.has(process.id)) continue;
      const alreadyViewing = selectedProject?.id === process.project_id
        && selection?.kind === 'agent'
        && selection.id === process.id;
      if (alreadyViewing) {
        void markAgentRead(process.id, process.project_id);
        continue;
      }
      if (notifiedUnreadProcessIds.has(process.id)) continue;
      notifiedUnreadProcessIds.add(process.id);
      agentDoneNotices = [
        ...agentDoneNotices,
        {
          id: `${process.id}:${++agentDoneNoticeSequence}`,
          processId: process.id,
          projectId: process.project_id,
          name: process.name
        }
      ].slice(-4);
    }
  }

  function openAgentDoneNotice(notice: AgentDoneNotice): void {
    void markAgentRead(notice.processId, notice.projectId);
    appNavigation.navigate(
      {
        type: 'item',
        selection: projectTreeSelection(
          'agent',
          notice.processId,
          notice.projectId,
          notice.name
        )
      },
      'api'
    );
  }

  function projectUnreadAgentCount(projectId: number): number {
    return (navigationIndex[projectId]?.processes ?? [])
      .filter((process) => process.kind === 'agent' && process.agent_state.unread)
      .length;
  }

  function cacheProjectProcesses(projectId: number, next: ProcessView[]): void {
    navigationIndex = {
      ...navigationIndex,
      [projectId]: {
        processes: next,
        coordination: navigationIndex[projectId]?.coordination ?? null
      }
    };
  }

  function cacheProjectCoordination(projectId: number, next: CoordinationSnapshot): void {
    navigationIndex = {
      ...navigationIndex,
      [projectId]: {
        processes: navigationIndex[projectId]?.processes ?? [],
        coordination: next
      }
    };
  }

  async function refreshQuickJumpIndex(force: boolean): Promise<void> {
    if (connection.status !== 'connected') return;
    const request = ++navigationIndexRequest;
    const projectList = [...projects];
    quickJumpLoading = true;
    try {
      const [tools, snapshots] = await Promise.all([
        client.listAgentTools().catch(() => agentTools),
        Promise.all(
          projectList.map(async (project): Promise<[number, NavigationProjectSnapshot]> => {
            const cached = navigationIndex[project.id];
            if (!force && cached?.coordination) return [project.id, cached];
            const [projectProcesses, projectCoordination] = await Promise.all([
              client.processes(project.id).catch(() => cached?.processes ?? []),
              connection.version_compatible
                ? client.coordinationSnapshot(project.id).catch(() => cached?.coordination ?? null)
                : Promise.resolve(null)
            ]);
            return [
              project.id,
              { processes: projectProcesses, coordination: projectCoordination }
            ];
          })
        )
      ]);
      if (request !== navigationIndexRequest) return;
      agentTools = tools.filter((tool) => tool.enabled);
      navigationIndex = Object.fromEntries(snapshots);
    } finally {
      if (request === navigationIndexRequest) quickJumpLoading = false;
    }
  }

  async function resolveNavigationRequest(request: AppNavigationRequest): Promise<void> {
    try {
      const target = request.target;
      const projectId = navigationProjectId(target);
      if (projectId !== null && !(await activateProject(projectId))) return;

      recordRecentNavigation(target);
      quickJumpRecentKeys = readRecentNavigationKeys();
      dialog = null;

      switch (target.type) {
        case 'project':
          settingsOpen = false;
          clearSelection();
          return;
        case 'item':
          await selectTreeItem(target.selection);
          return;
        case 'settings':
          if (selectedProject) {
            todoBrowserOpen = false;
            settingsOpen = true;
          }
          return;
        case 'new-worktree':
          if (selectedProject) await openWorktreeDialog('create', selectedProject);
          return;
        case 'new-terminal':
          await spawnTerminal();
          return;
        case 'new-agent':
          await openAgentDialog();
          return;
        case 'spawn-agent': {
          let tool = agentTools.find((candidate) => candidate.id === target.agentToolId);
          if (!tool) {
            const tools = await client.listAgentTools();
            agentTools = tools.filter((candidate) => candidate.enabled);
            tool = agentTools.find((candidate) => candidate.id === target.agentToolId);
          }
          if (!tool) throw new Error(`Agent tool ${target.agentToolName} is no longer enabled`);
          await spawnAgent(tool);
          return;
        }
        case 'add-command':
          settingsOpen = false;
          dialog = 'command';
          return;
        case 'new-todo':
          settingsOpen = false;
          resetTodoForm();
          dialog = 'todo';
          return;
        case 'new-scratchpad':
          settingsOpen = false;
          await createScratchpad();
          return;
      }
    } catch (cause) {
      reportError(cause);
    }
  }

  function navigationProjectId(target: AppNavigationTarget): number | null {
    if (target.type === 'item') return target.selection.projectId;
    if ('projectId' in target && typeof target.projectId === 'number') return target.projectId;
    return selectedProject?.id ?? projects[0]?.id ?? null;
  }

  async function activateProject(projectId: number): Promise<boolean> {
    if (selectedProject?.id === projectId) return true;
    if (!projects.some((project) => project.id === projectId)) return false;

    busy = true;
    try {
      projects = await client.select(projectId);
      loadedProjectId = projectId;
      processes = [];
      coordination = null;
      selection = null;
      todoDetail = null;
      scratchpadRead = null;
      settingsOpen = false;
      todoBrowserOpen = false;
      await tick();
      await loadProject(projectId);
      await refreshWorktreeMetadata(projects);
      const activeProject = projects.find((project) => project.id === projectId);
      const activeList = activeProject ? worktreeListFor(activeProject) : null;
      if (activeList) maybeOfferExistingWorktrees(activeList);
      return selectedProject?.id === projectId;
    } finally {
      busy = false;
    }
  }

  async function loadProject(projectId: number): Promise<void> {
    try {
      await client.syncConfig(projectId);
    } catch (cause) {
      reportError(cause);
    }
    if (connection.version_compatible) {
      await Promise.all([refreshProcesses(projectId), refreshCoordination(projectId, true)]);
    } else {
      coordination = null;
      await refreshProcesses(projectId);
    }
  }

  function rootProjectFor(project: Project): Project | null {
    if (project.repository_id === null) return null;
    return projects.find((candidate) =>
      candidate.repository_id === project.repository_id && candidate.parent_project_id === null
    ) ?? null;
  }

  function worktreeListFor(project: Project): WorktreeList | null {
    return project.repository_id === null ? null : worktreeLists[project.repository_id] ?? null;
  }

  function worktreeEntryFor(project: Project): WorktreeEntry | null {
    return worktreeListFor(project)?.worktrees.find((entry) => entry.project_id === project.id) ?? null;
  }

  function worktreeRepositoryFor(project: Project): WorktreeRepository | null {
    return worktreeListFor(project)?.repository ?? null;
  }

  function worktreeOperationsFor(repositoryId: number | null): WorktreeOperation[] {
    if (repositoryId === null) return [];
    return $worktreeOperations.filter((operation) =>
      operation.repository_id === repositoryId
      && (
        operation.status !== 'completed'
        || !operation.project
        || !projects.some((project) => project.id === operation.project?.id)
      )
    );
  }

  async function reconcileCompletedWorktree(operation: WorktreeOperation): Promise<void> {
    if (!operation.project) return;
    const optimisticProject: Project = {
      ...operation.project,
      status: operation.project.status ?? 'idle'
    };
    if (!projects.some((project) => project.id === optimisticProject.id)) {
      projects = [...projects, optimisticProject].sort(
        (left, right) => left.sort_order - right.sort_order || left.id - right.id
      );
    }
    await tick();
    appNavigation.navigate({ type: 'project', projectId: optimisticProject.id }, 'api');
  }

  function showWorktreeOperation(operation: WorktreeOperation): void {
    activeWorktreeOperationId = operation.id;
    settingsOpen = false;
    todoBrowserOpen = false;
    selection = null;
  }

  function dismissActiveWorktreeOperation(): void {
    if (activeWorktreeOperationId) dismissWorktreeOperation(activeWorktreeOperationId);
    activeWorktreeOperationId = null;
  }

  async function retryWorktreeOperation(operation: WorktreeOperation): Promise<void> {
    const source = operation.source_project_id === null
      ? projects.find((project) =>
          project.repository_id === operation.repository_id && project.parent_project_id === null
        )
      : projects.find((project) => project.id === operation.source_project_id);
    dismissWorktreeOperation(operation.id);
    activeWorktreeOperationId = null;
    if (!source) {
      reportError(new Error('The source project is no longer available'));
      return;
    }
    await openWorktreeDialog(operation.mode, source);
  }

  async function refreshWorktreeMetadata(
    projectList: Project[],
    refreshPullRequests = false,
    force = false,
    onlyRepositoryId: number | null = null
  ): Promise<void> {
    const roots = projectList.filter((project) =>
      project.repository_id !== null &&
      project.parent_project_id === null &&
      (onlyRepositoryId === null || project.repository_id === onlyRepositoryId)
    );
    for (const root of roots) {
      const repositoryId = root.repository_id!;
      if (!force && worktreeLists[repositoryId]) continue;
      worktreeRefreshingRepositoryId = repositoryId;
      try {
        const list = await client.worktrees(root.id, refreshPullRequests);
        worktreeLists = { ...worktreeLists, [repositoryId]: list };
        maybeOfferExistingWorktrees(list, projectList);
      } catch (cause) {
        console.warn(`workman worktree metadata failed for project ${root.id}`, cause);
      } finally {
        if (worktreeRefreshingRepositoryId === repositoryId) worktreeRefreshingRepositoryId = null;
      }
    }
  }

  function maybeOfferExistingWorktrees(list: WorktreeList, projectList = projects): void {
    if (offeredImportRepositories.has(list.repository.id)) return;
    const selected = projectList.find((project) => project.selected);
    if (!selected || selected.repository_id !== list.repository.id || selected.parent_project_id !== null) return;
    const entries = list.worktrees.filter((entry) => entry.can_adopt);
    if (entries.length === 0) return;
    offeredImportRepositories.add(list.repository.id);
    importError = null;
    importOffer = { repository: list.repository, entries };
  }

  async function refreshWorktreeRepository(project: Project, refreshPullRequests = true): Promise<void> {
    const root = rootProjectFor(project);
    if (!root || root.repository_id === null) return;
    await refreshWorktreeMetadata(projects, refreshPullRequests, true, root.repository_id);
  }

  async function refreshProjects(): Promise<void> {
    busy = true;
    try {
      projects = await client.projects();
      void refreshWorktreeMetadata(projects);
      void refreshQuickJumpIndex(false);
    } catch (cause) {
      reportError(cause);
    } finally {
      busy = false;
    }
  }

  async function refreshProcesses(projectId: number): Promise<void> {
    const request = ++processRequest;
    try {
      const next = await client.processes(projectId);
      cacheProjectProcesses(projectId, next);
      if (request === processRequest && selectedProject?.id === projectId) applyProcesses(next);
    } catch (cause) {
      if (request === processRequest) reportError(cause);
    }
  }

  function applyProcesses(next: ProcessView[]): void {
    processes = next;
    if (selection && isProcessSelection(selection)) {
      const process = next.find((candidate) => candidate.id === selection?.id);
      if (process) selection = projectTreeSelection(process.kind, process.id, process.project_id, processLabel(process));
    }
  }

  async function refreshCoordination(projectId: number, showLoading: boolean): Promise<void> {
    const request = ++coordinationRequest;
    if (showLoading) detailLoading = true;
    try {
      const next = await client.coordinationSnapshot(projectId);
      cacheProjectCoordination(projectId, next);
      if (request === coordinationRequest && selectedProject?.id === projectId) {
        coordination = next;
        if (selection?.kind === 'scratchpad') {
          const summary = next.scratchpads.find((scratchpad) => scratchpad.id === selection?.id);
          if (summary && summary.revision !== scratchpadRead?.scratchpad.revision) {
            void loadScratchpad(selection.id, false);
          }
        }
      }
    } catch (cause) {
      if (request === coordinationRequest) reportError(cause);
    } finally {
      if (showLoading && request === coordinationRequest) detailLoading = false;
    }
  }

  async function selectTreeItem(next: ProjectTreeSelection): Promise<void> {
    if (!selectedProject || next.projectId !== selectedProject.id) return;
    scratchpadFocusRequest = 0;
    recordRecentNavigation({ type: 'item', selection: next });
    quickJumpRecentKeys = readRecentNavigationKeys();
    settingsOpen = false;
    todoBrowserOpen = false;
    activeWorktreeOperationId = null;
    selection = next;
    todoDetail = null;
    scratchpadRead = null;

    if (next.kind === 'agent') {
      const process = processes.find((candidate) => candidate.id === next.id);
      if (process?.agent_state.unread) void markAgentRead(process.id, process.project_id);
    }

    if (next.kind === 'todo') {
      await loadTodo(next.id);
    } else if (next.kind === 'scratchpad') {
      await loadScratchpad(next.id);
    } else if (next.kind === 'command') {
      const process = processes.find((candidate) => candidate.id === next.id);
      if (process && process.status !== 'running' && process.status !== 'starting') {
        if (process.source === 'yml' && process.trust_hash === null) await openTrustReview(process);
        else await startProcess(process);
      }
    }
  }

  function openClaimedTodo(claim: ClaimedTodo): void {
    appNavigation.navigate(
      {
        type: 'item',
        selection: projectTreeSelection('todo', claim.id, claim.project_id, claim.title)
      },
      'api'
    );
  }

  async function loadTodo(todoId: number): Promise<void> {
    if (!selectedProject) return;
    detailLoading = true;
    try {
      todoDetail = await client.coordinationTodo(selectedProject.id, todoId);
    } catch (cause) {
      reportError(cause);
    } finally {
      detailLoading = false;
    }
  }

  async function loadScratchpad(scratchpadId: number, showLoading = true): Promise<void> {
    if (!selectedProject) return;
    const projectId = selectedProject.id;
    if (showLoading) detailLoading = true;
    try {
      const next = await client.coordinationScratchpad(projectId, scratchpadId);
      if (
        selectedProject?.id === projectId &&
        selection?.kind === 'scratchpad' &&
        selection.id === scratchpadId
      ) {
        scratchpadRead = next;
        selection = projectTreeSelection(
          'scratchpad',
          scratchpadId,
          projectId,
          next.scratchpad.name
        );
      }
    } catch (cause) {
      reportError(cause);
    } finally {
      if (showLoading) detailLoading = false;
    }
  }

  async function startProcess(process: ProcessView): Promise<void> {
    processBusyId = process.id;
    try {
      await client.startProcess(process.id);
      await refreshProcesses(process.project_id);
    } catch (cause) {
      reportError(cause);
    } finally {
      processBusyId = null;
    }
  }

  async function spawnTerminal(): Promise<void> {
    if (!selectedProject || processBusyId !== null) return;
    processBusyId = -1;
    try {
      const process = await client.spawnTerminal(selectedProject.id);
      await refreshProcesses(selectedProject.id);
      await selectTreeItem(projectTreeSelection('terminal', process.id, process.project_id, processLabel(process)));
    } catch (cause) {
      reportError(cause);
    } finally {
      processBusyId = null;
    }
  }

  async function commandAdded(
    process: Pick<ProcessView, 'id' | 'project_id' | 'name'>,
    optimisticId: number | null = null
  ): Promise<void> {
    const projectId = process.project_id;
    dialog = null;
    await refreshProcesses(projectId);
    if (optimisticId !== null) {
      optimisticProcesses = optimisticProcesses.filter(
        (optimistic) => optimistic.process.id !== optimisticId
      );
    }
    if (selectedProject?.id !== projectId) return;
    const added = processes.find((candidate) => candidate.id === process.id) ?? process;
    selection = projectTreeSelection('command', added.id, projectId, added.name);
  }

  function beginOptimisticCommand(input: {
    project_id: number;
    name: string;
    command: string;
  }): number | null {
    const project = selectedProject;
    if (!project || project.id !== input.project_id) return null;
    const id = nextOptimisticProcessId--;
    optimisticProcesses = [
      ...optimisticProcesses,
      createOptimisticProcess({
        id,
        project,
        kind: 'command',
        name: input.name,
        command: input.command,
        retry: 'command'
      })
    ];
    dialog = null;
    activeWorktreeOperationId = null;
    settingsOpen = false;
    todoBrowserOpen = false;
    selection = projectTreeSelection('command', id, project.id, input.name);
    return id;
  }

  function failPendingProcess(cause: unknown, optimisticId: number): void {
    optimisticProcesses = optimisticProcesses.map((optimistic) =>
      optimistic.process.id === optimisticId
        ? failOptimisticProcess(optimistic, cause)
        : optimistic
    );
  }

  function dismissOptimisticProcess(optimisticId: number): void {
    optimisticProcesses = optimisticProcesses.filter(
      (optimistic) => optimistic.process.id !== optimisticId
    );
    if (selection?.id === optimisticId && isProcessSelection(selection)) selection = null;
  }

  function retryOptimisticProcess(optimistic: OptimisticProcess): void {
    const retry = optimistic.retry;
    const tool = optimistic.process.agent_tool_id === null
      ? null
      : agentTools.find((candidate) => candidate.id === optimistic.process.agent_tool_id) ?? null;
    dismissOptimisticProcess(optimistic.process.id);
    if (retry === 'agent' && tool) void spawnAgent(tool);
    else if (retry === 'command') dialog = 'command';
    else if (retry === 'agent') void openAgentDialog();
  }

  async function openAgentDialog(): Promise<void> {
    dialog = 'agent';
    agentToolsLoading = true;
    try {
      agentTools = (await client.listAgentTools()).filter((tool) => tool.enabled);
    } catch (cause) {
      reportError(cause);
    } finally {
      agentToolsLoading = false;
    }
  }

  async function spawnAgent(tool: AgentTool): Promise<void> {
    const project = selectedProject;
    if (!project) return;
    const optimisticId = nextOptimisticProcessId--;
    const optimistic = createOptimisticProcess({
      id: optimisticId,
      project,
      kind: 'agent',
      name: tool.name,
      command: tool.command,
      agentToolId: tool.id,
      retry: 'agent'
    });
    optimisticProcesses = [...optimisticProcesses, optimistic];
    dialog = null;
    activeWorktreeOperationId = null;
    settingsOpen = false;
    todoBrowserOpen = false;
    selection = projectTreeSelection('agent', optimisticId, project.id, tool.name);
    await tick();
    try {
      const result = await client.spawnAgent({
        project_id: project.id,
        agent_tool_id: tool.id,
        extra_args: []
      });
      await refreshProcesses(project.id);
      const process = processes.find((candidate) => candidate.id === result.process_id);
      optimisticProcesses = optimisticProcesses.filter(
        (candidate) => candidate.process.id !== optimisticId
      );
      if (process && selectedProject?.id === project.id) {
        selection = projectTreeSelection('agent', process.id, process.project_id, process.name);
      }
    } catch (cause) {
      failPendingProcess(cause, optimisticId);
    }
  }

  async function createTodo(): Promise<void> {
    if (!selectedProject || !todoTitle.trim()) return;
    detailBusy = true;
    const input: NewTodoInput = {
      title: todoTitle.trim(),
      body: todoBody.trim(),
      priority: todoPriority,
      tags: todoTags.split(',').map((tag) => tag.trim()).filter(Boolean)
    };
    try {
      const todo = await client.coordinationTodoCreate(selectedProject.id, input);
      resetTodoForm();
      dialog = null;
      await refreshCoordination(selectedProject.id, false);
      await selectTreeItem(projectTreeSelection('todo', todo.id, todo.project_id, todo.title));
    } catch (cause) {
      reportError(cause);
    } finally {
      detailBusy = false;
    }
  }

  async function createScratchpad(): Promise<void> {
    if (!selectedProject) return;
    detailBusy = true;
    try {
      const scratchpad = await client.coordinationScratchpadCreate(selectedProject.id, {
        name: 'Unnamed scratchpad', content: '', tags: []
      });
      dialog = null;
      await refreshCoordination(selectedProject.id, false);
      await selectTreeItem(
        projectTreeSelection('scratchpad', scratchpad.id, scratchpad.project_id, scratchpad.name)
      );
      scratchpadFocusRequest += 1;
    } catch (cause) {
      reportError(cause);
    } finally {
      detailBusy = false;
    }
  }

  async function saveScratchpad(content: string, expectedRevision: number): Promise<ScratchpadRead> {
    if (!selectedProject || selection?.kind !== 'scratchpad') {
      throw new Error('Select a scratchpad before saving');
    }
    const projectId = selectedProject.id;
    const scratchpadId = selection.id;
    const saved = await client.coordinationScratchpadUpdate(
      projectId,
      scratchpadId,
      expectedRevision,
      content
    );
    setTimeout(() => {
      if (
        selectedProject?.id === projectId &&
        selection?.kind === 'scratchpad' &&
        selection.id === scratchpadId
      ) {
        scratchpadRead = saved;
        selection = projectTreeSelection(
          'scratchpad',
          scratchpadId,
          projectId,
          saved.scratchpad.name
        );
      }
      void refreshCoordination(projectId, false);
    }, 0);
    return saved;
  }

  async function completeTodo(completed: boolean): Promise<void> {
    if (!selectedProject || selection?.kind !== 'todo') return;
    detailBusy = true;
    try {
      await client.coordinationTodoComplete(selectedProject.id, selection.id, completed);
      await Promise.all([loadTodo(selection.id), refreshCoordination(selectedProject.id, false)]);
    } catch (cause) {
      reportError(cause);
    } finally {
      detailBusy = false;
    }
  }

  async function commentTodo(body: string): Promise<void> {
    if (!selectedProject || selection?.kind !== 'todo') return;
    detailBusy = true;
    try {
      await client.coordinationTodoComment(selectedProject.id, selection.id, body);
      await Promise.all([loadTodo(selection.id), refreshCoordination(selectedProject.id, false)]);
    } catch (cause) {
      reportError(cause);
    } finally {
      detailBusy = false;
    }
  }

  function resetTodoForm(): void {
    todoTitle = '';
    todoBody = '';
    todoPriority = 'medium';
    todoTags = '';
  }

  function clearSelection(): void {
    selection = null;
    todoDetail = null;
    scratchpadRead = null;
    todoBrowserOpen = false;
    activeWorktreeOperationId = null;
  }

  function openTodosBrowser(): void {
    if (!selectedProject) return;
    settingsOpen = false;
    todoBrowserOpen = true;
    activeWorktreeOperationId = null;
    selection = null;
    todoDetail = null;
    scratchpadRead = null;
  }

  function selectProcessById(processId: number): void {
    const process = treeProcesses.find((candidate) => candidate.id === processId);
    if (process) void selectTreeItem(projectTreeSelection(process.kind, process.id, process.project_id, processLabel(process)));
  }

  function processLabel(process: ProcessView): string {
    if (process.kind !== 'terminal') return process.name;
    const parts = process.working_dir.split('/').filter(Boolean);
    return parts[0] === 'Users' && parts.length > 2 ? `~/${parts.slice(2).join('/')}` : process.working_dir;
  }

  async function openTrustReview(process: ProcessView): Promise<void> {
    processBusyId = process.id;
    try {
      trustReview = await client.trustReview(process.id);
    } catch (cause) {
      reportError(cause);
    } finally {
      processBusyId = null;
    }
  }

  async function approveTrust(): Promise<void> {
    if (!trustReview || !selectedProject) return;
    const review = trustReview;
    trustBusy = true;
    try {
      await client.trustYmlProcess(review.process_id, review.expected_hash);
      trustReview = null;
      await refreshProcesses(selectedProject.id);
      const process = processes.find((candidate) => candidate.id === review.process_id);
      if (process) await startProcess(process);
    } catch (cause) {
      reportError(cause);
    } finally {
      trustBusy = false;
    }
  }

  async function registerProject(): Promise<void> {
    const path = await open({ directory: true, multiple: false, title: 'Register a project folder' });
    if (typeof path !== 'string') return;
    busy = true;
    try {
      projects = await client.register(path);
      await refreshWorktreeMetadata(projects, false, true);
    } catch (cause) {
      reportError(cause);
    } finally {
      busy = false;
    }
  }

  async function openWorktreeDialog(
    mode: 'create' | 'fork' | 'adopt',
    sourceProject: Project
  ): Promise<void> {
    const root = rootProjectFor(sourceProject) ?? sourceProject;
    if (root.repository_id === null) {
      reportError(new Error(`${projectLabel(sourceProject)} is not linked to a Git worktree repository`));
      return;
    }
    if (!worktreeLists[root.repository_id]) {
      await refreshWorktreeMetadata(projects, false, true, root.repository_id);
    }
    const list = worktreeLists[root.repository_id];
    if (!list) {
      reportError(new Error(`Could not load worktrees for ${projectLabel(root)}`));
      return;
    }
    worktreeDialogError = null;
    branchOptions = [];
    worktreeDialog = {
      mode,
      sourceProject: mode === 'create' || mode === 'adopt' ? root : sourceProject,
      repository: list.repository,
      sourceEntry: mode === 'fork' ? worktreeEntryFor(sourceProject) : null
    };
  }

  function closeWorktreeDialog(): void {
    if (worktreeDialogBusy) return;
    worktreeDialog = null;
    worktreeDialogError = null;
    branchOptions = [];
  }

  async function loadOriginBranches(): Promise<void> {
    const state = worktreeDialog;
    if (!state || originBranchesLoading) return;
    originBranchesLoading = true;
    worktreeDialogError = null;
    try {
      const response = await client.originWorktreeBranches(state.sourceProject.id);
      branchOptions = response.options
        ?? response.branches.map((name) => ({ name, source: 'origin' as const }));
    } catch (cause) {
      worktreeDialogError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      originBranchesLoading = false;
    }
  }

  async function submitWorktreeDialog(submission: WorktreeDialogSubmission): Promise<void> {
    const state = worktreeDialog;
    if (!state || worktreeDialogBusy) return;
    worktreeDialogBusy = true;
    worktreeDialogError = null;
    const operationId = crypto.randomUUID();
    const operation = beginWorktreeOperation({
      id: operationId,
      mode: submission.mode,
      sourceProjectId: state.sourceProject.id,
      repositoryId: state.repository.id,
      branch: submission.mode === 'adopt' ? null : submission.branch,
      path: submission.mode === 'adopt' ? submission.path : null
    });
    collapsedRepositories = { ...collapsedRepositories, [state.repository.id]: false };
    persistRepositoryCollapse();
    worktreeDialog = null;
    branchOptions = [];
    showWorktreeOperation(operation);
    await tick();
    try {
      if (submission.mode === 'create') {
        await client.createWorktreeAsync(operationId, {
            project_id: state.sourceProject.id,
            branch: submission.branch,
            from_ref: submission.fromRef,
            env_policy: submission.envPolicy,
            remember_env_policy: submission.rememberEnvPolicy
        });
      } else if (submission.mode === 'fork') {
        await client.forkWorktreeAsync(operationId, {
              project_id: state.sourceProject.id,
              branch: submission.branch,
              env_policy: submission.envPolicy,
              remember_env_policy: submission.rememberEnvPolicy
        });
      } else {
        await client.adoptWorktreeAsync(operationId, submission.path);
      }
    } catch (cause) {
      failWorktreeOperation(
        operationId,
        cause instanceof Error ? cause.message : String(cause)
      );
    } finally {
      worktreeDialogBusy = false;
    }
  }

  function openRemoveWorktree(project: Project): void {
    const repository = worktreeRepositoryFor(project);
    const entry = worktreeEntryFor(project);
    if (!repository || !entry?.can_remove) {
      reportError(new Error('Only a Workman-managed linked worktree can be removed here'));
      return;
    }
    removeWorktreeError = null;
    removeWorktreeDialog = { project, repository, entry };
  }

  async function confirmRemoveWorktree(forceDirty: boolean, confirmBranch?: string): Promise<void> {
    const state = removeWorktreeDialog;
    if (!state || removeWorktreeBusy) return;
    removeWorktreeBusy = true;
    removeWorktreeError = null;
    try {
      await client.removeWorktree({
        project_id: state.project.id,
        confirm_remove: true,
        confirm_stop_running: true,
        force_dirty: forceDirty,
        confirm_branch: confirmBranch
      });
      removeWorktreeDialog = null;
      projects = await client.projects();
      await refreshWorktreeMetadata(projects, true, true, state.repository.id);
      const root = projects.find((project) =>
        project.repository_id === state.repository.id && project.parent_project_id === null
      );
      if (root) appNavigation.navigate({ type: 'project', projectId: root.id }, 'api');
    } catch (cause) {
      removeWorktreeError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      removeWorktreeBusy = false;
    }
  }

  async function adoptImportPath(path: string, navigate = true): Promise<number | null> {
    if (!importOffer) return null;
    const repositoryId = importOffer.repository.id;
    importError = null;
    const offer = importOffer;
    const remaining = offer.entries.filter((entry) => entry.path !== path);
    importOffer = remaining.length > 0 ? { ...offer, entries: remaining } : null;
    void startAdoptOperation(path, repositoryId, navigate);
    return null;
  }

  async function adoptAllImports(): Promise<void> {
    const offer = importOffer;
    if (!offer || importBusyPath) return;
    importBusyPath = '*';
    importError = null;
    importOffer = null;
    for (const [index, entry] of offer.entries.entries()) {
      void startAdoptOperation(
        entry.path,
        offer.repository.id,
        index === offer.entries.length - 1
      );
    }
    importBusyPath = null;
  }

  async function startAdoptOperation(
    path: string,
    repositoryId: number,
    navigate: boolean
  ): Promise<void> {
    const source = projects.find((project) =>
      project.repository_id === repositoryId && project.parent_project_id === null
    );
    const operationId = crypto.randomUUID();
    const operation = beginWorktreeOperation({
      id: operationId,
      mode: 'adopt',
      sourceProjectId: source?.id ?? null,
      repositoryId,
      path
    });
    collapsedRepositories = { ...collapsedRepositories, [repositoryId]: false };
    persistRepositoryCollapse();
    if (navigate) showWorktreeOperation(operation);
    await tick();
    try {
      await client.adoptWorktreeAsync(operationId, path);
    } catch (cause) {
      failWorktreeOperation(
        operationId,
        cause instanceof Error ? cause.message : String(cause)
      );
    }
  }

  function selectProject(project: Project): void {
    if (project.selected) {
      activeWorktreeOperationId = null;
      clearSelection();
      return;
    }
    if (busy || projectReorderBusy) return;
    appNavigation.navigate({ type: 'project', projectId: project.id }, 'project-rail');
  }

  function handleProjectDrop(drop: ReorderDrop): void {
    const orderedIds = moveTreeOrderBlock(
      projects.map((project) => ({
        id: project.id,
        parentId: project.parent_project_id ?? null
      })),
      drop.sourceId,
      drop.targetId,
      drop.placement
    );
    void persistProjectOrder(orderedIds);
  }

  function moveProjectFromKeyboard(projectId: number, direction: ReorderDirection): void {
    const order = projects.map((project) => ({
      id: project.id,
      parentId: project.parent_project_id ?? null
    }));
    const targetId = siblingTarget(order, projectId, direction);
    if (targetId === null) return;
    handleProjectDrop({
      sourceId: projectId,
      targetId,
      placement: direction < 0 ? 'before' : 'after'
    });
  }

  async function persistProjectOrder(orderedIds: number[]): Promise<void> {
    const currentIds = projects.map((project) => project.id);
    if (projectReorderBusy || orderedIds.join(',') === currentIds.join(',')) return;
    const previous = projects;
    const byId = new Map(previous.map((project) => [project.id, project]));
    projects = orderedIds.map((id, sortOrder) => ({ ...byId.get(id)!, sort_order: sortOrder }));
    projectReorderBusy = true;
    try {
      projects = await client.reorderProjects(orderedIds);
    } catch (cause) {
      projects = previous;
      reportError(cause);
    } finally {
      projectReorderBusy = false;
    }
  }

  async function persistProcessOrder(kind: ProcessView['kind'], orderedIds: number[]): Promise<void> {
    if (!selectedProject || processReorderBusy) return;
    const currentIds = processes
      .filter((process) => process.kind === kind)
      .map((process) => process.id);
    if (orderedIds.join(',') === currentIds.join(',')) return;

    const projectId = selectedProject.id;
    const previous = processes;
    const byId = new Map(previous.map((process) => [process.id, process]));
    const reordered = orderedIds.map((id, sortOrder) => ({
      ...byId.get(id)!,
      sort_order: sortOrder
    }));
    let groupIndex = 0;
    const optimistic = previous.map((process) =>
      process.kind === kind ? reordered[groupIndex++] : process
    );
    applyProcesses(optimistic);
    cacheProjectProcesses(projectId, optimistic);
    processReorderBusy = true;
    try {
      const next = await client.reorderProcesses(projectId, kind, orderedIds);
      if (selectedProject?.id === projectId) applyProcesses(next);
      cacheProjectProcesses(projectId, next);
    } catch (cause) {
      if (selectedProject?.id === projectId) applyProcesses(previous);
      cacheProjectProcesses(projectId, previous);
      reportError(cause);
    } finally {
      processReorderBusy = false;
    }
  }

  function beginRename(project: Project): void {
    renameId = project.id;
    renameValue = project.display_name ?? project.name;
  }

  function cancelRename(): void {
    renameId = null;
    renameValue = '';
  }

  async function commitRename(): Promise<void> {
    if (renameId === null || !renameValue.trim()) return;
    const projectId = renameId;
    const name = renameValue.trim();
    cancelRename();
    try {
      projects = await client.rename(projectId, name);
    } catch (cause) {
      reportError(cause);
    }
  }

  function showContextMenu(request: ContextMenuRequest): void {
    treeRenameTarget = null;
    contextRequest = request;
  }

  function projectContextTarget(project: Project): Extract<ContextMenuTarget, { kind: 'project' }> {
    return {
      kind: 'project',
      project,
      repository: worktreeRepositoryFor(project),
      worktree: worktreeEntryFor(project)
    };
  }

  function showProjectPointerMenu(event: MouseEvent, project: Project): void {
    showContextMenu(contextMenuRequest(event, projectContextTarget(project)));
  }

  function showProjectKeyboardMenu(event: KeyboardEvent, project: Project): void {
    const request = keyboardContextMenuRequest(event, projectContextTarget(project));
    if (request) showContextMenu(request);
  }

  function closeContextMenu(): void {
    const restoreFocus = contextRequest?.restoreFocus ?? null;
    contextRequest = null;
    if (restoreFocus) {
      queueMicrotask(() => {
        if (restoreFocus.isConnected) restoreFocus.focus();
      });
    }
  }

  function selectedContextTarget(): ContextMenuTarget | null {
    if (!selection) return null;
    if (selectedProcess) {
      return { kind: 'process', process: selectedProcess, selection };
    }
    if (selection.kind === 'todo') {
      const todo = coordination?.todos.find((candidate) => candidate.id === selection?.id)
        ?? todoDetail?.todo;
      return todo ? { kind: 'todo', todo, selection } : null;
    }
    if (selection.kind === 'scratchpad') {
      const scratchpad = coordination?.scratchpads.find(
        (candidate) => candidate.id === selection?.id
      ) ?? scratchpadRead?.scratchpad;
      return scratchpad ? { kind: 'scratchpad', scratchpad, selection } : null;
    }
    return null;
  }

  function showViewerContextMenu(event: MouseEvent): void {
    const target = selectedContextTarget();
    if (target) showContextMenu(contextMenuRequest(event, target));
  }

  async function runContextAction(action: ContextActionId): Promise<void> {
    const request = contextRequest;
    contextRequest = null;
    if (!request) return;
    const target = request.target;

    try {
      if (target.kind === 'project') {
        await runProjectContextAction(action, target);
      } else if (target.kind === 'process') {
        await runProcessContextAction(action, target);
      } else if (target.kind === 'todo') {
        await runTodoContextAction(action, target);
      } else {
        await runScratchpadContextAction(action, target);
      }
    } catch (cause) {
      reportError(cause);
    }
  }

  async function runProjectContextAction(
    action: ContextActionId,
    target: Extract<ContextMenuTarget, { kind: 'project' }>
  ): Promise<void> {
    const project = target.project;
    switch (action) {
      case 'select':
        appNavigation.navigate({ type: 'project', projectId: project.id }, 'context-menu');
        return;
      case 'rename':
        beginRename(project);
        return;
      case 'new-agent':
        if (!(await activateProject(project.id))) return;
        await openAgentDialog();
        return;
      case 'new-terminal':
        if (!(await activateProject(project.id))) return;
        await spawnTerminal();
        return;
      case 'add-command':
        if (!(await activateProject(project.id))) return;
        settingsOpen = false;
        dialog = 'command';
        return;
      case 'new-todo':
        if (!(await activateProject(project.id))) return;
        settingsOpen = false;
        resetTodoForm();
        dialog = 'todo';
        return;
      case 'new-scratchpad':
        if (!(await activateProject(project.id))) return;
        settingsOpen = false;
        await createScratchpad();
        return;
      case 'new-worktree':
        await openWorktreeDialog('create', project);
        return;
      case 'adopt-worktree':
        await openWorktreeDialog('adopt', project);
        return;
      case 'fork-worktree':
        await openWorktreeDialog('fork', project);
        return;
      case 'remove-worktree':
        openRemoveWorktree(project);
        return;
      case 'refresh-worktrees':
      case 'refresh-pull-request':
        await refreshWorktreeRepository(project, true);
        return;
      case 'open-pull-request':
        if (target.worktree?.pull_request?.url) {
          window.open(target.worktree.pull_request.url, '_blank', 'noopener,noreferrer');
        }
        return;
      case 'open-herd-site':
        if (target.worktree?.site_url) {
          window.open(target.worktree.site_url, '_blank', 'noopener,noreferrer');
        }
        return;
      case 'start-all-commands':
      case 'stop-all-commands': {
        const method = action === 'start-all-commands'
          ? 'process.start_all_commands'
          : 'process.stop_all_commands';
        const result = await client.control<{
          failures: Array<{ process_id: number; message: string }>;
        }>(method, { project_id: project.id });
        if (selectedProject?.id === project.id) await refreshProcesses(project.id);
        if (result.failures.length > 0) {
          throw new Error(result.failures.map((failure) => failure.message).join('; '));
        }
        return;
      }
      case 'remove-project':
        if (!window.confirm(`Remove ${projectLabel(project)} from Workman? Files stay on disk.`)) return;
        await client.control('projects.remove', {
          project_id: project.id,
          confirm_remove: true
        });
        projects = await client.projects();
        return;
      case 'open-in-editor':
        await openProjectEditor(project.path, $openerSettings);
        return;
      case 'open-in-finder':
        await openProjectFinder(project.path);
        return;
      case 'open-custom':
        await openProjectCustom(project.path, $openerSettings);
        return;
      case 'copy-path':
        await navigator.clipboard.writeText(project.path);
        return;
      default:
        return;
    }
  }

  async function runProcessContextAction(
    action: ContextActionId,
    target: Extract<ContextMenuTarget, { kind: 'process' }>
  ): Promise<void> {
    const process = target.process;
    switch (action) {
      case 'start':
        if (process.source === 'yml' && process.trust_hash === null) {
          await openTrustReview(process);
        } else {
          await startProcess(process);
        }
        return;
      case 'stop':
        await client.stopProcess(process.id);
        await refreshProcesses(process.project_id);
        return;
      case 'restart':
        await client.restartProcess(process.id);
        await refreshProcesses(process.project_id);
        return;
      case 'kill':
        if (!window.confirm(`Kill ${process.name} immediately? Unsaved terminal state may be lost.`)) return;
        await client.control('process.kill', { process_id: process.id, confirm_kill: true });
        await refreshProcesses(process.project_id);
        return;
      case 'close':
        if (!window.confirm(`Close ${process.name}? Its saved terminal entry will be removed.`)) return;
        await client.closeProcess(process.id);
        if (selection?.id === process.id && isProcessSelection(selection)) clearSelection();
        await refreshProcesses(process.project_id);
        return;
      case 'rename':
        treeRenameTarget = target;
        return;
      case 'copy-name':
        await navigator.clipboard.writeText(process.name);
        return;
      case 'copy-id':
        await navigator.clipboard.writeText(String(process.id));
        return;
      case 'send-prompt':
        appNavigation.navigate({ type: 'item', selection: target.selection }, 'context-menu');
        setTimeout(() => focusTerminalInput(process.id), 80);
        return;
      case 'view-parent': {
        const parent = processes.find(
          (candidate) => candidate.id === process.spawned_by_process_id
        );
        if (!parent) throw new Error('The parent agent is no longer open');
        appNavigation.navigate(
          {
            type: 'item',
            selection: projectTreeSelection(
              parent.kind,
              parent.id,
              parent.project_id,
              processLabel(parent)
            )
          },
          'context-menu'
        );
        return;
      }
      case 'mark-read':
        await markAgentRead(process.id, process.project_id);
        return;
      case 'reveal-config':
        await openWorkspacePath(`${projectForProcess(process)?.path ?? process.working_dir}/workman.yml`, 'reveal');
        return;
      default:
        return;
    }
  }

  async function runTodoContextAction(
    action: ContextActionId,
    target: Extract<ContextMenuTarget, { kind: 'todo' }>
  ): Promise<void> {
    if (action === 'copy-title') {
      await navigator.clipboard.writeText(target.todo.title);
      return;
    }
    if (action !== 'complete-todo' && action !== 'reopen-todo') return;
    const completed = action === 'complete-todo';
    await client.coordinationTodoComplete(
      target.selection.projectId,
      target.todo.id,
      completed
    );
    await refreshCoordination(target.selection.projectId, false);
    if (selection?.kind === 'todo' && selection.id === target.todo.id) {
      await loadTodo(target.todo.id);
    }
  }

  async function runScratchpadContextAction(
    action: ContextActionId,
    target: Extract<ContextMenuTarget, { kind: 'scratchpad' }>
  ): Promise<void> {
    if (action === 'rename') {
      treeRenameTarget = target;
      return;
    }
    if (action !== 'archive-scratchpad' && action !== 'delete-scratchpad') return;
    if (action === 'delete-scratchpad'
      && !window.confirm(`Delete ${target.scratchpad.name}? This cannot be undone.`)) return;

    const method = action === 'archive-scratchpad'
      ? 'coordination.scratchpad_archive'
      : 'coordination.scratchpad_delete';
    await client.control(method, {
      project_id: target.selection.projectId,
      scratchpad_id: target.scratchpad.id,
      expected_revision: target.scratchpad.revision
    });
    if (selection?.kind === 'scratchpad' && selection.id === target.scratchpad.id) clearSelection();
    await refreshCoordination(target.selection.projectId, false);
  }

  async function commitTreeRename(name: string): Promise<void> {
    const target = treeRenameTarget;
    treeRenameTarget = null;
    if (!target) return;
    try {
      if (target.kind === 'process') {
        const process = await client.control<ProcessView>('process.rename', {
          process_id: target.process.id,
          name
        });
        await refreshProcesses(process.project_id);
        if (selection?.id === process.id && isProcessSelection(selection)) {
          selection = projectTreeSelection(
            process.kind,
            process.id,
            process.project_id,
            processLabel(process)
          );
        }
      } else if (target.kind === 'scratchpad') {
        await client.control('coordination.scratchpad_rename', {
          project_id: target.selection.projectId,
          scratchpad_id: target.scratchpad.id,
          name,
          expected_revision: target.scratchpad.revision
        });
        await refreshCoordination(target.selection.projectId, false);
        if (selection?.kind === 'scratchpad' && selection.id === target.scratchpad.id) {
          selection = { ...selection, label: name };
          await loadScratchpad(target.scratchpad.id);
        }
      }
    } catch (cause) {
      reportError(cause);
    }
  }

  function projectForProcess(process: ProcessView): Project | undefined {
    return projects.find((project) => project.id === process.project_id);
  }

  function focusRename(node: HTMLInputElement): void {
    queueMicrotask(() => { node.focus(); node.select(); });
  }

  function focusDialogInput(node: HTMLInputElement): void {
    queueMicrotask(() => node.focus());
  }

  function projectLabel(project: Project): string {
    return project.display_name ?? project.name;
  }

  function projectRailLabel(project: Project, nested = false): string {
    return nested ? projectBranchLabel(project) : projectLabel(project);
  }

  function projectTitle(project: Project): string {
    return projectRepositoryTitle(project, worktreeRepositoryFor(project));
  }

  function projectReorderGroup(project: Project): string {
    return project.parent_project_id === null
      ? 'project-roots'
      : `project-children:${project.parent_project_id}`;
  }

  function repositoryCollapsed(repositoryId: number | null): boolean {
    return repositoryId !== null && collapsedRepositories[repositoryId] === true;
  }

  function toggleRepository(repositoryId: number): void {
    collapsedRepositories = {
      ...collapsedRepositories,
      [repositoryId]: !collapsedRepositories[repositoryId]
    };
    persistRepositoryCollapse();
  }

  function persistRepositoryCollapse(): void {
    try {
      localStorage.setItem(worktreeCollapseStorageKey, JSON.stringify(collapsedRepositories));
    } catch {
      // Collapsing stays functional when webview storage is unavailable.
    }
  }

  function persistProjectRail(): void {
    savePanelPreference('project-rail', { collapsed: projectRailCollapsed, width: projectRailWidth });
  }

  function persistTreeRail(): void {
    savePanelPreference('section-rail', { collapsed: treeRailCollapsed, width: treeRailWidth });
  }

  function toggleProjectRail(): void {
    projectRailCollapsed = !projectRailCollapsed;
    persistProjectRail();
  }

  function toggleTreeRail(): void {
    if (!selectedProject) return;
    treeRailCollapsed = !treeRailCollapsed;
    persistTreeRail();
  }

  function resizeRailFromKeyboard(event: KeyboardEvent, rail: 'project' | 'tree'): void {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    event.preventDefault();
    const delta = event.key === 'ArrowLeft' ? -12 : 12;
    if (rail === 'project') {
      projectRailWidth = clampPanelWidth(projectRailWidth + delta, projectRailBounds.min, projectRailBounds.max);
      persistProjectRail();
    } else {
      treeRailWidth = clampPanelWidth(treeRailWidth + delta, treeRailBounds.min, treeRailBounds.max);
      persistTreeRail();
    }
  }

  function reportError(cause: unknown): void {
    if (isUnsupportedControlMethod(cause)) return;
    error = cause instanceof Error ? cause.message : String(cause);
  }

  async function restartOutdatedDaemon(): Promise<void> {
    if (versionRestarting) return;
    if (!window.confirm('Restart Workman daemon? All running project processes will stop.')) return;
    versionRestarting = true;
    try {
      await client.restartDaemon();
    } catch (cause) {
      versionRestarting = false;
      reportError(cause);
    }
  }

  async function applyAvailableUpdate(): Promise<void> {
    if (versionRestarting || !startupUpdate?.check.available) return;
    if (!window.confirm('Update Workman and restart the daemon? All running project processes will stop.')) return;
    versionRestarting = true;
    try {
      const report = await applyUpdate(client);
      startupUpdate = null;
      if (report.desktop_instruction) error = report.desktop_instruction;
    } catch (cause) {
      versionRestarting = false;
      reportError(cause);
    }
  }
</script>

<svelte:window onkeydown={handleShortcut} />

<svelte:head>
  <title>{windowTitle}</title>
</svelte:head>

{#if showVersionBanner}
  <section class="version-banner" aria-live="assertive">
    <div>
      <strong>{versionSkew ? 'Workman daemon is running an older version' : `Workman ${startupUpdate?.check.latest} is available`}</strong>
      <span>{versionSkew ? 'Restarting loads this app’s control protocol and agent config.' : 'The release is downloaded and SHA256 verified before workman and workmand are replaced.'} All running project processes will stop.</span>
    </div>
    <small>{versionSkew ? `app ${connection.app_build_id || 'current'} · daemon ${connection.daemon_build_id ?? 'legacy'}` : `current ${startupUpdate?.check.current} · latest ${startupUpdate?.check.latest}`}</small>
    <Button class="border-warning/50 text-warning hover:bg-warning/10" size="sm" variant="outline" disabled={versionRestarting} onclick={() => void (versionSkew ? restartOutdatedDaemon() : applyAvailableUpdate())}>
      {versionRestarting ? 'Restarting daemon…' : versionSkew ? 'Restart daemon' : 'Update now'}
    </Button>
  </section>
{/if}

<AgentDoneToasts
  notices={agentDoneNotices}
  onOpen={openAgentDoneNotice}
  onDismiss={(id) => (agentDoneNotices = agentDoneNotices.filter((notice) => notice.id !== id))}
/>

{#snippet projectRailRow(project: Project, nested: boolean, group: ProjectRailGroup)}
  {@const repository = worktreeRepositoryFor(project)}
  {@const worktree = worktreeEntryFor(project)}
  {@const rowLabel = projectRailLabel(project, nested)}
  {@const fullTitle = projectTitle(project)}
  {@const projectKind = nested ? 'worktree' : project.repository_id !== null ? 'repository' : 'project'}
  {@const hasRepositoryChildren = group.grouped || worktreeOperationsFor(group.repositoryId).length > 0}
  {@const unreadAgentCount = projectUnreadAgentCount(project.id)}
  <article
    class:active={project.selected}
    class:nested
    class:repository-root={!nested && project.repository_id !== null}
    class="project-row group/project group/repository"
  >
    {#if !nested && hasRepositoryChildren && group.repositoryId !== null && !projectRailCollapsed}
      <IconButton
        class="ml-0.5 size-7 shrink-0 rounded-none border-r border-border"
        label={`${repositoryCollapsed(group.repositoryId) ? 'Expand' : 'Collapse'} worktrees under ${repository?.name ?? rowLabel}`}
        aria-expanded={!repositoryCollapsed(group.repositoryId)}
        onclick={() => toggleRepository(group.repositoryId!)}
      >
        {#snippet icon()}
          {#if repositoryCollapsed(group.repositoryId)}<ChevronRightIcon size={13} />{:else}<ChevronDownIcon size={13} />{/if}
        {/snippet}
      </IconButton>
    {/if}
    {#if renameId === project.id}
      <form class="rename-form" onsubmit={(event) => { event.preventDefault(); void commitRename(); }}>
        <input aria-label="Project name" bind:value={renameValue} use:focusRename onkeydown={(event) => { if (event.key === 'Escape') cancelRename(); }} />
        <Button size="sm" type="submit">Save</Button>
      </form>
    {:else}
      <TooltipLabel label={fullTitle} side={projectRailCollapsed ? 'right' : 'top'}>
        <button
          class="project-select"
          type="button"
          aria-current={project.selected ? 'page' : undefined}
          aria-label={`${fullTitle} · ${projectKind} · ${project.status}${unreadAgentCount > 0 ? ` · ${unreadAgentCount} unread agents` : ''}`}
          use:reorderItem={{
            id: project.id,
            group: projectReorderGroup(project),
            disabled: busy || projectReorderBusy || renameId !== null || projects.length < 2,
            label: fullTitle,
            onDrop: handleProjectDrop,
            onKeyboardMove: moveProjectFromKeyboard
          }}
          onclick={() => selectProject(project)}
          oncontextmenu={(event) => showProjectPointerMenu(event, project)}
          onkeydown={(event) => showProjectKeyboardMenu(event, project)}
          data-context-kind="project"
          data-context-id={project.id}
        >
          <StatusIndicator
            class={projectRailCollapsed ? 'project-status-badge' : ''}
            tone={project.status === 'error' ? 'danger' : project.status === 'running' ? 'success' : 'neutral'}
            label={`${fullTitle} · ${project.status}`}
          />
          <span class="project-icon-anchor">
            <span class="project-kind-icon" aria-hidden="true">
              {#if nested}<GitBranchIcon size={15} strokeWidth={1.8} />{:else if project.repository_id !== null}<FolderGit2Icon size={15} strokeWidth={1.8} />{:else}<FolderIcon size={15} strokeWidth={1.8} />{/if}
            </span>
          </span>
          <span class="project-copy">
            <strong>{rowLabel}</strong>
            <small>{project.path}</small>
          </span>
          {#if unreadAgentCount > 0}
            <TooltipLabel label={`${unreadAgentCount} unread finished agent${unreadAgentCount === 1 ? '' : 's'} in ${fullTitle}`}>
              <span class="project-unread-rollup" aria-label={`${unreadAgentCount} unread agents`}>
                <span aria-hidden="true"></span>{unreadAgentCount}
              </span>
            </TooltipLabel>
          {/if}
        </button>
      </TooltipLabel>
      {#if repository && !projectRailCollapsed}
        <WorktreeRowMeta
          entry={worktree}
          repositoryName={repository.name}
          refreshing={worktreeRefreshingRepositoryId === repository.id}
          showRefresh={!nested}
          onRefresh={() => void refreshWorktreeRepository(project, true)}
        />
      {/if}
      <ProjectOpeners
        path={project.path}
        projectName={fullTitle}
        collapsed={projectRailCollapsed}
        siteUrl={worktree?.site_url ?? null}
        onError={reportError}
      />
      <IconButton
        class="project-actions size-7 opacity-0 group-hover/project:opacity-100 focus-visible:opacity-100"
        label={`Actions for ${fullTitle}`}
        onclick={(event) => {
          const bounds = event.currentTarget.getBoundingClientRect();
          showContextMenu({
            target: projectContextTarget(project),
            x: bounds.right,
            y: bounds.bottom,
            restoreFocus: event.currentTarget
          });
        }}
      >
        {#snippet icon()}<MoreHorizontalIcon size={14} />{/snippet}
      </IconButton>
    {/if}
  </article>
{/snippet}

<main
  class="app-shell"
  class:no-project={selectedProject === null}
  class:with-version-banner={showVersionBanner}
  style={`--project-rail-width: ${projectRailCollapsed ? collapsedProjectRailWidth : projectRailWidth}px; --tree-rail-width: ${treeRailCollapsed ? collapsedTreeRailWidth : treeRailWidth}px;`}
>
  <aside
    class="project-rail"
    class:collapsed={projectRailCollapsed}
    aria-label="Projects"
    data-app-panel="projects"
    tabindex="-1"
  >
    <header class="brand" data-tauri-drag-region>
      <NotificationsCenter
        {notifications}
        busy={notificationBusy}
        onRefresh={() => void refreshNotifications()}
        onOpen={openNotification}
        onMarkRead={(notification) => void markCenterNotificationRead(notification)}
        onMarkAll={() => void markAllNotificationsRead()}
      />
      <div class="brand-mark" aria-hidden="true"><span></span><span></span><span></span></div>
      <div class="brand-copy"><strong>Workman</strong><span>local workspaces</span></div>
      <IconButton
        class="size-7 shrink-0 rounded border border-border bg-card"
        label={`${projectRailCollapsed ? 'Expand' : 'Collapse'} project rail`}
        shortcut="⌘B"
        onclick={toggleProjectRail}
      >
        {#snippet icon()}
          {#if projectRailCollapsed}<ChevronRightIcon size={15} />{:else}<ChevronLeftIcon size={15} />{/if}
        {/snippet}
      </IconButton>
    </header>

    <div class="rail-label"><span>Projects</span><small>{projectRailCount.toString().padStart(2, '0')}</small></div>
    <div class="project-list" aria-live="polite">
      {#if projects.length === 0 && connection.status === 'connected' && !busy}
        <div class="project-empty"><strong>No projects</strong><p>Register a folder to begin.</p><Button size="sm" onclick={() => void registerProject()}>Register folder</Button></div>
      {/if}
      {#each projectRailGroups as group (group.key)}
        {@const pendingWorktrees = worktreeOperationsFor(group.repositoryId)}
        {@render projectRailRow(group.root, false, group)}
        {#if (group.grouped || pendingWorktrees.length > 0) && (projectRailCollapsed || !repositoryCollapsed(group.repositoryId))}
          <div class="repository-children" aria-label={`${worktreeRepositoryFor(group.root)?.name ?? projectLabel(group.root)} worktrees`}>
            {#each group.children as child (child.id)}
              {@render projectRailRow(child, true, group)}
            {/each}
            {#each pendingWorktrees as operation (operation.id)}
              <WorktreeOperationRow
                {operation}
                collapsed={projectRailCollapsed}
                onSelect={() => showWorktreeOperation(operation)}
              />
            {/each}
          </div>
        {/if}
      {/each}
    </div>
    <footer class="project-footer">
      <Button class="w-full justify-center" variant="outline" size="sm" disabled={connection.status !== 'connected' || busy} onclick={() => void registerProject()}>
        <PlusIcon size={14} aria-hidden="true" /><span class="button-copy">Register project</span>
      </Button>
    </footer>
    {#if !projectRailCollapsed}
      <button
        type="button"
        class="resize-handle"
        aria-label="Resize project rail"
        title={`Resize project rail · ${projectRailWidth}px · arrow keys`}
        onkeydown={(event) => resizeRailFromKeyboard(event, 'project')}
        onpointerdown={(event) => startPanelResize(event, {
          current: projectRailWidth, min: projectRailBounds.min, max: projectRailBounds.max,
          onResize: (width) => (projectRailWidth = width), onEnd: persistProjectRail
        })}
      ></button>
    {/if}
  </aside>

  {#if selectedProject}
    <aside
      class="tree-rail"
      aria-label={`${projectTitle(selectedProject)} items`}
      data-app-panel="tree"
      tabindex="-1"
    >
      <ProjectTree
        project={selectedProject}
        {processes}
        todos={coordination?.todos ?? []}
        scratchpads={coordination?.scratchpads ?? []}
        {selection}
        collapsed={treeRailCollapsed}
        onSelect={(next) => void selectTreeItem(next)}
        onCreateTodo={() => (dialog = 'todo')}
        onBrowseTodos={openTodosBrowser}
        onAddAgent={() => void openAgentDialog()}
        onAddTerminal={() => void spawnTerminal()}
        onAddCommand={() => (dialog = 'command')}
        onAddScratchpad={() => void createScratchpad()}
        onOpenSettings={() => { todoBrowserOpen = false; settingsOpen = true; dialog = null; }}
        onToggleCollapse={toggleTreeRail}
        reordering={processReorderBusy}
        onReorderProcesses={(kind, orderedIds) => void persistProcessOrder(kind, orderedIds)}
        renameTarget={treeRenameTarget}
        onContextMenu={showContextMenu}
        onRenameSubmit={(name) => void commitTreeRename(name)}
        onRenameCancel={() => (treeRenameTarget = null)}
      />
      {#if !treeRailCollapsed}
        <button
          type="button"
          class="resize-handle"
          aria-label="Resize project tree"
          title={`Resize project tree · ${treeRailWidth}px · arrow keys`}
          onkeydown={(event) => resizeRailFromKeyboard(event, 'tree')}
          onpointerdown={(event) => startPanelResize(event, {
            current: treeRailWidth, min: treeRailBounds.min, max: treeRailBounds.max,
            onResize: (width) => (treeRailWidth = width), onEnd: persistTreeRail
          })}
        ></button>
      {/if}
    </aside>
  {/if}

  <section
    class="main-frame"
    class:empty={selectedProject === null}
    class:has-error={error !== null}
    data-app-panel="main"
    tabindex="-1"
  >
    {#if selectedProject}
      {#if error}
        <button class="error-banner" type="button" onclick={() => (error = null)}><span>{error}</span><strong>Dismiss</strong></button>
      {/if}
      <div
        class="item-viewer"
        role="region"
        aria-label={`${frameItemLabel} detail`}
        oncontextmenu={showViewerContextMenu}
      >
        {#if settingsOpen}
          <SettingsPanel {client} project={selectedProject} {connection} onError={reportError} />
        {:else if activeWorktreeOperation}
          <WorktreeProgressPanel
            operation={activeWorktreeOperation}
            onRetry={() => void retryWorktreeOperation(activeWorktreeOperation!)}
            onDismiss={dismissActiveWorktreeOperation}
          />
        {:else if selectedOptimisticProcess}
          <OptimisticProcessPanel
            kind={selectedOptimisticProcess.process.kind}
            name={selectedOptimisticProcess.process.name}
            error={selectedOptimisticProcess.error}
            onRetry={() => retryOptimisticProcess(selectedOptimisticProcess!)}
            onDismiss={() => dismissOptimisticProcess(selectedOptimisticProcess!.process.id)}
          />
        {:else if selectedProcess}
          {#key selectedProcess.id}
            <div class="terminal-view">
              <TerminalView {client} process={selectedProcess} connected={connection.status === 'connected'} onError={reportError} onUnfocus={unfocusSelectedProcess} />
              <ClaimedTodoOverlay claims={selectedProcess.claimed_todos ?? []} onOpen={openClaimedTodo} />
            </div>
          {/key}
        {:else if todoBrowserOpen}
          <TodoBrowser
            todos={coordination?.todos ?? []}
            onSelect={(todo) => void selectTreeItem(projectTreeSelection('todo', todo.id, todo.project_id, todo.title))}
            onCreate={() => (dialog = 'todo')}
          />
        {:else if selection?.kind === 'todo'}
          <TodoDetailView detail={todoDetail} loading={detailLoading} busy={detailBusy} onComplete={(completed) => void completeTodo(completed)} onComment={(body) => void commentTodo(body)} />
        {:else if selection?.kind === 'scratchpad'}
          <ScratchpadDetailView
            read={scratchpadRead}
            loading={detailLoading}
            focusRequest={scratchpadFocusRequest}
            onRefresh={() => loadScratchpad(selection?.id ?? 0, false)}
            onSave={saveScratchpad}
          />
        {:else}
          <EmptyState eyebrow="Project tree" title="Select an item" body="Choose a todo, agent, terminal, command, or scratchpad from the project tree." actionLabel="New terminal" icon="↖" onAction={() => void spawnTerminal()} />
        {/if}
      </div>
      {#if selectedProcess && !selectedOptimisticProcess}
        <ProcessStatusBar
          {client}
          project={selectedProject}
          process={selectedProcess}
          processes={treeProcesses}
          connected={connection.status === 'connected'}
          daemonPort={connection.port}
          onUnfocus={unfocusSelectedProcess}
          onSelectProcess={selectProcessById}
          onError={reportError}
        />
      {/if}
    {:else}
      <div class="onboarding">
        <span>Local workspaces</span><h1>Register a project</h1><p>Choose a repository to see its work tree.</p>
        <Button disabled={connection.status !== 'connected' || busy} onclick={() => void registerProject()}><PlusIcon size={14} />Register project</Button>
      </div>
    {/if}
  </section>
</main>

{#if contextRequest && contextMenuDescriptor}
  <ContextMenu
    x={contextRequest.x}
    y={contextRequest.y}
    title={contextMenuDescriptor.title}
    subtitle={contextMenuDescriptor.subtitle}
    items={contextMenuDescriptor.items}
    onSelect={(action) => void runContextAction(action)}
    onClose={closeContextMenu}
  />
{/if}

{#if quickJumpOpen}
  <QuickJumpPalette
    {projects}
    index={navigationIndex}
    currentProjectId={selectedProject?.id ?? null}
    {agentTools}
    recentKeys={quickJumpRecentKeys}
    loading={quickJumpLoading}
    onChoose={chooseQuickJumpTarget}
    onClose={closeQuickJump}
  />
{/if}

{#if shortcutsOpen}
  <KeyboardShortcuts onClose={closeShortcuts} />
{/if}

{#if worktreeDialog}
  <WorktreeDialog
    mode={worktreeDialog.mode}
    sourceProject={worktreeDialog.sourceProject}
    repository={worktreeDialog.repository}
    sourceEntry={worktreeDialog.sourceEntry}
    {branchOptions}
    branchesLoading={originBranchesLoading}
    busy={worktreeDialogBusy}
    error={worktreeDialogError}
    onLoadBranches={() => void loadOriginBranches()}
    onSubmit={(submission) => void submitWorktreeDialog(submission)}
    onClose={closeWorktreeDialog}
  />
{/if}

{#if removeWorktreeDialog}
  <WorktreeRemoveDialog
    repository={removeWorktreeDialog.repository}
    entry={removeWorktreeDialog.entry}
    busy={removeWorktreeBusy}
    error={removeWorktreeError}
    onConfirm={(forceDirty, confirmBranch) => void confirmRemoveWorktree(forceDirty, confirmBranch)}
    onClose={() => { if (!removeWorktreeBusy) removeWorktreeDialog = null; }}
  />
{/if}

{#if importOffer}
  <WorktreeImportDialog
    repository={importOffer.repository}
    entries={importOffer.entries}
    busyPath={importBusyPath}
    error={importError}
    onAdopt={(path) => void adoptImportPath(path)}
    onAdoptAll={() => void adoptAllImports()}
    onClose={() => { if (!importBusyPath) importOffer = null; }}
  />
{/if}

{#if dialog && dialog !== 'command'}
  <Dialog.Root open onOpenChange={(open) => { if (!open) dialog = null; }}>
    {#if dialog === 'todo'}
      <Dialog.Content class="max-w-[500px] gap-0 overflow-hidden rounded-md border border-border bg-popover p-0 shadow-2xl" showCloseButton={false} aria-label="Create todo">
      <form class="dialog-surface" onsubmit={(event) => { event.preventDefault(); void createTodo(); }}>
        <header>
          <div><span>New todo</span><h2>Add work to the tree</h2></div>
          <IconButton label="Close new todo" onclick={() => (dialog = null)}>{#snippet icon()}<XIcon size={14} />{/snippet}</IconButton>
        </header>
        <label><span>Title</span><input bind:value={todoTitle} placeholder="What needs to happen?" use:focusDialogInput /></label>
        <label><span>Notes <small>optional</small></span><textarea bind:value={todoBody} rows="4" placeholder="Outcome, constraints, or context"></textarea></label>
        <div class="dialog-row"><label><span>Priority</span><select bind:value={todoPriority}><option value="high">High</option><option value="medium">Medium</option><option value="low">Low</option></select></label><label><span>Tags</span><input bind:value={todoTags} placeholder="ui, follow-up" /></label></div>
        <footer><Button variant="outline" type="button" onclick={() => (dialog = null)}>Cancel</Button><Button type="submit" disabled={detailBusy || !todoTitle.trim()}>Create todo</Button></footer>
      </form>
      </Dialog.Content>
    {:else if dialog === 'agent'}
      <Dialog.Content class="max-w-[500px] gap-0 overflow-hidden rounded-md border border-border bg-popover p-0 shadow-2xl" showCloseButton={false} aria-label="Add agent">
      <section class="dialog-surface">
        <header>
          <div><span>New agent</span><h2>Choose an agent tool</h2></div>
          <IconButton label="Close agent picker" onclick={() => (dialog = null)}>{#snippet icon()}<XIcon size={14} />{/snippet}</IconButton>
        </header>
        <div class="agent-choices">
          {#if agentToolsLoading}<p>Loading agent tools…</p>{:else}{#each agentTools as tool (tool.id)}<button type="button" disabled={detailBusy} onclick={() => void spawnAgent(tool)}><strong>{tool.name}</strong><small>{tool.command}</small><span>Spawn</span></button>{:else}<p>No enabled agent tools. Add one in Settings.</p>{/each}{/if}
        </div>
        <footer><Button variant="outline" onclick={() => { dialog = null; todoBrowserOpen = false; settingsOpen = true; }}>Open Settings</Button><Button variant="ghost" onclick={() => (dialog = null)}>Cancel</Button></footer>
      </section>
      </Dialog.Content>
    {/if}
  </Dialog.Root>
{/if}

{#if dialog === 'command' && selectedProject}
  <AddCommandDialog
    {client}
    project={selectedProject}
    onPending={beginOptimisticCommand}
    onAdded={(process, optimisticId) => void commandAdded(process, optimisticId)}
    onFailed={failPendingProcess}
    onClose={() => (dialog = null)}
  />
{/if}

{#if trustReview}
  <TrustReviewDialog review={trustReview} busy={trustBusy} onApprove={() => void approveTrust()} onClose={() => (trustReview = null)} />
{/if}

<style>
  .app-shell { display: grid; width: 100%; height: 100%; min-height: 0; max-height: 100%; grid-template-columns: var(--project-rail-width) var(--tree-rail-width) minmax(0, 1fr); overflow: hidden; background: var(--night); }
  .app-shell.with-version-banner { height: calc(100% - 38px); }
  .app-shell.no-project { grid-template-columns: var(--project-rail-width) minmax(0, 1fr); }
  .version-banner { display: grid; width: 100%; height: 38px; grid-template-columns: minmax(0, 1fr) auto auto; align-items: center; gap: 14px; border-bottom: 1px solid color-mix(in srgb, var(--warning) 55%, var(--border)); padding: 5px 8px 5px 11px; background: color-mix(in srgb, var(--warning) 9%, var(--card)); color: var(--text); }
  .version-banner div { min-width: 0; }
  .version-banner strong, .version-banner span { display: block; }
  .version-banner strong { color: #f2d69a; font-size: var(--font-size-sm); }
  .version-banner span { overflow: hidden; margin-top: 2px; color: #b3a382; font-size: var(--font-size-xs); text-overflow: ellipsis; white-space: nowrap; }
  .version-banner small { color: #91866f; font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; white-space: nowrap; }
  .project-rail, .tree-rail, .main-frame { min-width: 0; min-height: 0; }
  [data-app-panel] { isolation: isolate; outline: 0; }
  [data-app-panel]:focus-within, [data-app-panel]:focus { box-shadow: inset 0 0 0 1px var(--muted-foreground); }
  .project-rail, .tree-rail { position: relative; border-right: 1px solid var(--border); }
  .project-rail { display: flex; flex-direction: column; background: var(--card); }

  .brand { position: relative; display: flex; min-height: 46px; align-items: center; gap: 8px; padding: 7px 7px 7px 9px; user-select: none; }
  .brand-mark { display: flex; width: 24px; height: 24px; align-items: flex-end; gap: 3px; padding: 4px; border: 1px solid #454a51; background: var(--popover); }
  .brand-mark span { width: 3px; background: #9ca3ad; }
  .brand-mark span:nth-child(1) { height: 6px; } .brand-mark span:nth-child(2) { height: 14px; } .brand-mark span:nth-child(3) { height: 10px; }
  .brand-copy { min-width: 0; flex: 1; }
  .brand-copy strong, .brand-copy span { display: block; }
  .brand-copy strong { color: #f3f4f6; font-size: 13px; font-weight: 680; }
  .brand-copy span { margin-top: 1px; color: var(--muted-foreground); font-size: var(--font-size-xs); }

  .rail-label { display: flex; align-items: center; justify-content: space-between; min-height: 26px; border-top: 1px solid var(--border); padding: 4px 8px; color: var(--text-soft); font-size: var(--font-size-xs); font-weight: 680; letter-spacing: 0.04em; text-transform: uppercase; }
  .rail-label small { color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .project-list { min-height: 0; flex: 1; overflow-y: auto; padding: 2px 5px 6px; scrollbar-color: var(--border-strong) transparent; scrollbar-width: thin; }
  .project-row { position: relative; display: flex; min-height: 40px; align-items: center; margin: 1px 0; border: 1px solid transparent; border-radius: 3px; }
  .project-row:hover { background: var(--popover); }
  .project-row.active { border-color: var(--border-strong); background: var(--accent); box-shadow: inset 2px 0 var(--muted-foreground); }
  .project-row > :global(.tooltip-anchor) { min-width: 0; flex: 1; align-self: stretch; }
  .project-select { position: relative; display: flex; width: 100%; height: 100%; min-width: 0; flex: 1; align-items: center; gap: 7px; border: 0; padding: 5px 7px; background: transparent; text-align: left; cursor: pointer; }
  .project-select:focus-visible { outline: 1px solid #737b84; outline-offset: -2px; background: var(--border); }
  .repository-children { margin-left: 12px; border-left: 1px solid var(--border-token); padding-left: 3px; }
  .project-row.nested { min-height: 34px; }
  .project-row.nested .project-select { padding-block: 3px; }
  .project-row.nested .project-copy small { color: var(--muted-foreground); }
  .app-shell :global(.project-select[data-reorderable='true']) { cursor: grab; }
  .app-shell :global(.project-select[data-reorder-dragging='true']) { opacity: 0.42; }
  .app-shell :global(.project-select[data-reorder-drop]::after) { position: absolute; z-index: 3; right: 6px; left: 6px; height: 2px; background: var(--ring); content: ''; pointer-events: none; }
  .app-shell :global(.project-select[data-reorder-drop='before']::after) { top: -2px; }
  .app-shell :global(.project-select[data-reorder-drop='after']::after) { bottom: -2px; }
  .project-kind-icon { display: grid; width: 20px; height: 20px; flex: none; place-items: center; color: var(--muted-foreground); }
  .project-icon-anchor { display: inline-flex; flex: none; }
  .project-row.active .project-kind-icon { color: var(--foreground); }
  .project-copy { min-width: 0; }
  .project-copy strong, .project-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .project-copy strong { color: var(--foreground); font-size: var(--font-size-sm); font-weight: 620; }
  .project-copy small { margin-top: 1px; color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .project-unread-rollup { display: inline-flex; min-width: 20px; height: 18px; flex: none; align-items: center; justify-content: center; gap: 3px; border: 1px solid color-mix(in srgb, #8fb8ff 45%, var(--border)); border-radius: 999px; padding: 0 5px; color: #b9d2ff; background: color-mix(in srgb, #8fb8ff 9%, var(--popover)); font: 650 var(--font-size-xs)/1 'JetBrains Mono Variable', monospace; }
  .project-unread-rollup > span { width: 5px; height: 5px; border-radius: 999px; background: #8fb8ff; }
  .rename-form { display: flex; width: 100%; align-items: center; gap: 4px; padding: 4px; }
  .rename-form input { min-width: 0; flex: 1; border: 1px solid var(--border-strong); padding: 5px; background: var(--background); color: var(--text); font-size: var(--font-size-sm); }
  .project-empty { margin: 5px; border: 1px dashed var(--border-strong); padding: 10px; }
  .project-empty strong { color: var(--foreground); font-size: var(--font-size-sm); } .project-empty p { margin: 3px 0 8px; color: var(--muted); font-size: var(--font-size-sm); }
  .project-footer { padding: 6px; border-top: 1px solid var(--border); }

  .resize-handle { position: absolute; z-index: 8; top: 0; right: -3px; bottom: 0; width: 6px; border: 0; padding: 0; background: transparent; cursor: col-resize; touch-action: none; }
  .resize-handle::after { position: absolute; top: 0; right: 2px; bottom: 0; width: 1px; background: transparent; content: ''; }
  .resize-handle:hover::after, .resize-handle:focus-visible::after { background: var(--muted-foreground); }

  .project-rail.collapsed .brand { min-height: 36px; gap: 0; padding-inline: 1px; }
  .project-rail.collapsed .brand-copy, .project-rail.collapsed .rail-label span, .project-rail.collapsed .project-copy, .project-rail.collapsed .button-copy, .project-rail.collapsed .project-empty { display: none; }
  .project-rail.collapsed .brand-mark { display: none; }
  .project-rail.collapsed .rail-label { justify-content: center; padding-inline: 0; }
  .project-rail.collapsed .project-list { display: flex; flex-direction: column; gap: 4px; padding: 6px 7px; }
  .project-rail.collapsed .repository-children { position: relative; display: flex; flex-direction: column; gap: 2px; margin: 0 0 2px; border-left: 0; padding: 2px 0; }
  .project-rail.collapsed .repository-children::before { position: absolute; top: 2px; bottom: 2px; left: 1px; width: 1px; background: var(--border-token); content: ''; }
  .project-rail.collapsed .project-row { width: 100%; height: 40px; min-height: 40px; flex: 0 0 40px; margin: 0; }
  .project-rail.collapsed .project-row.nested { height: 36px; min-height: 36px; flex-basis: 36px; }
  .project-rail.collapsed .project-select { position: relative; width: 100%; height: 100%; flex: 0 0 100%; justify-content: center; gap: 0; padding: 4px; }
  .project-rail.collapsed .project-kind-icon { width: 30px; height: 30px; border: 1px solid var(--border-strong); border-radius: 3px; color: var(--foreground); background: var(--popover); }
  .project-rail.collapsed :global(.project-actions) { display: none; }
  .project-rail.collapsed :global(.project-status-badge) { position: absolute; z-index: 2; right: 2px; bottom: 2px; width: 12px; height: 12px; border: 1px solid var(--card); border-radius: 999px; background: var(--card); }
  .project-rail.collapsed :global(.project-status-badge > span) { width: 6px; height: 6px; }
  .project-rail.collapsed .project-unread-rollup { position: absolute; z-index: 2; top: 2px; right: 2px; min-width: 14px; height: 14px; gap: 0; padding: 0 3px; border-color: var(--information); font-size: 9px; }
  .project-rail.collapsed .project-unread-rollup > span { display: none; }
  .project-rail.collapsed .project-footer { padding: 5px; }

  .main-frame { position: relative; display: grid; width: 100%; height: 100%; max-height: 100%; grid-template-rows: minmax(0, 1fr) minmax(0, auto); overflow: hidden; background: var(--night); }
  .main-frame.has-error { grid-template-rows: minmax(0, auto) minmax(0, 1fr) minmax(0, auto); }
  .main-frame.empty { display: flex; }
  .error-banner { display: flex; align-items: center; justify-content: space-between; gap: 10px; border: 0; border-bottom: 1px solid rgb(220 107 107 / 38%); padding: 5px 8px; background: rgb(120 44 44 / 18%); color: #efa5a5; font-size: var(--font-size-sm); text-align: left; cursor: pointer; }
  .error-banner strong { font-size: var(--font-size-xs); }
  .item-viewer { width: 100%; height: 100%; min-width: 0; min-height: 0; max-height: 100%; overflow: hidden; }
  .terminal-view { position: relative; width: 100%; height: 100%; min-height: 0; max-height: 100%; overflow: hidden; padding: 5px; }
  .terminal-view > :global(.terminal-frame) { width: 100%; height: 100%; }
  .onboarding { display: grid; width: min(440px, calc(100% - 36px)); place-items: start; align-content: center; margin: auto; }
  .onboarding > span { color: var(--muted); font-size: var(--font-size-sm); text-transform: uppercase; }
  .onboarding h1 { margin: 5px 0 0; color: var(--foreground); font-size: 28px; }
  .onboarding p { margin: 7px 0 13px; color: var(--text-soft); font-size: 12px; }

  .dialog-surface { width: 100%; color: var(--foreground); }
  .dialog-surface > header { display: flex; align-items: start; justify-content: space-between; border-bottom: 1px solid var(--border); padding: 11px 13px 9px; }
  .dialog-surface > header span, .dialog-surface label > span { color: var(--muted-foreground); font: 700 var(--font-size-xs) 'JetBrains Mono Variable', monospace; text-transform: uppercase; }
  .dialog-surface h2 { margin: 3px 0 0; color: var(--foreground); font-size: 17px; }
  .dialog-surface > label, .dialog-row { margin: 10px 13px 0; }
  .dialog-surface label { display: grid; gap: 4px; }
  .dialog-surface label small { color: var(--muted-foreground); font: inherit; }
  .dialog-surface input, .dialog-surface textarea, .dialog-surface select { width: 100%; border: 1px solid var(--input); border-radius: var(--radius); outline: 0; padding: 7px 8px; background: var(--background); color: var(--text); font-size: var(--font-size-sm); }
  .dialog-surface textarea { resize: vertical; line-height: 1.4; }
  .dialog-row { display: grid; grid-template-columns: 0.45fr 1fr; gap: 8px; }
  .dialog-surface > footer { display: flex; justify-content: flex-end; gap: 6px; margin-top: 12px; border-top: 1px solid var(--border); padding: 8px 13px; }
  .agent-choices { display: grid; max-height: 280px; overflow-y: auto; padding: 5px; }
  .agent-choices > button { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 2px 8px; border: 0; border-bottom: 1px solid var(--border); padding: 8px; background: transparent; color: var(--foreground); text-align: left; cursor: pointer; }
  .agent-choices > button:hover { background: var(--accent); }
  .agent-choices strong { font-size: var(--font-size-sm); } .agent-choices small { overflow: hidden; color: var(--muted); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; text-overflow: ellipsis; white-space: nowrap; }
  .agent-choices span { grid-row: 1 / 3; grid-column: 2; align-self: center; color: var(--text-soft); font-size: var(--font-size-sm); }
  .agent-choices p { margin: 0; padding: 13px; color: var(--text-soft); font-size: var(--font-size-sm); }

  @media (max-width: 760px) {
    .project-copy small { display: none; }
  }
</style>
