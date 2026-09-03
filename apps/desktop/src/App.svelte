<script lang="ts">
  import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';
  import FolderPlusIcon from '@lucide/svelte/icons/folder-plus';
  import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
  import MoreHorizontalIcon from '@lucide/svelte/icons/more-horizontal';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import { open } from '@tauri-apps/plugin-dialog';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount, tick } from 'svelte';

  import AddCommandDialog from './lib/AddCommandDialog.svelte';
  import AddProjectDialog from './lib/AddProjectDialog.svelte';
  import AgentCascadeDialog from './lib/AgentCascadeDialog.svelte';
  import AgentDoneToasts, { type AgentDoneNotice } from './lib/AgentDoneToasts.svelte';
  import IconButton from './lib/components/ds/IconButton.svelte';
  import TooltipLabel from './lib/components/ds/TooltipLabel.svelte';
  import { Button } from './lib/components/ui/button';
  import ContextMenu from './lib/ContextMenu.svelte';
  import ClaimedTodoOverlay from './lib/ClaimedTodoOverlay.svelte';
  import ConfirmationDialog from './lib/ConfirmationDialog.svelte';
  import KeyboardShortcuts from './lib/KeyboardShortcuts.svelte';
  import KeepAwakeControl from './lib/KeepAwakeControl.svelte';
  import { shouldSubscribeProcessStatuses } from './lib/keepAwake';
  import NotificationsCenter from './lib/NotificationsCenter.svelte';
  import NewAgentDraftPanel from './lib/NewAgentDraftPanel.svelte';
  import NewCommandDraftPanel from './lib/NewCommandDraftPanel.svelte';
  import NewTodoDraftPanel from './lib/NewTodoDraftPanel.svelte';
  import OptimisticProcessPanel from './lib/OptimisticProcessPanel.svelte';
  import ProcessOverview from './lib/ProcessOverview.svelte';
  import ProcessStatusBar from './lib/ProcessStatusBar.svelte';
  import ProjectIcon from './lib/ProjectIcon.svelte';
  import ProjectOperationStatus from './lib/ProjectOperationStatus.svelte';
  import { PROJECT_RAIL_TOOLTIP_DELAY_MS } from './lib/projectRailTooltip';
  import ProjectKindIndicators from './lib/ProjectKindIndicators.svelte';
  import ProjectFolderCreateRow from './lib/ProjectFolderCreateRow.svelte';
  import ProjectFolderHeader from './lib/ProjectFolderHeader.svelte';
  import ProjectFolderMenu from './lib/ProjectFolderMenu.svelte';
  import ProjectFolderSettingsDialog from './lib/ProjectFolderSettingsDialog.svelte';
  import ProjectOverview from './lib/ProjectOverview.svelte';
  import RegisterProjectDialog from './lib/RegisterProjectDialog.svelte';
  import ProjectSettingsDialog from './lib/ProjectSettingsDialog.svelte';
  import ProjectTree from './lib/ProjectTree.svelte';
  import RecordedFeedbackBrowser from './lib/RecordedFeedbackBrowser.svelte';
  import RecordedFeedbackDetailView from './lib/RecordedFeedbackDetailView.svelte';
  import RecordedFeedbackPreflight from './lib/RecordedFeedbackPreflight.svelte';
  import QuickJumpPalette from './lib/QuickJumpPalette.svelte';
  import QuickPromptPalette from './lib/QuickPromptPalette.svelte';
  import ScratchpadBrowser from './lib/ScratchpadBrowser.svelte';
  import ScratchpadDetailView from './lib/ScratchpadDetailView.svelte';
  import SettingsPanel from './lib/SettingsPanel.svelte';
  import {
    applyUpdate,
    autoImportTerminalProfile,
    checkForUpdates,
    type UpdateInstallReport,
    type UpdateProgress,
    type UpdateStage,
    type UpdateStatus
  } from './lib/settings';
  import { updateActionAvailable, updateActionCopy } from './lib/updateRecovery';
  import {
    canPresentUpdateProgress,
    idleUpdateFlow,
    manualUpdateFlow,
    updateBannerState,
    updateCompletionAction,
    type UpdateFlow
  } from './lib/updateFlow';
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
  import workmanMark24 from '../../../assets/branding/workman-icon-cropped-24-transparent.png';
  import workmanMark48 from '../../../assets/branding/workman-icon-cropped-48-transparent.png';
  import workmanLogoWide from '../../../assets/branding/workman-logo-wide-transparent.png';
  import {
    getAgentToolsStore,
    type AgentTool,
    type SpawnAgentInput,
    type SpawnAgentResult
  } from './lib/agentTools';
  import type { QuickPrompt } from './lib/quickPrompts';
  import type { CommandInput } from './lib/commandCreation';
  import {
    getAgentTemplatesStore,
    type AgentTemplate
  } from './lib/agentTemplates';
  import {
    planAgentCascade,
    type AgentCascadeAction,
    type AgentCascadeRequest
  } from './lib/agentCascade';
  import type { ClaimedTodo } from './lib/claimedTodos';
  import type {
    CoordinationSnapshot,
    NewScratchpadCommentInput,
    NewTodoInput,
    ScratchpadRead,
    ScratchpadSummary,
    TodoDetail,
    UpdateTodoInput
  } from './lib/coordination';
  import {
    creationDraftHasContent,
    creationDraftLabel,
    creationDraftsForCycle,
    createCreationDraft,
    findUntouchedCreationDraft,
    loadCreationDrafts,
    nextCreationDraftId,
    pruneCreationDraftsToProjects,
    saveCreationDrafts,
    type AgentCreationDraft,
    type CommandCreationDraft,
    type CreationDraft,
    type CreationDraftKind,
    type TodoCreationDraft
  } from './lib/creationDrafts';
  import {
    contextMenuRequest,
    describeContextMenu,
    dispatchTerminalContextAction,
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
  import {
    findHotkeyAction,
    hotkeyDisplayLabel,
    hotkeyPreferences,
    projectHotkeyActions,
    projectHotkeyIndex,
    recordingHotkeyActions,
    type CreationHotkeyAction
  } from './lib/hotkeys';
  import { recordingHotkeyBindings } from './lib/recordedFeedbackHotkeys';
  import { deliverAgentInput, type AgentInputStep } from './lib/agentInputDelivery';
  import { agentDraftPromptInputSteps } from './lib/agentAttachmentDrafts';
  import { feedbackAgentInputSteps } from './lib/recordedFeedbackAgentDelivery';
  import {
    recordedFeedbackCapability,
    recordedFeedbackPreferences,
    recordedFeedbackSupported,
    refreshRecordedFeedbackCapability,
    showRecordedFeedbackSection
  } from './lib/recordedFeedbackAvailability';
  import { renderRecordedFeedbackPrompt } from './lib/recordedFeedbackPrompt';
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
    emptyWorkspaceViewHistory,
    recordWorkspaceView,
    sameWorkspaceView,
    swapWorkspaceViews,
    type WorkspaceViewHistory,
    type WorkspaceViewState
  } from './lib/workspaceViewHistory';
  import {
    deliverNativeNotification,
    listenForNativeNotificationActions,
    refreshNativeNotificationPermission,
    syncDockUnreadBadge
  } from './lib/nativeNotifications';
  import {
    sidebarIdentityColorValue,
    type ProjectSettingsInput
  } from './lib/projectAppearance';
  import { registrationTitleForPath, resolvedProjectTitle } from './lib/projectTitles';
  import {
    createProjectFolder,
    deleteProjectFolder,
    loadProjectRail,
    renameProjectFolder,
    setProjectFolderCollapsed,
    updateProjectFolderSettings,
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
    type ProjectFolderMenuRequest,
    type ProjectFolderSettingsInput
  } from './lib/projectFolders';
  import {
    NATIVE_MENU_EVENT,
    requestNativeUpdateCheck,
    syncNativeMenuAccelerators,
    type NativeMenuAction
  } from './lib/nativeMenu';
  import { primaryModifier, secondaryModifier } from './lib/primaryModifier';
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
    agentCanReceiveFeedback,
    agentCanReceiveInitialTurn,
    type NativeFeedbackFinished,
    type NativeFeedbackPreflight,
    type NativeFeedbackSession,
    type NativeFeedbackSnapshot,
    type NativeFeedbackTranscript,
    type RecordedFeedback,
    type RecordedFeedbackBlock,
    type RecordedFeedbackSummary,
    type RecordedFeedbackView
  } from './lib/recordedFeedback';
  import { compileFeedbackTimeline } from './lib/recordedFeedbackTimeline';
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
    projectKindActivity,
    type ProjectKindActivityRollup
  } from './lib/processActivity';
  import {
    initialFlatProjectOrder,
    projectDisplayName,
    projectRepositoryTitle,
    pullRequestsForWorktree,
    worktreeParentLabel,
    type WorktreeBranchOption,
    type WorktreeCreateConflict,
    type WorktreeDialogSubmission,
    type WorktreeEntry,
    type WorktreeList,
    type WorktreeRemoval,
    type WorktreeRefOption,
    type WorktreeRefValidation,
    type WorktreeRepository
  } from './lib/worktrees';
  import {
    beginWorktreeOperation,
    dismissWorktreeOperation,
    failWorktreeOperation,
    standaloneWorktreeOperations,
    worktreeOperations,
    worktreeOperationForProject,
    worktreeOperationStateLabel,
    type WorktreeOperation
  } from './lib/worktreeProgress';

  const client = new DaemonClient();
  const agentToolsStore = getAgentToolsStore(client);
  const agentTemplatesStore = getAgentTemplatesStore(client);
  const projectRailBounds = { min: 176, max: 340 };
  const treeRailBounds = { min: 220, max: 420 };
  const collapsedProjectRailWidth = 58;
  const collapsedTreeRailWidth = 54;
  const flatProjectOrderStorageKey = 'workman.project-rail.flat-order.v1';

  let projects = $state<Project[]>([]);
  let projectFolders = $state<ProjectFolder[]>([]);
  let processes = $state<ProcessView[]>([]);
  let profileProcesses = $state<ProcessView[]>([]);
  let documentVisible = $state(true);
  let terminalProfileAutoImportStarted = false;
  let keepAwakeArmed = $state(false);
  let autoKeepAwakeEnabled = $state(false);
  let keepAwakeSupported = $state(false);
  let optimisticProcesses = $state<OptimisticProcess[]>([]);
  let nextOptimisticProcessId = -1;
  let coordination = $state<CoordinationSnapshot | null>(null);
  let feedbackSummaries = $state<RecordedFeedbackSummary[]>([]);
  let feedbackDetail = $state<RecordedFeedback | null>(null);
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
  let folderSettingsFolder = $state<ProjectFolder | null>(null);
  let folderSettingsBusy = $state(false);
  let projectSettingsProject = $state<Project | null>(null);
  let projectSettingsBusy = $state(false);
  let settingsOpen = $state(false);
  let todoBrowserOpen = $state(false);
  let todoNavigationIds = $state<number[]>([]);
  let scratchpadBrowserOpen = $state(false);
  let feedbackBrowserOpen = $state(false);
  let feedbackBrowserView = $state<RecordedFeedbackView>('active');
  let feedbackBrowserBusyId = $state<number | null>(null);
  let feedbackPreflightOpen = $state(false);
  let feedbackPreflight = $state<NativeFeedbackPreflight | null>(null);
  let feedbackPreflightLoading = $state(false);
  let feedbackModelInstalling = $state(false);
  let feedbackStarting = $state(false);
  let feedbackPreflightError = $state<string | null>(null);
  let feedbackModelProgress = $state<{ downloaded: number; total: number } | null>(null);
  let activeFeedbackSession = $state<NativeFeedbackSession | null>(null);
  const feedbackLeaseOwner = `desktop-${crypto.randomUUID()}`;
  let feedbackEventQueue = Promise.resolve();
  const pendingFeedbackTranscripts = new Map<number, NativeFeedbackTranscript>();
  let feedbackLeaseErrorId: number | null = null;
  let processOverviewKind = $state<ProcessKind | null>(null);
  let scratchpadBrowserBusyId = $state<number | null>(null);
  let trustReview = $state<TrustReview | null>(null);
  let trustBusy = $state(false);
  let projectRailWidth = $state(238);
  let projectRailCollapsed = $state(false);
  let treeRailWidth = $state(280);
  let treeRailCollapsed = $state(false);

  let dialog = $state<'command' | null>(null);
  let commandDialogProcess = $state<ProcessView | null>(null);
  let creationDrafts = $state<CreationDraft[]>([]);
  let activeProfileId = $state<number | null>(null);
  let creationDraftsLoaded = $state(false);
  let draftFocusRequestId = $state<number | null>(null);
  let creationDraftSaveTimer: ReturnType<typeof setTimeout> | null = null;
  let pendingCreationDraftSave: {
    profileId: number;
    drafts: readonly CreationDraft[];
  } | null = null;
  let scratchpadFocusRequest = $state(0);
  let agentTools = $state<AgentTool[]>([]);
  let registeredAgentTools = $state<AgentTool[]>([]);
  let agentTemplates = $state<AgentTemplate[]>([]);
  let agentToolsLoading = $state(false);
  let agentDraftMetadataLoaded = $state(false);
  let agentDraftMetadataPromise: Promise<void> | null = null;

  $effect(() => agentToolsStore.subscribe((snapshot) => {
    registeredAgentTools = snapshot.tools;
    agentTools = snapshot.tools.filter((tool) => tool.enabled);
  }));
  $effect(() => agentTemplatesStore.subscribe((snapshot) => {
    agentTemplates = snapshot.templates;
  }));
  $effect(() => {
    if (connection.status !== 'connected') return;
    const request = shouldSubscribeProcessStatuses(
      documentVisible,
      keepAwakeArmed,
      autoKeepAwakeEnabled
    )
      ? client.subscribeProcessStatuses()
      : client.unsubscribeProcessStatuses();
    void request.catch(reportError);
  });
  $effect(() => {
    if (connection.status !== 'connected' || terminalProfileAutoImportStarted) return;
    terminalProfileAutoImportStarted = true;
    void autoImportTerminalProfile(client).catch(() => {
      // Native profile discovery is best-effort; Settings retains the explicit import action.
    });
  });
  let versionRestarting = $state(false);
  let startupUpdate = $state<UpdateStatus | null>(null);
  let startupUpdatePort = $state<number | null>(null);
  let updateFlow = $state<UpdateFlow>(idleUpdateFlow);
  let nativeRelaunchAvailable = $state(false);
  let nativeRelaunchAppBundle = $state<string | null>(null);
  let updateBannerDismissed = $state(false);
  let updatedVersionNotice = $state<string | null>(null);
  let justUpdatedVersion: string | null = null;
  let daemonOnlyRestartPending = false;
  let updateInstallActive = false;
  let installedUpdateReport: UpdateInstallReport | null = null;
  let updateRestartTimer: ReturnType<typeof setTimeout> | null = null;
  let updateProgressPresentedAt = 0;
  let updateProgressTimer: ReturnType<typeof setTimeout> | null = null;
  let updateProgressQueue: UpdateProgress[] = [];
  let updateProgressWaiters: Array<() => void> = [];
  const updateStageMinimumMs = 300;
  const updateRestartTimeoutMs = 20_000;
  let quickJumpOpen = $state(false);
  let quickPromptOpen = $state(false);
  let shortcutsOpen = $state(false);
  let projectHotkeyHintsVisible = $state(false);
  let keepAwakeOpen = $state(false);
  let quickJumpLoading = $state(false);
  let quickJumpRecentKeys = $state<string[]>([]);
  let navigationIndex = $state<Record<number, NavigationProjectSnapshot>>({});
  let projectPaneMemory = $state<ProjectPaneMemory>(loadProjectPaneMemory());
  let workspaceViewHistory = $state<WorkspaceViewHistory>(emptyWorkspaceViewHistory);
  let workspaceViewSwapTarget: WorkspaceViewState | null = null;
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
  let projectRailPopoverKey = $state<string | null>(null);
  let projectRailTooltipOpenId = $state<number | null>(null);
  let treeRenameTarget = $state<ContextMenuTarget | null>(null);
  let worktreeLists = $state<Record<number, WorktreeList>>({});
  let worktreeRefreshingRepositoryId = $state<number | null>(null);
  let addProjectDialogOpen = $state(false);
  let addProjectFolderBusy = $state(false);
  let addProjectWorktreeBusyId = $state<number | null>(null);
  let registerProjectDialog = $state<{ path: string; defaultTitle: string } | null>(null);
  let registerProjectBusy = $state(false);
  let registerProjectError = $state<string | null>(null);
  let worktreeDialog = $state<{
    mode: 'create' | 'fork' | 'adopt';
    sourceProject: Project;
    repository: WorktreeRepository;
    sourceEntry: WorktreeEntry | null;
  } | null>(null);
  let worktreeDialogBusy = $state(false);
  let worktreeDialogError = $state<string | null>(null);
  let worktreeDialogConflict = $state<WorktreeCreateConflict | null>(null);
  let branchOptions = $state<WorktreeBranchOption[]>([]);
  let worktreeRefOptions = $state<WorktreeRefOption[]>([]);
  let worktreeDefaultRef = $state<string | null>(null);
  let originBranchesLoading = $state(false);
  let activeWorktreeOperationId = $state<string | null>(null);
  let agentDoneNotices = $state<AgentDoneNotice[]>([]);
  let agentDoneNoticeSequence = 0;
  let notifications = $state<Notification[]>([]);
  let notificationBusy = $state(false);
  const notificationIdleWaiters = new Set<() => void>();
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
    repository: WorktreeRepository | null;
    entry: WorktreeEntry | null;
  } | null>(null);
  let removeWorktreeBusy = $state(false);
  let removeWorktreeError = $state<string | null>(null);
  let removeWorktreeForceRequired = $state(false);
  let removeWorktreeNotice = $state<string | null>(null);
  let terminalView = $state<{
    insertQuickPrompt: (text: string, submit?: boolean) => boolean;
    focusInput: () => void;
    openSearch: () => void;
  } | null>(null);
  let importOffer = $state<{ repository: WorktreeRepository; entries: WorktreeEntry[] } | null>(null);
  let importBusyPath = $state<string | null>(null);
  let importError = $state<string | null>(null);
  let confirmationDialog = $state<{
    title: string;
    description: string;
    confirmLabel: string;
    destructive: boolean;
    resolve: (confirmed: boolean) => void;
  } | null>(null);

  let selectedProject = $derived(projects.find((project) => project.selected) ?? null);
  let projectRailLayout = $derived(buildProjectRailLayout(projects, projectFolders));
  let projectHotkeyProjectIds = $derived(
    projectRailLayout.flatMap((entry) =>
      entry.kind === 'project' ? [entry.id] : entry.project_ids
    )
  );
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
  let selectedDraft = $derived.by(() => {
    const currentSelection = selection;
    if (currentSelection?.kind !== 'draft') return null;
    return creationDrafts.find((draft) =>
      draft.id === currentSelection.id && draft.projectId === currentSelection.projectId
    ) ?? null;
  });
  $effect(() => {
    if (selectedDraft?.kind === 'agent') void ensureAgentDraftMetadata();
  });
  let activeWorktreeOperation = $derived(
    $worktreeOperations.find((operation) => operation.id === activeWorktreeOperationId) ?? null
  );
  let projectRailCount = $derived(
    projects.length
      + standaloneWorktreeOperations($worktreeOperations, projects).filter((operation) =>
        operation.status === 'pending' || operation.status === 'running'
      ).length
  );
  let treeProcesses = $derived([
    ...visibleProcesses.filter((process) => process.kind === 'agent'),
    ...visibleProcesses.filter((process) => process.kind === 'terminal'),
    ...visibleProcesses.filter((process) => process.kind === 'command')
  ]);
  let feedbackTargetProcesses = $derived.by(() => {
    const projectId = selectedProject?.id;
    if (projectId === undefined) return [];
    const byId = new Map<number, ProcessView>();
    for (const process of navigationIndex[projectId]?.processes ?? []) {
      if (process.kind === 'agent') byId.set(process.id, process);
    }
    for (const process of profileProcesses) {
      if (process.project_id === projectId && process.kind === 'agent') byId.set(process.id, process);
    }
    for (const process of treeProcesses) {
      if (process.kind === 'agent') byId.set(process.id, process);
    }
    return [...byId.values()];
  });
  let treeCycleSelections = $derived.by(() => {
    if (!selectedProject) return [];
    const projectId = selectedProject.id;
    return [
      ...visibleProcesses
        .filter((process) => process.kind === 'agent')
        .map((process) => projectTreeSelection('agent', process.id, projectId, processLabel(process))),
      ...creationDraftsForCycle(creationDrafts, projectId, 'agent')
        .map((draft) => projectTreeSelection('draft', draft.id, projectId, creationDraftLabel(draft))),
      ...visibleProcesses
        .filter((process) => process.kind === 'terminal')
        .map((process) => projectTreeSelection('terminal', process.id, projectId, processLabel(process))),
      ...visibleProcesses
        .filter((process) => process.kind === 'command')
        .map((process) => projectTreeSelection('command', process.id, projectId, processLabel(process))),
      ...creationDraftsForCycle(creationDrafts, projectId, 'command')
        .map((draft) => projectTreeSelection('draft', draft.id, projectId, creationDraftLabel(draft)))
    ];
  });
  let projectOverviewOpen = $derived(
    selectedProject !== null &&
      !settingsOpen &&
      activeWorktreeOperation === null &&
      selectedOptimisticProcess === null &&
      selectedProcess === null &&
      !todoBrowserOpen &&
      !scratchpadBrowserOpen &&
      !feedbackBrowserOpen &&
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
            : feedbackBrowserOpen
              ? 'Feedback'
              : processOverviewKind
                ? `${processOverviewKind[0].toUpperCase()}${processOverviewKind.slice(1)}s`
                : projectOverviewOpen && selectedProject
                  ? projectLabel(selectedProject)
                  : (selection?.label ?? 'Project')
  );
  let windowTitle = $derived(
    settingsOpen
      ? 'Settings — Workman'
      : selectedProject && (selectedProcess || selectedDraft)
        ? `${projectLabel(selectedProject)}: ${selectedProcess?.name ?? creationDraftLabel(selectedDraft!)}`
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
  let updateBanner = $derived(updateBannerState(startupUpdate, updateFlow));
  let updateFlowBlocksSkew = $derived(
    updateFlow.kind === 'running'
      || updateFlow.kind === 'restarting'
      || (updateFlow.kind === 'needs-restart' && !updateBannerDismissed)
  );
  let showVersionSkew = $derived(versionSkew && !updateFlowBlocksSkew);
  let showVersionBanner = $derived(showVersionSkew || (updateBanner.visible && !updateBannerDismissed));

  $effect(() => {
    void syncNativeMenuAccelerators($hotkeyPreferences).catch(() => {
      // Browser-only development and older desktop shells keep webview hotkeys functional.
    });
  });
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
        && (operation.project || operation.removal)
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
    if (projectId === null) {
      processes = [];
      optimisticProcesses = [];
      coordination = null;
      selection = null;
      todoDetail = null;
      scratchpadRead = null;
      settingsOpen = false;
      todoBrowserOpen = false;
      scratchpadBrowserOpen = false;
      feedbackBrowserOpen = false;
      processOverviewKind = null;
      loadedProjectId = null;
      return;
    }
    // Keep the active xterm and its process snapshot mounted while the daemon reconnects. This
    // lets TerminalView continue accepting physical input into the reconnect queue instead of
    // destroying the focused textarea at the exact moment the transport becomes unhealthy.
    if (!connected) return;
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
    const projectId = selectedProject?.id ?? null;
    if (
      connection.status !== 'connected'
      || projectId === null
      || loadedProjectId !== projectId
    ) return;
    const pane = currentProjectPane();
    if (!pane) return;
    const next = { projectId, pane };
    if (workspaceViewSwapTarget) {
      if (!sameWorkspaceView(workspaceViewSwapTarget, next)) return;
      workspaceViewSwapTarget = null;
    }
    workspaceViewHistory = recordWorkspaceView(workspaceViewHistory, next);
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

  $effect(() => {
    const capability = $recordedFeedbackCapability;
    if (!capability.checked || capability.supported) return;
    feedbackPreflightOpen = false;
    feedbackBrowserOpen = false;
    if (selection?.kind === 'feedback') clearSelection();
  });

  onMount(() => {
    justUpdatedVersion = localStorage.getItem('workman.just-updated-to');
    void refreshRecordedFeedbackCapability();
    void invoke<{ supported: boolean; app_bundle: string | null }>('desktop_relaunch_capability')
      .then((capability) => {
        nativeRelaunchAvailable = capability.supported;
        nativeRelaunchAppBundle = capability.app_bundle;
      })
      .catch(() => {
        nativeRelaunchAvailable = false;
        nativeRelaunchAppBundle = null;
      });
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
      if (connection.status !== 'connected' || !documentVisible) return;
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
    window.addEventListener('pagehide', flushCreationDraftPersistence);
    let stopWindowResize = (): void => {};
    void getCurrentWindow().onResized(updateDocumentVisibility).then((stop) => {
      if (active) stopWindowResize = stop;
      else stop();
    }).catch(reportError);
    let stopWindowFocus = (): void => {};
    void getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      updateDocumentVisibility();
      // The Screen Recording sheet lives in System Settings. Refresh as soon as Workman regains
      // focus so the setup modal reflects a permission change without another button press.
      if (
        focused
        && feedbackPreflightOpen
        && !feedbackPreflight?.screen_capture_available
        && !feedbackPreflightLoading
        && !feedbackModelInstalling
        && !feedbackStarting
      ) {
        void refreshFeedbackPreflight();
      }
    }).then((stop) => {
      if (active) stopWindowFocus = stop;
      else stop();
    }).catch(reportError);
    const stopStatuses = client.onProcessStatuses((next) => {
      if (!active) return;
      profileProcesses = next;
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
    let stopFeedbackEvents = (): void => {};
    void Promise.all([
      listen<NativeFeedbackSession>('feedback://status', ({ payload }) => {
        if (active) activeFeedbackSession = payload;
      }),
      listen<NativeFeedbackSnapshot>('feedback://snapshot', ({ payload }) => {
        if (active) enqueueFeedbackEvent(() => persistFeedbackSnapshot(payload));
      }),
      listen<NativeFeedbackFinished>('feedback://finished', ({ payload }) => {
        if (active) enqueueFeedbackEvent(() => beginFeedbackTranscription(payload));
      }),
      listen<NativeFeedbackTranscript>('feedback://transcript', ({ payload }) => {
        if (active) enqueueFeedbackEvent(() => completeFeedbackTranscription(payload));
      }),
      listen<{ feedback_id: number; project_id: number; code: string; message: string }>('feedback://error', ({ payload }) => {
        if (!active) return;
        reportError(new Error(payload.message));
        enqueueFeedbackEvent(async () => {
          if (activeFeedbackSession?.feedback_id === payload.feedback_id) {
            activeFeedbackSession = null;
            feedbackLeaseErrorId = null;
          }
          await invoke<boolean>('feedback_abort', { feedbackId: payload.feedback_id }).catch(() => false);
          await client.recordedFeedbackFailed(payload.project_id, payload.feedback_id, payload.code)
            .catch((cause) => recordDaemonLog('warning', 'Could not persist feedback failure', messageForCause(cause)));
          pendingFeedbackTranscripts.delete(payload.feedback_id);
          if (selectedProject?.id === payload.project_id) {
            await refreshFeedback(payload.project_id);
            if (selection?.kind === 'feedback' && selection.id === payload.feedback_id) {
              await loadFeedback(payload.feedback_id, false);
            }
          }
        });
      }),
      listen<{ downloaded: number; total: number }>('feedback://model-progress', ({ payload }) => {
        if (active) feedbackModelProgress = payload;
      })
    ]).then((unlisteners) => {
      stopFeedbackEvents = () => unlisteners.forEach((unlisten) => unlisten());
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
    const feedbackLeaseTimer = setInterval(() => {
      const session = activeFeedbackSession;
      if (!active || connection.status !== 'connected') return;
      if (!session) {
        // A native crash clears the in-memory recorder but not its durable lease. Keep checking
        // while cached UI still says it is active so the daemon can mark the expired run as
        // interrupted without requiring a project switch or another relaunch.
        if (
          selectedProject
          && feedbackSummaries.some((feedback) =>
            feedback.status === 'recording' || feedback.status === 'transcribing')
        ) void refreshFeedback(selectedProject.id);
        return;
      }
      void client.recordedFeedbackRenewLease(
        session.project_id,
        session.feedback_id,
        feedbackLeaseOwner
      ).then(() => {
        if (feedbackLeaseErrorId === session.feedback_id) feedbackLeaseErrorId = null;
      }).catch((cause) => {
        const terminal = cause instanceof DaemonRequestError
          && ['feedback_not_found', 'feedback_invalid_state', 'project_not_found'].includes(cause.code);
        if (terminal && activeFeedbackSession?.feedback_id === session.feedback_id) {
          activeFeedbackSession = null;
          feedbackLeaseErrorId = null;
          void invoke<boolean>('feedback_abort', { feedbackId: session.feedback_id }).catch(() => false);
          reportError(cause);
          return;
        }
        if (feedbackLeaseErrorId === session.feedback_id) return;
        feedbackLeaseErrorId = session.feedback_id;
        reportError(cause);
      });
    }, 5_000);

    void client
      .start(
        (status) => { if (active) applyConnectionStatus(status); },
        (message) => {
          if (active) recordDaemonLog('warning', 'Control message issue', message);
        }
      )
      .then((status) => {
        if (!active) return;
        applyConnectionStatus(status);
        void invoke<NativeFeedbackSession | null>('feedback_status')
          .then((session) => { if (active) activeFeedbackSession = session; })
          .catch(() => undefined);
      })
      .catch(reportError);

    return () => {
      active = false;
      document.removeEventListener('visibilitychange', updateDocumentVisibility);
      window.removeEventListener('pagehide', flushCreationDraftPersistence);
      flushCreationDraftPersistence();
      stopWindowResize();
      stopWindowFocus();
      document.documentElement.classList.remove('workman-document-hidden');
      clearInterval(projectTimer);
      clearInterval(coordinationTimer);
      clearInterval(notificationTimer);
      clearInterval(feedbackLeaseTimer);
      if (updateProgressTimer) clearTimeout(updateProgressTimer);
      if (updateRestartTimer) clearTimeout(updateRestartTimer);
      stopStatuses();
      stopNavigation();
      stopNativeMenu();
      stopFeedbackEvents();
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
      if (status.version_compatible && updateFlow.kind !== 'running' && updateFlow.kind !== 'restarting') {
        versionRestarting = false;
      }
      if (status.version_compatible && daemonOnlyRestartPending && reconnected) {
        daemonOnlyRestartPending = false;
        clearUpdateRestartWatchdog();
        updateFlow = idleUpdateFlow;
        startupUpdate = null;
        versionRestarting = false;
      }
      if (justUpdatedVersion && status.app_version === justUpdatedVersion) {
        updatedVersionNotice = justUpdatedVersion;
        justUpdatedVersion = null;
        localStorage.removeItem('workman.just-updated-to');
        setTimeout(() => (updatedVersionNotice = null), 8_000);
      }
      if (status.version_compatible && status.port !== startupUpdatePort) {
        startupUpdatePort = status.port;
        void startupUpdateCheck();
      }
    } else {
      startupUpdatePort = null;
    }
    if (reconnected) {
      if (documentVisible) {
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
        openSettingsSection('about');
        return;
      case 'check_updates':
        requestNativeUpdateCheck();
        openSettingsSection('about');
        return;
      case 'previous_view':
        switchToPreviousWorkspaceView();
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
    projectHotkeyHintsVisible = projectHotkeyModifiersActive(event);
    const target = event.target as HTMLElement | null;
    if (folderMenuRequest) {
      if (event.key === 'Escape') closeProjectFolderMenu();
      return;
    }
    if (contextRequest) {
      if (event.key === 'Escape') closeContextMenu();
      return;
    }
    if (handleAppShortcut(event)) return;
    if (isTerminalInputTarget(target)) return;
    if (quickJumpOpen || quickPromptOpen || shortcutsOpen) return;
    if (event.key === 'Escape' && (treeMultiSelection?.ids.length ?? 0) > 0) {
      event.preventDefault();
      treeMultiSelection = null;
      return;
    }
    if (isTextEditingTarget(target)) return;
    if (panelForTarget(target) === 'projects') handleProjectListKeys(event);
    if (event.defaultPrevented) return;
    if (event.key === 'Escape') {
      if (quickJumpOpen) closeQuickJump();
      else if (dialog) dialog = null;
      else if (settingsOpen) settingsOpen = false;
      else if (selection?.kind === 'todo') openTodosBrowser();
      else if (selection?.kind === 'scratchpad') openScratchpadsBrowser();
      else if (selection?.kind === 'feedback') openFeedbackBrowser();
      else clearSelection();
    }
  }

  function handleShortcutKeyup(event: KeyboardEvent): void {
    projectHotkeyHintsVisible = projectHotkeyModifiersActive(event);
  }

  function projectHotkeyModifiersActive(event: KeyboardEvent): boolean {
    const primary = primaryModifier(event);
    const secondary = secondaryModifier(event);
    return projectHotkeyActions.some((action) => {
      const chord = $hotkeyPreferences[action];
      return chord !== null
        && chord.primary === primary
        && chord.secondary === secondary
        && chord.alt === event.altKey
        && chord.shift === event.shiftKey;
    });
  }

  function hideProjectHotkeyHints(): void {
    projectHotkeyHintsVisible = false;
  }

  function handleAppShortcut(event: KeyboardEvent): boolean {
    return handleConfiguredHotkey(event);
  }

  function handleConfiguredHotkey(event: KeyboardEvent): boolean {
    const action = findHotkeyAction(event, $hotkeyPreferences);
    if (!action) return false;

    const target = event.target as HTMLElement | null;
    const terminalTarget = isTerminalInputTarget(target);
    const textTarget = isTextEditingTarget(target);
    const draftTarget = target?.closest('[data-creation-draft]') !== null;
    if (
      action === 'reorder-up'
      || action === 'reorder-down'
      || action === 'open-context-menu'
      || action === 'submit-focused-form'
      || action === 'toggle-scratchpad-list'
      || action === 'toggle-todo-inspector'
      || action === 'new-quick-prompt'
    ) return false;
    if (quickJumpOpen && action !== 'quick-jump') return false;
    if (quickPromptOpen && action !== 'quick-prompts') return false;
    if (shortcutsOpen && action !== 'keyboard-reference') return false;
    if ((action === 'navigate-left' || action === 'navigate-right') && textTarget) return false;
    if (
      (action === 'previous-process' || action === 'next-process')
      && textTarget
      && !terminalTarget
      && !draftTarget
    ) return false;
    if (action === 'unfocus-terminal' && !terminalTarget) return false;
    if (action === 'search-terminal' && (!terminalTarget || terminalView === null)) return false;
    if ((recordingHotkeyActions as readonly string[]).includes(action)) return false;
    if (action === 'start-feedback' && !$recordedFeedbackSupported) return false;

    event.preventDefault();
    event.stopPropagation();
    const repeatable = action === 'navigate-left'
      || action === 'navigate-right'
      || action === 'previous-process'
      || action === 'next-process';
    if (event.repeat && !repeatable) return true;

    switch (action) {
      case 'previous-view':
        switchToPreviousWorkspaceView();
        return true;
      case 'quick-jump':
        if (quickJumpOpen) closeQuickJump();
        else openQuickJump();
        return true;
      case 'keyboard-reference':
        if (shortcutsOpen) closeShortcuts();
        else openShortcuts();
        return true;
      case 'open-settings':
        appNavigation.navigate({ type: 'settings' }, 'keyboard');
        return true;
      case 'toggle-project-rail':
        toggleProjectRail();
        return true;
      case 'toggle-project-tree':
        toggleTreeRail();
        return true;
      case 'quick-prompts':
        if (quickPromptOpen) closeQuickPrompts();
        else openQuickPrompts();
        return true;
      case 'start-feedback':
        void openFeedbackPreflight();
        return true;
      case 'navigate-left':
      case 'navigate-right': {
        const direction = action === 'navigate-left' ? -1 : 1;
        if (selection?.kind === 'todo') void navigateAdjacentTodo(direction);
        else if (selection?.kind === 'scratchpad') void navigateAdjacentScratchpad(direction);
        else focusAdjacentPanel(panelForTarget(target), direction);
        return true;
      }
      case 'previous-process':
      case 'next-process':
        cycleProcess(action === 'previous-process' ? -1 : 1, panelForTarget(target));
        return true;
      case 'unfocus-terminal':
        unfocusSelectedProcess();
        return true;
      case 'search-terminal':
        terminalView?.openSearch();
        return true;
    }

    const projectIndex = projectHotkeyIndex(action);
    if (projectIndex !== null) {
      const projectId = projectHotkeyProjectIds[projectIndex];
      if (projectId !== undefined) {
        appNavigation.navigate({ type: 'project', projectId }, 'keyboard');
      }
      return true;
    }

    const projectId = selectedProject?.id;
    if (projectId === undefined) return true;
    const targetByAction: Record<CreationHotkeyAction, AppNavigationTarget> = {
      'new-agent': { type: 'new-agent', projectId },
      'new-terminal': { type: 'new-terminal', projectId },
      'new-command': { type: 'add-command', projectId },
      'new-scratchpad': { type: 'new-scratchpad', projectId },
      'new-todo': { type: 'new-todo', projectId }
    };
    appNavigation.navigate(targetByAction[action as CreationHotkeyAction], 'keyboard');
    return true;
  }

  function projectHotkeyLabel(projectId: number): string | null {
    const index = projectHotkeyProjectIds.indexOf(projectId);
    const action = projectHotkeyActions[index];
    return action ? hotkeyDisplayLabel($hotkeyPreferences[action]) || null : null;
  }

  function openQuickJump(): void {
    shortcutsOpen = false;
    quickPromptOpen = false;
    quickJumpRecentKeys = readRecentNavigationKeys();
    quickJumpOpen = true;
    void refreshQuickJumpIndex(true);
  }

  function closeQuickJump(): void {
    quickJumpOpen = false;
  }

  function openQuickPrompts(): void {
    quickJumpOpen = false;
    shortcutsOpen = false;
    quickPromptOpen = true;
  }

  function closeQuickPrompts(): void {
    quickPromptOpen = false;
  }

  function insertQuickPrompt(prompt: QuickPrompt, submit: boolean): boolean {
    if (
      selectedProcess?.kind !== 'agent'
      || selectedProcess.status !== 'running'
      || terminalView === null
    ) {
      reportError(new Error('Open a running agent terminal before inserting a quick prompt.'));
      return false;
    }
    if (!terminalView.insertQuickPrompt(prompt.body, submit)) {
      reportError(new Error('The selected agent terminal is no longer available.'));
      return false;
    }
    closeQuickPrompts();
    void tick().then(() => terminalView?.focusInput());
    return true;
  }

  function openShortcuts(): void {
    quickJumpOpen = false;
    quickPromptOpen = false;
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
    if (treeCycleSelections.length === 0) return;
    const current = selection
      ? treeCycleSelections.findIndex((candidate) => candidate.key === selection?.key)
      : -1;
    const next = current < 0
      ? direction > 0 ? 0 : treeCycleSelections.length - 1
      : (current + direction + treeCycleSelections.length) % treeCycleSelections.length;
    const nextSelection = treeCycleSelections[next];
    if (!nextSelection) return;
    draftFocusRequestId = null;
    void selectTreeItem(nextSelection);
    if (returnPanel === 'projects' || returnPanel === 'tree') {
      void tick().then(() => focusPanel(returnPanel));
    } else if (returnPanel === 'main') {
      void tick().then(() => {
        if (!focusPanel('main')) terminalView?.focusInput();
      });
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
        coordination: updated[projectId]?.coordination ?? null,
        feedback: updated[projectId]?.feedback ?? []
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

  function clearProjectUnreadLocally(projectId: number): void {
    const clear = (process: ProcessView): ProcessView =>
      process.project_id === projectId && process.kind === 'agent' && process.agent_state.unread
        ? { ...process, agent_state: { ...process.agent_state, unread: false } }
        : process;
    const knownProcesses = [
      ...processes,
      ...(navigationIndex[projectId]?.processes ?? [])
    ];
    for (const process of knownProcesses) {
      if (process.project_id === projectId) notifiedUnreadProcessIds.delete(process.id);
    }
    processes = processes.map(clear);
    const snapshot = navigationIndex[projectId];
    if (snapshot) {
      navigationIndex = {
        ...navigationIndex,
        [projectId]: { ...snapshot, processes: snapshot.processes.map(clear) }
      };
    }
    agentDoneNotices = agentDoneNotices.filter((notice) => notice.projectId !== projectId);
    const readAt = Date.now();
    notifications = notifications.map((notification) =>
      notification.project_id === projectId && notification.read_at === null
        ? { ...notification, read_at: readAt }
        : notification
    );
  }

  function setNotificationBusy(busy: boolean): void {
    notificationBusy = busy;
    if (busy) return;
    for (const resolve of notificationIdleWaiters) resolve();
    notificationIdleWaiters.clear();
  }

  async function waitForNotificationIdle(): Promise<void> {
    while (notificationBusy) {
      await new Promise<void>((resolve) => notificationIdleWaiters.add(resolve));
    }
  }

  async function markProjectRead(projectId: number): Promise<void> {
    await waitForNotificationIdle();
    const projectAgents = new Map<number, ProcessView>();
    for (const process of [...processes, ...(navigationIndex[projectId]?.processes ?? [])]) {
      if (process.project_id === projectId && process.kind === 'agent') {
        projectAgents.set(process.id, process);
      }
    }
    const pendingProcessIds = new Set(projectAgents.keys());
    const unreadProcessIds = new Set(
      [...projectAgents.values()]
        .filter((process) => process.agent_state.unread)
        .map((process) => process.id)
    );
    const unreadNotificationIds = new Set(
      notifications
        .filter((notification) =>
          notification.project_id === projectId && notification.read_at === null
        )
        .map((notification) => notification.id)
    );
    const removedNotices = agentDoneNotices.filter((notice) => notice.projectId === projectId);
    const removedNotifiedIds = new Set(
      [...pendingProcessIds].filter((processId) => notifiedUnreadProcessIds.has(processId))
    );
    for (const processId of pendingProcessIds) markReadPending.add(processId);
    setNotificationBusy(true);
    clearProjectUnreadLocally(projectId);
    let succeeded = false;
    try {
      await client.markProjectRead(projectId);
      succeeded = true;
    } catch (cause) {
      const restore = (process: ProcessView): ProcessView =>
        process.project_id === projectId && unreadProcessIds.has(process.id)
          ? { ...process, agent_state: { ...process.agent_state, unread: true } }
          : process;
      processes = processes.map(restore);
      const snapshot = navigationIndex[projectId];
      if (snapshot) {
        navigationIndex = {
          ...navigationIndex,
          [projectId]: { ...snapshot, processes: snapshot.processes.map(restore) }
        };
      }
      notifications = notifications.map((notification) =>
        notification.project_id === projectId && unreadNotificationIds.has(notification.id)
          ? { ...notification, read_at: null }
          : notification
      );
      for (const processId of removedNotifiedIds) notifiedUnreadProcessIds.add(processId);
      const currentNoticeIds = new Set(agentDoneNotices.map((notice) => notice.id));
      agentDoneNotices = [
        ...removedNotices.filter((notice) => !currentNoticeIds.has(notice.id)),
        ...agentDoneNotices
      ].slice(-4);
      reportError(cause);
    } finally {
      for (const processId of pendingProcessIds) markReadPending.delete(processId);
    }
    try {
      if (succeeded) {
        await Promise.all([refreshNotifications(), refreshProcesses(projectId)]);
      }
    } finally {
      setNotificationBusy(false);
    }
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
    setNotificationBusy(true);
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
      setNotificationBusy(false);
    }
  }

  async function markAllNotificationsRead(): Promise<void> {
    if (notificationBusy || notifications.every((notification) => notification.read_at !== null)) return;
    const previous = notifications;
    setNotificationBusy(true);
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
      setNotificationBusy(false);
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

  function projectHasUnread(projectId: number): boolean {
    return projectUnreadAgentCount(projectId) > 0
      || notifications.some(
        (notification) => notification.project_id === projectId && notification.read_at === null
      );
  }

  function projectRailProcesses(project: Project): ProcessView[] {
    return navigationIndex[project.id]?.processes
      ?? (selectedProject?.id === project.id ? processes : []);
  }

  function projectRailActivityLabel(
    project: Project,
    activity: ProjectKindActivityRollup
  ): string {
    const running = (['agent', 'terminal', 'command'] as const)
      .filter((kind) => activity[kind].active > 0)
      .map((kind) => activity[kind].activeLabel);
    const activitySummary = running.length > 0 ? running.join(' · ') : 'no processes running';
    return project.status === 'error' ? `project error · ${activitySummary}` : activitySummary;
  }

  function openProjectRailProcess(project: Project, process: ProcessView): void {
    appNavigation.navigate(
      {
        type: 'item',
        selection: projectTreeSelection(
          process.kind,
          process.id,
          project.id,
          processLabel(process)
        )
      },
      'project-rail'
    );
  }

  function openProjectRailOverview(project: Project, kind: ProcessKind): void {
    appNavigation.navigate(
      { type: 'processes', projectId: project.id, kind },
      'project-rail'
    );
  }

  function cacheProjectProcesses(projectId: number, next: ProcessView[]): void {
    navigationIndex = {
      ...navigationIndex,
      [projectId]: {
        processes: next,
        coordination: navigationIndex[projectId]?.coordination ?? null,
        feedback: navigationIndex[projectId]?.feedback ?? []
      }
    };
  }

  function cacheProjectCoordination(projectId: number, next: CoordinationSnapshot): void {
    navigationIndex = {
      ...navigationIndex,
      [projectId]: {
        processes: navigationIndex[projectId]?.processes ?? [],
        coordination: next,
        feedback: navigationIndex[projectId]?.feedback ?? []
      }
    };
  }

  function cacheProjectFeedback(projectId: number, next: RecordedFeedbackSummary[]): void {
    navigationIndex = {
      ...navigationIndex,
      [projectId]: {
        processes: navigationIndex[projectId]?.processes ?? [],
        coordination: navigationIndex[projectId]?.coordination ?? null,
        feedback: next
      }
    };
  }

  async function listProjectFeedback(projectId: number): Promise<RecordedFeedbackSummary[]> {
    const [active, archived] = await Promise.all([
      client.recordedFeedback(projectId),
      client.recordedFeedback(projectId, true)
    ]);
    return [...active, ...archived];
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
            const [projectProcesses, projectCoordination, projectFeedback] = await Promise.all([
              client.processes(project.id).catch(() => cached?.processes ?? []),
              connection.version_compatible
                ? client.coordinationSnapshot(project.id).catch(() => cached?.coordination ?? null)
                : Promise.resolve(null),
              connection.version_compatible
                ? listProjectFeedback(project.id).catch(() => cached?.feedback ?? [])
                : Promise.resolve([])
            ]);
            return [
              project.id,
              { processes: projectProcesses, coordination: projectCoordination, feedback: projectFeedback }
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
      if (
        target.type === 'item'
        && target.selection.kind === 'feedback'
        && !$recordedFeedbackSupported
      ) return;
      const projectId = navigationProjectId(target);
      const switchingProjects = projectId !== null && selectedProject?.id !== projectId;
      if (projectId !== null && !activateProject(projectId)) return;

      if (target.type !== 'item' || target.selection.kind !== 'draft') {
        recordRecentNavigation(target);
        quickJumpRecentKeys = readRecentNavigationKeys();
      }
      dialog = null;

      switch (target.type) {
        case 'project':
          if (!switchingProjects) {
            settingsOpen = false;
            clearSelection();
          }
          if (selectedProject) void refreshWorktreeRepository(selectedProject, false);
          return;
        case 'processes':
          openProcessOverview(target.kind);
          return;
        case 'item':
          await selectTreeItem(target.selection);
          return;
        case 'settings':
          if (selectedProject) {
            todoBrowserOpen = false;
            scratchpadBrowserOpen = false;
            feedbackBrowserOpen = false;
            processOverviewKind = null;
            settingsOpen = true;
          }
          return;
        case 'keep-awake':
          keepAwakeOpen = true;
          return;
        case 'new-worktree':
          if (selectedProject) await openWorktreeDialog('create', selectedProject);
          return;
        case 'new-terminal':
          await spawnTerminal();
          return;
        case 'new-agent':
          await openAgentDraft();
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
          await openAgentDraft(tool.id);
          return;
        }
        case 'add-command':
          settingsOpen = false;
          openCommandDraft();
          return;
        case 'new-todo':
          settingsOpen = false;
          openTodoDraft();
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
    feedbackSummaries = cached?.feedback ?? [];
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
      ].map((scratchpad) => scratchpad.id)),
      feedbackIds: new Set(snapshot?.feedback.map((item) => item.id) ?? []),
      draftIds: new Set(
        creationDrafts.filter((draft) => draft.projectId === projectId).map((draft) => draft.id)
      )
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
    else if (selection.kind === 'feedback') await loadFeedback(selection.id);
  }

  async function loadProject(projectId: number): Promise<void> {
    try {
      await client.syncConfig(projectId);
    } catch (cause) {
      reportError(cause);
    }
    if (connection.version_compatible) {
      await Promise.all([refreshProcesses(projectId), refreshCoordination(projectId, true), refreshFeedback(projectId)]);
    } else {
      coordination = null;
      feedbackSummaries = [];
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
    return standaloneWorktreeOperations($worktreeOperations, projects).filter((operation) =>
      operation.repository_id === repositoryId
    );
  }

  function unattachedWorktreeOperations(): WorktreeOperation[] {
    return standaloneWorktreeOperations($worktreeOperations, projects).filter((operation) =>
      operation.repository_id === null
      || !projects.some((project) =>
        project.parent_project_id === null
        && project.repository_id === operation.repository_id
      )
    );
  }

  async function reconcileCompletedWorktree(operation: WorktreeOperation): Promise<void> {
    if (operation.removal) {
      try {
        projects = await client.projects();
        if (operation.removal.post_delete_warning) {
          removeWorktreeNotice = operation.removal.post_delete_warning;
        } else if (
          operation.removal.registration_issue
          || (operation.removal.delete_from_disk && operation.removal.files_untouched)
        ) {
          removeWorktreeNotice = `Files left untouched at ${operation.removal.path}.${operation.removal.registration_issue ? ` ${operation.removal.registration_issue}` : ''}`;
        }
        dismissTrackedWorktreeOperation(operation.id);
        if (activeWorktreeOperationId === operation.id) activeWorktreeOperationId = null;
        if (
          operation.repository_id !== null
          && projects.some((project) => project.repository_id === operation.repository_id)
        ) {
          await refreshWorktreeMetadata(projects, true, true, operation.repository_id);
        }
      } catch (cause) {
        reconciledWorktreeOperations.delete(operation.id);
        reportError(cause);
      }
      return;
    }
    if (!operation.project) return;
    try {
      const refreshedProjects = await client.projects();
      projects = refreshedProjects;
      const project = refreshedProjects.find((candidate) => candidate.id === operation.project?.id);
      if (!project) {
        dismissTrackedWorktreeOperation(operation.id);
        if (activeWorktreeOperationId === operation.id) activeWorktreeOperationId = null;
        return;
      }
      if (operation.repository_id !== null) {
        await refreshWorktreeMetadata(projects, true, true, operation.repository_id);
      }
      await tick();
      dismissTrackedWorktreeOperation(operation.id);
      if (activeWorktreeOperationId === operation.id) activeWorktreeOperationId = null;
      appNavigation.navigate({ type: 'project', projectId: project.id }, 'api');
    } catch (cause) {
      reconciledWorktreeOperations.delete(operation.id);
      reportError(cause);
    }
  }

  function showWorktreeOperation(operation: WorktreeOperation): void {
    activeWorktreeOperationId = operation.id;
    settingsOpen = false;
    todoBrowserOpen = false;
    scratchpadBrowserOpen = false;
    feedbackBrowserOpen = false;
    processOverviewKind = null;
    selection = null;
  }

  function dismissTrackedWorktreeOperation(operationId: string): void {
    dismissWorktreeOperation(
      operationId,
      (id) => client.dismissWorktreeOperation(id)
    );
  }

  function dismissActiveWorktreeOperation(): void {
    if (activeWorktreeOperationId) dismissTrackedWorktreeOperation(activeWorktreeOperationId);
    activeWorktreeOperationId = null;
  }

  async function retryWorktreeOperation(operation: WorktreeOperation): Promise<void> {
    const source = operation.source_project_id === null
      ? projects.find((project) =>
          project.repository_id === operation.repository_id && project.parent_project_id === null
        )
      : projects.find((project) => project.id === operation.source_project_id);
    dismissTrackedWorktreeOperation(operation.id);
    activeWorktreeOperationId = null;
    if (!source) {
      reportError(new Error('The source project is no longer available'));
      return;
    }
    if (operation.mode === 'remove') {
      if (source.repository_id !== null) {
        await refreshWorktreeMetadata(projects, true, true, source.repository_id);
      }
      openRemoveWorktree(source, operation.error_code === 'dirty_worktree');
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
      const scopedSnapshot = await loadStableProfileProjectRail();
      if (!scopedSnapshot) return;
      const { snapshot, profileId } = scopedSnapshot;
      activateCreationDraftProfile(profileId);
      const nextProjects = snapshot.projects;
      const currentSelectionId = pendingProjectSelectionId ?? selectedProject?.id ?? null;
      const preserveLocalSelection = pendingProjectSelectionId !== null
        || activationAtStart !== projectActivationRequest;
      projects = preserveLocalSelection
        && currentSelectionId !== null
        && nextProjects.some((project) => project.id === currentSelectionId)
        ? selectProjectOptimistically(nextProjects, currentSelectionId)
        : nextProjects;
      if (creationDraftsLoaded && activeProfileId === profileId) {
        const projectIds = new Set(nextProjects.map((project) => project.id));
        const nextDrafts = pruneCreationDraftsToProjects(creationDrafts, projectIds);
        if (nextDrafts !== creationDrafts) replaceCreationDrafts([...nextDrafts]);
      }
      projectFolders = snapshot.folders;
      void refreshWorktreeMetadata(projects);
      void refreshQuickJumpIndex(false);
    } catch (cause) {
      reportError(cause);
    } finally {
      busy = false;
    }
  }

  async function loadStableProfileProjectRail(): Promise<{
    snapshot: ProjectRailSnapshot;
    profileId: number;
  } | null> {
    for (let attempt = 0; attempt < 2; attempt += 1) {
      const profileIdBefore = await resolveActiveProfileId();
      if (profileIdBefore === null) return null;
      const snapshot = await loadProjectRail(client);
      const profileIdAfter = await resolveActiveProfileId();
      if (profileIdBefore === profileIdAfter) {
        return { snapshot, profileId: profileIdAfter };
      }
    }
    console.warn('workman project rail changed profiles while refreshing; skipping stale snapshot');
    return null;
  }

  async function resolveActiveProfileId(): Promise<number | null> {
    return (await client.profiles()).find((profile) => profile.active)?.id ?? null;
  }

  function activateCreationDraftProfile(profileId: number): void {
    if (creationDraftsLoaded && activeProfileId === profileId) return;
    flushCreationDraftPersistence();
    activeProfileId = profileId;
    creationDrafts = loadCreationDrafts(profileId);
    creationDraftsLoaded = true;
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
          if (
            summary &&
            (summary.revision !== scratchpadRead?.scratchpad.revision ||
              summary.comments_revision !== scratchpadRead?.comments_revision)
          ) {
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

  async function refreshFeedback(projectId: number): Promise<void> {
    try {
      const next = await listProjectFeedback(projectId);
      cacheProjectFeedback(projectId, next);
      if (selectedProject?.id !== projectId) return;
      feedbackSummaries = next;
      if (selection?.kind === 'feedback') {
        const summary = next.find((item) => item.id === selection?.id);
        if (summary) {
          selection = projectTreeSelection('feedback', summary.id, projectId, summary.title);
          if (
            summary.status !== feedbackDetail?.status
            || (feedbackDetail?.status !== 'ready' && summary.revision !== feedbackDetail?.revision)
          ) void loadFeedback(summary.id, false);
        }
      }
    } catch (cause) {
      reportError(cause);
    }
  }

  async function selectTreeItem(next: ProjectTreeSelection): Promise<void> {
    if (!selectedProject || next.projectId !== selectedProject.id) return;
    if (next.kind === 'feedback' && !$recordedFeedbackSupported) return;
    treeMultiSelection = null;
    todoCommentFocusId = next.kind === 'todo' && pendingTodoCommentFocus?.todoId === next.id
      ? pendingTodoCommentFocus.commentId
      : null;
    pendingTodoCommentFocus = null;
    scratchpadFocusRequest = 0;
    if (next.kind !== 'draft') {
      recordRecentNavigation({ type: 'item', selection: next });
      quickJumpRecentKeys = readRecentNavigationKeys();
    }
    settingsOpen = false;
    todoBrowserOpen = false;
    scratchpadBrowserOpen = false;
    feedbackBrowserOpen = false;
    processOverviewKind = null;
    activeWorktreeOperationId = null;
    selection = next;
    todoDetail = null;
    scratchpadRead = null;
    feedbackDetail = null;

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
    } else if (next.kind === 'feedback') {
      // Feedback can be opened from cached navigation state before the periodic process stream
      // publishes. Fetch targets alongside the document so Send to agent is immediately usable.
      await Promise.all([loadFeedback(next.id), refreshProcesses(next.projectId)]);
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

  async function loadFeedback(feedbackId: number, showLoading = true): Promise<void> {
    if (!selectedProject) return;
    const projectId = selectedProject.id;
    const request = ++detailRequest;
    if (showLoading) detailLoading = true;
    try {
      const next = await client.recordedFeedbackGet(projectId, feedbackId);
      if (
        request === detailRequest
        && selectedProject?.id === projectId
        && selection?.kind === 'feedback'
        && selection.id === feedbackId
      ) feedbackDetail = next;
    } catch (cause) {
      if (request === detailRequest) reportError(cause);
    } finally {
      if (showLoading && request === detailRequest) detailLoading = false;
    }
  }

  async function openFeedbackPreflight(): Promise<void> {
    if (!selectedProject || !$recordedFeedbackSupported) return;
    feedbackPreflightOpen = true;
    feedbackPreflightError = null;
    feedbackPreflightLoading = true;
    try {
      const active = await invoke<NativeFeedbackSession | null>('feedback_status');
      if (active) {
        activeFeedbackSession = active;
        feedbackPreflightOpen = false;
        appNavigation.navigate({
          type: 'item',
          selection: projectTreeSelection('feedback', active.feedback_id, active.project_id, 'Recorded feedback')
        }, 'api');
        return;
      }
      feedbackPreflight = await invoke<NativeFeedbackPreflight>('feedback_preflight');
    } catch (cause) {
      feedbackPreflightError = messageForCause(cause);
    } finally {
      feedbackPreflightLoading = false;
    }
  }

  async function refreshFeedbackPreflight(): Promise<void> {
    if (!feedbackPreflightOpen || feedbackPreflightLoading) return;
    feedbackPreflightError = null;
    feedbackPreflightLoading = true;
    try {
      feedbackPreflight = await invoke<NativeFeedbackPreflight>('feedback_preflight');
    } catch (cause) {
      feedbackPreflightError = messageForCause(cause);
    } finally {
      feedbackPreflightLoading = false;
    }
  }

  async function requestFeedbackScreenAccess(): Promise<void> {
    if (feedbackPreflightLoading) return;
    feedbackPreflightError = null;
    feedbackPreflightLoading = true;
    try {
      feedbackPreflight = await invoke<NativeFeedbackPreflight>('feedback_request_screen_access');
    } catch (cause) {
      feedbackPreflightError = messageForCause(cause);
    } finally {
      feedbackPreflightLoading = false;
    }
  }

  async function installFeedbackModel(): Promise<void> {
    if (feedbackModelInstalling) return;
    feedbackModelInstalling = true;
    feedbackModelProgress = null;
    feedbackPreflightError = null;
    try {
      feedbackPreflight = await invoke<NativeFeedbackPreflight>('feedback_install_model');
    } catch (cause) {
      feedbackPreflightError = messageForCause(cause);
    } finally {
      feedbackModelInstalling = false;
    }
  }

  async function startFeedbackRecording(): Promise<void> {
    const project = selectedProject;
    if (!project || feedbackStarting) return;
    feedbackStarting = true;
    feedbackPreflightError = null;
    let created: Awaited<ReturnType<DaemonClient['recordedFeedbackCreate']>> | null = null;
    try {
      const title = `Feedback · ${new Intl.DateTimeFormat(undefined, {
        month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit'
      }).format(new Date())}`;
      created = await client.recordedFeedbackCreate(project.id, title, feedbackLeaseOwner);
      activeFeedbackSession = await invoke<NativeFeedbackSession>('feedback_start', {
        feedbackId: created.feedback.id,
        projectId: project.id,
        mediaDir: created.media_dir,
        shortcuts: recordingHotkeyBindings($hotkeyPreferences)
      });
      feedbackPreflightOpen = false;
      await refreshFeedback(project.id);
      await selectTreeItem(projectTreeSelection('feedback', created.feedback.id, project.id, title));
    } catch (cause) {
      if (created) {
        await client.recordedFeedbackDelete(project.id, created.feedback.id).catch(() => undefined);
        await refreshFeedback(project.id);
      }
      feedbackPreflightError = messageForCause(cause);
    } finally {
      feedbackStarting = false;
    }
  }

  function enqueueFeedbackEvent(action: () => Promise<void>): void {
    feedbackEventQueue = feedbackEventQueue.then(action).catch(reportError);
  }

  async function persistFeedbackSnapshot(snapshot: NativeFeedbackSnapshot): Promise<void> {
    const { project_id, ...input } = snapshot;
    await client.recordedFeedbackAddSnapshot(project_id, input);
    if (selectedProject?.id === project_id) {
      await refreshFeedback(project_id);
      if (selection?.kind === 'feedback' && selection.id === snapshot.feedback_id) {
        await loadFeedback(snapshot.feedback_id, false);
      }
    }
  }

  async function beginFeedbackTranscription(finished: NativeFeedbackFinished): Promise<void> {
    const previous = activeFeedbackSession;
    activeFeedbackSession = {
      feedback_id: finished.feedback_id,
      project_id: finished.project_id,
      started_at_ms: previous?.started_at_ms ?? Date.now() - finished.duration_ms,
      elapsed_ms: finished.duration_ms,
      audio_samples: previous?.audio_samples ?? 0,
      sample_rate: previous?.sample_rate ?? 0,
      snapshot_count: previous?.snapshot_count ?? 0,
      paused: false,
      muted: previous?.muted ?? false,
      input_device_id: previous?.input_device_id ?? '',
      input_device_name: previous?.input_device_name ?? '',
      phase: 'transcribing',
      error: null
    };
    await client.recordedFeedbackBeginTranscription(
      finished.project_id,
      finished.feedback_id,
      finished.duration_ms,
      finished.audio_path
    );
    const pendingTranscript = pendingFeedbackTranscripts.get(finished.feedback_id);
    if (pendingTranscript) {
      pendingFeedbackTranscripts.delete(finished.feedback_id);
      await completeFeedbackTranscription(pendingTranscript);
      return;
    }
    if (selectedProject?.id === finished.project_id) {
      await refreshFeedback(finished.project_id);
      if (selection?.kind === 'feedback' && selection.id === finished.feedback_id) {
        await loadFeedback(finished.feedback_id, false);
      }
    }
  }

  async function completeFeedbackTranscription(result: NativeFeedbackTranscript): Promise<void> {
    const current = await client.recordedFeedbackGet(result.project_id, result.feedback_id);
    const interrupted = current.status === 'failed' && current.error_code === 'recording_interrupted';
    if (current.status === 'recording' || (interrupted && activeFeedbackSession?.phase === 'recording')) {
      pendingFeedbackTranscripts.set(result.feedback_id, result);
      return;
    }
    if (current.status === 'ready' || (current.status === 'failed' && !interrupted)) return;
    const blocks = compileFeedbackTimeline(result.segments, current.snapshots);
    const next = await client.recordedFeedbackComplete(
      result.project_id,
      result.feedback_id,
      result.segments,
      blocks
    );
    activeFeedbackSession = null;
    if (selectedProject?.id === result.project_id) {
      await refreshFeedback(result.project_id);
      if (selection?.kind === 'feedback' && selection.id === result.feedback_id) feedbackDetail = next;
    }
  }

  async function saveSelectedFeedback(
    title: string,
    blocks: RecordedFeedbackBlock[],
    captions: Array<{ snapshot_id: number; caption: string }>
  ): Promise<void> {
    if (!selectedProject || selection?.kind !== 'feedback' || !feedbackDetail) return;
    const next = await client.recordedFeedbackUpdate(
      selectedProject.id,
      selection.id,
      feedbackDetail.revision,
      title,
      blocks,
      captions
    );
    feedbackDetail = next;
    await refreshFeedback(selectedProject.id);
  }

  async function sendSelectedFeedbackToAgent(processId: number): Promise<void> {
    if (!selectedProject || selection?.kind !== 'feedback' || !feedbackDetail) return;
    const feedback = feedbackDetail;
    if (!feedbackTargetProcesses.some((process) => process.id === processId)) {
      throw new Error('That agent is no longer available in this project.');
    }
    await deliverFeedbackToAgent(feedback, processId);
    await loadFeedback(selection.id, false);
  }

  async function deliverFeedbackToAgent(
    feedback: RecordedFeedback,
    processId: number
  ): Promise<void> {
    // Validate the live target and retain an immutable packet before touching its composer. The
    // actual turn is assembled below from transcript text and real clipboard-image pastes.
    await client.recordedFeedbackDeliverAgent(feedback.project_id, feedback.id, processId, true);
    await deliverAgentSteps(
      processId,
      feedbackAgentInputSteps(feedback, $recordedFeedbackPreferences.agentPrompt)
    );
  }

  async function deliverAgentSteps(processId: number, steps: AgentInputStep[]): Promise<void> {
    await deliverAgentInput(steps, {
      send: (data) => client.sendInput(processId, data),
      writeImageToClipboard: async (path) => {
        const image = await invoke<{ bytes: number[]; mime_type: string }>(
          'terminal_read_attachment_image',
          { path }
        );
        await invoke('terminal_write_clipboard_image', {
          bytes: image.bytes,
          mimeType: image.mime_type
        });
      },
      // Codex and Claude import clipboard images asynchronously. Keep each image on the
      // pasteboard until its Ctrl+V has been consumed before moving to the next image.
      waitForImageImport: () => new Promise((resolve) => setTimeout(resolve, 500))
    });
  }

  function appendAgentInputGroup(
    target: AgentInputStep[],
    addition: AgentInputStep[]
  ): void {
    if (target.length > 0 && addition.length > 0) {
      target.push({ kind: 'text', text: '\n\n' });
    }
    target.push(...addition);
  }

  function waitForAgentInitialInput(
    processId: number,
    projectId: number,
    settleInitialPrompt: boolean
  ): Promise<void> {
    return new Promise((resolve, reject) => {
      let settled = false;
      let readyTimer: ReturnType<typeof setTimeout> | null = null;
      let stopListening = (): void => {};
      const finish = (cause?: Error): void => {
        if (settled) return;
        settled = true;
        if (readyTimer !== null) clearTimeout(readyTimer);
        stopListening();
        if (cause) reject(cause);
        else resolve();
      };
      const inspect = (candidates: ProcessView[], confirmed = false): void => {
        const process = candidates.find((candidate) =>
          candidate.id === processId && candidate.project_id === projectId
        );
        if (!process) return;
        if (
          ['stopped', 'exited', 'crashed'].includes(process.status)
          || process.agent_state.state === 'exited'
        ) {
          finish(new Error(`${process.name} stopped before its initial message could be sent.`));
          return;
        }
        const ready = settleInitialPrompt
          ? agentCanReceiveFeedback(process)
          : agentCanReceiveInitialTurn(process);
        if (!ready) {
          if (readyTimer !== null) clearTimeout(readyTimer);
          readyTimer = null;
          return;
        }
        if (confirmed) {
          finish();
        } else if (readyTimer === null) {
          // Let a legacy daemon's prompt scheduler claim the first idle edge before we deliver
          // feedback. New daemons return the full deferred turn, so their recheck is immediate.
          readyTimer = setTimeout(() => {
            readyTimer = null;
            void client.processes(projectId).then((current) => inspect(current, true)).catch((cause) => finish(
              new Error(`Could not verify that the new agent was ready: ${messageForCause(cause)}`)
            ));
          }, settleInitialPrompt ? 750 : 0);
        }
      };
      stopListening = client.onProcessStatuses(inspect);
      inspect(profileProcesses);
    });
  }

  async function deliverSpawnedAgentInitialTurn(
    projectId: number,
    feedbackId: number | null,
    result: SpawnAgentResult
  ): Promise<void> {
    try {
      const legacyDaemon = result.deferred_initial_prompt === undefined;
      if (legacyDaemon && feedbackId === null) return;
      await waitForAgentInitialInput(
        result.process_id,
        projectId,
        legacyDaemon
      );
      if (legacyDaemon && feedbackId !== null) {
        const feedback = await client.recordedFeedbackGet(projectId, feedbackId);
        await deliverFeedbackToAgent(feedback, result.process_id);
        if (selectedProject?.id === projectId) await refreshFeedback(projectId);
        return;
      }

      const steps = agentDraftPromptInputSteps(
        result.deferred_initial_prompt ?? '',
        result.deferred_attachments ?? []
      );
      if (feedbackId !== null) {
        const feedback = await client.recordedFeedbackGet(projectId, feedbackId);
        await client.recordedFeedbackDeliverAgent(projectId, feedbackId, result.process_id, true);
        appendAgentInputGroup(
          steps,
          feedbackAgentInputSteps(feedback, $recordedFeedbackPreferences.agentPrompt)
        );
      }
      await deliverAgentSteps(result.process_id, steps);
      if (feedbackId !== null && selectedProject?.id === projectId) {
        await refreshFeedback(projectId);
      }
    } catch (cause) {
      throw new Error(`The agent started, but its initial message could not be sent automatically: ${messageForCause(cause)}`);
    }
  }

  async function sendSelectedFeedbackToScratchpad(): Promise<void> {
    if (!selectedProject || selection?.kind !== 'feedback') return;
    const projectId = selectedProject.id;
    const result = await client.recordedFeedbackToScratchpad(projectId, selection.id);
    await refreshCoordination(projectId, false);
    await selectTreeItem(projectTreeSelection(
      'scratchpad', result.scratchpad.id, projectId, result.scratchpad.name
    ));
  }

  async function copySelectedFeedbackPacket(): Promise<void> {
    if (!selectedProject || selection?.kind !== 'feedback' || !feedbackDetail) return;
    const packet = await client.recordedFeedbackPreparePacket(selectedProject.id, selection.id);
    await invoke('terminal_write_clipboard_text', {
      text: renderRecordedFeedbackPrompt(
        $recordedFeedbackPreferences.agentPrompt,
        feedbackDetail.title,
        `The feedback packet is at ${packet.packet_path}. Read feedback.md in order; its images directory contains the referenced screenshots.`
      )
    });
    await loadFeedback(selection.id, false);
  }

  async function sendSelectedFeedbackToNewAgent(): Promise<void> {
    if (!selectedProject || selection?.kind !== 'feedback' || !feedbackDetail) return;
    const feedback = feedbackDetail;
    const draft = openCreationDraft('agent');
    if (draft?.kind !== 'agent') return;
    patchCreationDraft(draft.id, {
      name: `Feedback: ${feedback.title}`,
      feedbackId: feedback.id
    });
    await ensureAgentDraftMetadata();
  }

  async function archiveSelectedFeedback(): Promise<void> {
    if (!selectedProject || selection?.kind !== 'feedback' || !feedbackDetail) return;
    await setFeedbackArchived(
      { kind: 'feedback', feedback: feedbackDetail, selection },
      !feedbackDetail.archived
    );
  }

  async function setBrowserFeedbackArchived(
    feedback: RecordedFeedbackSummary,
    archived: boolean
  ): Promise<void> {
    if (feedbackBrowserBusyId !== null || !selectedProject) return;
    feedbackBrowserBusyId = feedback.id;
    try {
      await setFeedbackArchived({
        kind: 'feedback',
        feedback,
        selection: projectTreeSelection('feedback', feedback.id, selectedProject.id, feedback.title)
      }, archived);
    } catch (cause) {
      reportError(cause);
    } finally {
      feedbackBrowserBusyId = null;
    }
  }

  async function deleteBrowserFeedback(feedback: RecordedFeedbackSummary): Promise<void> {
    if (feedbackBrowserBusyId !== null || !selectedProject) return;
    feedbackBrowserBusyId = feedback.id;
    try {
      await deleteFeedback({
        kind: 'feedback',
        feedback,
        selection: projectTreeSelection('feedback', feedback.id, selectedProject.id, feedback.title)
      });
    } catch (cause) {
      reportError(cause);
    } finally {
      feedbackBrowserBusyId = null;
    }
  }

  async function deleteSelectedFeedback(): Promise<void> {
    if (!selectedProject || selection?.kind !== 'feedback') return;
    const feedback = feedbackDetail
      ?? feedbackSummaries.find((candidate) => candidate.id === selection?.id);
    if (!feedback) return;
    await deleteFeedback({ kind: 'feedback', feedback, selection });
  }

  function messageForCause(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
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
        const actionLabel = request.action === 'close' ? 'remove' : request.action === 'kill' ? 'force stop' : 'stop';
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
      if (!(await confirmInApp({
        title: `Delete ${selected.ids.length} ${noun}?`,
        description: 'This cannot be undone.',
        confirmLabel: `Delete ${noun}`
      }))) return;
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
      await tick();
      terminalView?.focusInput();
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
  }, commandDraft: CommandCreationDraft | null = null): number | null {
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
        retry: 'command',
        commandDraft
      })
    ];
    dialog = null;
    activeWorktreeOperationId = null;
    settingsOpen = false;
    todoBrowserOpen = false;
    scratchpadBrowserOpen = false;
    feedbackBrowserOpen = false;
    processOverviewKind = null;
    selection = projectTreeSelection('command', id, project.id, input.name);
    return id;
  }

  function replaceCreationDrafts(next: CreationDraft[]): void {
    creationDrafts = next;
    if (!creationDraftsLoaded || activeProfileId === null) return;
    pendingCreationDraftSave = { profileId: activeProfileId, drafts: next };
    if (creationDraftSaveTimer !== null) clearTimeout(creationDraftSaveTimer);
    creationDraftSaveTimer = setTimeout(flushCreationDraftPersistence, 400);
  }

  function flushCreationDraftPersistence(): void {
    if (creationDraftSaveTimer !== null) {
      clearTimeout(creationDraftSaveTimer);
      creationDraftSaveTimer = null;
    }
    const pending = pendingCreationDraftSave;
    pendingCreationDraftSave = null;
    if (pending) saveCreationDrafts(pending.profileId, pending.drafts);
  }

  function openCreationDraft(kind: CreationDraftKind): CreationDraft | null {
    const project = selectedProject;
    if (!project) return null;
    const existing = findUntouchedCreationDraft(creationDrafts, project.id, kind);
    const draft = existing ?? createCreationDraft(
      kind,
      project.id,
      nextCreationDraftId(creationDrafts)
    );
    if (!existing) {
      replaceCreationDrafts([...creationDrafts, draft]);
    }
    draftFocusRequestId = draft.id;
    void selectTreeItem(
      projectTreeSelection('draft', draft.id, project.id, creationDraftLabel(draft))
    );
    return draft;
  }

  function openCommandDraft(): void {
    openCreationDraft('command');
  }

  function openTodoDraft(): void {
    openCreationDraft('todo');
  }

  function patchCreationDraft(
    draftId: number,
    patch: Partial<AgentCreationDraft> | Partial<CommandCreationDraft> | Partial<TodoCreationDraft>,
    markTouched = true
  ): void {
    const nextDrafts = creationDrafts.map((draft) => {
      if (draft.id !== draftId) return draft;
      const next = {
        ...draft,
        ...patch,
        touched: markTouched ? true : draft.touched
      } as CreationDraft;
      if (next.kind === 'todo') next.blockerIds = [...next.blockerIds];
      return next;
    });
    replaceCreationDrafts(nextDrafts);
    const updatedDraft = nextDrafts.find((draft) => draft.id === draftId) ?? null;
    if (updatedDraft && selection?.kind === 'draft' && selection.id === draftId) {
      selection = projectTreeSelection(
        'draft',
        updatedDraft.id,
        updatedDraft.projectId,
        creationDraftLabel(updatedDraft)
      );
    }
  }

  function removeCreationDraft(draftId: number): void {
    replaceCreationDrafts(creationDrafts.filter((draft) => draft.id !== draftId));
    if (draftFocusRequestId === draftId) draftFocusRequestId = null;
    if (selection?.kind === 'draft' && selection.id === draftId) clearSelection();
  }

  async function requestDiscardCreationDraft(draftId: number): Promise<void> {
    const draft = creationDrafts.find((candidate) => candidate.id === draftId);
    if (!draft) return;
    if (creationDraftHasContent(draft) && !(await confirmInApp({
      title: `Discard ${creationDraftLabel(draft)}?`,
      description: 'The content in this draft will be lost.',
      confirmLabel: 'Discard draft'
    }))) return;
    removeCreationDraft(draftId);
  }

  function beginOptimisticCommandDraft(
    draft: CommandCreationDraft,
    input: CommandInput
  ): number | null {
    const optimisticId = beginOptimisticCommand(input, draft);
    if (optimisticId !== null) removeCreationDraft(draft.id);
    return optimisticId;
  }

  function createAgentFromDraft(
    draft: AgentCreationDraft,
    submission: { input: SpawnAgentInput; tool: AgentTool; template: AgentTemplate | null }
  ): void {
    const feedbackId = draft.feedbackId;
    const deferInitialPrompt = feedbackId !== null || Boolean(submission.input.attachments?.length);
    const input = deferInitialPrompt
      ? { ...submission.input, defer_initial_prompt: true }
      : submission.input;
    if (spawnAgent(
      submission.tool,
      input,
      submission.template,
      () => restoreAgentCreationDraft(draft),
      feedbackId
    )) {
      removeCreationDraft(draft.id);
    }
  }

  function restoreAgentCreationDraft(draft: AgentCreationDraft): void {
    if (
      !projects.some((project) => project.id === draft.projectId)
      || creationDrafts.some((candidate) =>
        candidate.id === draft.id && candidate.projectId === draft.projectId
      )
    ) return;
    replaceCreationDrafts([...creationDrafts, { ...draft }]);
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
    if (!(await confirmInApp({
      title: `Remove ${process.name}?`,
      description: message,
      confirmLabel: 'Remove command'
    }))) return;

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
    if (retry === 'agent' && tool && optimistic.agentSpawnInput) {
      void spawnAgent(tool, optimistic.agentSpawnInput, null, undefined, optimistic.feedbackId);
    }
    else if (retry === 'command' && optimistic.commandDraft) {
      const restored = {
        ...optimistic.commandDraft,
        id: nextCreationDraftId(creationDrafts),
        createdAt: Date.now(),
        touched: true
      };
      replaceCreationDrafts([...creationDrafts, restored]);
      void selectTreeItem(projectTreeSelection(
        'draft', restored.id, restored.projectId, creationDraftLabel(restored)
      ));
    }
    else if (retry === 'command') openCommandDraft();
    else if (retry === 'agent') void openAgentDraft();
  }

  async function openAgentDraft(preferredToolId: number | null = null): Promise<void> {
    const draft = openCreationDraft('agent');
    if (draft?.kind === 'agent' && preferredToolId !== null) {
      patchCreationDraft(draft.id, { templateId: null, agentToolId: preferredToolId });
    }
    await ensureAgentDraftMetadata();
  }

  function ensureAgentDraftMetadata(): Promise<void> {
    if (agentDraftMetadataLoaded) return Promise.resolve();
    if (agentDraftMetadataPromise) return agentDraftMetadataPromise;
    agentToolsLoading = true;
    const request = Promise.all([
      agentToolsStore.refresh(true),
      agentTemplatesStore.refresh(true)
    ]).then(([tools, templates]) => {
      registeredAgentTools = tools;
      agentTools = tools.filter((tool) => tool.enabled);
      agentTemplates = templates;
      agentDraftMetadataLoaded = true;
    }).catch((cause) => {
      reportError(cause);
    }).finally(() => {
      agentToolsLoading = false;
      if (agentDraftMetadataPromise === request) agentDraftMetadataPromise = null;
    });
    agentDraftMetadataPromise = request;
    return request;
  }

  function consumeDraftInitialFocus(draftId: number): void {
    if (draftFocusRequestId === draftId) draftFocusRequestId = null;
  }

  function spawnAgent(
    tool: AgentTool,
    requestedInput?: SpawnAgentInput,
    template: AgentTemplate | null = null,
    onFailure?: () => void,
    feedbackId: number | null = null
  ): boolean {
    const currentProject = selectedProject;
    if (!currentProject) return false;
    const requested = requestedInput ?? {
      project_id: currentProject.id,
      agent_tool_id: tool.id,
      extra_args: []
    };
    const project = requested.project_id === currentProject.id
      ? currentProject
      : projects.find((candidate) => candidate.id === requested.project_id) ?? null;
    if (!project) return false;
    const input = { ...requested, extra_args: [...requested.extra_args] };
    const optimisticName = input.name || template?.name || tool.name;
    const optimisticId = nextOptimisticProcessId--;
    const optimistic = createOptimisticProcess({
      id: optimisticId,
      project,
      kind: 'agent',
      name: optimisticName,
      command: tool.command,
      agentToolId: tool.id,
      retry: 'agent',
      agentSpawnInput: input,
      feedbackId
    });
    optimisticProcesses = [...optimisticProcesses, optimistic];
    dialog = null;
    activeWorktreeOperationId = null;
    settingsOpen = false;
    todoBrowserOpen = false;
    scratchpadBrowserOpen = false;
    feedbackBrowserOpen = false;
    processOverviewKind = null;
    selection = projectTreeSelection('agent', optimisticId, project.id, optimisticName);
    void finishAgentSpawn(project, input, optimisticId, onFailure, feedbackId);
    return true;
  }

  async function finishAgentSpawn(
    project: Project,
    input: SpawnAgentInput,
    optimisticId: number,
    onFailure?: () => void,
    feedbackId: number | null = null
  ): Promise<void> {
    await tick();
    try {
      const result = await client.spawnAgent(input);
      await refreshProcesses(project.id);
      const process = processes.find((candidate) => candidate.id === result.process_id);
      optimisticProcesses = optimisticProcesses.filter(
        (candidate) => candidate.process.id !== optimisticId
      );
      if (process && selectedProject?.id === project.id) {
        selection = projectTreeSelection('agent', process.id, process.project_id, process.name);
      }
      if (input.defer_initial_prompt) {
        void deliverSpawnedAgentInitialTurn(project.id, feedbackId, result).catch(reportError);
      }
    } catch (cause) {
      failPendingProcess(cause, optimisticId);
      onFailure?.();
    }
  }

  async function createTodo(draft: TodoCreationDraft): Promise<void> {
    const project = projects.find((candidate) => candidate.id === draft.projectId) ?? null;
    if (!project || selectedProject?.id !== project.id || !draft.title.trim()) return;
    detailBusy = true;
    const input: NewTodoInput = {
      title: draft.title.trim(),
      body: draft.body.trim(),
      priority: draft.priority,
      tags: draft.tags.split(',').map((tag) => tag.trim()).filter(Boolean),
      blocker_ids: draft.blockerIds
    };
    try {
      const todo = await client.coordinationTodoCreate(project.id, input);
      removeCreationDraft(draft.id);
      await refreshCoordination(project.id, false);
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
    if (!(await confirmInApp({
      title: `Delete #${selection.id} ${todoDetail.todo.title}?`,
      description: 'This cannot be undone.',
      confirmLabel: 'Delete todo'
    }))) return;
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

  async function createScratchpadComment(input: NewScratchpadCommentInput): Promise<void> {
    if (!selectedProject || selection?.kind !== 'scratchpad') return;
    const projectId = selectedProject.id;
    const scratchpadId = selection.id;
    detailBusy = true;
    try {
      await client.coordinationScratchpadCommentCreate(projectId, scratchpadId, input);
      await refreshCoordination(projectId, false);
    } catch (cause) {
      reportError(cause);
      throw cause;
    } finally {
      detailBusy = false;
    }
  }

  async function updateScratchpadComment(commentId: number, body: string): Promise<void> {
    if (!selectedProject || selection?.kind !== 'scratchpad') return;
    detailBusy = true;
    try {
      await client.coordinationScratchpadCommentUpdate(selectedProject.id, commentId, body);
    } catch (cause) {
      reportError(cause);
      throw cause;
    } finally {
      detailBusy = false;
    }
  }

  async function resolveScratchpadComment(commentId: number, resolved: boolean): Promise<void> {
    if (!selectedProject || selection?.kind !== 'scratchpad') return;
    detailBusy = true;
    try {
      await client.coordinationScratchpadCommentResolve(selectedProject.id, commentId, resolved);
      await refreshCoordination(selectedProject.id, false);
    } catch (cause) {
      reportError(cause);
      throw cause;
    } finally {
      detailBusy = false;
    }
  }

  async function deleteScratchpadComment(commentId: number): Promise<void> {
    if (!selectedProject || selection?.kind !== 'scratchpad') return;
    detailBusy = true;
    try {
      await client.coordinationScratchpadCommentDelete(selectedProject.id, commentId);
      await refreshCoordination(selectedProject.id, false);
    } catch (cause) {
      reportError(cause);
      throw cause;
    } finally {
      detailBusy = false;
    }
  }

  function currentProjectPane(): ProjectPane | null {
    if (activeWorktreeOperation) return null;
    if (settingsOpen) return { type: 'settings' };
    if (todoBrowserOpen) return { type: 'todos' };
    if (scratchpadBrowserOpen) return { type: 'scratchpads' };
    if (feedbackBrowserOpen) return { type: 'feedback' };
    if (processOverviewKind) return { type: 'processes', kind: processOverviewKind };
    if (selection) {
      if (selection.id <= 0 && selection.kind !== 'draft') return null;
      return { type: 'selection', selection: { ...selection } };
    }
    return { type: 'overview' };
  }

  function switchToPreviousWorkspaceView(): void {
    if (workspaceViewNavigationBlocked()) return;
    const target = workspaceViewHistory.previous;
    if (!target) return;
    if (!workspaceViewAvailable(target)) {
      workspaceViewHistory = { ...workspaceViewHistory, previous: null };
      return;
    }

    const nextHistory = swapWorkspaceViews(workspaceViewHistory);
    if (!nextHistory.current) return;
    workspaceViewSwapTarget = nextHistory.current;
    workspaceViewHistory = nextHistory;
    rememberProjectPane(target.projectId, target.pane);
    if (!activateProject(target.projectId)) {
      workspaceViewSwapTarget = null;
      workspaceViewHistory = swapWorkspaceViews(nextHistory);
      return;
    }

    if (target.pane.type === 'selection') {
      void selectTreeItem({ ...target.pane.selection, projectId: target.projectId });
    } else {
      applyProjectPane(target.projectId, target.pane);
    }
    void tick().then(() => {
      if (target.pane.type === 'selection' && isProcessSelection(target.pane.selection)) {
        terminalView?.focusInput();
      } else {
        focusPanel('main');
      }
    });
  }

  function workspaceViewNavigationBlocked(): boolean {
    return quickJumpOpen
      || quickPromptOpen
      || shortcutsOpen
      || folderMenuRequest !== null
      || contextRequest !== null
      || folderSettingsFolder !== null
      || projectSettingsProject !== null
      || dialog !== null
      || trustReview !== null
      || keepAwakeOpen
      || feedbackPreflightOpen
      || agentCascadeRequest !== null
      || addProjectDialogOpen
      || registerProjectDialog !== null
      || worktreeDialog !== null
      || removeWorktreeDialog !== null
      || importOffer !== null
      || confirmationDialog !== null;
  }

  function workspaceViewAvailable(view: WorkspaceViewState): boolean {
    if (!projects.some((project) => project.id === view.projectId)) return false;
    const snapshot = navigationIndex[view.projectId];
    const isCurrentProject = selectedProject?.id === view.projectId;
    const projectProcesses = isCurrentProject
      ? visibleProcesses.filter((process) => process.project_id === view.projectId)
      : (snapshot?.processes ?? []);
    const projectCoordination = isCurrentProject ? coordination : snapshot?.coordination;
    return projectPaneSelectionExists(view.pane, {
      processIds: new Set(projectProcesses.map((process) => process.id)),
      todoIds: new Set((projectCoordination?.todos ?? []).map((todo) => todo.id)),
      scratchpadIds: new Set([
        ...(projectCoordination?.scratchpads ?? []),
        ...(projectCoordination?.archived_scratchpads ?? [])
      ].map((scratchpad) => scratchpad.id)),
      feedbackIds: new Set(isCurrentProject ? feedbackSummaries.map((item) => item.id) : (snapshot?.feedback ?? []).map((item) => item.id)),
      draftIds: new Set(
        creationDrafts
          .filter((draft) => draft.projectId === view.projectId)
          .map((draft) => draft.id)
      )
    });
  }

  function rememberProjectPane(projectId: number, pane: ProjectPane): void {
    if (sameProjectPane(projectPaneMemory[projectId], pane)) return;
    projectPaneMemory = { ...projectPaneMemory, [projectId]: pane };
    saveProjectPaneMemory(projectPaneMemory);
  }

  function applyRememberedProjectPane(projectId: number): void {
    const pane = projectPaneMemory[projectId] ?? { type: 'overview' };
    applyProjectPane(projectId, pane);
  }

  function applyProjectPane(projectId: number, pane: ProjectPane): void {
    selection = null;
    todoDetail = null;
    scratchpadRead = null;
    feedbackDetail = null;
    detailLoading = false;
    settingsOpen = false;
    todoBrowserOpen = false;
    scratchpadBrowserOpen = false;
    feedbackBrowserOpen = false;
    processOverviewKind = null;
    activeWorktreeOperationId = null;

    switch (pane.type) {
      case 'selection':
        selection = { ...pane.selection, projectId };
        detailLoading = selection.kind === 'todo' || selection.kind === 'scratchpad' || selection.kind === 'feedback';
        return;
      case 'todos':
        todoBrowserOpen = true;
        return;
      case 'scratchpads':
        scratchpadBrowserOpen = true;
        return;
      case 'feedback':
        feedbackBrowserOpen = true;
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
    feedbackDetail = null;
    todoBrowserOpen = false;
    scratchpadBrowserOpen = false;
    feedbackBrowserOpen = false;
    processOverviewKind = null;
    activeWorktreeOperationId = null;
  }

  function openTodosBrowser(): void {
    if (!selectedProject) return;
    treeMultiSelection = null;
    settingsOpen = false;
    todoBrowserOpen = true;
    scratchpadBrowserOpen = false;
    feedbackBrowserOpen = false;
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
    feedbackBrowserOpen = false;
    processOverviewKind = null;
    activeWorktreeOperationId = null;
    selection = null;
    todoDetail = null;
    scratchpadRead = null;
  }

  function openFeedbackBrowser(view: RecordedFeedbackView = feedbackBrowserView): void {
    if (!selectedProject || !$recordedFeedbackSupported) return;
    feedbackBrowserView = view;
    treeMultiSelection = null;
    settingsOpen = false;
    todoBrowserOpen = false;
    scratchpadBrowserOpen = false;
    feedbackBrowserOpen = true;
    processOverviewKind = null;
    activeWorktreeOperationId = null;
    selection = null;
    todoDetail = null;
    scratchpadRead = null;
    feedbackDetail = null;
  }

  function openBrowserFeedback(feedback: RecordedFeedbackSummary): void {
    feedbackBrowserView = feedback.archived ? 'archived' : 'active';
    void selectTreeItem(projectTreeSelection(
      'feedback',
      feedback.id,
      feedback.project_id,
      feedback.title
    ));
  }

  function openProcessOverview(kind: ProcessKind): void {
    if (!selectedProject) return;
    treeMultiSelection = null;
    settingsOpen = false;
    todoBrowserOpen = false;
    scratchpadBrowserOpen = false;
    feedbackBrowserOpen = false;
    processOverviewKind = kind;
    activeWorktreeOperationId = null;
    selection = null;
    todoDetail = null;
    scratchpadRead = null;
  }

  function createFromProcessOverview(kind: ProcessKind): void {
    if (kind === 'agent') {
      void openAgentDraft();
    } else if (kind === 'terminal') {
      void spawnTerminal();
    } else {
      openCommandDraft();
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
    if (!(await confirmInApp({
      title: `Delete ${scratchpadRead.scratchpad.name}?`,
      description: 'This cannot be undone.',
      confirmLabel: 'Delete scratchpad'
    }))) return;
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
    if (!(await confirmInApp({
      title: `Delete ${scratchpad.name}?`,
      description: 'This cannot be undone.',
      confirmLabel: 'Delete scratchpad'
    }))) return;
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

  async function chooseRegisterProjectFolder(): Promise<string | null> {
    const path = await open({ directory: true, multiple: false, title: 'Choose a project folder' });
    return typeof path === 'string' ? path : null;
  }

  function showRegisterProjectTitle(path: string): void {
    registerProjectError = null;
    registerProjectDialog = {
      path,
      defaultTitle: registrationTitleForPath(path, projects)
    };
  }

  function showAddProject(): void {
    addProjectDialogOpen = true;
  }

  async function chooseFolderFromAddProject(): Promise<void> {
    if (addProjectFolderBusy || addProjectWorktreeBusyId !== null) return;
    addProjectFolderBusy = true;
    try {
      const path = await chooseRegisterProjectFolder();
      if (!path) return;
      addProjectDialogOpen = false;
      showRegisterProjectTitle(path);
    } finally {
      addProjectFolderBusy = false;
    }
  }

  function returnToAddProject(): void {
    if (registerProjectBusy) return;
    registerProjectDialog = null;
    registerProjectError = null;
    addProjectDialogOpen = true;
  }

  async function createWorktreeFromAddProject(project: Project): Promise<void> {
    if (addProjectFolderBusy || addProjectWorktreeBusyId !== null) return;
    addProjectWorktreeBusyId = project.id;
    try {
      await openWorktreeDialog('create', project);
      if (worktreeDialog) addProjectDialogOpen = false;
    } finally {
      addProjectWorktreeBusyId = null;
    }
  }

  async function submitRegisterProject(title: string): Promise<void> {
    const state = registerProjectDialog;
    if (!state || registerProjectBusy) return;
    registerProjectBusy = true;
    registerProjectError = null;
    try {
      projects = await client.register(
        state.path,
        resolvedProjectTitle(title, state.defaultTitle)
      );
      registerProjectDialog = null;
      await refreshWorktreeMetadata(projects, false, true);
    } catch (cause) {
      registerProjectError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      registerProjectBusy = false;
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
    worktreeDialogConflict = null;
    branchOptions = [];
    worktreeRefOptions = [];
    worktreeDefaultRef = null;
    worktreeDialog = {
      mode,
      sourceProject: mode === 'create' || mode === 'adopt' ? root : sourceProject,
      repository: list.repository,
      sourceEntry: mode === 'fork' ? worktreeEntryFor(sourceProject) : null
    };
    if (mode === 'create') void loadOriginBranches();
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
    worktreeDialogConflict = null;
    branchOptions = [];
    worktreeRefOptions = [];
    worktreeDefaultRef = null;
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
      worktreeRefOptions = response.ref_options ?? [];
      worktreeDefaultRef = response.default_ref ?? null;
    } catch (cause) {
      worktreeDialogError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      originBranchesLoading = false;
    }
  }

  async function validateWorktreeRef(ref: string): Promise<WorktreeRefValidation> {
    const state = worktreeDialog;
    if (!state) throw new Error('The worktree dialog is no longer open.');
    return client.validateWorktreeRef(state.sourceProject.id, ref);
  }

  async function submitWorktreeDialog(submission: WorktreeDialogSubmission): Promise<void> {
    const state = worktreeDialog;
    if (!state || worktreeDialogBusy) return;
    worktreeDialogBusy = true;
    worktreeDialogError = null;
    worktreeDialogConflict = null;
    if (submission.mode !== 'adopt') {
      try {
        const check = await client.checkWorktreeCreate({
          project_id: state.sourceProject.id,
          branch: submission.branch,
          from_ref: submission.mode === 'create' ? submission.fromRef : undefined,
          resolution: submission.resolution
        });
        if (check?.conflict) {
          worktreeDialogConflict = check.conflict;
          return;
        }
      } catch (cause) {
        worktreeDialogError = cause instanceof Error ? cause.message : String(cause);
        return;
      } finally {
        if (worktreeDialog) worktreeDialogBusy = false;
      }
    }
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
    worktreeRefOptions = [];
    worktreeDefaultRef = null;
    showWorktreeOperation(operation);
    await tick();
    try {
      if (submission.mode === 'create') {
        await client.createWorktreeAsync(operationId, {
            project_id: state.sourceProject.id,
            branch: submission.branch,
            display_name: submission.title,
            from_ref: submission.fromRef,
            resolution: submission.resolution,
            env_policy: submission.envPolicy,
            remember_env_policy: submission.rememberEnvPolicy
        });
      } else if (submission.mode === 'fork') {
        await client.forkWorktreeAsync(operationId, {
              project_id: state.sourceProject.id,
              branch: submission.branch,
              display_name: submission.title,
              resolution: submission.resolution,
              env_policy: submission.envPolicy,
              remember_env_policy: submission.rememberEnvPolicy
        });
      } else {
        await client.adoptWorktreeAsync(operationId, submission.path, submission.title);
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

  function openRegisteredConflictProject(projectId: number): void {
    const project = projects.find((candidate) => candidate.id === projectId);
    if (!project) {
      worktreeDialogError = `Project ${projectId} is registered in another profile and cannot be opened here.`;
      return;
    }
    closeWorktreeDialog();
    selectProject(project);
  }

  function openRemoveWorktree(project: Project, serverForceRequired = false): void {
    const repository = worktreeRepositoryFor(project);
    const entry = worktreeEntryFor(project);
    removeWorktreeError = null;
    removeWorktreeForceRequired = serverForceRequired;
    removeWorktreeDialog = { project, repository, entry };
  }

  async function confirmRemoveWorktree(
    deleteFromDisk: boolean,
    forceDirty: boolean
  ): Promise<void> {
    const state = removeWorktreeDialog;
    if (!state || removeWorktreeBusy) return;
    removeWorktreeBusy = true;
    removeWorktreeError = null;
    const operationId = crypto.randomUUID();
    const operation = beginWorktreeOperation({
      id: operationId,
      mode: 'remove',
      sourceProjectId: state.project.id,
      repositoryId: state.project.repository_id,
      branch: state.entry?.branch ?? null,
      path: state.project.path,
      label: projectDisplayName(state.project)
    });
    removeWorktreeDialog = null;
    showWorktreeOperation(operation);
    await tick();
    try {
      await client.removeWorktreeAsync(operationId, {
        project_id: state.project.id,
        confirm_remove: true,
        confirm_stop_running: true,
        delete_from_disk: deleteFromDisk,
        force_dirty: forceDirty
      });
    } catch (cause) {
      if (isUnsupportedControlMethod(cause)) {
        dismissTrackedWorktreeOperation(operationId);
        if (activeWorktreeOperationId === operationId) activeWorktreeOperationId = null;
        try {
          const removal = await client.control<WorktreeRemoval>('projects.remove', {
            project_id: state.project.id,
            confirm_remove: true,
            confirm_stop_running: true,
            delete_from_disk: deleteFromDisk,
            force_dirty: forceDirty
          });
          await reconcileSynchronousRemoval(state, removal, deleteFromDisk);
        } catch (fallbackCause) {
          if (isDaemonRequestTimeoutError(fallbackCause)) {
            removeWorktreeNotice = `Removal of ${projectDisplayName(state.project)} is taking longer than the request window and may still complete. Workman will refresh the project list to reconcile the result.`;
            void refreshProjects();
          } else {
            removeWorktreeDialog = state;
            if (fallbackCause instanceof DaemonRequestError && fallbackCause.code === 'dirty_worktree') {
              removeWorktreeForceRequired = true;
            }
            removeWorktreeError = fallbackCause instanceof Error ? fallbackCause.message : String(fallbackCause);
          }
        }
      } else if (cause instanceof DaemonRequestError && cause.code === 'worktree_operation_in_progress') {
        dismissTrackedWorktreeOperation(operationId);
        if (activeWorktreeOperationId === operationId) activeWorktreeOperationId = null;
        removeWorktreeDialog = state;
        removeWorktreeError = `Removal already in progress for this project. ${cause.message}`;
      } else {
        failWorktreeOperation(
          operationId,
          cause instanceof Error ? cause.message : String(cause)
        );
      }
    } finally {
      removeWorktreeBusy = false;
    }
  }

  async function reconcileSynchronousRemoval(
    state: NonNullable<typeof removeWorktreeDialog>,
    removal: WorktreeRemoval,
    deleteFromDisk: boolean
  ): Promise<void> {
    if (removal.registration_issue || (deleteFromDisk && removal.files_untouched)) {
      removeWorktreeNotice = `Files left untouched at ${removal.path}.${removal.registration_issue ? ` ${removal.registration_issue}` : ''}`;
    }
    projects = await client.projects();
    if (state.repository && !(deleteFromDisk && state.entry?.kind === 'main')) {
      await refreshWorktreeMetadata(projects, true, true, state.repository.id);
    }
    const next = projects.find((project) => project.selected) ?? projects[0];
    if (next) appNavigation.navigate({ type: 'project', projectId: next.id }, 'api');
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

  function openProjectFolderSettings(folder: ProjectFolder): void {
    folderMenuRequest = null;
    folderSettingsFolder = folder;
  }

  async function saveProjectFolderSettings(
    settings: ProjectFolderSettingsInput
  ): Promise<void> {
    const folder = folderSettingsFolder;
    if (!folder || folderSettingsBusy) return;
    folderSettingsBusy = true;
    try {
      applyProjectRailSnapshot(
        await updateProjectFolderSettings(client, folder.id, settings)
      );
      folderSettingsFolder = null;
    } catch (cause) {
      reportError(cause);
    } finally {
      folderSettingsBusy = false;
    }
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
    if (!(await confirmInApp({
      title: `Delete “${request.folder.name}”?`,
      description: `${childCopy} will return to the top level; no projects are deleted.`,
      confirmLabel: 'Delete folder'
    }))) return;
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
        settings.iconColor,
        settings.nameColor
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
    projectRailPopoverKey = null;
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
      hasUnread: projectHasUnread(project.id),
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
    if (selectedDraft) {
      return { kind: 'draft', draft: selectedDraft, selection };
    }
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
    if (selection.kind === 'feedback') {
      const feedback = feedbackSummaries.find((candidate) => candidate.id === selection?.id)
        ?? feedbackDetail;
      return feedback ? { kind: 'feedback', feedback, selection } : null;
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
      } else if (target.kind === 'terminal') {
        dispatchTerminalContextAction(action, target);
      } else if (target.kind === 'todo') {
        await runTodoContextAction(action, target);
      } else if (target.kind === 'draft') {
        if (action === 'discard-draft') await requestDiscardCreationDraft(target.draft.id);
      } else if (target.kind === 'scratchpad') {
        await runScratchpadContextAction(action, target);
      } else {
        await runFeedbackContextAction(action, target);
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
      case 'mark-read':
        await markProjectRead(project.id);
        return;
      case 'project-settings':
        openProjectSettings(project);
        return;
      case 'rename':
        beginRename(project);
        return;
      case 'new-agent':
        if (!(await activateProject(project.id))) return;
        await openAgentDraft();
        return;
      case 'new-terminal':
        if (!(await activateProject(project.id))) return;
        await spawnTerminal();
        return;
      case 'add-command':
        if (!(await activateProject(project.id))) return;
        settingsOpen = false;
        openCommandDraft();
        return;
      case 'new-todo':
        if (!(await activateProject(project.id))) return;
        settingsOpen = false;
        openTodoDraft();
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
        openRemoveWorktree(project);
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
        if (!(await confirmInApp({
          title: `Force stop ${process.name}?`,
          description: 'This ends the process immediately without a graceful shutdown. Unsaved terminal state may be lost.',
          confirmLabel: 'Force stop'
        }))) return;
        await client.control('process.kill', { process_id: process.id, confirm_kill: true });
        await refreshProcesses(process.project_id);
        return;
      case 'close':
        if (openAgentCascadeDialog(process, 'close')) return;
        if (!(await confirmInApp({
          title: `Remove ${process.name} from Workman?`,
          description: process.status === 'running' || process.status === 'starting'
            ? 'This stops the process first, then removes its saved sidebar entry from Workman.'
            : 'This removes its saved sidebar entry from Workman.',
          confirmLabel: `Remove ${process.kind}`
        }))) return;
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
      && !(await confirmInApp({
        title: `Delete ${target.scratchpad.name}?`,
        description: 'This cannot be undone.',
        confirmLabel: 'Delete scratchpad'
      }))) return;

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

  async function setFeedbackArchived(
    target: Extract<ContextMenuTarget, { kind: 'feedback' }>,
    archived: boolean
  ): Promise<void> {
    if (target.feedback.archived === archived) return;
    const next = await client.recordedFeedbackArchive(
      target.selection.projectId,
      target.feedback.id,
      archived
    );
    const selected = selection?.kind === 'feedback' && selection.id === target.feedback.id;
    if (selected) {
      feedbackDetail = next;
    }
    await refreshFeedback(target.selection.projectId);
    if (selected && archived) openFeedbackBrowser('active');
  }

  async function deleteFeedback(
    target: Extract<ContextMenuTarget, { kind: 'feedback' }>
  ): Promise<void> {
    if (!(await confirmInApp({
      title: `Delete ${target.feedback.title}?`,
      description: 'The transcript, microphone audio, screenshots, and generated packets will be permanently deleted.',
      confirmLabel: 'Delete feedback',
      destructive: true
    }))) return;
    await client.recordedFeedbackDelete(target.selection.projectId, target.feedback.id);
    const selected = selection?.kind === 'feedback' && selection.id === target.feedback.id;
    await refreshFeedback(target.selection.projectId);
    if (selected) openFeedbackBrowser();
  }

  async function runFeedbackContextAction(
    action: ContextActionId,
    target: Extract<ContextMenuTarget, { kind: 'feedback' }>
  ): Promise<void> {
    if (action === 'copy-title') {
      await navigator.clipboard.writeText(target.feedback.title);
    } else if (action === 'archive-feedback') {
      await setFeedbackArchived(target, !target.feedback.archived);
    } else if (action === 'delete-feedback') {
      await deleteFeedback(target);
    }
  }

  async function runTreeMiddleClick(target: ContextMenuTarget): Promise<void> {
    if (treeBulkBusy || agentCascadeBusy) return;
    treeRenameTarget = null;
    contextRequest = null;

    if (target.kind === 'process') {
      const process = target.process;
      if (process.kind === 'command' || processBusyId !== null) return;
      const removed = planAgentCascade(processes, [process], true);
      const removedIds = new Set([
        ...removed.selected.map((candidate) => candidate.id),
        ...removed.additionalDescendants.map((candidate) => candidate.id)
      ]);
      processBusyId = process.id;
      try {
        if (process.status === 'running' || process.status === 'starting') {
          await client.control('process.kill', {
            process_id: process.id,
            confirm_kill: true
          });
        }
        await client.closeProcess(process.id);
        if (selection && isProcessSelection(selection) && removedIds.has(selection.id)) {
          clearSelection();
        }
        treeMultiSelection = null;
        await refreshProcesses(process.project_id);
      } catch (cause) {
        reportError(cause);
      } finally {
        processBusyId = null;
      }
      return;
    }

    treeBulkBusy = true;
    try {
      if (target.kind === 'todo') {
        if (!target.todo.completed) await runTodoContextAction('complete-todo', target);
      } else if (target.kind === 'scratchpad') {
        await runScratchpadContextAction('archive-scratchpad', target);
      } else if (target.kind === 'feedback') {
        await setFeedbackArchived(target, true);
      }
    } catch (cause) {
      reportError(cause);
    } finally {
      treeBulkBusy = false;
    }
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

  function closeProjectRailTooltip(): void {
    projectRailTooltipOpenId = null;
  }

  function changeProjectRailTooltipOpen(projectId: number, open: boolean): void {
    if (open) {
      projectRailTooltipOpenId = projectId;
    } else if (projectRailTooltipOpenId === projectId) {
      projectRailTooltipOpenId = null;
    }
  }

  function closeProjectRailTooltipOnUnmount(_node: HTMLElement, projectId: number) {
    return {
      destroy(): void {
        if (projectRailTooltipOpenId === projectId) closeProjectRailTooltip();
      }
    };
  }

  function toggleProjectRail(): void {
    closeProjectRailTooltip();
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

  function confirmInApp(options: {
    title: string;
    description: string;
    confirmLabel?: string;
    destructive?: boolean;
  }): Promise<boolean> {
    if (confirmationDialog) return Promise.resolve(false);
    return new Promise((resolve) => {
      confirmationDialog = {
        title: options.title,
        description: options.description,
        confirmLabel: options.confirmLabel ?? 'Continue',
        destructive: options.destructive ?? true,
        resolve
      };
    });
  }

  function settleConfirmation(confirmed: boolean): void {
    const pending = confirmationDialog;
    if (!pending) return;
    confirmationDialog = null;
    pending.resolve(confirmed);
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
    if (!(await confirmInApp({
      title: 'Restart Workman daemon?',
      description: 'All running project processes will stop.',
      confirmLabel: 'Restart daemon'
    }))) return;
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
    if (!(await confirmInApp({
      title: copy.dialogTitle,
      description: copy.dialogDescription,
      confirmLabel: copy.confirmLabel
    }))) return;
    await performAvailableUpdate();
  }

  async function performAvailableUpdate(update: UpdateStatus | null = startupUpdate): Promise<void> {
    if (update) startupUpdate = update;
    if (
      !update
      || !updateActionAvailable(update)
      || updateInstallActive
      || updateFlow.kind === 'running'
      || updateFlow.kind === 'restarting'
    ) return;
    updateInstallActive = true;
    updateBannerDismissed = false;
    versionRestarting = true;
    installedUpdateReport = null;
    resetUpdateProgressPresentation();
    updateFlow = {
      kind: 'running',
      progress: updateProgress('checking', 'Checking for the latest Workman release')
    };
    updateProgressPresentedAt = Date.now();
    try {
      const report = await applyUpdate(client, (progress) => {
        if (canPresentUpdateProgress(updateFlow, updateInstallActive)) {
          presentUpdateProgress(progress);
        }
      });
      installedUpdateReport = report;
      presentUpdateProgress(
        updateProgress('restarting', `Installed Workman ${report.latest} — restarting…`)
      );
      await waitForUpdateProgressPresentation();
      await completeInstalledUpdate(report);
    } catch (cause) {
      resetUpdateProgressPresentation();
      const stage = failedUpdateStage(updateFlow);
      updateFlow = {
        kind: 'failed',
        stage,
        message: cause instanceof Error ? cause.message : String(cause)
      };
      versionRestarting = false;
    } finally {
      updateInstallActive = false;
    }
  }

  async function completeInstalledUpdate(report: UpdateInstallReport): Promise<void> {
    const action = updateCompletionAction(report, {
      nativeRelaunchAvailable,
      appVersion: connection.app_version,
      appBundle: nativeRelaunchAppBundle
    });
    if (action === 'manual-restart') {
      const canRetryNativeRestart = report.restart_plan?.app === true
        && report.installed_app_bundle !== undefined
        && report.installed_app_bundle !== null
        && report.installed_app_bundle.replace(/\/+$/, '') === nativeRelaunchAppBundle?.replace(/\/+$/, '');
      updateFlow = manualUpdateFlow(report, null, canRetryNativeRestart ? 'app' : null);
      startupUpdate = null;
      versionRestarting = false;
      return;
    }

    const restartTarget = action === 'relaunch' ? 'app' : 'daemon';
    updateFlow = { kind: 'restarting', version: report.latest, target: restartTarget };
    await delay(1_000);
    if (action === 'restart-daemon-only') {
      daemonOnlyRestartPending = true;
      armUpdateRestartWatchdog(report, 'daemon');
      try {
        await client.restartDaemon();
      } catch (cause) {
        daemonOnlyRestartPending = false;
        showUpdateRestartFallback(report, cause, 'daemon');
      }
      return;
    }

    localStorage.setItem('workman.just-updated-to', report.latest);
    armUpdateRestartWatchdog(report, 'app');
    try {
      await invoke('desktop_restart_after_update', {
        confirmProcessesStopped: true,
        restartDaemon: report.restart_plan?.daemon ?? true,
        installedAppBundle: report.installed_app_bundle
      });
    } catch (cause) {
      showUpdateRestartFallback(report, cause, 'app');
    }
  }

  async function restartInstalledUpdate(): Promise<void> {
    const report = installedUpdateReport;
    if (!report || updateFlow.kind !== 'needs-restart') return;
    const restartAction = updateFlow.restartAction;
    if (!restartAction) return;
    updateBannerDismissed = false;
    updateFlow = { kind: 'restarting', version: report.latest, target: restartAction };
    versionRestarting = true;
    armUpdateRestartWatchdog(report, restartAction);
    if (restartAction === 'daemon') {
      daemonOnlyRestartPending = true;
      try {
        await client.restartDaemon();
      } catch (cause) {
        daemonOnlyRestartPending = false;
        showUpdateRestartFallback(report, cause, 'daemon');
      }
      return;
    }
    localStorage.setItem('workman.just-updated-to', report.latest);
    try {
      await invoke('desktop_restart_after_update', {
        confirmProcessesStopped: true,
        restartDaemon: report.restart_plan?.daemon ?? true,
        installedAppBundle: report.installed_app_bundle
      });
    } catch (cause) {
      showUpdateRestartFallback(report, cause, 'app');
    }
  }

  function dismissInstalledUpdate(): void {
    if (updateFlow.kind === 'needs-restart') {
      updateBannerDismissed = true;
      startupUpdate = null;
      versionRestarting = false;
      return;
    }
    resetUpdateProgressPresentation();
    clearUpdateRestartWatchdog();
    updateFlow = idleUpdateFlow;
    installedUpdateReport = null;
    startupUpdate = null;
    versionRestarting = false;
  }

  function showUpdateRestartFallback(
    report: UpdateInstallReport,
    cause: unknown,
    restartAction: 'app' | 'daemon'
  ): void {
    clearUpdateRestartWatchdog();
    localStorage.removeItem('workman.just-updated-to');
    const message = cause instanceof Error ? cause.message : String(cause);
    updateFlow = manualUpdateFlow(
      report,
      `The update is installed, but automatic restart was unavailable: ${message}`,
      restartAction
    );
    updateBannerDismissed = false;
    versionRestarting = false;
  }

  function armUpdateRestartWatchdog(
    report: UpdateInstallReport,
    restartAction: 'app' | 'daemon'
  ): void {
    clearUpdateRestartWatchdog();
    updateRestartTimer = setTimeout(() => {
      updateRestartTimer = null;
      if (updateFlow.kind !== 'restarting') return;
      daemonOnlyRestartPending = false;
      showUpdateRestartFallback(
        report,
        restartAction === 'app'
          ? 'the desktop process did not relaunch within 20 seconds'
          : 'the updated daemon did not reconnect within 20 seconds',
        restartAction
      );
    }, updateRestartTimeoutMs);
  }

  function clearUpdateRestartWatchdog(): void {
    if (updateRestartTimer) clearTimeout(updateRestartTimer);
    updateRestartTimer = null;
  }

  function updateProgress(stage: UpdateStage, message: string): UpdateProgress {
    return {
      stage,
      message,
      bytes_done: null,
      bytes_total: null,
      percent: null,
      failed: false
    };
  }

  function presentUpdateProgress(progress: UpdateProgress): void {
    if (!canPresentUpdateProgress(updateFlow, updateInstallActive)) return;
    if (progress.failed) {
      resetUpdateProgressPresentation();
      updateFlow = { kind: 'failed', stage: progress.stage, message: progress.message };
      versionRestarting = false;
      return;
    }
    versionRestarting = true;
    if (
      updateProgressQueue.length === 0
      && updateFlow.kind === 'running'
      && updateFlow.progress.stage === progress.stage
    ) {
      updateFlow = { kind: 'running', progress };
      return;
    }
    const queued = updateProgressQueue.at(-1);
    if (queued?.stage === progress.stage) updateProgressQueue[updateProgressQueue.length - 1] = progress;
    else updateProgressQueue.push(progress);
    scheduleUpdateProgressDrain();
  }

  function scheduleUpdateProgressDrain(): void {
    if (updateProgressTimer) return;
    const elapsed = Date.now() - updateProgressPresentedAt;
    const wait = Math.max(0, updateStageMinimumMs - elapsed);
    updateProgressTimer = setTimeout(drainUpdateProgress, wait);
  }

  function drainUpdateProgress(): void {
    updateProgressTimer = null;
    const next = updateProgressQueue.shift();
    if (next && canPresentUpdateProgress(updateFlow, updateInstallActive)) {
      updateFlow = { kind: 'running', progress: next };
      updateProgressPresentedAt = Date.now();
    }
    if (
      updateProgressQueue.length > 0
      || (next && updateProgressWaiters.length > 0)
    ) {
      scheduleUpdateProgressDrain();
      return;
    }
    settleUpdateProgressWaiters();
  }

  function waitForUpdateProgressPresentation(): Promise<void> {
    const elapsed = Date.now() - updateProgressPresentedAt;
    if (updateProgressQueue.length === 0 && elapsed >= updateStageMinimumMs) {
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      updateProgressWaiters.push(resolve);
      scheduleUpdateProgressDrain();
    });
  }

  function resetUpdateProgressPresentation(): void {
    if (updateProgressTimer) clearTimeout(updateProgressTimer);
    updateProgressTimer = null;
    updateProgressQueue = [];
    settleUpdateProgressWaiters();
  }

  function settleUpdateProgressWaiters(): void {
    const waiters = updateProgressWaiters.splice(0);
    for (const resolve of waiters) resolve();
  }

  function failedUpdateStage(flow: UpdateFlow): UpdateStage {
    if (flow.kind === 'running') return flow.progress.stage;
    if (flow.kind === 'failed') return flow.stage;
    return 'restarting';
  }

  function delay(milliseconds: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, milliseconds));
  }
</script>

<svelte:window
  onkeydown={handleShortcut}
  onkeyup={handleShortcutKeyup}
  onblur={hideProjectHotkeyHints}
/>

<svelte:head>
  <title>{windowTitle}</title>
</svelte:head>

{#if showVersionBanner}
  <section class="version-banner" aria-live={updateBanner.mode === 'failed' ? 'assertive' : 'polite'}>
    <div>
      <strong>{showVersionSkew ? 'Workman daemon is running an older version' : updateBanner.title}</strong>
      <span>{showVersionSkew ? 'Restarting loads this app’s control protocol and agent config.' : updateBanner.description}</span>
    </div>
    {#if !showVersionSkew && (updateBanner.mode === 'running' || updateBanner.mode === 'restarting')}
      <div
        class:indeterminate={updateBanner.indeterminate}
        class="update-progress"
        role="progressbar"
        aria-label={updateBanner.title}
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={updateBanner.percent ?? undefined}
      >
        <span style={`width: ${updateBanner.percent ?? 32}%`}></span>
      </div>
    {:else}
      <small>{showVersionSkew ? `app ${connection.app_build_id || 'current'} · daemon ${connection.daemon_build_id ?? 'legacy'}` : cliRecoveryRequired && !updateAvailable ? `Workman ${startupUpdate?.check.current}` : startupUpdate ? `current ${startupUpdate.check.current} · latest ${startupUpdate.check.latest}` : ''}</small>
    {/if}
    <div class="version-banner-actions">
      {#if showVersionSkew}
        <Button class="border-warning/50 text-warning hover:bg-warning/10" size="sm" variant="outline" disabled={versionRestarting} onclick={() => void restartOutdatedDaemon()}>
          {versionRestarting ? 'Restarting daemon…' : 'Restart daemon'}
        </Button>
      {:else if updateBanner.mode === 'available'}
        <Button class="border-warning/50 text-warning hover:bg-warning/10" size="sm" variant="outline" disabled={versionRestarting} onclick={() => void applyAvailableUpdate()}>
          {startupUpdateCopy?.buttonLabel}
        </Button>
      {:else if updateBanner.retry}
        <Button class="border-warning/50 text-warning hover:bg-warning/10" size="sm" variant="outline" onclick={() => void performAvailableUpdate()}>Retry</Button>
        {#if updateBanner.dismiss}
          <Button size="sm" variant="ghost" onclick={dismissInstalledUpdate}>Later</Button>
        {/if}
      {:else if updateBanner.restart}
        <Button class="border-warning/50 text-warning hover:bg-warning/10" size="sm" variant="outline" onclick={() => void restartInstalledUpdate()}>{updateBanner.restartLabel}</Button>
        <Button size="sm" variant="ghost" onclick={dismissInstalledUpdate}>Later</Button>
      {:else if updateBanner.dismiss}
        <Button size="sm" variant="ghost" onclick={dismissInstalledUpdate}>Later</Button>
      {/if}
    </div>
  </section>
{/if}

{#if updatedVersionNotice}
  <button class:with-version-banner={showVersionBanner} class="updated-version-notice" type="button" onclick={() => (updatedVersionNotice = null)}>
    Updated to Workman {updatedVersionNotice}
  </button>
{/if}

{#if removeWorktreeNotice}
  <button class:with-version-banner={showVersionBanner} class="remove-worktree-notice" type="button" aria-live="polite" onclick={() => (removeWorktreeNotice = null)}>
    {removeWorktreeNotice}
  </button>
{/if}

<AgentDoneToasts
  notices={agentDoneNotices}
  onOpen={openAgentDoneNotice}
  onDismiss={(id) => (agentDoneNotices = agentDoneNotices.filter((notice) => notice.id !== id))}
/>

{#snippet projectRailRow(project: Project, nested: boolean)}
  {@const projectOperation = worktreeOperationForProject($worktreeOperations, project)}
  {@const operationLabel = projectOperation ? worktreeOperationStateLabel(projectOperation) : null}
  {@const repository = worktreeRepositoryFor(project)}
  {@const worktree = worktreeEntryFor(project)}
  {@const rowLabel = projectLabel(project)}
  {@const fullTitle = projectTitle(project)}
  {@const parentLabel = worktreeParentLabel(project, projects, repository?.name)}
  {@const projectKind = parentLabel !== null ? 'worktree' : project.repository_id !== null ? 'repository' : 'project'}
  {@const tooltipLabel = `${fullTitle} · ${project.path}${parentLabel !== null ? ` · Worktree of ${parentLabel}` : ''}`}
  {@const unreadAgentCount = projectUnreadAgentCount(project.id)}
  {@const projectProcesses = projectRailProcesses(project)}
  {@const activity = projectKindActivity(projectProcesses, $liveStats.processes)}
  {@const activityLabel = projectRailActivityLabel(project, activity)}
  {@const hotkeyLabel = projectHotkeyLabel(project.id)}
  <article
    class:active={project.selected}
    class:has-unread={unreadAgentCount > 0}
    class:has-operation={projectOperation !== null}
    class:operation-active={projectOperation?.id === activeWorktreeOperationId}
    class:nested
    class="project-row group/project group/repository"
    data-operation-status={projectOperation?.status}
    use:closeProjectRailTooltipOnUnmount={project.id}
  >
    {#if renameId === project.id}
      <form class="rename-form" onsubmit={(event) => { event.preventDefault(); void commitRename(); }}>
        <input aria-label="Project name" bind:value={renameValue} use:focusRename onkeydown={(event) => { if (event.key === 'Escape') cancelRename(); }} />
        <Button size="sm" type="submit">Save</Button>
      </form>
    {:else}
      <span class="project-content">
        <button
              class="project-select"
              type="button"
              aria-current={project.selected ? 'page' : undefined}
              aria-label={`${tooltipLabel} · ${projectKind} · ${activityLabel}${operationLabel ? ` · ${operationLabel}` : ''}${hotkeyLabel ? ` · Shortcut ${hotkeyLabel}` : ''}${unreadAgentCount > 0 ? ` · ${unreadAgentCount} unread agents` : ''}`}
              use:reorderItem={{
                id: project.id,
                group: 'projects',
                disabled: projectOperation !== null || busy || projectReorderBusy || renameId !== null || folderRenameId !== null || projects.length + projectFolders.length < 2,
                label: fullTitle,
                onDrop: handleProjectDrop,
                onKeyboardMove: moveProjectRailFromKeyboard
              }}
              onclick={() => projectOperation ? showWorktreeOperation(projectOperation) : selectProject(project)}
              oncontextmenu={(event) => { if (!projectOperation) showProjectPointerMenu(event, project); }}
              onkeydown={(event) => { if (!projectOperation) showProjectKeyboardMenu(event, project); }}
              data-context-kind="project"
              data-context-id={project.id}
            >
              <span class="project-icon-anchor">
                <TooltipLabel
                  label={tooltipLabel}
                  side={projectRailCollapsed ? 'right' : 'top'}
                  sideOffset={8}
                  delayDuration={PROJECT_RAIL_TOOLTIP_DELAY_MS}
                  disableHoverableContent={true}
                  skipDelayDuration={0}
                  contentClass="project-rail-tooltip"
                  tabindex={-1}
                  open={projectRailTooltipOpenId === project.id}
                  onOpenChange={(open) => changeProjectRailTooltipOpen(project.id, open)}
                  onpointerleave={closeProjectRailTooltip}
                  onpointerdown={closeProjectRailTooltip}
                >
                  {#snippet children()}
                    <span class="project-kind-icon" aria-hidden="true">
                      <ProjectIcon
                        icon={project.icon}
                        color={project.icon_color}
                        image={project.icon_image?.data_url}
                        fallback={parentLabel !== null ? 'worktree' : project.repository_id !== null ? 'repository' : 'project'}
                        worktree={parentLabel !== null}
                        worktreeTooltip={false}
                        size={15}
                      />
                    </span>
                  {/snippet}
                  {#snippet content()}
                    <span class="project-tooltip-copy">
                      <strong>{fullTitle}</strong>
                      <span>{project.path}</span>
                      {#if parentLabel !== null}
                        <span class="project-tooltip-parent">
                          <GitBranchIcon size={12} strokeWidth={1.8} aria-hidden="true" />
                          Worktree of {parentLabel}
                        </span>
                      {/if}
                    </span>
                  {/snippet}
                </TooltipLabel>
              </span>
              <span class="project-copy"><strong style:color={sidebarIdentityColorValue(project.name_color)}>{rowLabel}</strong></span>
              {#if hotkeyLabel || unreadAgentCount > 0}
                <span class="project-row-badges">
                  {#if hotkeyLabel}
                    <kbd
                      class:visible={projectHotkeyHintsVisible}
                      class="project-hotkey"
                      aria-hidden="true"
                    >{hotkeyLabel}</kbd>
                  {/if}
                  {#if unreadAgentCount > 0}
                    <span class="project-unread-rollup" aria-label={`${unreadAgentCount} unread agents`}>
                      <span aria-hidden="true"></span>{unreadAgentCount}
                    </span>
                  {/if}
                </span>
              {/if}
        </button>
      </span>
      {#if !projectRailCollapsed}
        <span class="project-meta-strip" data-project-meta-strip>
          {#if projectOperation}
            <ProjectOperationStatus operation={projectOperation} />
          {:else}
            <ProjectKindIndicators
              {activity}
              processes={projectProcesses}
              projectId={project.id}
              projectTitle={fullTitle}
              openPopoverKey={projectRailPopoverKey}
              onOpenPopoverChange={(key) => (projectRailPopoverKey = key)}
              onSelect={(process) => openProjectRailProcess(project, process)}
              onShowAll={(kind) => openProjectRailOverview(project, kind)}
            />
            {#if repository}
              <WorktreeRowMeta
                entry={worktree}
                pullRequestCache={worktreeListFor(project)?.pull_requests ?? null}
                projectId={project.id}
                repositoryName={repository.name}
                openPopoverKey={projectRailPopoverKey}
                onOpenPopoverChange={(key) => (projectRailPopoverKey = key)}
                refreshing={worktreeRefreshingRepositoryId === repository.id}
                showNoPullRequest={false}
                onRefresh={() => void refreshWorktreeRepository(project, true)}
              />
            {/if}
          {/if}
        </span>
      {:else}
        <span class="project-compact-meta" data-project-compact-meta>
          {#if projectOperation}
            <ProjectOperationStatus operation={projectOperation} compact />
          {:else}
            <ProjectKindIndicators
              {activity}
              processes={projectProcesses}
              projectId={project.id}
              projectTitle={fullTitle}
              openPopoverKey={projectRailPopoverKey}
              onOpenPopoverChange={(key) => (projectRailPopoverKey = key)}
              compact
              onSelect={(process) => openProjectRailProcess(project, process)}
              onShowAll={(kind) => openProjectRailOverview(project, kind)}
            />
            {#if repository}
              <WorktreeRowMeta
                entry={worktree}
                pullRequestCache={worktreeListFor(project)?.pull_requests ?? null}
                projectId={project.id}
                repositoryName={repository.name}
                openPopoverKey={projectRailPopoverKey}
                onOpenPopoverChange={(key) => (projectRailPopoverKey = key)}
                refreshing={worktreeRefreshingRepositoryId === repository.id}
                showNoPullRequest={false}
                compact
                onRefresh={() => void refreshWorktreeRepository(project, true)}
              />
            {/if}
          {/if}
        </span>
      {/if}
      {#if !projectOperation}
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
    onpointerdown={closeProjectRailTooltip}
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
        shortcut={hotkeyDisplayLabel($hotkeyPreferences['toggle-project-rail']) || undefined}
        onclick={toggleProjectRail}
      >
        {#snippet icon()}
          {#if projectRailCollapsed}<ChevronRightIcon size={15} />{:else}<ChevronLeftIcon size={15} />{/if}
        {/snippet}
      </IconButton>
      <div class="keep-awake-slot">
        <KeepAwakeControl
          processes={profileProcesses}
          {projects}
          connectionStatus={connection.status}
          visible={documentVisible}
          bind:open={keepAwakeOpen}
          bind:armed={keepAwakeArmed}
          bind:autoEnabled={autoKeepAwakeEnabled}
          bind:supported={keepAwakeSupported}
        />
      </div>
      <div class="notification-slot">
        <NotificationsCenter
          {notifications}
          {projects}
          busy={notificationBusy}
          onRefresh={() => void refreshNotifications()}
          onOpen={openNotification}
          onMarkRead={(notification) => void markCenterNotificationRead(notification)}
          onMarkAll={() => void markAllNotificationsRead()}
        />
      </div>
    </header>

    <div class="rail-label"><span>Projects</span><small>{projectRailCount.toString().padStart(2, '0')}</small></div>
    <div class="project-list" aria-live="polite" onscroll={closeProjectRailTooltip}>
      {#if projects.length === 0 && projectFolders.length === 0 && connection.status === 'connected' && !busy}
        <div class="project-empty"><strong>No projects</strong><p>Add a folder or switch profiles.</p><Button size="sm" onclick={showAddProject}>Add project</Button><Button size="sm" variant="ghost" onclick={() => { selectSettingsSection('profiles'); settingsOpen = true; }}>Profiles</Button></div>
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
      {#each unattachedWorktreeOperations() as operation (operation.id)}
        <WorktreeOperationRow
          {operation}
          collapsed={projectRailCollapsed}
          onSelect={() => showWorktreeOperation(operation)}
        />
      {/each}
    </div>
    <footer class="project-footer">
      <Button class="min-w-0 flex-1 justify-center" variant="outline" size="sm" disabled={connection.status !== 'connected' || busy} onclick={showAddProject}>
        <PlusIcon size={14} aria-hidden="true" /><span class="button-copy">Add project</span>
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
        feedback={feedbackSummaries}
        showFeedback={$showRecordedFeedbackSection}
        drafts={creationDrafts.filter((draft) => draft.projectId === selectedProject.id)}
        {selection}
        multiSelection={treeMultiSelection}
        collapsed={treeRailCollapsed}
        onSelect={(next) => void selectTreeItem(next)}
        onMultiSelectionChange={(next) => (treeMultiSelection = next)}
        onBulkAction={(action) => void runTreeBulkAction(action)}
        bulkBusy={treeBulkBusy || agentCascadeBusy}
        onCreateTodo={openTodoDraft}
        onBrowseTodos={openTodosBrowser}
        onBrowseScratchpads={openScratchpadsBrowser}
        onBrowseFeedback={openFeedbackBrowser}
        onBrowseProcesses={openProcessOverview}
        onAddAgent={() => void openAgentDraft()}
        onAddTerminal={() => void spawnTerminal()}
        onAddCommand={openCommandDraft}
        onAddScratchpad={() => void createScratchpad()}
        onStartFeedback={() => void openFeedbackPreflight()}
        {processBusyId}
        onStartProcess={(process) => void startOrReviewProcess(process)}
        onStopCommand={(process) => void stopProcess(process)}
        onRestartCommand={(process) => void restartProcess(process)}
        onOpenSettings={() => { todoBrowserOpen = false; scratchpadBrowserOpen = false; feedbackBrowserOpen = false; processOverviewKind = null; settingsOpen = true; dialog = null; }}
        onToggleCollapse={toggleTreeRail}
        reordering={processReorderBusy || coordinationReorderBusy}
        onReorderProcesses={(kind, orderedIds) => void persistProcessOrder(kind, orderedIds)}
        onReorderTodos={(orderedIds) => void persistTodoOrder(orderedIds)}
        onReorderScratchpads={(orderedIds) => void persistScratchpadOrder(orderedIds)}
        renameTarget={treeRenameTarget}
        onContextMenu={showContextMenu}
        onMiddleClick={(target) => void runTreeMiddleClick(target)}
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
    class:empty={selectedProject === null && activeWorktreeOperation === null}
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
          {updateFlow}
          onApplyUpdate={performAvailableUpdate}
          onRestartUpdate={restartInstalledUpdate}
          onDismissUpdate={dismissInstalledUpdate}
          onError={reportError}
          onProfileSwitched={() => window.location.reload()}
        />
      </div>
    {:else if activeWorktreeOperation}
      {#if error}
        <button class="error-banner" type="button" onclick={() => (error = null)}><span>{error}</span><strong>Dismiss</strong></button>
      {/if}
      <div class="item-viewer" role="region" aria-label={`${frameItemLabel} detail`}>
        <WorktreeProgressPanel
          operation={activeWorktreeOperation}
          onRetry={() => void retryWorktreeOperation(activeWorktreeOperation!)}
          onDismiss={dismissActiveWorktreeOperation}
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
        {#if selectedDraft}
          {#key selectedDraft.id}
            {@const draft = selectedDraft}
            {#if draft.kind === 'agent'}
              <NewAgentDraftPanel
                {draft}
                projectName={projectDisplayName(selectedProject)}
                tools={registeredAgentTools}
                templates={agentTemplates}
                loading={agentToolsLoading}
                metadataLoaded={agentDraftMetadataLoaded}
                focusOnMount={draftFocusRequestId === draft.id}
                busy={detailBusy}
                onChange={(patch) => patchCreationDraft(draft.id, patch)}
                onInitialize={(patch) => patchCreationDraft(draft.id, patch, false)}
                onCreate={(submission) => createAgentFromDraft(draft, submission)}
                onDiscard={() => void requestDiscardCreationDraft(draft.id)}
                onInitialFocusHandled={() => consumeDraftInitialFocus(draft.id)}
                onOpenSettings={() => {
                  todoBrowserOpen = false;
                  scratchpadBrowserOpen = false;
                  feedbackBrowserOpen = false;
                  processOverviewKind = null;
                  selectSettingsSection('templates');
                  settingsOpen = true;
                }}
                onError={(message) => reportError(new Error(message))}
              />
            {:else if draft.kind === 'command'}
              <NewCommandDraftPanel
                {client}
                project={selectedProject}
                {draft}
                focusOnMount={draftFocusRequestId === draft.id}
                onChange={(patch) => patchCreationDraft(draft.id, patch)}
                onPending={(input) => beginOptimisticCommandDraft(draft, input)}
                onAdded={(process, optimisticId) => void commandAdded(process, optimisticId)}
                onFailed={failPendingProcess}
                onDiscard={() => void requestDiscardCreationDraft(draft.id)}
                onInitialFocusHandled={() => consumeDraftInitialFocus(draft.id)}
              />
            {:else}
              <NewTodoDraftPanel
                {draft}
                projectName={projectDisplayName(selectedProject)}
                todos={coordination?.todos ?? []}
                focusOnMount={draftFocusRequestId === draft.id}
                busy={detailBusy}
                onChange={(patch) => patchCreationDraft(draft.id, patch)}
                onCreate={() => void createTodo(draft)}
                onDiscard={() => void requestDiscardCreationDraft(draft.id)}
                onInitialFocusHandled={() => consumeDraftInitialFocus(draft.id)}
              />
            {/if}
          {/key}
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
                bind:this={terminalView}
                {client}
                process={selectedProcess}
                connected={connection.status === 'connected'}
                visible={documentVisible}
                busy={processBusyId === selectedProcess.id}
                onStart={(process) => void startOrReviewProcess(process)}
                onError={reportError}
                onContextMenu={showContextMenu}
                onAppShortcut={handleAppShortcut}
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
            onCreate={openTodoDraft}
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
        {:else if feedbackBrowserOpen}
          <RecordedFeedbackBrowser
            project={selectedProject}
            feedback={feedbackSummaries}
            view={feedbackBrowserView}
            busyId={feedbackBrowserBusyId}
            onOpen={openBrowserFeedback}
            onViewChange={(view) => (feedbackBrowserView = view)}
            onRecord={() => void openFeedbackPreflight()}
            onArchive={(feedback, archived) => void setBrowserFeedbackArchived(feedback, archived)}
            onDelete={(feedback) => void deleteBrowserFeedback(feedback)}
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
        {:else if selection?.kind === 'feedback'}
          <RecordedFeedbackDetailView
            feedback={feedbackDetail}
            loading={detailLoading}
            busy={detailBusy}
            processes={feedbackTargetProcesses}
            onBack={openFeedbackBrowser}
            onSave={saveSelectedFeedback}
            onSendAgent={sendSelectedFeedbackToAgent}
            onSendNewAgent={sendSelectedFeedbackToNewAgent}
            onSendScratchpad={sendSelectedFeedbackToScratchpad}
            onCopy={copySelectedFeedbackPacket}
            onArchive={archiveSelectedFeedback}
            onDelete={() => void deleteSelectedFeedback()}
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
            onCreateComment={createScratchpadComment}
            onUpdateComment={updateScratchpadComment}
            onResolveComment={resolveScratchpadComment}
            onDeleteComment={deleteScratchpadComment}
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
          pullRequests={pullRequestsForWorktree(worktreeEntryFor(selectedProject))}
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
        <span>Local workspaces</span><h1>Add a project</h1><p>Choose a folder from this computer, or create a worktree from a project already here.</p>
        <div class="flex gap-2">
          <Button disabled={connection.status !== 'connected' || busy} onclick={showAddProject}><PlusIcon size={14} />Add project</Button>
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

{#if feedbackPreflightOpen && selectedProject}
  <RecordedFeedbackPreflight
    projectName={projectDisplayName(selectedProject)}
    preflight={feedbackPreflight}
    loading={feedbackPreflightLoading}
    installing={feedbackModelInstalling}
    starting={feedbackStarting}
    progress={feedbackModelProgress}
    error={feedbackPreflightError}
    onRefresh={() => void refreshFeedbackPreflight()}
    onRequestScreen={() => void requestFeedbackScreenAccess()}
    onInstall={() => void installFeedbackModel()}
    onStart={() => void startFeedbackRecording()}
    onClose={() => (feedbackPreflightOpen = false)}
  />
{/if}

{#if folderMenuRequest}
  {@const request = folderMenuRequest}
  <ProjectFolderMenu
    {request}
    onSettings={() => openProjectFolderSettings(request.folder)}
    onRename={() => beginRenameProjectFolder(request.folder)}
    onDelete={() => void confirmDeleteProjectFolder(request)}
    onClose={closeProjectFolderMenu}
  />
{/if}

{#if folderSettingsFolder}
  <ProjectFolderSettingsDialog
    folder={folderSettingsFolder}
    busy={folderSettingsBusy}
    onSave={(settings) => void saveProjectFolderSettings(settings)}
    onClose={() => { if (!folderSettingsBusy) folderSettingsFolder = null; }}
  />
{/if}

{#if quickJumpOpen}
  <QuickJumpPalette
    {projects}
    pullRequests={Object.fromEntries(projects.map((project) => [
      project.id,
      pullRequestsForWorktree(worktreeEntryFor(project))
    ]))}
    index={navigationIndex}
    currentProjectId={selectedProject?.id ?? null}
    {agentTools}
    {keepAwakeSupported}
    feedbackSupported={$recordedFeedbackSupported}
    recentKeys={quickJumpRecentKeys}
    loading={quickJumpLoading}
    onChoose={chooseQuickJumpTarget}
    onClose={closeQuickJump}
  />
{/if}

{#if quickPromptOpen}
  <QuickPromptPalette
    {client}
    canInsert={terminalView !== null && selectedProcess?.kind === 'agent' && selectedProcess.status === 'running'}
    onInsert={insertQuickPrompt}
    onClose={closeQuickPrompts}
    onError={reportError}
  />
{/if}

{#if addProjectDialogOpen}
  <AddProjectDialog
    {projects}
    folderBusy={addProjectFolderBusy}
    worktreeBusyProjectId={addProjectWorktreeBusyId}
    onChooseFolder={() => void chooseFolderFromAddProject()}
    onCreateWorktree={(project) => void createWorktreeFromAddProject(project)}
    onClose={() => {
      if (!addProjectFolderBusy && addProjectWorktreeBusyId === null) addProjectDialogOpen = false;
    }}
  />
{/if}

{#if registerProjectDialog}
  <RegisterProjectDialog
    path={registerProjectDialog.path}
    defaultTitle={registerProjectDialog.defaultTitle}
    busy={registerProjectBusy}
    error={registerProjectError}
    onSubmit={(title) => void submitRegisterProject(title)}
    onBack={returnToAddProject}
    onClose={() => {
      if (!registerProjectBusy) {
        registerProjectDialog = null;
        registerProjectError = null;
      }
    }}
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
  <KeyboardShortcuts {keepAwakeSupported} onClose={closeShortcuts} />
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
    refOptions={worktreeRefOptions}
    defaultRef={worktreeDefaultRef}
    branchesLoading={originBranchesLoading}
    busy={worktreeDialogBusy}
    error={worktreeDialogError}
    conflict={worktreeDialogConflict}
    onLoadBranches={() => void loadOriginBranches()}
    onValidateRef={validateWorktreeRef}
    onSubmit={(submission) => void submitWorktreeDialog(submission)}
    onOpenProject={openRegisteredConflictProject}
    onClearConflict={() => { worktreeDialogConflict = null; worktreeDialogError = null; }}
    onClose={closeWorktreeDialog}
  />
{/if}

{#if removeWorktreeDialog}
  <WorktreeRemoveDialog
    project={removeWorktreeDialog.project}
    repository={removeWorktreeDialog.repository}
    entry={removeWorktreeDialog.entry}
    busy={removeWorktreeBusy}
    error={removeWorktreeError}
    serverForceRequired={removeWorktreeForceRequired}
    onConfirm={(deleteFromDisk, forceDirty) => void confirmRemoveWorktree(deleteFromDisk, forceDirty)}
    onClose={() => { if (!removeWorktreeBusy) { removeWorktreeDialog = null; removeWorktreeForceRequired = false; } }}
  />
{/if}

{#if confirmationDialog}
  <ConfirmationDialog
    title={confirmationDialog.title}
    description={confirmationDialog.description}
    confirmLabel={confirmationDialog.confirmLabel}
    destructive={confirmationDialog.destructive}
    onConfirm={() => settleConfirmation(true)}
    onClose={() => settleConfirmation(false)}
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

{#if dialog === 'command' && selectedProject && commandDialogProcess}
  <AddCommandDialog
    {client}
    project={selectedProject}
    initialProcess={commandDialogProcess}
    onAdded={(process) => void commandAdded(process)}
    onClose={() => { dialog = null; commandDialogProcess = null; }}
  />
{/if}

{#if trustReview}
  <TrustReviewDialog review={trustReview} busy={trustBusy} onApprove={() => void approveTrust()} onClose={() => (trustReview = null)} />
{/if}

<style>
  .app-shell { display: grid; width: 100%; height: 100%; min-height: 0; max-height: 100%; grid-template-columns: var(--project-rail-width) var(--tree-rail-width) minmax(0, 1fr); overflow: hidden; background: var(--night); }
  .app-shell.with-version-banner { height: calc(100% - 42px); }
  .app-shell.no-project { grid-template-columns: var(--project-rail-width) minmax(0, 1fr); }
  .version-banner { display: grid; width: 100%; height: 42px; grid-template-columns: minmax(0, 1fr) auto auto; align-items: center; gap: 14px; border-bottom: 1px solid color-mix(in srgb, var(--warning) 55%, var(--border)); padding: 5px 8px 5px 11px; background: color-mix(in srgb, var(--warning) 9%, var(--card)); color: var(--text); }
  .version-banner > div:first-child { min-width: 0; }
  .version-banner > div:first-child strong, .version-banner > div:first-child span { display: block; }
  .version-banner strong { color: color-mix(in srgb, var(--warning) 78%, var(--foreground)); font-size: var(--font-size-sm); }
  .version-banner > div:first-child span { overflow: hidden; margin-top: 2px; color: color-mix(in srgb, var(--warning) 44%, var(--muted-foreground)); font-size: var(--font-size-xs); text-overflow: ellipsis; white-space: nowrap; }
  .version-banner small { color: color-mix(in srgb, var(--warning) 35%, var(--muted-foreground)); font: var(--font-size-xs) 'JetBrains Mono Variable', monospace; white-space: nowrap; }
  .version-banner-actions { display: flex; align-items: center; gap: var(--space-1); }
  .update-progress { position: relative; width: 150px; height: 3px; overflow: hidden; border-radius: 1px; background: color-mix(in srgb, var(--warning) 18%, var(--border)); }
  .update-progress span { display: block; height: 100%; background: var(--warning); transition: width 120ms linear; }
  .update-progress.indeterminate span { width: 38% !important; animation: update-progress-scan 900ms ease-in-out infinite; }
  .updated-version-notice { position: fixed; z-index: 80; top: 12px; right: 12px; min-height: 32px; border: 1px solid color-mix(in srgb, var(--success) 50%, var(--border)); border-radius: var(--radius); padding: 6px 10px; background: color-mix(in srgb, var(--success) 12%, var(--popover)); color: var(--foreground); box-shadow: 0 6px 20px color-mix(in srgb, var(--background) 35%, transparent); font-size: var(--font-size-sm); }
  .updated-version-notice.with-version-banner { top: 54px; }
  .remove-worktree-notice { position: fixed; z-index: 80; right: 12px; bottom: 12px; max-width: min(560px, calc(100vw - 24px)); border: 1px solid color-mix(in srgb, var(--warning) 50%, var(--border)); border-radius: var(--radius); padding: 8px 10px; background: color-mix(in srgb, var(--warning) 12%, var(--popover)); color: var(--foreground); box-shadow: 0 6px 20px color-mix(in srgb, var(--background) 35%, transparent); font-size: var(--font-size-sm); text-align: left; }

  @keyframes update-progress-scan {
    from { transform: translateX(-110%); }
    to { transform: translateX(270%); }
  }

  @media (prefers-reduced-motion: reduce) {
    .update-progress.indeterminate span { animation: none; transform: none; }
  }
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
  .keep-awake-slot { display: flex; flex: none; }

  .rail-label { display: flex; align-items: center; justify-content: space-between; min-height: 26px; border-top: 1px solid var(--border); padding: 4px 8px; color: var(--text-soft); font-size: var(--font-size-xs); font-weight: 680; letter-spacing: 0.04em; text-transform: uppercase; }
  .rail-label small { color: var(--muted-foreground); font-size: var(--font-size-xs); }
  .project-list { min-height: 0; flex: 1; overflow-y: auto; padding: 2px 5px 6px; scrollbar-color: var(--border-strong) transparent; scrollbar-width: thin; }
  .folder-children { margin-left: 17px; border-left: 1px solid var(--border-strong); padding-left: 4px; }
  .project-row { --project-icon-badge-background: var(--card); position: relative; display: grid; min-height: 44px; grid-template-columns: minmax(0, 1fr) auto; align-items: center; margin: 1px 0; border: 1px solid transparent; border-radius: 3px; }
  .project-row.nested { min-height: 42px; }
  .project-row:hover { --project-icon-badge-background: var(--popover); background: var(--popover); }
  .project-row.active { --project-icon-badge-background: var(--accent); border-color: var(--border-strong); background: var(--accent); box-shadow: inset 2px 0 var(--muted-foreground); }
  .project-row.has-operation { border-color: color-mix(in srgb, var(--agent-state-working) 34%, var(--border)); background: color-mix(in srgb, var(--agent-state-working) 6%, var(--card)); box-shadow: inset 2px 0 var(--agent-state-working); }
  .project-row.has-operation:hover, .project-row.operation-active { background: color-mix(in srgb, var(--agent-state-working) 10%, var(--card)); }
  .project-row[data-operation-status='failed'] { border-color: color-mix(in srgb, var(--destructive) 38%, var(--border)); background: color-mix(in srgb, var(--destructive) 7%, var(--card)); box-shadow: inset 2px 0 var(--destructive); }
  .project-row[data-operation-status='failed']:hover, .project-row[data-operation-status='failed'].operation-active { background: color-mix(in srgb, var(--destructive) 11%, var(--card)); }
  .project-row[data-operation-status='completed'] { border-color: color-mix(in srgb, var(--success) 34%, var(--border)); background: color-mix(in srgb, var(--success) 5%, var(--card)); box-shadow: inset 2px 0 var(--success); }
  .project-content { position: relative; display: block; width: 100%; min-width: 0; min-height: 42px; grid-column: 1; grid-row: 1; align-self: stretch; }
  .project-select { position: relative; display: grid; width: 100%; min-height: 42px; grid-template-columns: 20px minmax(0, 1fr) auto; grid-template-rows: minmax(20px, auto) 20px; align-items: center; column-gap: 7px; border: 0; padding: 3px 7px; background: transparent; text-align: left; cursor: pointer; }
  .project-select:focus-visible { --project-icon-badge-background: var(--border); outline: 1px solid #737b84; outline-offset: -2px; background: var(--border); }
  .app-shell :global(.project-select[data-reorderable='true']) { cursor: grab; }
  .app-shell :global(.project-select[data-reorder-dragging='true']) { opacity: 0.42; cursor: grabbing; }
  .app-shell :global(.project-select[data-reorder-drop]::after) { position: absolute; z-index: 3; right: 6px; left: 6px; height: 2px; background: var(--ring); content: ''; pointer-events: none; }
  .app-shell :global(.project-select[data-reorder-drop='before']::after) { top: -2px; }
  .app-shell :global(.project-select[data-reorder-drop='after']::after) { bottom: -2px; }
  .project-kind-icon { display: grid; width: 20px; height: 20px; flex: none; place-items: center; color: var(--muted-foreground); }
  .project-icon-anchor { display: inline-flex; grid-row: 1 / 3; flex: none; align-self: center; }
  .project-icon-anchor > :global(.tooltip-anchor) { display: inline-grid; width: 20px; height: 20px; place-items: center; }
  .project-row.active .project-kind-icon { color: var(--foreground); }
  .project-copy { min-width: 0; grid-column: 2; grid-row: 1; }
  .project-copy strong { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .project-copy strong { color: var(--foreground); font-size: var(--font-size-sm); font-weight: 620; }
  .project-meta-strip { position: relative; z-index: 3; display: inline-flex; min-width: 0; height: 20px; grid-column: 1; grid-row: 1; align-self: end; justify-self: stretch; align-items: center; justify-content: flex-start; gap: 1px; overflow: visible; margin: 0 3px 3px 34px; pointer-events: none; }
  .project-meta-strip :global(.worktree-meta), .project-meta-strip :global(.project-kind-indicators) { pointer-events: auto; }
  .project-compact-meta { position: absolute; z-index: 4; right: 0; bottom: 0; display: inline-flex; height: 8px; align-items: end; pointer-events: none; }
  .project-tooltip-copy { display: contents; }
  .project-tooltip-copy > strong, .project-tooltip-copy > span { display: block; min-width: 0; overflow-wrap: anywhere; }
  .project-tooltip-copy > strong { color: inherit; font-size: var(--font-size-xs); font-weight: 650; }
  .project-tooltip-copy > span { color: inherit; font-family: var(--terminal-font-family); font-size: var(--font-size-xs); opacity: .78; }
  .project-tooltip-parent { display: flex !important; align-items: center; gap: var(--space-1); }
  .project-tooltip-parent :global(svg) { flex: none; }
  :global(.project-rail-tooltip) { pointer-events: none; }
  .project-row-badges { position: relative; display: inline-flex; min-width: 0; grid-column: 3; grid-row: 1; align-items: center; justify-content: flex-end; gap: 4px; }
  .project-hotkey { position: absolute; top: 50%; right: calc(100% + 4px); display: inline-grid; min-width: 24px; height: 18px; place-items: center; visibility: hidden; border: 1px solid var(--border-strong); border-radius: 3px; padding: 0 4px; opacity: 0; background: var(--night); color: var(--muted-foreground); font: 600 9px/1 'JetBrains Mono Variable', monospace; transform: translateY(calc(-50% + 2px)); transition: opacity 80ms ease-out, transform 80ms ease-out, visibility 0s linear 80ms; pointer-events: none; }
  .project-hotkey.visible { visibility: visible; opacity: 1; transform: translateY(-50%); transition-delay: 0s; }
  .project-unread-rollup { display: inline-flex; min-width: 20px; height: 18px; flex: none; align-items: center; justify-content: center; gap: 3px; border: 1px solid color-mix(in srgb, var(--notification-unread) 45%, var(--border)); border-radius: 999px; padding: 0 5px; color: var(--notification-unread-foreground); background: color-mix(in srgb, var(--notification-unread) 9%, var(--popover)); font: 650 var(--font-size-xs)/1 'JetBrains Mono Variable', monospace; }
  .project-unread-rollup > span { width: 5px; height: 5px; border-radius: 999px; background: var(--notification-unread); }
  .rename-form { display: flex; width: 100%; grid-column: 1 / -1; align-items: center; gap: 4px; padding: 4px; }
  .rename-form input { min-width: 0; flex: 1; border: 1px solid var(--border-strong); padding: 5px; background: var(--background); color: var(--text); font-size: var(--font-size-sm); }
  .project-empty { margin: 5px; border: 1px dashed var(--border-strong); padding: 10px; }
  .project-empty strong { color: var(--foreground); font-size: var(--font-size-sm); } .project-empty p { margin: 3px 0 8px; color: var(--muted); font-size: var(--font-size-sm); }
  .project-footer { display: flex; gap: var(--space-1); padding: 6px; border-top: 1px solid var(--border); }
  .project-footer :global(.folder-button) { flex: none; }

  .resize-handle { position: absolute; z-index: 8; top: 0; right: -3px; bottom: 0; width: 6px; border: 0; padding: 0; background: transparent; cursor: col-resize; touch-action: none; }
  .resize-handle::after { position: absolute; top: 0; right: 2px; bottom: 0; width: 1px; background: transparent; content: ''; }
  .resize-handle:hover::after, .resize-handle:focus-visible::after { background: var(--muted-foreground); }

  .project-rail.collapsed .brand { display: grid; min-height: 112px; grid-template-rows: 28px 28px 28px 24px; align-content: center; justify-content: center; gap: 0; padding: 2px; }
  .project-rail.collapsed .rail-label span, .project-rail.collapsed .project-copy, .project-rail.collapsed .button-copy, .project-rail.collapsed .project-empty { display: none; }
  .project-rail.collapsed .brand-mark { grid-row: 1; place-self: center; }
  .project-rail.collapsed .keep-awake-slot { grid-row: 2; place-self: center; }
  .project-rail.collapsed .notification-slot { grid-row: 3; place-self: center; }
  .project-rail.collapsed :global(.brand-collapse) { grid-row: 4; width: 24px; height: 24px; place-self: center; }
  .project-rail.collapsed .rail-label { justify-content: center; padding-inline: 0; }
  .project-rail.collapsed .project-list { display: flex; flex-direction: column; gap: 4px; padding: 6px 7px; }
  .project-rail.collapsed .folder-children { display: contents; }
  .project-rail.collapsed :global(.folder-row) { width: 100%; height: 36px; min-height: 36px; flex: 0 0 36px; margin: 0; }
  .project-rail.collapsed .project-row { width: 100%; height: 40px; min-height: 40px; flex: 0 0 40px; margin: 0; }
  .project-rail.collapsed .project-content { min-height: 40px; }
  .project-rail.collapsed .project-select { position: relative; inset: auto; display: flex; width: 100%; height: 100%; flex: 0 0 100%; justify-content: center; gap: 0; padding: 4px; }
  .project-rail.collapsed .project-kind-icon { width: 30px; height: 30px; border: 1px solid var(--border-strong); border-radius: 3px; color: var(--foreground); background: var(--popover); }
  .project-rail.collapsed .project-row.has-operation .project-compact-meta { right: 2px; bottom: 2px; height: 14px; }
  .project-rail.collapsed :global(.project-actions) { display: none; }
  .project-rail.collapsed .project-row-badges { position: absolute; z-index: 2; top: 2px; right: 2px; }
  .project-rail.collapsed .project-hotkey { top: 18px; right: -2px; min-width: 14px; height: 14px; padding: 0 2px; font-size: 8px; }
  .project-rail.collapsed .project-unread-rollup { min-width: 14px; height: 14px; gap: 0; padding: 0 3px; border-color: var(--notification-unread); font-size: 9px; }
  .project-rail.collapsed .project-unread-rollup > span { display: none; }
  .project-rail.collapsed .project-footer { padding: 5px; }

  @media (prefers-reduced-motion: reduce) {
    .project-hotkey { transform: translateY(-50%); transition: none; }
  }

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

</style>
