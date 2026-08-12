<script lang="ts">
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';
  import FolderPlusIcon from '@lucide/svelte/icons/folder-plus';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import MoreHorizontalIcon from '@lucide/svelte/icons/more-horizontal';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import XIcon from '@lucide/svelte/icons/x';
  import { open } from '@tauri-apps/plugin-dialog';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount, tick } from 'svelte';

  import AddCommandDialog from './lib/AddCommandDialog.svelte';
  import AgentCascadeDialog from './lib/AgentCascadeDialog.svelte';
  import AgentDoneToasts, { type AgentDoneNotice } from './lib/AgentDoneToasts.svelte';
  import IconButton from './lib/components/ds/IconButton.svelte';
  import StatusIndicator from './lib/components/ds/StatusIndicator.svelte';
  import TooltipLabel from './lib/components/ds/TooltipLabel.svelte';
  import { Button } from './lib/components/ui/button';
  import * as Dialog from './lib/components/ui/dialog';
  import ContextMenu from './lib/ContextMenu.svelte';
  import ClaimedTodoOverlay from './lib/ClaimedTodoOverlay.svelte';
  import KeyboardShortcuts from './lib/KeyboardShortcuts.svelte';
  import NotificationsCenter from './lib/NotificationsCenter.svelte';
  import OptimisticProcessPanel from './lib/OptimisticProcessPanel.svelte';
  import ProcessOverview from './lib/ProcessOverview.svelte';
  import ProcessStatusBar from './lib/ProcessStatusBar.svelte';
  import ProjectIcon from './lib/ProjectIcon.svelte';
  import ProjectFolderCreateRow from './lib/ProjectFolderCreateRow.svelte';
  import ProjectFolderHeader from './lib/ProjectFolderHeader.svelte';
  import ProjectFolderMenu from './lib/ProjectFolderMenu.svelte';
  import ProjectOverview from './lib/ProjectOverview.svelte';
  import ProjectSettingsDialog from './lib/ProjectSettingsDialog.svelte';
  import ProjectTree from './lib/ProjectTree.svelte';
  import QuickJumpPalette from './lib/QuickJumpPalette.svelte';
  import ScratchpadBrowser from './lib/ScratchpadBrowser.svelte';
  import ScratchpadDetailView from './lib/ScratchpadDetailView.svelte';
  import { submitOnEnter } from './lib/formInputConventions';
  import SettingsPanel from './lib/SettingsPanel.svelte';
  import { applyUpdate, checkForUpdates, type UpdateStatus } from './lib/settings';
  import { updateActionAvailable, updateActionCopy } from './lib/updateRecovery';
  import TerminalView from './lib/TerminalView.svelte';
  import TodoBrowser from './lib/TodoBrowser.svelte';
  import TodoBlockerPicker from './lib/TodoBlockerPicker.svelte';
  import TodoDetailView from './lib/TodoDetailView.svelte';
  import TrustReviewDialog from './lib/TrustReview.svelte';
  import WorktreeDialog from './lib/WorktreeDialog.svelte';
  import WorktreeImportDialog from './lib/WorktreeImportDialog.svelte';
  import WorktreeOperationRow from './lib/WorktreeOperationRow.svelte';
  import WorktreeProgressPanel from './lib/WorktreeProgressPanel.svelte';
  import WorktreeRemoveDialog from './lib/WorktreeRemoveDialog.svelte';
  import WorktreeRowMeta from './lib/WorktreeRowMeta.svelte';
  import workmanMark24 from '../../../assets/branding/workman-icon-cropped-24-transparent.png';
  import workmanMark48 from '../../../assets/branding/workman-icon-cropped-48-transparent.png';
  import workmanLogoWide from '../../../assets/branding/workman-logo-wide-transparent.png';
  import { getAgentToolsStore, type AgentTool } from './lib/agentTools';
  import {
    planAgentCascade,
    type AgentCascadeAction,
    type AgentCascadeRequest
  } from './lib/agentCascade';
  import type { ClaimedTodo } from './lib/claimedTodos';
  import type {
    CoordinationSnapshot,
    NewTodoInput,
    ScratchpadRead,
    ScratchpadSummary,
    TodoDetail,
    TodoPriority,
    UpdateTodoInput
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
    DaemonRequestError,
    isUnsupportedControlMethod,
    type ConnectionStatus,
    type Notification,
    type ProcessKind,
    type ProcessView,
    type Project,
    type ProjectIconImage,
    type TrustReview
  } from './lib/daemon';
  import {
    appendDaemonLogEntry,
    isDaemonRequestTimeoutError,
    type DaemonLogEntry,
    type DaemonLogTone
  } from './lib/daemonLog';
  import {
    appNavigation,
    readRecentNavigationKeys,
    recordRecentNavigation,
    type AppNavigationRequest,
    type AppNavigationTarget,
    type NavigationProjectSnapshot
  } from './lib/navigation';
  import { liveStats } from './lib/liveStats';
  import {
    beginOptimisticNavigation,
    selectProjectOptimistically
  } from './lib/optimisticNavigation';
  import {
    loadProjectPaneMemory,
    projectPaneSelectionExists,
    sameProjectPane,
    saveProjectPaneMemory,
    type ProjectPane,
    type ProjectPaneMemory
  } from './lib/projectPaneMemory';
  import {
    deliverNativeNotification,
    listenForNativeNotificationActions,
    refreshNativeNotificationPermission,
    syncDockUnreadBadge
  } from './lib/nativeNotifications';
  import type { ProjectSettingsInput } from './lib/projectAppearance';
  import {
    createProjectFolder,
    deleteProjectFolder,
    loadProjectRail,
    renameProjectFolder,
    setProjectFolderCollapsed,
    updateProjectLayout,
    type ProjectRailSnapshot
  } from './lib/projectFolderClient';
  import {
    applyProjectRailLayout,
    buildProjectRailLayout,
    moveProjectRailEntry,
    moveProjectRailEntryFromKeyboard,
    projectRailLayoutSignature,
    type ProjectFolder,
    type ProjectFolderMenuRequest
  } from './lib/projectFolders';
  import {
    NATIVE_MENU_EVENT,
    requestNativeUpdateCheck,
    type NativeMenuAction
  } from './lib/nativeMenu';
  import {
    openerSettings,
    openBrowserUrl,
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
    bulkFailureMessage,
    type ProjectTreeBulkAction,
    type ProjectTreeMultiSelection
  } from './lib/projectTreeMultiSelect';
  import {
    reorderItem,
    type ReorderDirection,
    type ReorderDrop
  } from './lib/reorder';
  import { openSettingsSection, selectSettingsSection } from './lib/settingsSections';
  import {
    createOptimisticProcess,
    failOptimisticProcess,
    type OptimisticProcess
  } from './lib/optimisticProcesses';
  import {
    processActivityTone,
    projectActivityRollup,
    type ProjectActivityRollup
  } from './lib/processActivity';
  import {
    initialFlatProjectOrder,
    projectDisplayName,
    projectRepositoryTitle,
    worktreeParentLabel,
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
  const agentToolsStore = getAgentToolsStore(client);
  const projectRailBounds = { min: 176, max: 340 };
  const treeRailBounds = { min: 220, max: 420 };
  const collapsedProjectRailWidth = 58;
  const collapsedTreeRailWidth = 54;
  const flatProjectOrderStorageKey = 'workman.project-rail.flat-order.v1';

  let projects = $state<Project[]>([]);
  let projectFolders = $state<ProjectFolder[]>([]);
  let processes = $state<ProcessView[]>([]);
  let documentVisible = $state(true);
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
  let treeMultiSelection = $state<ProjectTreeMultiSelection | null>(null);
  let todoDetail = $state<TodoDetail | null>(null);
  let todoCommentFocusId = $state<number | null>(null);
  let pendingTodoCommentFocus = $state<{ todoId: number; commentId: number } | null>(null);
  let scratchpadRead = $state<ScratchpadRead | null>(null);
  let detailLoading = $state(false);
  let detailBusy = $state(false);
  let busy = $state(false);
  let processBusyId = $state<number | null>(null);
  let loadedProjectId = $state<number | null>(null);
  let processRequest = 0;
  let coordinationRequest = 0;
  let detailRequest = 0;
  let projectActivationRequest = 0;
  let pendingProjectSelectionId: number | null = null;
  let error = $state<string | null>(null);
  let daemonLog = $state<DaemonLogEntry[]>([]);
  let daemonLogSequence = 0;
  let renameId = $state<number | null>(null);
  let renameValue = $state('');
  let folderCreateOpen = $state(false);
  let folderCreateValue = $state('');
  let folderRenameId = $state<number | null>(null);
  let folderRenameValue = $state('');
  let folderMenuRequest = $state<ProjectFolderMenuRequest | null>(null);
  let projectSettingsProject = $state<Project | null>(null);
  let projectSettingsBusy = $state(false);
  let settingsOpen = $state(false);
  let todoBrowserOpen = $state(false);
  let todoNavigationIds = $state<number[]>([]);
  let scratchpadBrowserOpen = $state(false);
  let processOverviewKind = $state<ProcessKind | null>(null);
  let scratchpadBrowserBusyId = $state<number | null>(null);
  let trustReview = $state<TrustReview | null>(null);
  let trustBusy = $state(false);
  let projectRailWidth = $state(238);
  let projectRailCollapsed = $state(false);
  let treeRailWidth = $state(280);
  let treeRailCollapsed = $state(false);

  let dialog = $state<'todo' | 'agent' | 'command' | null>(null);
  let commandDialogProcess = $state<ProcessView | null>(null);
  let todoTitle = $state('');
  let todoBody = $state('');
  let todoPriority = $state<TodoPriority>('medium');
  let todoTags = $state('');
  let todoBlockerIds = $state<number[]>([]);
  let scratchpadFocusRequest = $state(0);
  let agentTools = $state<AgentTool[]>([]);
  let registeredAgentTools = $state<AgentTool[]>([]);
  let agentToolsLoading = $state(false);

  $effect(() => agentToolsStore.subscribe((snapshot) => {
    registeredAgentTools = snapshot.tools;
    agentTools = snapshot.tools.filter((tool) => tool.enabled);
  }));
  let versionRestarting = $state(false);
  let startupUpdate = $state<UpdateStatus | null>(null);
  let startupUpdatePort = $state<number | null>(null);
  let quickJumpOpen = $state(false);
  let shortcutsOpen = $state(false);
  let quickJumpLoading = $state(false);
  let quickJumpRecentKeys = $state<string[]>([]);
  let navigationIndex = $state<Record<number, NavigationProjectSnapshot>>({});
  let projectPaneMemory = $state<ProjectPaneMemory>(loadProjectPaneMemory());
  let navigationIndexRequest = 0;
  let projectReorderBusy = $state(false);
  let flatProjectOrderChecked = false;
  let processReorderBusy = $state(false);
  let coordinationReorderBusy = $state(false);
  let agentCascadeRequest = $state<AgentCascadeRequest | null>(null);
  let agentCascadeBusy = $state(false);
  let agentCascadeError = $state<string | null>(null);
  let treeBulkBusy = $state(false);
  let contextRequest = $state<ContextMenuRequest | null>(null);
  let treeRenameTarget = $state<ContextMenuTarget | null>(null);
  let worktreeLists = $state<Record<number, WorktreeList>>({});
  let worktreeRefreshingRepositoryId = $state<number | null>(null);
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
  let nativeNotificationBaselineReady = false;
  let nativeDeliveryQueue = Promise.resolve();
  let dockUnreadCount = -1;
  const reconciledWorktreeOperations = new Set<string>();
  const notifiedUnreadProcessIds = new Set<number>();
  const seenNativeNotificationIds = new Set<number>();
  const markReadPending = new Set<number>();
  let removeWorktreeDialog = $state<{
    project: Project;
    repository: WorktreeRepository;
    entry: WorktreeEntry;
  } | null>(null);
  let removeWorktreeBusy = $state(false);
  let removeWorktreeError = $state<string | null>(null);
  let removeWorktreeForceRequired = $state(false);
  let importOffer = $state<{ repository: WorktreeRepository; entries: WorktreeEntry[] } | null>(null);
  let importBusyPath = $state<string | null>(null);
  let importError = $state<string | null>(null);

  let selectedProject = $derived(projects.find((project) => project.selected) ?? null);
  let projectRailLayout = $derived(buildProjectRailLayout(projects, projectFolders));
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
  let projectOverviewOpen = $derived(
    selectedProject !== null &&
      !settingsOpen &&
      activeWorktreeOperation === null &&
      selectedOptimisticProcess === null &&
      selectedProcess === null &&
      !todoBrowserOpen &&
      !scratchpadBrowserOpen &&
      processOverviewKind === null &&
      selection === null
  );
  let frameItemLabel = $derived(
    settingsOpen
      ? 'Settings'
      : activeWorktreeOperation
        ? activeWorktreeOperation.label
        : todoBrowserOpen
          ? 'Todos'
          : scratchpadBrowserOpen
            ? 'Scratchpads'
            : processOverviewKind
              ? `${processOverviewKind[0].toUpperCase()}${processOverviewKind.slice(1)}s`
              : projectOverviewOpen && selectedProject
                ? projectLabel(selectedProject)
                : (selection?.label ?? 'Project')
  );
  let windowTitle = $derived(
    selectedProject && selectedProcess
      ? `${projectLabel(selectedProject)}: ${selectedProcess.name}`
      : projectOverviewOpen && selectedProject
        ? projectLabel(selectedProject)
        : 'workman'
  );
  let contextMenuDescriptor = $derived(
    contextRequest ? describeContextMenu(contextRequest.target, $openerSettings) : null
  );
  let versionSkew = $derived(
    connection.status === 'connected' && !connection.version_compatible
  );
  let updateAvailable = $derived(startupUpdate?.check.available === true);
  let cliRecoveryRequired = $derived(startupUpdate?.cli_recovery_required === true);
  let startupUpdateCopy = $derived(startupUpdate ? updateActionCopy(startupUpdate) : null);
  let showVersionBanner = $derived(versionSkew || updateAvailable || cliRecoveryRequired);

  $effect(() => {
    void getCurrentWindow().setTitle(windowTitle).catch(() => undefined);
  });

  $effect(() => {
    const unreadCount = notifications.filter((notification) => notification.read_at === null).length;
    if (unreadCount === dockUnreadCount) return;
    dockUnreadCount = unreadCount;
    void syncDockUnreadBadge(unreadCount);
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
      scratchpadBrowserOpen = false;
      processOverviewKind = null;
      activeWorktreeOperationId = null;
      loadedProjectId = null;
      return;
    }
    if (loadedProjectId !== projectId) {
      applyProjectActivationState(projectId);
      void loadAndReconcileProject(projectId);
    }
  });

  $effect(() => {
    const projectId = selectedProject?.id ?? null;
    if (
      connection.status !== 'connected'
      || projectId === null
      || loadedProjectId !== projectId
    ) return;
    const pane = currentProjectPane();
    if (!pane || sameProjectPane(projectPaneMemory[projectId], pane)) return;
    projectPaneMemory = { ...projectPaneMemory, [projectId]: pane };
    saveProjectPaneMemory(projectPaneMemory);
  });

  $effect(() => {
    if (
      connection.status !== 'connected'
      || projects.length === 0
      || projectFolders.length > 0
      || flatProjectOrderChecked
    ) return;
    flatProjectOrderChecked = true;
    void seedFlatProjectOrder();
  });

  onMount(() => {
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
    let visibilityRequest = 0;
    const applyDocumentVisibility = (visible: boolean): void => {
      if (documentVisible === visible) return;
      documentVisible = visible;
      document.documentElement.classList.toggle('workman-document-hidden', !documentVisible);
      if (connection.status !== 'connected') return;
      if (!documentVisible) {
        void client.unsubscribeProcessStatuses().catch(reportError);
        return;
      }
      void client.subscribeProcessStatuses().catch(reportError);
      if (!busy) void refreshProjects();
      if (selectedProject) {
        void refreshProcesses(selectedProject.id);
        if (connection.version_compatible) {
          void refreshCoordination(selectedProject.id, false);
        }
      }
      if (connection.version_compatible) void refreshNotifications();
    };
    const updateDocumentVisibility = (): void => {
      const request = ++visibilityRequest;
      if (document.hidden) {
        applyDocumentVisibility(false);
        return;
      }
      void getCurrentWindow().isMinimized().then((minimized) => {
        if (active && request === visibilityRequest) applyDocumentVisibility(!minimized);
      }).catch(reportError);
    };
    updateDocumentVisibility();
    document.addEventListener('visibilitychange', updateDocumentVisibility);
    let stopWindowResize = (): void => {};
    void getCurrentWindow().onResized(updateDocumentVisibility).then((stop) => {
      if (active) stopWindowResize = stop;
      else stop();
    }).catch(reportError);
    let stopWindowFocus = (): void => {};
    void getCurrentWindow().onFocusChanged(updateDocumentVisibility).then((stop) => {
      if (active) stopWindowFocus = stop;
      else stop();
    }).catch(reportError);
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
    let stopNativeNotifications = (): void => {};
    void listenForNativeNotificationActions((notificationId) => {
      if (active) void openNativeNotification(notificationId);
    }).then((stop) => {
      if (active) stopNativeNotifications = stop;
      else stop();
    }).catch(reportError);
    void refreshNativeNotificationPermission();
    const projectTimer = setInterval(() => {
      if (active && documentVisible && connection.status === 'connected' && !busy) {
        void refreshProjects();
      }
    }, 5000);
    const coordinationTimer = setInterval(() => {
      if (
        active &&
        documentVisible &&
        connection.status === 'connected' &&
        connection.version_compatible &&
        selectedProject
      ) {
        void refreshCoordination(selectedProject.id, false);
      }
    }, 2500);
    const notificationTimer = setInterval(() => {
      if (
        active
        && documentVisible
        && connection.status === 'connected'
        && connection.version_compatible
      ) {
        void refreshNotifications();
      }
    }, 1000);

    void client
      .start(
        (status) => { if (active) applyConnectionStatus(status); },
        (message) => {
          if (active) recordDaemonLog('warning', 'Control message issue', message);
        }
      )
      .then((status) => { if (active) applyConnectionStatus(status); })
      .catch(reportError);

    return () => {
      active = false;
      document.removeEventListener('visibilitychange', updateDocumentVisibility);
      stopWindowResize();
      stopWindowFocus();
      document.documentElement.classList.remove('workman-document-hidden');
      clearInterval(projectTimer);
      clearInterval(coordinationTimer);
      clearInterval(notificationTimer);
      stopStatuses();
      stopNavigation();
      stopNativeMenu();
      stopNativeNotifications();
      client.close();
    };
  });

  function applyConnectionStatus(status: ConnectionStatus): void {
    const previous = connection;
    const reconnected = connection.status !== 'connected' && status.status === 'connected';
    connection = status;
    if (
      previous.status !== status.status
      || previous.port !== status.port
      || previous.message !== status.message
    ) {
      if (status.status === 'connected') {
        recordDaemonLog(
          'info',
          'Daemon connected',
          status.port === null ? null : `Local control port ${status.port}`
        );
      } else if (status.status === 'disconnected') {
        recordDaemonLog('error', 'Daemon disconnected', status.message);
      } else {
        recordDaemonLog('info', 'Connecting to daemon', status.message);
      }
    }
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
      if (documentVisible) {
        void client.subscribeProcessStatuses().catch(reportError);
        void refreshProjects();
        if (status.version_compatible) void refreshNotifications();
      }
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
    if (folderMenuRequest) {
      if (event.key === 'Escape') closeProjectFolderMenu();
      return;
    }
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
    if (event.key === 'Escape' && (treeMultiSelection?.ids.length ?? 0) > 0) {
      event.preventDefault();
      treeMultiSelection = null;
      return;
    }
    if (isTextEditingTarget(target)) return;
    if (
      selection?.kind === 'todo' && event.metaKey && !event.altKey && !event.ctrlKey && !event.shiftKey
      && (event.key === 'ArrowLeft' || event.key === 'ArrowRight')
    ) {
      event.preventDefault();
      void navigateAdjacentTodo(event.key === 'ArrowLeft' ? -1 : 1);
      return;
    }
    if (
      selection?.kind === 'scratchpad' && event.metaKey && !event.altKey && !event.ctrlKey && !event.shiftKey
      && (event.key === 'ArrowLeft' || event.key === 'ArrowRight')
    ) {
      event.preventDefault();
      void navigateAdjacentScratchpad(event.key === 'ArrowLeft' ? -1 : 1);
      return;
    }
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
      else if (selection?.kind === 'todo') openTodosBrowser();
      else if (selection?.kind === 'scratchpad') openScratchpadsBrowser();
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
        '.project-select:not(:disabled), .folder-select:not(:disabled)',
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
      if (request !== notificationRequest) return;
      const fresh = nativeNotificationBaselineReady
        ? next
            .filter((notification) =>
              notification.read_at === null && !seenNativeNotificationIds.has(notification.id)
            )
            .sort((left, right) => left.created_at - right.created_at || left.id - right.id)
        : [];
      for (const notification of next) seenNativeNotificationIds.add(notification.id);
      nativeNotificationBaselineReady = true;
      notifications = next;
      if (fresh.length > 0) {
        nativeDeliveryQueue = nativeDeliveryQueue.catch(() => undefined).then(async () => {
          for (const notification of fresh) await deliverNativeNotification(notification);
        });
      }
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
    pendingTodoCommentFocus = null;
    todoCommentFocusId = null;
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
          process?.kind
            ?? (notification.type === 'agent_done' || notification.type === 'needs_input'
              ? 'agent'
              : 'command'),
          notification.process_id,
          projectId,
          process?.name ?? notification.body
        )
      }, 'api');
    } else if (notification.todo_id !== null && projectId !== null) {
      if (notification.comment_id !== null) {
        todoCommentFocusId = notification.comment_id;
        pendingTodoCommentFocus = {
          todoId: notification.todo_id,
          commentId: notification.comment_id
        };
      }
      appNavigation.navigate({
        type: 'item',
        selection: projectTreeSelection('todo', notification.todo_id, projectId, notification.body)
      }, 'api');
    } else if (projectId !== null) {
      appNavigation.navigate({ type: 'project', projectId }, 'api');
    }
  }

  async function openNativeNotification(notificationId: number): Promise<void> {
    let notification = notifications.find((candidate) => candidate.id === notificationId) ?? null;
    if (!notification) {
      await refreshNotifications();
      notification = notifications.find((candidate) => candidate.id === notificationId) ?? null;
    }
    if (notification) openNotification(notification);
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
      const kind: AgentDoneNotice['kind'] = process.agent_state.needs_input
        ? 'needs_input'
        : 'agent_done';
      agentDoneNotices = [
        ...agentDoneNotices,
        {
          id: `${process.id}:${++agentDoneNoticeSequence}`,
          processId: process.id,
          projectId: process.project_id,
          name: process.name,
          kind
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

  function projectRailActivity(project: Project): ProjectActivityRollup {
    const projectProcesses = navigationIndex[project.id]?.processes
      ?? (selectedProject?.id === project.id ? processes : []);
    const rollup = projectActivityRollup(projectProcesses, $liveStats.processes);
    if (projectProcesses.length === 0 && project.status === 'error') {
      return { ...rollup, state: 'crashed', crashed: 1 };
    }
    return rollup;
  }

  function projectRailActivityLabel(project: Project, rollup: ProjectActivityRollup): string {
    const title = projectTitle(project);
    const active = rollup.active > 0 ? ` · ${rollup.active} actively working` : '';
    if (rollup.state === 'needs_input') return `${title} · needs input${active}`;
    if (rollup.state === 'crashed') return `${title} · process error${active}`;
    if (rollup.state === 'working') return `${title}${active}`;
    if (rollup.state === 'waiting') return `${title} · waiting`;
    return `${title} · no active work`;
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
      registeredAgentTools = tools;
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
      const switchingProjects = projectId !== null && selectedProject?.id !== projectId;
      if (projectId !== null && !activateProject(projectId)) return;

      recordRecentNavigation(target);
      quickJumpRecentKeys = readRecentNavigationKeys();
      dialog = null;

      switch (target.type) {
        case 'project':
          if (!switchingProjects) {
            settingsOpen = false;
            clearSelection();
          }
          if (selectedProject) void refreshWorktreeRepository(selectedProject, false);
          return;
        case 'item':
          await selectTreeItem(target.selection);
          return;
        case 'settings':
          if (selectedProject) {
            todoBrowserOpen = false;
            scratchpadBrowserOpen = false;
            processOverviewKind = null;
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
            registeredAgentTools = tools;
            agentTools = tools.filter((candidate) => candidate.enabled);
            tool = agentTools.find((candidate) => candidate.id === target.agentToolId);
          }
          if (!tool) throw new Error(`Agent tool ${target.agentToolName} is no longer enabled`);
          await spawnAgent(tool);
          return;
        }
        case 'add-command':
          settingsOpen = false;
          openAddCommand();
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

  function activateProject(projectId: number): boolean {
    if (selectedProject?.id === projectId) return true;
    if (!projects.some((project) => project.id === projectId)) return false;

    const request = ++projectActivationRequest;
    const projectSnapshot = [...projects];
    pendingProjectSelectionId = projectId;
    beginOptimisticNavigation(
      () => applyOptimisticProjectActivation(projectId),
      () => hydrateProjectActivation(projectId, request, projectSnapshot),
      (cause) => {
        if (request !== projectActivationRequest) return;
        pendingProjectSelectionId = null;
        reportError(cause);
      }
    );
    return true;
  }

  function applyOptimisticProjectActivation(projectId: number): void {
    projects = selectProjectOptimistically(projects, projectId);
    applyProjectActivationState(projectId);
  }

  function applyProjectActivationState(projectId: number): void {
    const cached = navigationIndex[projectId];
    loadedProjectId = projectId;
    processRequest += 1;
    coordinationRequest += 1;
    detailRequest += 1;
    processes = cached?.processes ?? [];
    optimisticProcesses = [];
    coordination = cached?.coordination ?? null;
    treeMultiSelection = null;
    applyRememberedProjectPane(projectId);
  }

  async function hydrateProjectActivation(
    projectId: number,
    request: number,
    projectSnapshot: Project[]
  ): Promise<void> {
    const selectionRequest = client.select(projectId);
    void loadAndReconcileProject(projectId);
    void refreshWorktreeMetadata(projectSnapshot);
    const selectedProjects = await selectionRequest;
    if (request !== projectActivationRequest || selectedProject?.id !== projectId) return;
    pendingProjectSelectionId = null;
    projects = selectProjectOptimistically(selectedProjects, projectId);
  }

  async function loadAndReconcileProject(projectId: number): Promise<void> {
    await loadProject(projectId);
    if (selectedProject?.id !== projectId || loadedProjectId !== projectId) return;
    const pane = projectPaneMemory[projectId] ?? { type: 'overview' };
    if (pane.type !== 'selection') return;

    const snapshot = navigationIndex[projectId];
    const exists = projectPaneSelectionExists(pane, {
      processIds: new Set((snapshot?.processes ?? []).map((process) => process.id)),
      todoIds: new Set((snapshot?.coordination?.todos ?? []).map((todo) => todo.id)),
      scratchpadIds: new Set([
        ...(snapshot?.coordination?.scratchpads ?? []),
        ...(snapshot?.coordination?.archived_scratchpads ?? [])
      ].map((scratchpad) => scratchpad.id))
    });
    if (!exists) {
      if (selection?.projectId === projectId && selection.key === pane.selection.key) {
        settingsOpen = false;
        clearSelection();
      }
      return;
    }
    if (selection?.projectId !== projectId || selection.key !== pane.selection.key) return;
    if (selection.kind === 'todo') await loadTodo(selection.id);
    else if (selection.kind === 'scratchpad') await loadScratchpad(selection.id);
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
    if (operation.mode === 'adopt' && operation.repository_id !== null && operation.path) {
      const currentList = worktreeLists[operation.repository_id];
      if (currentList) {
        worktreeLists = {
          ...worktreeLists,
          [operation.repository_id]: {
            ...currentList,
            worktrees: currentList.worktrees.map((entry) => entry.path === operation.path
              ? {
                  ...entry,
                  project_id: optimisticProject.id,
                  parent_project_id: optimisticProject.parent_project_id,
                  kind: 'adopted',
                  registered: true,
                  can_adopt: false
                }
              : entry)
          }
        };
      }
    }
    await tick();
    appNavigation.navigate({ type: 'project', projectId: optimisticProject.id }, 'api');
  }

  function showWorktreeOperation(operation: WorktreeOperation): void {
    activeWorktreeOperationId = operation.id;
    settingsOpen = false;
    todoBrowserOpen = false;
    scratchpadBrowserOpen = false;
    processOverviewKind = null;
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
      const current = worktreeLists[repositoryId];
      const cacheExpiresAt = current?.pull_requests.expires_at ?? null;
      const cacheIsFresh = cacheExpiresAt !== null && cacheExpiresAt > Math.floor(Date.now() / 1000);
      if (!force && current && cacheIsFresh) continue;
      worktreeRefreshingRepositoryId = repositoryId;
      try {
        const list = await client.worktrees(root.id, refreshPullRequests);
        worktreeLists = { ...worktreeLists, [repositoryId]: list };
      } catch (cause) {
        console.warn(`workman worktree metadata failed for project ${root.id}`, cause);
      } finally {
        if (worktreeRefreshingRepositoryId === repositoryId) worktreeRefreshingRepositoryId = null;
      }
    }
  }

  async function refreshWorktreeRepository(project: Project, refreshPullRequests = true): Promise<void> {
    const root = rootProjectFor(project);
    if (!root || root.repository_id === null) return;
    await refreshWorktreeMetadata(projects, refreshPullRequests, true, root.repository_id);
  }

  async function refreshProjects(): Promise<void> {
    const activationAtStart = projectActivationRequest;
    busy = true;
    try {
      const snapshot = await loadProjectRail(client);
      const nextProjects = snapshot.projects;
      const currentSelectionId = pendingProjectSelectionId ?? selectedProject?.id ?? null;
      const preserveLocalSelection = pendingProjectSelectionId !== null
        || activationAtStart !== projectActivationRequest;
      projects = preserveLocalSelection
        && currentSelectionId !== null
        && nextProjects.some((project) => project.id === currentSelectionId)
        ? selectProjectOptimistically(nextProjects, currentSelectionId)
        : nextProjects;
      projectFolders = snapshot.folders;
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
    treeMultiSelection = null;
    todoCommentFocusId = next.kind === 'todo' && pendingTodoCommentFocus?.todoId === next.id
      ? pendingTodoCommentFocus.commentId
      : null;
    pendingTodoCommentFocus = null;
    scratchpadFocusRequest = 0;
    recordRecentNavigation({ type: 'item', selection: next });
    quickJumpRecentKeys = readRecentNavigationKeys();
    settingsOpen = false;
    todoBrowserOpen = false;
    scratchpadBrowserOpen = false;
    processOverviewKind = null;
    activeWorktreeOperationId = null;
    selection = next;
    todoDetail = null;
    scratchpadRead = null;

    if (next.kind === 'todo' && !todoNavigationIds.includes(next.id)) {
      todoNavigationIds = (coordination?.todos ?? []).map((todo) => todo.id);
    }

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
    const projectId = selectedProject.id;
    const request = ++detailRequest;
    detailLoading = true;
    try {
      const next = await client.coordinationTodo(projectId, todoId);
      if (
        request === detailRequest
        && selectedProject?.id === projectId
        && selection?.kind === 'todo'
        && selection.id === todoId
      ) todoDetail = next;
    } catch (cause) {
      if (request === detailRequest) reportError(cause);
    } finally {
      if (request === detailRequest) detailLoading = false;
    }
  }

  async function navigateAdjacentTodo(direction: -1 | 1): Promise<void> {
    if (!selectedProject || selection?.kind !== 'todo') return;
    const currentIndex = todoNavigationIds.indexOf(selection.id);
    const nextId = currentIndex < 0 ? null : todoNavigationIds[currentIndex + direction];
    if (nextId === null || nextId === undefined) return;
    const summary = coordination?.todos.find((todo) => todo.id === nextId);
    if (!summary) return;
    await selectTreeItem(projectTreeSelection('todo', summary.id, summary.project_id, summary.title));
  }

  async function navigateAdjacentScratchpad(direction: -1 | 1): Promise<void> {
    if (!selectedProject || selection?.kind !== 'scratchpad') return;
    const archived = scratchpadRead?.scratchpad.archived
      ?? coordination?.archived_scratchpads.some((scratchpad) => scratchpad.id === selection?.id)
      ?? false;
    const source = archived ? coordination?.archived_scratchpads ?? [] : coordination?.scratchpads ?? [];
    const currentIndex = source.findIndex((scratchpad) => scratchpad.id === selection?.id);
    const next = currentIndex < 0 ? null : source[currentIndex + direction];
    if (!next) return;
    await selectTreeItem(projectTreeSelection('scratchpad', next.id, next.project_id, next.name));
  }

  async function navigateToTodo(todoId: number): Promise<void> {
    const summary = coordination?.todos.find((todo) => todo.id === todoId);
    if (!summary) return;
    await selectTreeItem(projectTreeSelection('todo', summary.id, summary.project_id, summary.title));
  }

  async function navigateToScratchpad(scratchpadId: number): Promise<void> {
    const summary = [
      ...(coordination?.scratchpads ?? []),
      ...(coordination?.archived_scratchpads ?? [])
    ].find((scratchpad) => scratchpad.id === scratchpadId);
    if (!summary) return;
    await selectTreeItem(
      projectTreeSelection('scratchpad', summary.id, summary.project_id, summary.name)
    );
  }

  async function loadScratchpad(scratchpadId: number, showLoading = true): Promise<void> {
    if (!selectedProject) return;
    const projectId = selectedProject.id;
    const request = ++detailRequest;
    if (showLoading) detailLoading = true;
    try {
      const next = await client.coordinationScratchpad(projectId, scratchpadId);
      if (
        request === detailRequest &&
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
      if (request === detailRequest) reportError(cause);
    } finally {
      if (showLoading && request === detailRequest) detailLoading = false;
    }
  }

  async function startProcess(process: ProcessView): Promise<void> {
    if (processBusyId !== null) return;
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

  async function startOrReviewProcess(process: ProcessView): Promise<void> {
    if (process.source === 'yml' && process.trust_hash === null) {
      await openTrustReview(process);
    } else {
      await startProcess(process);
    }
  }

  async function stopProcess(process: ProcessView): Promise<void> {
    if (processBusyId !== null) return;
    if (openAgentCascadeDialog(process, 'stop')) return;
    processBusyId = process.id;
    try {
      await client.stopProcess(process.id);
      await refreshProcesses(process.project_id);
    } catch (cause) {
      reportError(cause);
    } finally {
      processBusyId = null;
    }
  }

  function openAgentCascadeDialog(
    process: ProcessView,
    action: AgentCascadeAction
  ): boolean {
    if (process.kind !== 'agent') return false;
    const plan = planAgentCascade(processes, [process], action === 'close');
    if (plan.additionalDescendants.length === 0) return false;
    agentCascadeError = null;
    agentCascadeRequest = {
      processes: plan.selected,
      actionRoots: plan.actionRoots,
      action,
      descendants: plan.additionalDescendants
    };
    return true;
  }

  function openBulkProcessDialog(
    selectedProcesses: ProcessView[],
    action: Extract<AgentCascadeAction, 'stop' | 'close'>
  ): void {
    const plan = planAgentCascade(processes, selectedProcesses, action === 'close');
    if (plan.selected.length === 0) return;
    agentCascadeError = null;
    agentCascadeRequest = {
      processes: plan.selected,
      actionRoots: plan.actionRoots,
      action,
      descendants: plan.additionalDescendants
    };
  }

  async function confirmAgentCascade(): Promise<void> {
    const request = agentCascadeRequest;
    if (!request || agentCascadeBusy || processBusyId !== null) return;
    agentCascadeBusy = true;
    processBusyId = request.actionRoots[0]?.id ?? null;
    agentCascadeError = null;
    const failures: Array<{ label: string; message: string }> = [];
    try {
      for (const process of request.actionRoots) {
        try {
          if (request.action === 'stop') {
            await client.stopProcess(process.id);
          } else if (request.action === 'kill') {
            await client.control('process.kill', {
              process_id: process.id,
              confirm_kill: true
            });
          } else {
            await client.closeProcess(process.id);
            if (selection?.id === process.id && isProcessSelection(selection)) clearSelection();
          }
        } catch (cause) {
          failures.push({
            label: process.name,
            message: cause instanceof Error ? cause.message : String(cause)
          });
        }
      }
      if (
        request.action === 'close'
        && selection
        && isProcessSelection(selection)
        && [...request.processes, ...request.descendants].some((process) => process.id === selection?.id)
      ) clearSelection();
      const projectId = request.processes[0]?.project_id;
      if (projectId !== undefined) await refreshProcesses(projectId);
      if (failures.length === 0) {
        treeMultiSelection = null;
        agentCascadeRequest = null;
      } else {
        const actionLabel = request.action === 'close' ? 'close' : request.action === 'kill' ? 'kill' : 'stop';
        const message = `Bulk ${actionLabel} finished with ${failures.length} failure${failures.length === 1 ? '' : 's'} for ${request.processes.length} selected ${request.processes[0]?.kind ?? 'process'}${request.processes.length === 1 ? '' : 's'}. ${failures.map((failure) => `${failure.label}: ${failure.message}`).join(' · ')}`;
        if (request.processes.length === 1) {
          agentCascadeError = message;
        } else {
          agentCascadeRequest = null;
          treeMultiSelection = null;
          reportError(new Error(message));
        }
      }
    } finally {
      processBusyId = null;
      agentCascadeBusy = false;
    }
  }

  async function runTreeBulkAction(action: ProjectTreeBulkAction): Promise<void> {
    const selected = treeMultiSelection;
    const projectId = selectedProject?.id;
    if (!selected || selected.ids.length < 2 || projectId === undefined || treeBulkBusy) return;

    if (selected.group === 'agents' || selected.group === 'terminals') {
      if (action !== 'stop' && action !== 'close') return;
      const kind = selected.group === 'agents' ? 'agent' : 'terminal';
      const selectedProcesses: ProcessView[] = [];
      const missingIds: number[] = [];
      for (const id of selected.ids) {
        const process = processes.find((candidate) => candidate.id === id && candidate.kind === kind);
        if (process) selectedProcesses.push(process);
        else missingIds.push(id);
      }
      if (missingIds.length > 0) {
        treeMultiSelection = selectedProcesses.length > 0
          ? { group: selected.group, ids: selectedProcesses.map((process) => process.id) }
          : null;
        reportError(new Error(
          `${missingIds.length} selected ${kind}${missingIds.length === 1 ? ' is' : 's are'} no longer available. Refresh the selection and try again.`
        ));
        return;
      }
      openBulkProcessDialog(selectedProcesses, action);
      return;
    }

    if (action === 'delete') {
      const noun = selected.group === 'todos' ? 'todos' : 'scratchpads';
      if (!window.confirm(`Delete ${selected.ids.length} ${noun}? This cannot be undone.`)) return;
    }

    treeBulkBusy = true;
    const failures: Array<{ id: number; label: string; message: string }> = [];
    let actionPast = '';
    let actionInfinitive = '';
    try {
      if (selected.group === 'todos') {
        if (action !== 'complete' && action !== 'delete') return;
        actionPast = action === 'complete' ? 'were completed' : 'were deleted';
        actionInfinitive = action === 'complete' ? 'complete' : 'delete';
        for (const id of selected.ids) {
          const todo = coordination?.todos.find((candidate) => candidate.id === id);
          if (!todo) {
            failures.push({ id, label: `Todo #${id}`, message: 'No longer available' });
            continue;
          }
          try {
            if (action === 'complete') {
              await client.coordinationTodoComplete(projectId, todo.id, true);
            } else {
              await client.control('coordination.todo_delete', {
                project_id: projectId,
                todo_id: todo.id
              });
            }
          } catch (cause) {
            failures.push({
              id: todo.id,
              label: `#${todo.id} ${todo.title}`,
              message: cause instanceof Error ? cause.message : String(cause)
            });
          }
        }
      } else {
        if (action !== 'archive' && action !== 'delete') return;
        actionPast = action === 'archive' ? 'were archived' : 'were deleted';
        actionInfinitive = action === 'archive' ? 'archive' : 'delete';
        for (const id of selected.ids) {
          const scratchpad = coordination?.scratchpads.find((candidate) => candidate.id === id);
          if (!scratchpad) {
            failures.push({ id, label: `Scratchpad #${id}`, message: 'No longer available' });
            continue;
          }
          try {
            await client.control(
              action === 'archive'
                ? 'coordination.scratchpad_archive'
                : 'coordination.scratchpad_delete',
              {
                project_id: projectId,
                scratchpad_id: scratchpad.id,
                expected_revision: scratchpad.revision
              }
            );
          } catch (cause) {
            failures.push({
              id: scratchpad.id,
              label: scratchpad.name,
              message: cause instanceof Error ? cause.message : String(cause)
            });
          }
        }
      }

      await refreshCoordination(projectId, false);
      const availableIds = new Set(
        selected.group === 'todos'
          ? (coordination?.todos ?? []).map((todo) => todo.id)
          : (coordination?.scratchpads ?? []).map((scratchpad) => scratchpad.id)
      );
      const selectedDetailSucceeded = selection !== null
        && selected.ids.includes(selection.id)
        && !failures.some((failure) => failure.id === selection?.id);
      if (
        selectedDetailSucceeded
        && selected.group === 'todos'
        && action === 'complete'
        && selection?.kind === 'todo'
      ) {
        try {
          await loadTodo(selection.id);
        } catch (cause) {
          reportError(cause);
        }
      }
      const selectedDetailWasRemoved = selectedDetailSucceeded
        && (
          (selected.group === 'todos' && action === 'delete' && selection?.kind === 'todo')
          || (
            selected.group === 'scratchpads'
            && (action === 'archive' || action === 'delete')
            && selection?.kind === 'scratchpad'
          )
        );
      if (selectedDetailWasRemoved) clearSelection();
      const retryIds = failures.map((failure) => failure.id).filter((id) => availableIds.has(id));
      treeMultiSelection = retryIds.length > 0
        ? { group: selected.group, ids: retryIds }
        : null;
      const message = bulkFailureMessage(actionPast, actionInfinitive, selected.ids.length, failures);
      if (message) reportError(new Error(message));
    } finally {
      treeBulkBusy = false;
    }
  }

  async function restartProcess(process: ProcessView): Promise<void> {
    if (processBusyId !== null) return;
    processBusyId = process.id;
    try {
      await client.restartProcess(process.id);
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
    commandDialogProcess = null;
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
    scratchpadBrowserOpen = false;
    processOverviewKind = null;
    selection = projectTreeSelection('command', id, project.id, input.name);
    return id;
  }

  function openAddCommand(): void {
    commandDialogProcess = null;
    dialog = 'command';
  }

  function openEditCommand(process: ProcessView): void {
    if (process.kind !== 'command') return;
    commandDialogProcess = process;
    settingsOpen = false;
    dialog = 'command';
  }

  async function removeCommand(process: ProcessView): Promise<void> {
    if (process.kind !== 'command' || processBusyId !== null) return;
    const running = process.status === 'running' || process.status === 'starting';
    const storage = process.source === 'yml'
      ? 'its definition from workman.yml'
      : 'its locally stored definition';
    const message = running
      ? `Remove ${process.name}? It is running and will be stopped first. This deletes ${storage} and cannot be undone.`
      : `Remove ${process.name}? This deletes ${storage} and cannot be undone.`;
    if (!window.confirm(message)) return;

    processBusyId = process.id;
    try {
      await client.control('config.command_delete', { process_id: process.id });
      if (selection?.id === process.id && isProcessSelection(selection)) clearSelection();
      await refreshProcesses(process.project_id);
    } catch (cause) {
      reportError(cause);
    } finally {
      processBusyId = null;
    }
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
    else if (retry === 'command') openAddCommand();
    else if (retry === 'agent') void openAgentDialog();
  }

  async function openAgentDialog(): Promise<void> {
    dialog = 'agent';
    agentToolsLoading = true;
    try {
      registeredAgentTools = await client.listAgentTools();
      agentTools = registeredAgentTools.filter((tool) => tool.enabled);
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
    scratchpadBrowserOpen = false;
    processOverviewKind = null;
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
      tags: todoTags.split(',').map((tag) => tag.trim()).filter(Boolean),
      blocker_ids: todoBlockerIds
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

  async function updateTodo(update: UpdateTodoInput): Promise<void> {
    if (!selectedProject || selection?.kind !== 'todo') return;
    const projectId = selectedProject.id;
    const todoId = selection.id;
    detailBusy = true;
    try {
      await client.control('coordination.todo_update', {
        project_id: projectId,
        todo_id: todoId,
        ...update
      });
      if (update.title) {
        selection = projectTreeSelection('todo', todoId, projectId, update.title);
      }
      await Promise.all([loadTodo(todoId), refreshCoordination(projectId, false)]);
    } catch (cause) {
      reportError(cause);
    } finally {
      detailBusy = false;
    }
  }

  async function setTodoLock(locked: boolean): Promise<void> {
    if (!selectedProject || selection?.kind !== 'todo') return;
    const projectId = selectedProject.id;
    const todoId = selection.id;
    detailBusy = true;
    try {
      await client.control(locked ? 'coordination.todo_lock' : 'coordination.todo_unlock', {
        project_id: projectId,
        todo_id: todoId
      });
      await Promise.all([loadTodo(todoId), refreshCoordination(projectId, false)]);
    } catch (cause) {
      reportError(cause);
    } finally {
      detailBusy = false;
    }
  }

  async function setTodoBlockers(blockerIds: number[]): Promise<void> {
    if (!selectedProject || selection?.kind !== 'todo') {
      throw new Error('Select a todo before editing blockers.');
    }
    const projectId = selectedProject.id;
    const todoId = selection.id;
    detailBusy = true;
    try {
      await client.control('coordination.todo_set_blockers', {
        project_id: projectId,
        todo_id: todoId,
        blocker_ids: blockerIds
      });
      await Promise.all([loadTodo(todoId), refreshCoordination(projectId, false)]);
    } finally {
      detailBusy = false;
    }
  }

  async function deleteSelectedTodo(): Promise<void> {
    if (!selectedProject || selection?.kind !== 'todo' || !todoDetail) return;
    if (!window.confirm(`Delete #${selection.id} ${todoDetail.todo.title}? This cannot be undone.`)) return;
    const projectId = selectedProject.id;
    const todoId = selection.id;
    detailBusy = true;
    try {
      await client.control('coordination.todo_delete', { project_id: projectId, todo_id: todoId });
      openTodosBrowser();
      await refreshCoordination(projectId, false);
    } catch (cause) {
      reportError(cause);
    } finally {
      detailBusy = false;
    }
  }

  async function transferSelectedTodo(targetProjectId: number): Promise<void> {
    if (!selectedProject || selection?.kind !== 'todo') return;
    const projectId = selectedProject.id;
    const todoId = selection.id;
    detailBusy = true;
    try {
      await client.control('coordination.todo_transfer', {
        project_id: projectId,
        todo_id: todoId,
        target_project_id: targetProjectId
      });
      openTodosBrowser();
      await refreshCoordination(projectId, false);
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
    todoBlockerIds = [];
  }

  function currentProjectPane(): ProjectPane | null {
    if (activeWorktreeOperation) return null;
    if (settingsOpen) return { type: 'settings' };
    if (todoBrowserOpen) return { type: 'todos' };
    if (scratchpadBrowserOpen) return { type: 'scratchpads' };
    if (processOverviewKind) return { type: 'processes', kind: processOverviewKind };
    if (selection) {
      if (selection.id <= 0) return null;
      return { type: 'selection', selection: { ...selection } };
    }
    return { type: 'overview' };
  }

  function applyRememberedProjectPane(projectId: number): void {
    const pane = projectPaneMemory[projectId] ?? { type: 'overview' };
    selection = null;
    todoDetail = null;
    scratchpadRead = null;
    detailLoading = false;
    settingsOpen = false;
    todoBrowserOpen = false;
    scratchpadBrowserOpen = false;
    processOverviewKind = null;
    activeWorktreeOperationId = null;

    switch (pane.type) {
      case 'selection':
        selection = { ...pane.selection, projectId };
        detailLoading = selection.kind === 'todo' || selection.kind === 'scratchpad';
        return;
      case 'todos':
        todoBrowserOpen = true;
        return;
      case 'scratchpads':
        scratchpadBrowserOpen = true;
        return;
      case 'processes':
        processOverviewKind = pane.kind;
        return;
      case 'settings':
        settingsOpen = true;
        return;
      case 'overview':
        return;
    }
  }

  function clearSelection(): void {
    treeMultiSelection = null;
    selection = null;
    todoDetail = null;
    scratchpadRead = null;
    todoBrowserOpen = false;
    scratchpadBrowserOpen = false;
    processOverviewKind = null;
    activeWorktreeOperationId = null;
  }

  function openTodosBrowser(): void {
    if (!selectedProject) return;
    treeMultiSelection = null;
    settingsOpen = false;
    todoBrowserOpen = true;
    scratchpadBrowserOpen = false;
    processOverviewKind = null;
    activeWorktreeOperationId = null;
    selection = null;
    todoCommentFocusId = null;
    pendingTodoCommentFocus = null;
    todoDetail = null;
    scratchpadRead = null;
    if (todoNavigationIds.length === 0) {
      todoNavigationIds = (coordination?.todos ?? []).map((todo) => todo.id);
    }
  }

  function openScratchpadsBrowser(): void {
    if (!selectedProject) return;
    treeMultiSelection = null;
    settingsOpen = false;
    todoBrowserOpen = false;
    scratchpadBrowserOpen = true;
    processOverviewKind = null;
    activeWorktreeOperationId = null;
    selection = null;
    todoDetail = null;
    scratchpadRead = null;
  }

  function openProcessOverview(kind: ProcessKind): void {
    if (!selectedProject) return;
    treeMultiSelection = null;
    settingsOpen = false;
    todoBrowserOpen = false;
    scratchpadBrowserOpen = false;
    processOverviewKind = kind;
    activeWorktreeOperationId = null;
    selection = null;
    todoDetail = null;
    scratchpadRead = null;
  }

  function createFromProcessOverview(kind: ProcessKind): void {
    if (kind === 'agent') {
      void openAgentDialog();
    } else if (kind === 'terminal') {
      void spawnTerminal();
    } else {
      openAddCommand();
    }
  }

  async function renameBrowserScratchpad(
    scratchpad: ScratchpadSummary,
    name: string
  ): Promise<void> {
    await runBrowserScratchpadAction(scratchpad, 'coordination.scratchpad_rename', { name });
  }

  async function archiveBrowserScratchpad(scratchpad: ScratchpadSummary): Promise<void> {
    await runBrowserScratchpadAction(scratchpad, 'coordination.scratchpad_archive');
  }

  async function setSelectedScratchpadTags(
    tags: string[],
    expectedRevision: number
  ): Promise<void> {
    if (!selectedProject || selection?.kind !== 'scratchpad') return;
    const projectId = selectedProject.id;
    const scratchpadId = selection.id;
    detailBusy = true;
    try {
      await client.control('coordination.scratchpad_set_tags', {
        project_id: projectId,
        scratchpad_id: scratchpadId,
        expected_revision: expectedRevision,
        tags
      });
      await Promise.all([
        loadScratchpad(scratchpadId, false),
        refreshCoordination(projectId, false)
      ]);
    } catch (cause) {
      reportError(cause);
      throw cause;
    } finally {
      detailBusy = false;
    }
  }

  async function archiveSelectedScratchpad(expectedRevision: number): Promise<void> {
    if (!selectedProject || selection?.kind !== 'scratchpad') return;
    const projectId = selectedProject.id;
    detailBusy = true;
    try {
      await client.control('coordination.scratchpad_archive', {
        project_id: projectId,
        scratchpad_id: selection.id,
        expected_revision: expectedRevision
      });
      await refreshCoordination(projectId, false);
      openScratchpadsBrowser();
    } catch (cause) {
      reportError(cause);
      throw cause;
    } finally {
      detailBusy = false;
    }
  }

  async function deleteSelectedScratchpad(expectedRevision: number): Promise<void> {
    if (!selectedProject || selection?.kind !== 'scratchpad' || !scratchpadRead) return;
    if (!window.confirm(`Delete ${scratchpadRead.scratchpad.name}? This cannot be undone.`)) return;
    const projectId = selectedProject.id;
    detailBusy = true;
    try {
      await client.control('coordination.scratchpad_delete', {
        project_id: projectId,
        scratchpad_id: selection.id,
        expected_revision: expectedRevision
      });
      await refreshCoordination(projectId, false);
      openScratchpadsBrowser();
    } catch (cause) {
      reportError(cause);
      throw cause;
    } finally {
      detailBusy = false;
    }
  }

  async function deleteBrowserScratchpad(scratchpad: ScratchpadSummary): Promise<void> {
    if (!window.confirm(`Delete ${scratchpad.name}? This cannot be undone.`)) return;
    await runBrowserScratchpadAction(scratchpad, 'coordination.scratchpad_delete');
  }

  async function runBrowserScratchpadAction(
    scratchpad: ScratchpadSummary,
    method: 'coordination.scratchpad_rename' | 'coordination.scratchpad_archive' | 'coordination.scratchpad_delete',
    extra: Record<string, unknown> = {}
  ): Promise<void> {
    const projectId = selectedProject?.id;
    if (projectId === undefined || scratchpadBrowserBusyId !== null) return;
    scratchpadBrowserBusyId = scratchpad.id;
    try {
      await client.control(method, {
        project_id: projectId,
        scratchpad_id: scratchpad.id,
        expected_revision: scratchpad.revision,
        ...extra
      });
      await refreshCoordination(projectId, false);
    } catch (cause) {
      reportError(cause);
    } finally {
      scratchpadBrowserBusyId = null;
    }
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

  async function openWorktreeImport(project: Project): Promise<void> {
    const root = rootProjectFor(project) ?? project;
    if (root.repository_id === null) return;
    await refreshWorktreeMetadata(projects, false, true, root.repository_id);
    const list = worktreeLists[root.repository_id];
    const entries = list?.worktrees.filter((entry) => entry.can_adopt) ?? [];
    if (!list || entries.length === 0) return;
    importError = null;
    importOffer = { repository: list.repository, entries };
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
    removeWorktreeForceRequired = false;
    removeWorktreeDialog = { project, repository, entry };
  }

  async function confirmRemoveWorktree(
    deleteFromDisk: boolean,
    forceDirty: boolean,
    confirmBranch?: string
  ): Promise<void> {
    const state = removeWorktreeDialog;
    if (!state || removeWorktreeBusy) return;
    removeWorktreeBusy = true;
    removeWorktreeError = null;
    try {
      await client.removeWorktree({
        project_id: state.project.id,
        confirm_remove: true,
        confirm_stop_running: true,
        delete_from_disk: deleteFromDisk,
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
      if (cause instanceof DaemonRequestError && cause.code === 'dirty_worktree') {
        removeWorktreeForceRequired = true;
      }
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
      settingsOpen = false;
      activeWorktreeOperationId = null;
      clearSelection();
      void refreshWorktreeRepository(project, false);
      return;
    }
    appNavigation.navigate({ type: 'project', projectId: project.id }, 'project-rail');
  }

  function handleProjectDrop(drop: ReorderDrop): void {
    const next = moveProjectRailEntry(projectRailLayout, drop);
    void persistProjectRailLayout(next);
  }

  function moveProjectRailFromKeyboard(id: number, direction: ReorderDirection): void {
    const next = moveProjectRailEntryFromKeyboard(projectRailLayout, id, direction);
    void persistProjectRailLayout(next);
  }

  async function seedFlatProjectOrder(): Promise<void> {
    try {
      if (localStorage.getItem(flatProjectOrderStorageKey) === '1') return;
    } catch {
      // Continue with the one-time backend order when webview storage is unavailable.
    }

    const seeded = await persistProjectOrder(initialFlatProjectOrder(projects));
    if (!seeded) {
      flatProjectOrderChecked = false;
      return;
    }
    try {
      localStorage.setItem(flatProjectOrderStorageKey, '1');
    } catch {
      // The backend order is still valid for this session.
    }
  }

  async function persistProjectOrder(orderedIds: number[]): Promise<boolean> {
    const currentIds = projects.map((project) => project.id);
    if (projectReorderBusy) return false;
    if (orderedIds.join(',') === currentIds.join(',')) return true;
    const previous = projects;
    const byId = new Map(previous.map((project) => [project.id, project]));
    projects = orderedIds.map((id, sortOrder) => ({ ...byId.get(id)!, sort_order: sortOrder }));
    projectReorderBusy = true;
    try {
      projects = await client.reorderProjects(orderedIds);
      return true;
    } catch (cause) {
      projects = previous;
      reportError(cause);
      return false;
    } finally {
      projectReorderBusy = false;
    }
  }

  function applyProjectRailSnapshot(snapshot: ProjectRailSnapshot): void {
    projects = snapshot.projects;
    projectFolders = snapshot.folders;
  }

  async function persistProjectRailLayout(
    nextLayout: ReturnType<typeof buildProjectRailLayout>
  ): Promise<boolean> {
    if (projectReorderBusy) return false;
    if (projectRailLayoutSignature(nextLayout) === projectRailLayoutSignature(projectRailLayout)) {
      return true;
    }
    const previousProjects = projects;
    const previousFolders = projectFolders;
    const optimistic = applyProjectRailLayout(projects, projectFolders, nextLayout);
    projects = optimistic.projects;
    projectFolders = optimistic.folders;
    projectReorderBusy = true;
    try {
      applyProjectRailSnapshot(await updateProjectLayout(client, nextLayout));
      return true;
    } catch (cause) {
      projects = previousProjects;
      projectFolders = previousFolders;
      reportError(cause);
      return false;
    } finally {
      projectReorderBusy = false;
    }
  }

  function beginCreateProjectFolder(): void {
    folderMenuRequest = null;
    folderCreateValue = '';
    folderCreateOpen = true;
  }

  async function commitCreateProjectFolder(): Promise<void> {
    const name = folderCreateValue.trim();
    if (!name || projectReorderBusy) return;
    projectReorderBusy = true;
    try {
      applyProjectRailSnapshot(await createProjectFolder(client, name));
      folderCreateOpen = false;
      folderCreateValue = '';
    } catch (cause) {
      reportError(cause);
    } finally {
      projectReorderBusy = false;
    }
  }

  function beginRenameProjectFolder(folder: ProjectFolder): void {
    folderMenuRequest = null;
    folderRenameId = folder.id;
    folderRenameValue = folder.name;
  }

  async function commitRenameProjectFolder(): Promise<void> {
    const folderId = folderRenameId;
    const name = folderRenameValue.trim();
    if (folderId === null || !name || projectReorderBusy) return;
    projectReorderBusy = true;
    try {
      applyProjectRailSnapshot(await renameProjectFolder(client, folderId, name));
      folderRenameId = null;
      folderRenameValue = '';
    } catch (cause) {
      reportError(cause);
    } finally {
      projectReorderBusy = false;
    }
  }

  async function toggleProjectFolder(folder: ProjectFolder): Promise<void> {
    if (projectReorderBusy) return;
    const previous = projectFolders;
    projectFolders = projectFolders.map((candidate) =>
      candidate.id === folder.id ? { ...candidate, collapsed: !folder.collapsed } : candidate
    );
    projectReorderBusy = true;
    try {
      applyProjectRailSnapshot(
        await setProjectFolderCollapsed(client, folder.id, !folder.collapsed)
      );
    } catch (cause) {
      projectFolders = previous;
      reportError(cause);
    } finally {
      projectReorderBusy = false;
    }
  }

  async function confirmDeleteProjectFolder(request: ProjectFolderMenuRequest): Promise<void> {
    const childCopy = request.projectCount === 1 ? '1 project' : `${request.projectCount} projects`;
    if (!window.confirm(
      `Delete “${request.folder.name}”? ${childCopy} will return to the top level; no projects are deleted.`
    )) return;
    folderMenuRequest = null;
    projectReorderBusy = true;
    try {
      applyProjectRailSnapshot(await deleteProjectFolder(client, request.folder.id));
    } catch (cause) {
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

  async function persistTodoOrder(orderedIds: number[]): Promise<void> {
    if (!selectedProject || !coordination || coordinationReorderBusy) return;
    const projectId = selectedProject.id;
    const previous = coordination;
    const open = previous.todos.filter((todo) => !todo.completed);
    const currentIds = [...open]
      .sort((left, right) => left.sort_order - right.sort_order || left.id - right.id)
      .map((todo) => todo.id);
    if (orderedIds.join(',') === currentIds.join(',')) return;

    const slots = open.map((todo) => todo.sort_order).sort((left, right) => left - right);
    const sortById = new Map(orderedIds.map((id, index) => [id, slots[index]]));
    coordination = {
      ...previous,
      todos: previous.todos.map((todo) => ({
        ...todo,
        sort_order: sortById.get(todo.id) ?? todo.sort_order
      }))
    };
    coordinationReorderBusy = true;
    try {
      const next = await client.coordinationTodoReorder(projectId, orderedIds);
      if (selectedProject?.id === projectId) coordination = next;
    } catch (cause) {
      if (selectedProject?.id === projectId) coordination = previous;
      reportError(cause);
    } finally {
      coordinationReorderBusy = false;
    }
  }

  async function persistScratchpadOrder(orderedIds: number[]): Promise<void> {
    if (!selectedProject || !coordination || coordinationReorderBusy) return;
    const projectId = selectedProject.id;
    const previous = coordination;
    const currentIds = [...previous.scratchpads]
      .sort((left, right) => left.sort_order - right.sort_order || left.id - right.id)
      .map((scratchpad) => scratchpad.id);
    if (orderedIds.join(',') === currentIds.join(',')) return;

    const slots = previous.scratchpads
      .map((scratchpad) => scratchpad.sort_order)
      .sort((left, right) => left - right);
    const sortById = new Map(orderedIds.map((id, index) => [id, slots[index]]));
    coordination = {
      ...previous,
      scratchpads: previous.scratchpads.map((scratchpad) => ({
        ...scratchpad,
        sort_order: sortById.get(scratchpad.id) ?? scratchpad.sort_order
      }))
    };
    coordinationReorderBusy = true;
    try {
      const next = await client.coordinationScratchpadReorder(projectId, orderedIds);
      if (selectedProject?.id === projectId) coordination = next;
    } catch (cause) {
      if (selectedProject?.id === projectId) coordination = previous;
      reportError(cause);
    } finally {
      coordinationReorderBusy = false;
    }
  }

  function beginRename(project: Project): void {
    renameId = project.id;
    renameValue = projectDisplayName(project);
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

  function openProjectSettings(project: Project): void {
    projectSettingsProject = project;
  }

  async function saveProjectSettings(settings: ProjectSettingsInput): Promise<void> {
    const project = projectSettingsProject;
    if (!project || projectSettingsBusy) return;
    projectSettingsBusy = true;
    try {
      projects = await client.updateProjectSettings(
        project.id,
        settings.displayName,
        settings.icon,
        settings.iconColor
      );
      projectSettingsProject = null;
    } catch (cause) {
      reportError(cause);
    } finally {
      projectSettingsBusy = false;
    }
  }

  async function chooseProjectIconImage(): Promise<Project | null> {
    const project = projectSettingsProject;
    if (!project || projectSettingsBusy) return null;
    const sourcePath = await open({
      directory: false,
      multiple: false,
      title: 'Choose a project image',
      filters: [{
        name: 'Images',
        extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp', 'ico', 'svg']
      }]
    });
    if (typeof sourcePath !== 'string') return null;
    projectSettingsBusy = true;
    try {
      projects = await client.setProjectCustomIcon(project.id, sourcePath);
      const updated = projects.find((candidate) => candidate.id === project.id) ?? null;
      if (updated) projectSettingsProject = updated;
      return updated;
    } catch (cause) {
      reportError(cause);
      return null;
    } finally {
      projectSettingsBusy = false;
    }
  }

  async function refreshProjectIcon(): Promise<ProjectIconImage | null> {
    const project = projectSettingsProject;
    if (!project || projectSettingsBusy) return null;
    projectSettingsBusy = true;
    try {
      return await client.refreshProjectIcon(project.id);
    } catch (cause) {
      reportError(cause);
      return null;
    } finally {
      projectSettingsBusy = false;
    }
  }

  function showContextMenu(request: ContextMenuRequest): void {
    folderMenuRequest = null;
    treeRenameTarget = null;
    contextRequest = request;
  }

  function closeProjectFolderMenu(): void {
    const restoreFocus = folderMenuRequest?.restoreFocus ?? null;
    folderMenuRequest = null;
    if (restoreFocus) {
      queueMicrotask(() => {
        if (restoreFocus.isConnected) restoreFocus.focus();
      });
    }
  }

  function projectContextTarget(project: Project): Extract<ContextMenuTarget, { kind: 'project' }> {
    const worktree = worktreeEntryFor(project);
    return {
      kind: 'project',
      project,
      repository: worktreeRepositoryFor(project),
      worktree,
      importableWorktreeCount: worktree?.kind === 'main'
        ? worktreeListFor(project)?.worktrees.filter((entry) => entry.can_adopt).length ?? 0
        : 0
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
      case 'project-settings':
        openProjectSettings(project);
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
        openAddCommand();
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
      case 'import-worktrees':
        await openWorktreeImport(project);
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
          await openBrowserUrl(target.worktree.pull_request.url);
        }
        return;
      case 'open-herd-site':
        if (target.worktree?.site_url) {
          await openBrowserUrl(target.worktree.site_url);
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
        await startOrReviewProcess(process);
        return;
      case 'stop':
        await stopProcess(process);
        return;
      case 'restart':
        await restartProcess(process);
        return;
      case 'kill':
        if (openAgentCascadeDialog(process, 'kill')) return;
        if (!window.confirm(`Kill ${process.name} immediately? Unsaved terminal state may be lost.`)) return;
        await client.control('process.kill', { process_id: process.id, confirm_kill: true });
        await refreshProcesses(process.project_id);
        return;
      case 'close':
        if (openAgentCascadeDialog(process, 'close')) return;
        if (!window.confirm(`Close ${process.name}? Its saved terminal entry will be removed.`)) return;
        await client.closeProcess(process.id);
        if (selection?.id === process.id && isProcessSelection(selection)) clearSelection();
        await refreshProcesses(process.project_id);
        return;
      case 'edit-command':
        openEditCommand(process);
        return;
      case 'remove-command':
        await removeCommand(process);
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
    return projectDisplayName(project);
  }

  function projectTitle(project: Project): string {
    return projectRepositoryTitle(project, worktreeRepositoryFor(project));
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
    if (isDaemonRequestTimeoutError(cause)) {
      const seconds = cause.timeoutMs / 1_000;
      recordDaemonLog(
        'warning',
        `${cause.method} timed out`,
        `No response after ${seconds} ${seconds === 1 ? 'second' : 'seconds'}${connection.status === 'connected' ? '; connection stayed online.' : '.'}`
      );
      console.warn('workman daemon request timed out', cause.method, cause.timeoutMs);
      return;
    }
    error = cause instanceof Error ? cause.message : String(cause);
  }

  function recordDaemonLog(tone: DaemonLogTone, title: string, detail: string | null): void {
    daemonLog = appendDaemonLogEntry(daemonLog, {
      id: ++daemonLogSequence,
      tone,
      title,
      detail,
      occurredAt: Date.now()
    });
  }

  function clearDaemonLog(): void {
    daemonLog = [];
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
    if (versionRestarting || !startupUpdate || !updateActionAvailable(startupUpdate)) return;
    const copy = updateActionCopy(startupUpdate);
    if (!window.confirm(copy.dialogDescription)) return;
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
      <strong>{versionSkew ? 'Workman daemon is running an older version' : startupUpdateCopy?.bannerTitle}</strong>
      <span>{versionSkew ? 'Restarting loads this app’s control protocol and agent config.' : startupUpdateCopy?.bannerDescription} All running project processes will stop.</span>
    </div>
    <small>{versionSkew ? `app ${connection.app_build_id || 'current'} · daemon ${connection.daemon_build_id ?? 'legacy'}` : cliRecoveryRequired && !updateAvailable ? `Workman ${startupUpdate?.check.current}` : `current ${startupUpdate?.check.current} · latest ${startupUpdate?.check.latest}`}</small>
    <Button class="border-warning/50 text-warning hover:bg-warning/10" size="sm" variant="outline" disabled={versionRestarting} onclick={() => void (versionSkew ? restartOutdatedDaemon() : applyAvailableUpdate())}>
      {versionRestarting ? (versionSkew ? 'Restarting daemon…' : startupUpdateCopy?.busyLabel) : versionSkew ? 'Restart daemon' : startupUpdateCopy?.buttonLabel}
    </Button>
  </section>
{/if}

<AgentDoneToasts
  notices={agentDoneNotices}
  onOpen={openAgentDoneNotice}
  onDismiss={(id) => (agentDoneNotices = agentDoneNotices.filter((notice) => notice.id !== id))}
/>

{#snippet projectRailRow(project: Project, nested: boolean)}
  {@const repository = worktreeRepositoryFor(project)}
  {@const worktree = worktreeEntryFor(project)}
  {@const rowLabel = projectLabel(project)}
  {@const fullTitle = projectTitle(project)}
  {@const parentLabel = worktreeParentLabel(project, projects, repository?.name)}
  {@const projectKind = parentLabel !== null ? 'worktree' : project.repository_id !== null ? 'repository' : 'project'}
  {@const unreadAgentCount = projectUnreadAgentCount(project.id)}
  {@const activity = projectRailActivity(project)}
  {@const activityLabel = projectRailActivityLabel(project, activity)}
  <article
    class:active={project.selected}
    class:nested
    class="project-row group/project group/repository"
  >
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
          aria-label={`${activityLabel} · ${projectKind}${unreadAgentCount > 0 ? ` · ${unreadAgentCount} unread agents` : ''}`}
          use:reorderItem={{
            id: project.id,
            group: 'projects',
            disabled: busy || projectReorderBusy || renameId !== null || folderRenameId !== null || projects.length + projectFolders.length < 2,
            label: fullTitle,
            onDrop: handleProjectDrop,
            onKeyboardMove: moveProjectRailFromKeyboard
          }}
          onclick={() => selectProject(project)}
          oncontextmenu={(event) => showProjectPointerMenu(event, project)}
          onkeydown={(event) => showProjectKeyboardMenu(event, project)}
          data-context-kind="project"
          data-context-id={project.id}
        >
          <StatusIndicator
            class={projectRailCollapsed ? 'project-status-badge' : ''}
            tone={processActivityTone(activity.state)}
            label={activityLabel}
          />
          <span class="project-icon-anchor">
            <span class="project-kind-icon" aria-hidden="true">
              <ProjectIcon
                icon={project.icon}
                color={project.icon_color}
                image={project.icon_image?.data_url}
                fallback={parentLabel !== null ? 'worktree' : project.repository_id !== null ? 'repository' : 'project'}
                size={15}
              />
            </span>
          </span>
          <span class="project-copy">
            <strong>{rowLabel}</strong>
            {#if parentLabel !== null}
              <small
                class="worktree-parent"
                title={`Worktree of ${parentLabel}`}
                aria-label={`Worktree of ${parentLabel}`}
              >
                <GitBranchIcon size={11} strokeWidth={1.8} aria-hidden="true" />
                <span>Worktree of {parentLabel}</span>
              </small>
            {:else}
              <small>{project.path}</small>
            {/if}
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
          pullRequestCache={worktreeListFor(project)?.pull_requests ?? null}
          repositoryName={repository.name}
          refreshing={worktreeRefreshingRepositoryId === repository.id}
          onRefresh={() => void refreshWorktreeRepository(project, true)}
        />
      {/if}
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
  {#if project.parent_project_id === null}
    {#each worktreeOperationsFor(project.repository_id) as operation (operation.id)}
      <WorktreeOperationRow
        {operation}
        collapsed={projectRailCollapsed}
        onSelect={() => showWorktreeOperation(operation)}
      />
    {/each}
  {/if}
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
      {#if projectRailCollapsed}
        <div class="brand-mark">
          <img
            src={workmanMark24}
            srcset={`${workmanMark24} 1x, ${workmanMark48} 2x`}
            width="24"
            height="24"
            alt="Workman"
            draggable="false"
          />
        </div>
      {:else}
        <div class="brand-logo">
          <img src={workmanLogoWide} width="108" height="54" alt="Workman" draggable="false" />
        </div>
      {/if}
      <IconButton
        class="brand-collapse size-7 shrink-0 rounded border border-border bg-card"
        label={`${projectRailCollapsed ? 'Expand' : 'Collapse'} project rail`}
        shortcut="⌘B"
        onclick={toggleProjectRail}
      >
        {#snippet icon()}
          {#if projectRailCollapsed}<ChevronRightIcon size={15} />{:else}<ChevronLeftIcon size={15} />{/if}
        {/snippet}
      </IconButton>
      <div class="notification-slot">
        <NotificationsCenter
          {notifications}
          busy={notificationBusy}
          onRefresh={() => void refreshNotifications()}
          onOpen={openNotification}
          onMarkRead={(notification) => void markCenterNotificationRead(notification)}
          onMarkAll={() => void markAllNotificationsRead()}
        />
      </div>
    </header>

    <div class="rail-label"><span>Projects</span><small>{projectRailCount.toString().padStart(2, '0')}</small></div>
    <div class="project-list" aria-live="polite">
      {#if projects.length === 0 && projectFolders.length === 0 && connection.status === 'connected' && !busy}
        <div class="project-empty"><strong>No projects</strong><p>Register a folder or switch profiles.</p><Button size="sm" onclick={() => void registerProject()}>Register folder</Button><Button size="sm" variant="ghost" onclick={() => { selectSettingsSection('profiles'); settingsOpen = true; }}>Profiles</Button></div>
      {/if}
      {#if folderCreateOpen && !projectRailCollapsed}
        <ProjectFolderCreateRow
          value={folderCreateValue}
          busy={projectReorderBusy}
          onValueChange={(value) => (folderCreateValue = value)}
          onSubmit={() => void commitCreateProjectFolder()}
          onCancel={() => { folderCreateOpen = false; folderCreateValue = ''; }}
        />
      {/if}
      {#each projectRailLayout as entry (`${entry.kind}-${entry.id}`)}
        {#if entry.kind === 'project'}
          {@const project = projects.find((candidate) => candidate.id === entry.id)}
          {#if project}{@render projectRailRow(project, false)}{/if}
        {:else}
          {@const folder = projectFolders.find((candidate) => candidate.id === entry.id)}
          {#if folder}
            <ProjectFolderHeader
              {folder}
              projectCount={entry.project_ids.length}
              railCollapsed={projectRailCollapsed}
              busy={busy || projectReorderBusy || folderRenameId !== null || renameId !== null || projects.length + projectFolders.length < 2}
              renaming={folderRenameId === folder.id}
              renameValue={folderRenameValue}
              onRenameValueChange={(value) => (folderRenameValue = value)}
              onRenameSubmit={() => void commitRenameProjectFolder()}
              onRenameCancel={() => { folderRenameId = null; folderRenameValue = ''; }}
              onToggle={() => void toggleProjectFolder(folder)}
              onDrop={handleProjectDrop}
              onKeyboardMove={moveProjectRailFromKeyboard}
              onContextMenu={(request) => (folderMenuRequest = request)}
            />
            {#if !folder.collapsed}
              <div class="folder-children" aria-label={`${folder.name} projects`}>
                {#each entry.project_ids as projectId (projectId)}
                  {@const project = projects.find((candidate) => candidate.id === projectId)}
                  {#if project}{@render projectRailRow(project, true)}{/if}
                {/each}
              </div>
            {/if}
          {/if}
        {/if}
      {/each}
    </div>
    <footer class="project-footer">
      <Button class="min-w-0 flex-1 justify-center" variant="outline" size="sm" disabled={connection.status !== 'connected' || busy} onclick={() => void registerProject()}>
        <PlusIcon size={14} aria-hidden="true" /><span class="button-copy">Register project</span>
      </Button>
      <Button class="folder-button min-w-0 justify-center" variant="outline" size="sm" disabled={connection.status !== 'connected' || busy || projectReorderBusy} onclick={beginCreateProjectFolder}>
        <FolderPlusIcon size={14} aria-hidden="true" /><span class="button-copy">Folder</span>
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
        agentTools={registeredAgentTools}
        todos={coordination?.todos ?? []}
        scratchpads={coordination?.scratchpads ?? []}
        {selection}
        multiSelection={treeMultiSelection}
        collapsed={treeRailCollapsed}
        onSelect={(next) => void selectTreeItem(next)}
        onMultiSelectionChange={(next) => (treeMultiSelection = next)}
        onBulkAction={(action) => void runTreeBulkAction(action)}
        bulkBusy={treeBulkBusy || agentCascadeBusy}
        onCreateTodo={() => (dialog = 'todo')}
        onBrowseTodos={openTodosBrowser}
        onBrowseScratchpads={openScratchpadsBrowser}
        onBrowseProcesses={openProcessOverview}
        onAddAgent={() => void openAgentDialog()}
        onAddTerminal={() => void spawnTerminal()}
        onAddCommand={openAddCommand}
        onAddScratchpad={() => void createScratchpad()}
        {processBusyId}
        onStartProcess={(process) => void startOrReviewProcess(process)}
        onStopCommand={(process) => void stopProcess(process)}
        onRestartCommand={(process) => void restartProcess(process)}
        onOpenSettings={() => { todoBrowserOpen = false; scratchpadBrowserOpen = false; processOverviewKind = null; settingsOpen = true; dialog = null; }}
        onToggleCollapse={toggleTreeRail}
        reordering={processReorderBusy || coordinationReorderBusy}
        onReorderProcesses={(kind, orderedIds) => void persistProcessOrder(kind, orderedIds)}
        onReorderTodos={(orderedIds) => void persistTodoOrder(orderedIds)}
        onReorderScratchpads={(orderedIds) => void persistScratchpadOrder(orderedIds)}
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
    {#if settingsOpen}
      {#if error}
        <button class="error-banner" type="button" onclick={() => (error = null)}><span>{error}</span><strong>Dismiss</strong></button>
      {/if}
      <div class="item-viewer" role="region" aria-label="Settings detail">
        <SettingsPanel
          {client}
          project={selectedProject}
          {connection}
          onError={reportError}
          onProfileSwitched={() => window.location.reload()}
        />
      </div>
    {:else if selectedProject}
      {#if error}
        <button class="error-banner" type="button" onclick={() => (error = null)}><span>{error}</span><strong>Dismiss</strong></button>
      {/if}
      <div
        class="item-viewer"
        role="region"
        aria-label={`${frameItemLabel} detail`}
        oncontextmenu={showViewerContextMenu}
      >
        {#if activeWorktreeOperation}
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
              <TerminalView
                {client}
                process={selectedProcess}
                connected={connection.status === 'connected'}
                visible={documentVisible}
                busy={processBusyId === selectedProcess.id}
                onStart={(process) => void startOrReviewProcess(process)}
                onError={reportError}
                onUnfocus={unfocusSelectedProcess}
              />
              <ClaimedTodoOverlay claims={selectedProcess.claimed_todos ?? []} onOpen={openClaimedTodo} />
            </div>
          {/key}
        {:else if selection && isProcessSelection(selection)}
          <section class="pane-restoring" aria-live="polite" aria-busy="true">
            <LoaderCircleIcon size={17} strokeWidth={1.8} aria-hidden="true" />
            <div>
              <strong>{selection.label}</strong>
              <small>Restoring {selection.kind}…</small>
            </div>
          </section>
        {:else if todoBrowserOpen}
          <TodoBrowser
            project={selectedProject}
            todos={coordination?.todos ?? []}
            processes={treeProcesses}
            onSelect={(todo, navigationIds) => {
              todoNavigationIds = navigationIds;
              void selectTreeItem(projectTreeSelection('todo', todo.id, todo.project_id, todo.title));
            }}
            onCreate={() => (dialog = 'todo')}
          />
        {:else if scratchpadBrowserOpen}
          <ScratchpadBrowser
            project={selectedProject}
            scratchpads={coordination?.scratchpads ?? []}
            archivedScratchpads={coordination?.archived_scratchpads ?? []}
            busyId={scratchpadBrowserBusyId}
            onOpen={(scratchpad) => void selectTreeItem(projectTreeSelection('scratchpad', scratchpad.id, scratchpad.project_id, scratchpad.name))}
            onCreate={() => void createScratchpad()}
            onRename={(scratchpad, name) => void renameBrowserScratchpad(scratchpad, name)}
            onArchive={(scratchpad) => void archiveBrowserScratchpad(scratchpad)}
            onDelete={(scratchpad) => void deleteBrowserScratchpad(scratchpad)}
          />
        {:else if processOverviewKind}
          <ProcessOverview
            project={selectedProject}
            kind={processOverviewKind}
            {processes}
            busyId={processBusyId}
            onSelect={(process) => void selectTreeItem(projectTreeSelection(process.kind, process.id, process.project_id, processLabel(process)))}
            onCreate={() => createFromProcessOverview(processOverviewKind!)}
            onStart={(process) => void startOrReviewProcess(process)}
            onStop={(process) => void stopProcess(process)}
            onRestart={(process) => void restartProcess(process)}
            onEdit={openEditCommand}
            onRemove={(process) => void removeCommand(process)}
          />
        {:else if selection?.kind === 'todo'}
          <TodoDetailView
            detail={todoDetail}
            loading={detailLoading}
            busy={detailBusy}
            projectName={projectDisplayName(selectedProject)}
            todos={coordination?.todos ?? []}
            navigationIds={todoNavigationIds}
            projectOptions={projects.filter((project) => project.id !== selectedProject.id).map((project) => ({ id: project.id, name: projectDisplayName(project) }))}
            processes={treeProcesses}
            focusCommentId={todoCommentFocusId}
            onBack={openTodosBrowser}
            onNavigateTodo={(todoId) => void navigateToTodo(todoId)}
            onNavigateClaimant={selectProcessById}
            onUpdate={updateTodo}
            onComplete={(completed) => void completeTodo(completed)}
            onComment={(body) => void commentTodo(body)}
            onLock={setTodoLock}
            onSetBlockers={setTodoBlockers}
            onDelete={deleteSelectedTodo}
            onTransfer={transferSelectedTodo}
          />
        {:else if selection?.kind === 'scratchpad'}
          <ScratchpadDetailView
            read={scratchpadRead}
            loading={detailLoading}
            busy={detailBusy}
            projectName={projectDisplayName(selectedProject)}
            navigationIds={(scratchpadRead?.scratchpad.archived
              ? coordination?.archived_scratchpads ?? []
              : coordination?.scratchpads ?? []).map((scratchpad) => scratchpad.id)}
            focusRequest={scratchpadFocusRequest}
            onBack={openScratchpadsBrowser}
            onNavigateScratchpad={(scratchpadId) => void navigateToScratchpad(scratchpadId)}
            onRefresh={() => loadScratchpad(selection?.id ?? 0, false)}
            onSave={saveScratchpad}
            onSetTags={setSelectedScratchpadTags}
            onArchive={archiveSelectedScratchpad}
            onDelete={deleteSelectedScratchpad}
          />
        {:else}
          <ProjectOverview
            project={selectedProject}
            repository={worktreeRepositoryFor(selectedProject)}
            worktree={worktreeEntryFor(selectedProject)}
            pullRequestCache={worktreeListFor(selectedProject)?.pull_requests ?? null}
            refreshing={worktreeRefreshingRepositoryId === selectedProject.repository_id}
            counts={{
              agent: visibleProcesses.filter((process) => process.kind === 'agent').length,
              terminal: visibleProcesses.filter((process) => process.kind === 'terminal').length,
              todo: coordination?.todos.filter((todo) => !todo.completed).length ?? 0
            }}
            onRefresh={() => refreshWorktreeRepository(selectedProject!, true)}
            onBrowse={(target) => {
              if (target === 'todo') openTodosBrowser();
              else openProcessOverview(target);
            }}
          />
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
          daemonEvents={daemonLog}
          onUnfocus={unfocusSelectedProcess}
          onSelectProcess={selectProcessById}
          onClearDaemonEvents={clearDaemonLog}
          onError={reportError}
        />
      {/if}
    {:else}
      <div class="onboarding">
        <span>Local workspaces</span><h1>Register a project</h1><p>Choose a repository, or switch back to another profile.</p>
        <div class="flex gap-2">
          <Button disabled={connection.status !== 'connected' || busy} onclick={() => void registerProject()}><PlusIcon size={14} />Register project</Button>
          <Button variant="outline" disabled={connection.status !== 'connected'} onclick={() => { selectSettingsSection('profiles'); settingsOpen = true; }}>Profiles</Button>
        </div>
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

{#if folderMenuRequest}
  {@const request = folderMenuRequest}
  <ProjectFolderMenu
    {request}
    onRename={() => beginRenameProjectFolder(request.folder)}
    onDelete={() => void confirmDeleteProjectFolder(request)}
    onClose={closeProjectFolderMenu}
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

{#if projectSettingsProject}
  <ProjectSettingsDialog
    project={projectSettingsProject}
    busy={projectSettingsBusy}
    onSave={(settings) => void saveProjectSettings(settings)}
    onChooseImage={chooseProjectIconImage}
    onRefreshAutomatic={refreshProjectIcon}
    onClose={() => { if (!projectSettingsBusy) projectSettingsProject = null; }}
  />
{/if}

{#if shortcutsOpen}
  <KeyboardShortcuts onClose={closeShortcuts} />
{/if}

{#if agentCascadeRequest}
  <AgentCascadeDialog
    processes={agentCascadeRequest.processes}
    descendants={agentCascadeRequest.descendants}
    action={agentCascadeRequest.action}
    busy={agentCascadeBusy}
    error={agentCascadeError}
    onConfirm={() => void confirmAgentCascade()}
    onClose={() => { if (!agentCascadeBusy) agentCascadeRequest = null; }}
  />
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
    serverForceRequired={removeWorktreeForceRequired}
    onConfirm={(deleteFromDisk, forceDirty, confirmBranch) => void confirmRemoveWorktree(deleteFromDisk, forceDirty, confirmBranch)}
    onClose={() => { if (!removeWorktreeBusy) { removeWorktreeDialog = null; removeWorktreeForceRequired = false; } }}
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
        <div class="dialog-body">
          <label><span>Title</span><input bind:value={todoTitle} placeholder="What needs to happen?" use:focusDialogInput /></label>
          <label><span>Notes <small>optional</small></span><textarea bind:value={todoBody} rows="4" placeholder="Outcome, constraints, or context" use:submitOnEnter></textarea></label>
          <div class="dialog-row"><label><span>Priority</span><select bind:value={todoPriority}><option value="high">High</option><option value="medium">Medium</option><option value="low">Low</option></select></label><label><span>Tags</span><input bind:value={todoTags} placeholder="ui, follow-up" /></label></div>
          <div class="todo-blockers-field">
            <TodoBlockerPicker
              todos={coordination?.todos ?? []}
              selectedIds={todoBlockerIds}
              label="Blocked by · optional"
              description="Create this todo with its prerequisites already linked."
              compact
              onChange={(blockerIds) => { todoBlockerIds = blockerIds; }}
            />
          </div>
        </div>
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
        <footer><Button variant="outline" onclick={() => { dialog = null; todoBrowserOpen = false; scratchpadBrowserOpen = false; processOverviewKind = null; settingsOpen = true; }}>Open Settings</Button><Button variant="ghost" onclick={() => (dialog = null)}>Cancel</Button></footer>
      </section>
      </Dialog.Content>
    {/if}
  </Dialog.Root>
{/if}

{#if dialog === 'command' && selectedProject}
  <AddCommandDialog
    {client}
    project={selectedProject}
    initialProcess={commandDialogProcess}
    onPending={beginOptimisticCommand}
    onAdded={(process, optimisticId) => void commandAdded(process, optimisticId)}
    onFailed={failPendingProcess}
    onClose={() => { dialog = null; commandDialogProcess = null; }}
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

  .brand { position: relative; display: flex; min-height: 48px; align-items: center; gap: 5px; padding: 7px 7px 7px 9px; user-select: none; }
  .brand-logo { display: flex; min-width: 0; height: 30px; flex: 1; align-items: center; overflow: hidden; pointer-events: none; }
  .brand-logo img { display: block; width: 108px; max-width: 100%; height: auto; flex: none; }
  .brand-mark { display: grid; width: 24px; height: 24px; flex: none; place-items: center; pointer-events: none; }
  .brand-mark img { display: block; width: 24px; height: 24px; }
  .notification-slot { display: flex; flex: none; }

  .rail-label { display: flex; align-items: center; justify-content: space-between; min-height: 26px; border-top: 1px solid var(--border); padding: 4px 8px; color: var(--text-soft); font-size: var(--font-size-xs); font-weight: 680; letter-spacing: 0.04em; text-transform: uppercase; }
  .rail-label small { color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .project-list { min-height: 0; flex: 1; overflow-y: auto; padding: 2px 5px 6px; scrollbar-color: var(--border-strong) transparent; scrollbar-width: thin; }
  .folder-children { margin-left: 17px; border-left: 1px solid var(--border-strong); padding-left: 4px; }
  .project-row { position: relative; display: flex; min-height: 40px; align-items: center; margin: 1px 0; border: 1px solid transparent; border-radius: 3px; }
  .project-row.nested { min-height: 36px; }
  .project-row:hover { background: var(--popover); }
  .project-row.active { border-color: var(--border-strong); background: var(--accent); box-shadow: inset 2px 0 var(--muted-foreground); }
  .project-row > :global(.tooltip-anchor) { min-width: 0; flex: 1; align-self: stretch; }
  .project-select { position: relative; display: flex; width: 100%; height: 100%; min-width: 0; flex: 1; align-items: center; gap: 7px; border: 0; padding: 5px 7px; background: transparent; text-align: left; cursor: pointer; }
  .project-select:focus-visible { outline: 1px solid #737b84; outline-offset: -2px; background: var(--border); }
  .app-shell :global(.project-select[data-reorderable='true']) { cursor: grab; }
  .app-shell :global(.project-select[data-reorder-dragging='true']) { opacity: 0.42; cursor: grabbing; }
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
  .project-copy .worktree-parent { display: flex; min-width: 0; align-items: center; gap: var(--space-1); }
  .worktree-parent :global(svg) { flex: none; }
  .worktree-parent span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .project-unread-rollup { display: inline-flex; min-width: 20px; height: 18px; flex: none; align-items: center; justify-content: center; gap: 3px; border: 1px solid color-mix(in srgb, var(--notification-unread) 45%, var(--border)); border-radius: 999px; padding: 0 5px; color: var(--notification-unread-foreground); background: color-mix(in srgb, var(--notification-unread) 9%, var(--popover)); font: 650 var(--font-size-xs)/1 'JetBrains Mono Variable', monospace; }
  .project-unread-rollup > span { width: 5px; height: 5px; border-radius: 999px; background: var(--notification-unread); }
  .rename-form { display: flex; width: 100%; align-items: center; gap: 4px; padding: 4px; }
  .rename-form input { min-width: 0; flex: 1; border: 1px solid var(--border-strong); padding: 5px; background: var(--background); color: var(--text); font-size: var(--font-size-sm); }
  .project-empty { margin: 5px; border: 1px dashed var(--border-strong); padding: 10px; }
  .project-empty strong { color: var(--foreground); font-size: var(--font-size-sm); } .project-empty p { margin: 3px 0 8px; color: var(--muted); font-size: var(--font-size-sm); }
  .project-footer { display: flex; gap: var(--space-1); padding: 6px; border-top: 1px solid var(--border); }
  .project-footer :global(.folder-button) { flex: none; }

  .resize-handle { position: absolute; z-index: 8; top: 0; right: -3px; bottom: 0; width: 6px; border: 0; padding: 0; background: transparent; cursor: col-resize; touch-action: none; }
  .resize-handle::after { position: absolute; top: 0; right: 2px; bottom: 0; width: 1px; background: transparent; content: ''; }
  .resize-handle:hover::after, .resize-handle:focus-visible::after { background: var(--muted-foreground); }

  .project-rail.collapsed .brand { display: grid; min-height: 84px; grid-template-rows: 28px 28px 24px; align-content: center; justify-content: center; gap: 0; padding: 2px; }
  .project-rail.collapsed .rail-label span, .project-rail.collapsed .project-copy, .project-rail.collapsed .button-copy, .project-rail.collapsed .project-empty { display: none; }
  .project-rail.collapsed .brand-mark { grid-row: 1; place-self: center; }
  .project-rail.collapsed .notification-slot { grid-row: 2; place-self: center; }
  .project-rail.collapsed :global(.brand-collapse) { grid-row: 3; width: 24px; height: 24px; place-self: center; }
  .project-rail.collapsed .rail-label { justify-content: center; padding-inline: 0; }
  .project-rail.collapsed .project-list { display: flex; flex-direction: column; gap: 4px; padding: 6px 7px; }
  .project-rail.collapsed .folder-children { display: contents; }
  .project-rail.collapsed :global(.folder-row) { width: 100%; height: 36px; min-height: 36px; flex: 0 0 36px; margin: 0; }
  .project-rail.collapsed .project-row { width: 100%; height: 40px; min-height: 40px; flex: 0 0 40px; margin: 0; }
  .project-rail.collapsed .project-select { position: relative; width: 100%; height: 100%; flex: 0 0 100%; justify-content: center; gap: 0; padding: 4px; }
  .project-rail.collapsed .project-kind-icon { width: 30px; height: 30px; border: 1px solid var(--border-strong); border-radius: 3px; color: var(--foreground); background: var(--popover); }
  .project-rail.collapsed :global(.project-actions) { display: none; }
  .project-rail.collapsed :global(.project-status-badge) { position: absolute; z-index: 2; right: 2px; bottom: 2px; width: 12px; height: 12px; border: 1px solid var(--card); border-radius: 999px; background: var(--card); }
  .project-rail.collapsed :global(.project-status-badge > span) { width: 6px; height: 6px; }
  .project-rail.collapsed .project-unread-rollup { position: absolute; z-index: 2; top: 2px; right: 2px; min-width: 14px; height: 14px; gap: 0; padding: 0 3px; border-color: var(--notification-unread); font-size: 9px; }
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
  .pane-restoring { display: flex; height: 100%; align-items: center; justify-content: center; gap: 10px; color: var(--muted-foreground); }
  .pane-restoring > :global(svg) { color: var(--agent-state-working); animation: pane-restoring-spin 800ms linear infinite; }
  .pane-restoring strong, .pane-restoring small { display: block; font-family: var(--terminal-font-family); }
  .pane-restoring strong { color: var(--foreground); font-size: var(--font-size-sm); font-weight: 600; }
  .pane-restoring small { margin-top: 3px; font-size: var(--font-size-xs); }
  @keyframes pane-restoring-spin { to { transform: rotate(360deg); } }
  .onboarding { display: grid; width: min(440px, calc(100% - 36px)); place-items: start; align-content: center; margin: auto; }
  .onboarding > span { color: var(--muted); font-size: var(--font-size-sm); text-transform: uppercase; }
  .onboarding h1 { margin: 5px 0 0; color: var(--foreground); font-size: 28px; }
  .onboarding p { margin: 7px 0 13px; color: var(--text-soft); font-size: 12px; }

  .dialog-surface { display: grid; width: 100%; min-height: 0; max-height: calc(100dvh - 2rem); grid-template-rows: auto minmax(0, 1fr) auto; color: var(--foreground); }
  .dialog-surface > header { display: flex; align-items: start; justify-content: space-between; border-bottom: 1px solid var(--border); padding: 11px 13px 9px; }
  .dialog-surface > header span, .dialog-surface label > span { color: var(--muted-foreground); font: 700 var(--font-size-xs) 'JetBrains Mono Variable', monospace; text-transform: uppercase; }
  .dialog-surface h2 { margin: 3px 0 0; color: var(--foreground); font-size: 17px; }
  .dialog-body { min-height: 0; overflow-y: auto; overscroll-behavior: contain; padding: 10px 13px 12px; }
  .dialog-body > label + label, .dialog-row { margin-top: 10px; }
  .dialog-surface label { display: grid; gap: 4px; }
  .dialog-surface label small { color: var(--muted-foreground); font: inherit; }
  .dialog-surface input, .dialog-surface textarea, .dialog-surface select { width: 100%; border: 1px solid var(--input); border-radius: var(--radius); outline: 0; padding: 7px 8px; background: var(--background); color: var(--text); font-size: var(--font-size-sm); }
  .dialog-surface textarea { max-height: 192px; resize: none; line-height: 1.4; }
  .dialog-row { display: grid; grid-template-columns: 0.45fr 1fr; gap: 8px; }
  .todo-blockers-field { margin-top: 10px; border-top: 1px solid var(--border); padding-top: 10px; }
  .dialog-surface > footer { display: flex; justify-content: flex-end; gap: 6px; border-top: 1px solid var(--border); padding: 8px 13px; }
  .agent-choices { display: grid; min-height: 0; overflow-y: auto; overscroll-behavior: contain; padding: 5px; }
  .agent-choices > button { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 2px 8px; border: 0; border-bottom: 1px solid var(--border); padding: 8px; background: transparent; color: var(--foreground); text-align: left; cursor: pointer; }
  .agent-choices > button:hover { background: var(--accent); }
  .agent-choices strong { font-size: var(--font-size-sm); } .agent-choices small { overflow: hidden; color: var(--muted); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; text-overflow: ellipsis; white-space: nowrap; }
  .agent-choices span { grid-row: 1 / 3; grid-column: 2; align-self: center; color: var(--text-soft); font-size: var(--font-size-sm); }
  .agent-choices p { margin: 0; padding: 13px; color: var(--text-soft); font-size: var(--font-size-sm); }

  @media (max-width: 760px) {
    .project-copy small { display: none; }
  }
</style>
