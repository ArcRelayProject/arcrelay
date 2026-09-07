// Generated from Rust. Run npm run generate:ipc; do not edit.

export type ActionImportResult = { importedCount: number, names: Array<string>, };

export type ActionOutputSnapshot = { actionId: string, revision: number, lines: Array<string>, };

export type ActionPresetView = { id: string, name: string, description: string, iconId: string, iconSvg: string, color: string, group: string, defaultEnabled: boolean, requiresConfirmation: boolean, installed: boolean, actionType: ActionType, };

export type ActionSnapshot = { name: string, definition: JsonValue, requiresConfirmation: boolean, requiresUnlockedSession: boolean, requirements: Array<string>, };

export type ActionType = { "type": "LaunchApp", app_name: string, app_path: string | null, } | { "type": "OpenPath", path: string, } | { "type": "OpenUrl", url: string, } | { "type": "ShellCommand", command: string, working_dir: string | null, } | { "type": "Hotkey", modifiers: Array<string>, key: string, } | { "type": "AppleScript", script: string, } | { "type": "System", operation: SystemOperation, } | { "type": "Media", operation: MediaOperation, } | { "type": "SetSystemVolume", volume: number, } | { "type": "SetSystemMuted", muted: boolean, } | { "type": "SetMicrophone", active: boolean, } | { "type": "ToggleShellCommand", start_command: string,
/**
 * If empty, the running process is killed (SIGTERM).
 */
stop_command: string | null, working_dir: string | null, };

export type ActionView = { actionTypeLabel: string, isToggle: boolean, isRunning: boolean, requiresConfirmation: boolean, globalShortcutError: string | null, id: string,
/**
 * Optimistic version, assigned by the catalog. Old configurations start at zero.
 */
revision: number, name: string, icon_id: string,
/**
 * Sanitized SVG markup rendered by both desktop and mobile clients.
 */
icon_svg: string, color: string, group: string, sort_order: number, source_preset_id: string | null, confirm_before_run: boolean,
/**
 * Local desktop trigger, independent from a Hotkey action's output keys.
 */
global_shortcut?: string | null, action_type: ActionType, };

export type ActivationPolicy = "Immediate" | { "RequireModifier": { hid_usage: number, } } | { "Dwell": { milliseconds: number, } };

export type ActivityStatus = "awaitingConfirmation" | "skipped" | "queued" | "running" | "succeeded" | "failed" | "canceled" | "interrupted";

export type AppSettings = { revision: number, deviceName: string, theme: ThemePreference, language: LanguagePreference, launchAtStartup: boolean, launchSilently: boolean, autoUpdateEnabled: boolean, updateChannel: UpdateChannel, notifications: NotificationPreferences, sounds: SoundPreferences, clipboardEnabled: boolean, clipboardShortcut: string, clipboardAutoFocusSearch: boolean, clipboardSortBy: ClipboardSortPreference, clipboardSyncEnabled: boolean, clipboardSyncUpdateSystemClipboard: boolean, clipboardSyncEditsAndDeletes: boolean, clipboardSyncFavorites: boolean, nearbyDiscoverable: boolean, enhancedScreenshotEnabled: boolean, screenshotShortcut: string, screenshotIncludeCursor: boolean, screenshotFormat: ScreenshotFormat, screenshotFileNameTemplate: string, webFiles: WebGatewaySettings, };

export type AppSettingsPatch = { deviceName?: string, theme?: ThemePreference, language?: LanguagePreference, launchAtStartup?: boolean, launchSilently?: boolean, autoUpdateEnabled?: boolean, updateChannel?: UpdateChannel, notifications?: NotificationPreferencesPatch, sounds?: SoundPreferencesPatch, clipboardEnabled?: boolean, clipboardShortcut?: string, clipboardAutoFocusSearch?: boolean, clipboardSortBy?: ClipboardSortPreference, clipboardSyncEnabled?: boolean, clipboardSyncUpdateSystemClipboard?: boolean, clipboardSyncEditsAndDeletes?: boolean, clipboardSyncFavorites?: boolean, nearbyDiscoverable?: boolean, enhancedScreenshotEnabled?: boolean, screenshotShortcut?: string, screenshotIncludeCursor?: boolean, screenshotFormat?: ScreenshotFormat, screenshotFileNameTemplate?: string, webFiles?: WebGatewaySettingsPatch, };

export type AppUpdateCheckResult = { currentVersion: string, channel: UpdateChannel, update: AppUpdateMetadata | null, };

export type AppUpdateMetadata = { version: string, notes: string | null, publishedAt: string | null, };

export type AppUpdateProgress = { phase: AppUpdateProgressPhase, downloadedBytes: number, totalBytes: number | null, };

export type AppUpdateProgressPhase = "downloading" | "downloaded" | "installed" | "failed";

export type ApplicationEvent = "started" | "exited" | "foreground" | "background";

export type ApplicationIdentity = { id: string, name: string, path: string, };

export type AutomationActivity = { id: string, automationId: string, definition: AutomationDefinition, event: AutomationEvent, status: ActivityStatus, reason: string | null, createdAt: string, finishedAt: string | null, steps: Array<StepRun>, confirmedSteps: Array<number>, runConfirmed: boolean, };

export type AutomationCondition = { "type": "timeRange", weekdays: Array<number>, start: string, end: string, timezone: string, } | { "type": "applicationRunning", app: ApplicationIdentity, running: boolean, } | { "type": "deviceConnected", deviceId: string, connected: boolean, };

export type AutomationDefinition = { id: string, name: string, enabled: boolean, trigger: AutomationTrigger, conditions: Array<AutomationCondition>, steps: Array<AutomationStep>, runMode: RunMode, revision: number, createdAt: string, updatedAt: string, legacyId: string | null, };

export type AutomationEvent = { id: string, kind: string, occurredAt: string, variables: { [key in string]: string }, originAutomationId: string | null, };

export type AutomationIssue = { code: string, message: string, remedy: string, stepIndex: number | null, };

export type AutomationStep = { "type": "quickAction", actionId: string, } | { "type": "shell", shell: ShellKind, script: string, workingDirectory: string | null, timeoutSeconds: number, } | { "type": "delay", durationSeconds: number, } | { "type": "notification", title: string, body: string, sendToConnectedDevices: boolean, };

export type AutomationTrigger = { "type": "manual" } | { "type": "schedule", time: string, weekdays: Array<number>, timezone: string, catchUp: boolean, } | { "type": "application", event: ApplicationEvent, apps: Array<ApplicationIdentity>, } | { "type": "system", event: SessionEvent, } | { "type": "device", connected: boolean, deviceIds: Array<string>, } | { "type": "transfer", received: boolean, deviceIds: Array<string>, fileKinds: Array<string>, } | { "type": "hotkey", shortcut: string, };

export type BootstrapState = { actions: Array<ActionView>, revision: number, port: number, serverRunning: boolean, connectedDevices: Array<ConnectedDeviceView>, pairedDevices: Array<ConnectedDeviceView>, pendingPairing: PendingPairingView | null, outgoingPairings: Array<OutgoingPairingView>, inputPermission: InputPermissionState, activeInputDevice: ConnectedDeviceView | null, inputMetrics: InputMetricsView | null, activity: Array<string>, unreadNotificationCount: number, mcpRunning: boolean, mcpPort: number, privacy: PrivacySnapshot, };

export type Capability = { id: string, available: boolean, reason: string | null, remedy: string | null, };

export type ClipboardContentKind = "text" | "html" | "image" | "files";

export type ClipboardCursorInput = { sortAtMs: number, id: number, };

export type ClipboardCursorView = { sortAtMs: number, id: number, };

export type ClipboardHistoryView = { revision: number, entries: Array<ClipboardItemView>, nextCursor: ClipboardCursorView | null, totalCount: number | null, };

export type ClipboardImageOcr = { text: string, blocks: Array<ClipboardOcrBlock>, modelVersion: string, updatedAtMs: number, };

export type ClipboardItemView = { id: number, kind: ClipboardContentKind, preview: string, sourceApp: string | null, firstCapturedAtMs: number, capturedAtMs: number, updatedAtMs: number, sizeBytes: number, characterCount: number | null, itemCount: number, width: number | null, height: number | null, sensitive: boolean, favorite: boolean, labels: Array<ClipboardLabel>, available: boolean, syncId: string, sourceDeviceName: string | null, textSyntax: ClipboardTextSyntax, };

export type ClipboardLabel = { id: string, name: string, color: string, revision: number, updated_by_device_id: string, deleted: boolean, };

export type ClipboardOcrBlock = { text: string, confidence: number, left: number, top: number, width: number, height: number,
/**
 * Four corners in source-image pixel coordinates when the detector
 * provides a rotated quadrilateral.
 */
points: [ClipboardOcrPoint, ClipboardOcrPoint, ClipboardOcrPoint, ClipboardOcrPoint] | null,
/**
 * CTC-aligned character geometry. Empty for results produced by OCR
 * engines that predate character alignment.
 */
characters: Array<ClipboardOcrCharacter>, };

export type ClipboardOcrCharacter = { text: string, confidence: number,
/**
 * Four corners in source-image pixel coordinates, ordered from the
 * recognition start edge around the character.
 */
points: [ClipboardOcrPoint, ClipboardOcrPoint, ClipboardOcrPoint, ClipboardOcrPoint], };

export type ClipboardOcrPoint = { x: number, y: number, };

export type ClipboardPasteMode = "source" | "plain_text" | "rich_text" | "json_compact" | "json_formatted" | "yaml";

export type ClipboardSortPreference = "createdAt" | "updatedAt";

export type ClipboardTextSyntax = "plain" | "json" | "yaml" | "markdown" | { "code": { language: string | null, } } | "xml" | "svg" | "mermaid" | "mx_graph" | "url" | "email" | "phone_number" | "color" | "ip_address" | "jwt_token" | "file_path" | "magnet_link";

export type ClipboardTimelinePositionInput = { "type": "around", id: number, } | { "type": "newer", cursor: ClipboardCursorInput, } | { "type": "older", cursor: ClipboardCursorInput, };

export type ClipboardTimelineView = { revision: number, entries: Array<ClipboardItemView>, anchor: ClipboardCursorView | null, newerCursor: ClipboardCursorView | null, olderCursor: ClipboardCursorView | null, totalCount: number, };

export type ColorMode = "monochrome" | "color";

export type ConfigurationChange = { id: string, actor: string, entity: string, targetId: string, status: string, createdAt: string, before: JsonValue | null, after: JsonValue | null, error: string | null, };

export type ConnectedDeviceView = { id: string, name: string, autoConnect: boolean, };

export type ConsumerKey = "PreviousTrack" | "PlayPause" | "NextTrack" | "Mute" | "VolumeDown" | "VolumeUp" | "BrightnessDown" | "BrightnessUp";

export type ConsumerShortcut = { key: number,
/**
 * Ctrl=1, Alt/Option=2, Shift=4, Meta/Command=8; left/right are equivalent.
 */
modifiers: number, action: ConsumerKey, };

export type ContinuousPasteProgress = { current: number, total: number, active: boolean, };

export type DeskRectUm = { x: number, y: number, width: number, height: number, };

export type DesktopRuntimeState = { revision: number, port: number, serverRunning: boolean, connectedDevices: Array<ConnectedDeviceView>, pairedDevices: Array<ConnectedDeviceView>, pendingPairing: PendingPairingView | null, outgoingPairings: Array<OutgoingPairingView>, inputPermission: InputPermissionState, activeInputDevice: ConnectedDeviceView | null, inputMetrics: InputMetricsView | null, activity: Array<string>, unreadNotificationCount: number, mcpRunning: boolean, mcpPort: number, privacy: PrivacySnapshot, };

export type DeviceId = string;

export type DiagnosticBundleInfo = { path: string, fileName: string, logFileCount: number, sizeBytes: number, };

export type DiagnosticRecord = { timestampMs: number, category: string, message: string, latencyMicros: number | null, };

export type DisplayAvailability = "Ready" | "Offline" | "SharingDisabled" | "PermissionRequired" | "DisplayDisconnected" | "Connecting";

export type DisplayFingerprint = string;

export type DisplayId = string;

export type DisplayRotation = "Degrees0" | "Degrees90" | "Degrees180" | "Degrees270";

export type DisplaySurface = { displayId: DisplayId, deviceId: ServiceInstanceId, fingerprint: DisplayFingerprint, name: string, pixelSize: SizeU32, logicalBounds: LogicalRect, scaleFactor: ScaleFactor, physicalSizeUm: SizeI64, rotation: DisplayRotation, deskRectUm: DeskRectUm, geometryConfidence: GeometryConfidence, inventoryRevision: InventoryRevision, };

export type DropSnapshot = { revision: number, hovering: boolean, paths: Array<string>, };

export type DuplexMode = "oneSided" | "twoSidedLongEdge" | "twoSidedShortEdge";

export type Edge = "Left" | "Right" | "Top" | "Bottom";

export type EdgeSegment = { startUm: number, endUm: number, };

export type EdgeTestRequest = { displayId: string, pointXUm: number, pointYUm: number, deltaXUm: number, deltaYUm: number, };

export type EdgeTestResult = { displayId: string, pointXUm: number, pointYUm: number, portalIds: Array<string>, };

export type FileKind = "image" | "pdf" | "archive" | "file";

export type FrontendLogEntry = { level: string, event: string, detail: string, stack: string | null, };

export type GeometryConfidence = "Unknown" | "Estimated" | "HardwareReported" | "UserProvided" | "UserCalibrated";

export type GestureDebugAction = "swipeLeft" | "swipeRight" | "swipeUp" | "swipeDown" | "pinchIn" | "pinchOut";

export type GestureDebugBatch = { id: number,
/**
 * User-supplied action label, never a detected finger count.
 */
label: string, mode: string, elapsedMs: number, finished: boolean, stopReason: string, totalSamples: number, markerSamples: number, suppressedMarkers: number, droppedSamples: number, capacityDroppedSamples: number, retainedSamples: number, triggers: Array<GestureDebugTriggerRecord>, };

export type GestureDebugBatchReport = { batch: GestureDebugBatch, samples: Array<GestureDebugSample>, };

export type GestureDebugReport = { osVersion: string, batches: Array<GestureDebugBatchReport>, };

export type GestureDebugSample = { id: number, batchId: number, elapsedMs: number,
/**
 * "tap" may include synthetic events: source tags do not always survive posting.
 */
source: string, eventType: number, subtype: number, motion: number, phase: number, progress: number, velocityX: number, velocityY: number, magnification: number, rotation: number, swipeMask: number, suppressed: boolean, };

export type GestureDebugSnapshot = { available: boolean, osVersion: string, accessibilityGranted: boolean, capturing: boolean, suppressing: boolean, injecting: boolean, remainingMs: number, totalSamples: number, droppedSamples: number, markerSamples: number, capacityDroppedSamples: number, sampleLimit: number, batchLimit: number, batches: Array<GestureDebugBatch>,
/**
 * Only the latest batch's tail for UI polling. Full export is a separate command.
 */
samples: Array<GestureDebugSample>, message: string, };

export type GestureDebugTrigger = { action: GestureDebugAction, durationMs: number, delayMs: number, amount: number, reverse: boolean, cancelAtEnd: boolean, };

export type GestureDebugTriggerRecord = { elapsedMs: number, request: GestureDebugTrigger, };

export type HorizontalScrollBehavior = "NativeScroll" | "NavigateHistory";

export type ImportActionsResponse = { result: ActionImportResult, state: BootstrapState, };

export type InputMetricsView = { receiveHz: number, quartzHz: number, averageGapUs: number, maximumGapUs: number, maximumSourceGapUs: number, maximumTransportStallUs: number, averageApplyUs: number, maximumApplyUs: number, };

export type InputPermissionState = "Granted" | "Denied" | "Unsupported";

export type InstalledAppView = { name: string, path: string, identifier: string | null, version: string | null, iconDataUrl: string | null, };

export type InventoryRevision = number;

export type IpcError = { code: IpcErrorCode, message: string, retryable: boolean, recoveryAction: IpcRecoveryAction, errorId: string | null, };

export type IpcErrorCode = "operationFailed" | "invalidArgument" | "notFound" | "conflict" | "permissionDenied" | "resourceExhausted" | "failedPrecondition" | "unavailable" | "cancelled" | "internal" | "directorySnapshotExpired" | "textSelectionExpired" | "unsupported";

export type IpcRecoveryAction = "none" | "retryWithBackoff" | "refresh" | "changeRequest";

export type JsonValue = number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null;

export type KeyChord = { modifiers: Array<number>, key: number, };

export type KeyboardMappingRule = { source: KeyChord, action: SemanticAction, };

export type KeyboardProfile = { name: string, kind: KeyboardProfileKind, revision: number, textStrategy: TextStrategy, semanticOverrides: Array<KeyboardMappingRule>, };

export type KeyboardProfileKind = "Productivity" | "Terminal" | "Ide" | "RemoteDesktop" | "GameRaw" | "Presentation";

export type LanguagePreference = "system" | "zhCn" | "enUs" | "jaJp" | "koKr" | "deDe" | "frFr" | "esEs" | "ptBr";

export type LocalPrinter = { id: LocalPrinterId, displayName: string, status: PrinterStatus, connectionKind: string, isDefault: boolean, capabilities: PrinterCapabilities, };

export type LocalPrinterId = string;

export type LocalSharedDirectory = { id: string, name: string, path: string, writable: boolean, web: WebSharePolicyView, };

export type LogStatus = { enabled: boolean, runId: string, startedAtMs: number, logDirectory: string, currentLogFile: string | null, droppedLines: number, schemaVersion: number, maxFileBytes: number, maxTotalBytes: number, retentionDays: number, currentFilter: string, detailedUntilMs: number | null, };

export type LogicalRect = { x: number, y: number, width: number, height: number, };

export type MaskStyle = "frosted" | "solid";

export type McpClientConfig = { clientId: string, endpoint: string, codex: string, claudeDesktop: string, httpJson: string, codexHttp: string, installPrompt: string, };

export type McpClientView = { id: string, name: string, permissions: McpPermissions, revoked: boolean, legacy: boolean, createdAt: string, };

export type McpConfigView = { endpoint: string, token: string, configText: string, };

export type McpPermissions = { notifications: boolean, read: boolean, manage: boolean, enable: boolean, execute: boolean, scripts: boolean, };

export type MediaOperation = "toggle_play_pause" | "play" | "pause" | "next" | "previous" | "seek_forward" | "seek_backward";

export type MediaSize = { name: string, widthMicrons: number, heightMicrons: number, };

export type ModuleState = "idle" | "starting" | "ready" | "unavailable" | "stopping" | "stopped";

export type ModuleStatus = { name: string, state: ModuleState, error: string | null, };

export type NearbyDesktopView = { deviceId: string, deviceName: string, host: string, paired: boolean, connecting: boolean, };

export type NearbyPeer = { id: string, name: string, platform: string, model: string, address: string, addresses: Array<string>, port: number, publicKey: string, certificateSha256: string, paired: boolean, automaticReceive: boolean, lastSeenAtMs: number, };

export type NearbyPeerView = { serviceInstanceId: string, displayName: string | null, addresses: Array<string>, port: number, certificateSha256: string, capabilityDigest: string, paired: boolean, connected: boolean, capabilities: PlatformCapabilities | null, };

export type NotificationKind = "info" | "task_completed" | "action_required";

export type NotificationPreferences = { enabled: boolean, onlyWhenInactive: boolean, showPreviews: boolean, pairingRequests: boolean, transferRequests: boolean, transferCompleted: boolean, transferFailed: boolean, remoteFileCompleted: boolean, remoteFileFailed: boolean, printCompleted: boolean, printFailed: boolean, deviceConnections: boolean, workflowActionRequired: boolean, workflowCompleted: boolean, workflowFailed: boolean, inputPermissionRequired: boolean, agentNotifications: boolean, updateAvailable: boolean, };

export type NotificationPreferencesPatch = { enabled?: boolean, onlyWhenInactive?: boolean, showPreviews?: boolean, pairingRequests?: boolean, transferRequests?: boolean, transferCompleted?: boolean, transferFailed?: boolean, remoteFileCompleted?: boolean, remoteFileFailed?: boolean, printCompleted?: boolean, printFailed?: boolean, deviceConnections?: boolean, workflowActionRequired?: boolean, workflowCompleted?: boolean, workflowFailed?: boolean, inputPermissionRequired?: boolean, agentNotifications?: boolean, updateAvailable?: boolean, };

export type NotificationView = { id: string, title: string, body: string, source: string, kind: NotificationKind, reference: string | null, createdAtMs: number, readAtMs: number | null, readByDeviceName: string | null, deliveryState: string, };

export type OsFamily = "Windows" | "MacOs" | "LinuxX11" | "LinuxWayland" | "Android" | "Ios" | "Unknown";

export type OutgoingPairingView = { deviceName: string, deviceId: string, pairingCode: string, };

export type OutgoingPrintJob = { localJobId: number, remoteDeviceId: string, remoteDeviceName: string, printerShareId: string, printerName: string, documentName: string, remoteJobId: string, state: PrintJobState | null, failureMessage: string | null, createdAtMs: number, updatedAtMs: number, sourceAvailable: boolean, };

export type PendingPairingGrantView = { id: string, capability: string, direction: string, label: string, };

export type PendingPairingView = { deviceName: string, deviceId: string, pairingCode: string, permissions: Array<string>, grants: Array<PendingPairingGrantView>, };

export type PlatformCapabilities = { canCapturePointer: boolean, canCaptureKeyboard: boolean, canSuppressLocalInput: boolean, canPlaceInternalBarrier: boolean, canInjectAbsolutePointer: boolean, canInjectKeyboard: boolean,
/**
 * Pointer input is confined to the receiver's foreground application.
 * This does not grant system-wide pointer or keyboard injection.
 */
canInjectAppPointer: boolean, canControlElevatedApps: boolean, canPersistPermission: boolean, canCaptureNativeQuartzEvents: boolean, canInjectNativeQuartzEvents: boolean, canCapturePrecisionTouchpadEvents: boolean, canInjectPrecisionTouchpadEvents: boolean, canCaptureSystemGestures: boolean, canInjectSystemGestures: boolean,
/**
 * Maximum supported format; absent/zero means v1 for legacy opt-in peers.
 */
systemGestureFormatVersion: number, consumerCaptureMask: number, consumerInjectMask: number, brightnessDisplayIds: Array<string>, limitation: string | null, };

export type Portal = { portalId: PortalId, sourceDisplay: DisplayId, sourceEdge: Edge, sourceSegment: EdgeSegment, targetDisplay: DisplayId, targetEdge: Edge, targetSegment: EdgeSegment, direction: PortalDirection, activationPolicy: ActivationPolicy, allowWhileDragging: boolean, insetUm: number, hysteresisUm: number, status: PortalStatus, };

export type PortalDirection = "OneWay" | "Bidirectional";

export type PortalId = string;

export type PortalStatus = "Active" | "SuspendedOffline" | "NeedsReconciliation" | "UnsupportedCapability";

export type PrintJobActivitySnapshot = { revision: number, received: Array<ReceivedPrintJobView>, sent: Array<OutgoingPrintJob>, };

export type PrintJobState = "offered" | "receiving" | "validating" | "ready" | "submitting" | "queued" | "printing" | "completed" | "held" | "cancelled" | "failed" | "ambiguous";

export type PrintResolution = { horizontalDpi: number, verticalDpi: number, };

export type PrinterCapabilities = { mediaSizes: Array<MediaSize>, colorModes: Array<ColorMode>, duplexModes: Array<DuplexMode>, resolutions: Array<PrintResolution>, maxCopies: number, supportsPageRanges: boolean, supportsCollation: boolean, acceptedDocumentTypes: Array<string>, };

export type PrinterShare = { id: PrinterShareId, localPrinterId: LocalPrinterId, displayName: string, scope: ShareScope, state: ShareState, capabilities: PrinterCapabilities, capabilityRevision: number, createdAtMs: number, updatedAtMs: number, };

export type PrinterShareId = string;

export type PrinterSharingSnapshot = { hostingSupported: boolean, localPrinters: Array<LocalPrinter>, shares: Array<PrinterShare>, remotePrinters: Array<RemotePrinter>, queueBindings: Array<RemoteQueueBinding>, };

export type PrinterStatus = "ready" | "busy" | "offline" | "error" | "unknown";

export type PrivacySettings = { autoEnableOnMirror: boolean, allowRemoteActions: boolean, maskStyle: MaskStyle, protectedApps: Array<ProtectedApp>, };

export type PrivacySnapshot = { active: boolean, manualEnabled: boolean, mirrorDetected: boolean, activationSource: string, visibleProtectedWindows: Array<ProtectedWindowView>, settings: PrivacySettings, };

export type ProtectedApp = { name: string, identifier: string | null, path: string | null, enabled: boolean, };

export type ProtectedWindowView = { windowId: number, appName: string, };

export type PublishedPrinter = { sourceDeviceId: DeviceId, sourceDeviceName: string, shareId: PrinterShareId, displayName: string, status: PrinterStatus, capabilities: PrinterCapabilities, capabilityRevision: number, };

export type QueueBindingId = string;

export type QueueBindingState = "installing" | "ready" | "sourceOffline" | "removing" | "failed";

export type QuickAction = { id: string,
/**
 * Optimistic version, assigned by the catalog. Old configurations start at zero.
 */
revision: number, name: string, icon_id: string,
/**
 * Sanitized SVG markup rendered by both desktop and mobile clients.
 */
icon_svg: string, color: string, group: string, sort_order: number, source_preset_id: string | null, confirm_before_run: boolean,
/**
 * Local desktop trigger, independent from a Hotkey action's output keys.
 */
global_shortcut?: string | null, action_type: ActionType, };

export type ReceivedPrintJobView = { id: string, documentName: string, documentSizeBytes: number, sourceDeviceId: string, printerName: string, state: PrintJobState, failureMessage: string | null, createdAtMs: number, updatedAtMs: number, };

export type RemoteDeleteResult = { path: string, error: string | null, };

export type RemoteFileDeviceView = { id: string, name: string, };

export type RemoteFileDirectoryPage = { entries: Array<RemoteFileEntry>, nextCursor: string | null, };

export type RemoteFileDragPreparation = { localPath: string, iconPath: string, };

export type RemoteFileEntry = { name: string, relativePath: string, kind: RemoteFileKind, size: number, modifiedAtMs: number, };

export type RemoteFileKind = "file" | "folder";

export type RemoteFileOpenResult = { localPath: string, editable: boolean, };

export type RemoteFileShare = { id: string, name: string, writable: boolean, };

export type RemoteFileSortDirection = "ascending" | "descending";

export type RemoteFileSortKey = "name" | "modified" | "type" | "size";

export type RemoteFileState = { devices: Array<RemoteFileDeviceView>, localShares: Array<LocalSharedDirectory>, };

export type RemoteFileTransferSession = { id: string, direction: string, status: string, name: string, peerId: string, shareId: string, directoryPath: string, bytesTransferred: number, totalBytes: number, filesTransferred: number, totalFiles: number, currentName: string, error: string | null, startedAtMs: number, updatedAtMs: number, };

export type RemotePrinter = { printer: PublishedPrinter, address: string, port: number, certificateSha256: string, };

export type RemoteQueueBinding = { id: QueueBindingId, remoteDeviceId: DeviceId, printerShareId: PrinterShareId, systemQueueName: string, systemQueueId: SystemQueueId | null, state: QueueBindingState, autoReconnect: boolean, failureMessage: string | null, createdAtMs: number, updatedAtMs: number, };

export type RunMode = "automatic" | "askBeforeRun";

export type RuntimeSnapshot = { revision: number, productId: string, serviceInstanceId: string, localOperatingSystem: OsFamily, remoteOperatingSystems: { [key in string]: OsFamily }, capabilities: PlatformCapabilities, configuration: WorkspaceConfiguration, discoveredPeers: Array<string>, nearbyPeers: Array<NearbyPeerView>, connectedPeers: Array<string>, displayAvailability: { [key in DisplayId]: DisplayAvailability }, controller: string | null, controlEpoch: number | null, captureActive: boolean, diagnostics: Array<DiagnosticRecord>, };

export type ScaleFactor = number;

export type ScreenshotFormat = "png" | "jpeg" | "webp";

export type SemanticAction = "SelectAll" | "Copy" | "Paste" | "Cut" | "Undo" | "Redo" | "ApplicationSwitch";

export type ServiceInstanceId = string;

export type SessionEvent = "locked" | "unlocked" | "sleeping" | "resumed" | "started";

export type ShareScope = "pairedDevices";

export type ShareState = "published" | "suspended" | "removed";

export type ShellKind = "system" | "sh" | "bash" | "zsh" | "powershell";

export type SizeI64 = { width: number, height: number, };

export type SizeU32 = { width: number, height: number, };

export type SoundEvent = "clipboardAdded" | "clipboardReceived" | "clipboardUsed" | "transferRequest" | "transferSent" | "transferReceived" | "transferFailed" | "actionStarted" | "actionSucceeded" | "actionFailed" | "automationStarted" | "automationConfirmation" | "automationSucceeded" | "automationFailed" | "automationInterrupted";

export type SoundPreferences = { enabled: boolean, volume: number, muteDuringPrivacy: boolean, mutedAutomationIds: Array<string>, clipboardAdded: boolean, clipboardReceived: boolean, clipboardUsed: boolean, transferRequest: boolean, transferSent: boolean, transferReceived: boolean, transferFailed: boolean, actionStarted: boolean, actionSucceeded: boolean, actionFailed: boolean, automationStarted: boolean, automationConfirmation: boolean, automationSucceeded: boolean, automationFailed: boolean, automationInterrupted: boolean, };

export type SoundPreferencesPatch = { enabled?: boolean, volume?: number, muteDuringPrivacy?: boolean, mutedAutomationIds?: Array<string>, clipboardAdded?: boolean, clipboardReceived?: boolean, clipboardUsed?: boolean, transferRequest?: boolean, transferSent?: boolean, transferReceived?: boolean, transferFailed?: boolean, actionStarted?: boolean, actionSucceeded?: boolean, actionFailed?: boolean, automationStarted?: boolean, automationConfirmation?: boolean, automationSucceeded?: boolean, automationFailed?: boolean, automationInterrupted?: boolean, };

export type StepResult = { exitCode: number | null, stdout: string, stderr: string, error: string | null, };

export type StepRun = { index: number, step: AutomationStep, action: ActionSnapshot | null, status: ActivityStatus, startedAt: string | null, finishedAt: string | null, result: StepResult, };

export type SystemFolder = { id: string, peerId: string, shareId: string, name: string, online: boolean, registered: boolean, error: string | null, recoveryCount: number, };

export type SystemOperation = "lock_screen" | "sleep" | "display_sleep" | "shutdown" | "restart" | "screenshot_full" | "screenshot_region";

export type SystemQueueId = string;

export type SystemShareItem = { path: string, name: string, size: number, modifiedAtMs: number | null, };

export type SystemShareRequest = { id: string, source: SystemShareSource, createdAtMs: number, targetPeerId: string | null, items: Array<SystemShareItem>, };

export type SystemShareSource = "commandLine" | "tray" | "windowsContextMenu" | "windowsShareTarget" | "macosShareExtension" | "linuxFileManager";

export type TextSlice = { id: string, level: TextSliceLevel, start: number, end: number, text: string, };

export type TextSliceLevel = "document" | "block" | "sentence" | "phrase" | "word";

export type TextSliceLevels = { document: Array<TextSlice>, block: Array<TextSlice>, sentence: Array<TextSlice>, phrase: Array<TextSlice>, word: Array<TextSlice>, };

export type TextSliceModel = { text: string, version: string, levels: TextSliceLevels, truncated: boolean, };

export type TextStrategy = "UseTargetLayout" | "FollowSourceText";

export type ThemePreference = "system" | "light" | "dark";

export type TopologyRevision = number;

export type TransferDirection = "sending" | "receiving";

export type TransferDraftFile = { path: string, canonicalPath: string | null, name: string, size: number | null, modifiedAtMs: number | null, kind: FileKind, thumbnailDataUrl: string | null, error: string | null, };

export type TransferFileProgress = { id: number, completedBytes: number, };

export type TransferFileView = { id: number, name: string, relativePath: string,
/**
 * Actual finalized local path, including collision renames. Never sent in a wire manifest.
 */
localPath?: string, receiveDirectory?: string, size: number, mediaType: string, kind: FileKind, thumbnailDataUrl: string | null, completedBytes: number, };

export type TransferProgress = { revision: number, baseRevision: number, transferId: string, completedBytes: number, speedBytesPerSecond: number, remainingSeconds: number | null, updatedAtMs: number, files: Array<TransferFileProgress>, };

export type TransferSnapshot = { revision: number, deviceId: string, deviceName: string, receiveDirectory: string, discoverable: boolean, peers: Array<NearbyPeer>, transfers: Array<TransferView>, };

export type TransferStatus = "preparing" | "awaitingApproval" | "connecting" | "transferring" | "paused" | "completed" | "rejected" | "cancelled" | "failed";

export type TransferView = { id: string, wireId: number, peerId: string, peerName: string, direction: TransferDirection, status: TransferStatus, files: Array<TransferFileView>, totalBytes: number, completedBytes: number, speedBytesPerSecond: number, remainingSeconds: number | null, errorMessage: string | null, createdAtMs: number, updatedAtMs: number, };

export type UpdateChannel = "stable" | "test";

export type WebAccessMode = "disabled" | "public" | "password";

export type WebGatewayBindMode = "lanOnly";

export type WebGatewaySettings = { enabled: boolean, port: number, bindMode: WebGatewayBindMode, siteName: string, sessionIdleMinutes: number, allowVpnPrivate: boolean, };

export type WebGatewaySettingsPatch = { enabled?: boolean, port?: number, bindMode?: WebGatewayBindMode, siteName?: string, sessionIdleMinutes?: number, allowVpnPrivate?: boolean, };

export type WebGatewayStatus = { enabled: boolean, running: boolean, port: number, siteName: string, addresses: Array<string>, sessionCount: number, lastError: string | null, };

export type WebSharePolicyUpdate = { mode: WebAccessMode, listed: boolean, allowPreview: boolean, allowDownload: boolean, };

export type WebSharePolicyView = { mode: WebAccessMode, listed: boolean, allowPreview: boolean, allowDownload: boolean, slug: string, hasPassword: boolean, credentialRevision: number, };

export type WorkspaceConfiguration = { version: number, inputSharingEnabled: boolean, topologyAuthor: string, layout: WorkspaceLayout | null, keyboardProfiles: Array<KeyboardProfile>, rememberedDisplays: { [key in DisplayId]: DisplaySurface }, excludedDisplays: Array<DisplayId>, activeKeyboardProfile: KeyboardProfileKind, rightOptionRawMode: boolean, horizontalScrollBehavior: HorizontalScrollBehavior, consumerShortcuts: Array<ConsumerShortcut>, };

export type WorkspaceId = string;

export type WorkspaceLayout = { workspaceId: WorkspaceId, revision: TopologyRevision, displays: { [key in DisplayId]: DisplaySurface }, portals: Array<Portal>, };

export interface CommandMap {
  add_remote_file_share: { args: { }; result: LocalSharedDirectory | null };
  add_system_folder: { args: { peerId: string; shareId: string; }; result: SystemFolder };
  arrange_input_workspace: { args: { configuration: WorkspaceConfiguration; }; result: WorkspaceConfiguration };
  automation_capabilities: { args: { }; result: Array<Capability> };
  cancel_automation_activity: { args: { activityId: string; }; result: null };
  cancel_transfer: { args: { transferId: string; }; result: null };
  check_automation: { args: { definition: AutomationDefinition; }; result: Array<AutomationIssue> };
  check_for_app_update: { args: { }; result: AppUpdateCheckResult };
  choose_transfer_receive_directory: { args: { }; result: TransferSnapshot };
  clear_automation_activities: { args: { }; result: null };
  clear_gesture_debug: { args: { }; result: GestureDebugSnapshot };
  clipboard_clear_history: { args: { }; result: null };
  clipboard_copy_record: { args: { id: number; }; result: null };
  clipboard_copy_text: { args: { content: string; }; result: null };
  clipboard_create_label: { args: { color: string; name: string; }; result: ClipboardLabel };
  clipboard_delete_label: { args: { labelId: string; }; result: null };
  clipboard_delete_record: { args: { id: number; }; result: null };
  clipboard_edit_text: { args: { content: string; id: number; }; result: null };
  clipboard_history: { args: { cursor: ClipboardCursorInput | null; favoriteOnly: boolean; kind: ClipboardContentKind | null; labelIds: Array<string> | null; limit: number | null; search: string | null; }; result: ClipboardHistoryView };
  clipboard_html_preview: { args: { id: number; }; result: string | null };
  clipboard_image_ocr: { args: { id: number; }; result: ClipboardImageOcr | null };
  clipboard_image_preview: { args: { id: number; }; result: string | null };
  clipboard_join_segments: { args: { id: number; ids: Array<string>; version: string; }; result: string };
  clipboard_labels: { args: { }; result: Array<ClipboardLabel> };
  clipboard_merge_devices: { args: { }; result: number };
  clipboard_paste_record: { args: { id: number; }; result: null };
  clipboard_paste_record_as: { args: { id: number; mode: ClipboardPasteMode; }; result: null };
  clipboard_paste_records: { args: { ids: Array<number>; }; result: number };
  clipboard_paste_text: { args: { content: string; }; result: null };
  clipboard_send_files: { args: { id: number; peerId: string; }; result: string };
  clipboard_set_favorite: { args: { favorite: boolean; id: number; }; result: null };
  clipboard_set_label_membership: { args: { attached: boolean; id: number; labelId: string; }; result: null };
  clipboard_set_labels: { args: { id: number; labelIds: Array<string>; }; result: null };
  clipboard_start_continuous_paste: { args: { ids: Array<number>; }; result: ContinuousPasteProgress };
  clipboard_stop_continuous_paste: { args: { }; result: ContinuousPasteProgress };
  clipboard_text_content: { args: { id: number; }; result: string };
  clipboard_text_segments: { args: { id: number; }; result: TextSliceModel };
  clipboard_thumbnail: { args: { id: number; }; result: string | null };
  clipboard_timeline: { args: { limit: number | null; position: ClipboardTimelinePositionInput; sortBy: ClipboardSortPreference; }; result: ClipboardTimelineView | null };
  clipboard_update_label: { args: { color: string; labelId: string; name: string; }; result: null };
  close_input_permission_guide: { args: { }; result: null };
  confirm_automation_activity: { args: { activityId: string; }; result: null };
  connect_desktop_address: { args: { host: string; port: number | null; }; result: string };
  connect_desktop_device: { args: { deviceId: string; }; result: string };
  connect_input_peer: { args: { addresses: Array<string>; port: number; serviceInstanceId: string; }; result: RuntimeSnapshot };
  create_mcp_client: { args: { name: string; permissions: McpPermissions; }; result: McpClientView };
  create_remote_directory: { args: { name: string; peerId: string; relativePath: string; shareId: string; }; result: RemoteFileEntry };
  create_test_notification: { args: { }; result: Array<NotificationView> };
  delete_action: { args: { actionId: string; }; result: BootstrapState };
  delete_automation: { args: { automationId: string; }; result: null };
  delete_notification: { args: { notificationId: string; }; result: Array<NotificationView> };
  delete_remote_entries: { args: { paths: Array<string>; peerId: string; shareId: string; }; result: Array<RemoteDeleteResult> };
  delete_remote_entry: { args: { peerId: string; relativePath: string; shareId: string; }; result: null };
  discard_system_share_request: { args: { requestId: string; }; result: null };
  disconnect_device: { args: { deviceId: string; }; result: null };
  discover_desktop_devices: { args: { }; result: Array<NearbyDesktopView> };
  download_remote_entries: { args: { peerId: string; relativePaths: Array<string>; shareId: string; }; result: Array<string> | null };
  download_remote_entry: { args: { peerId: string; relativePath: string; shareId: string; }; result: string | null };
  execute_action: { args: { actionId: string; }; result: string };
  export_actions_text: { args: { }; result: string };
  export_diagnostic_bundle: { args: { }; result: DiagnosticBundleInfo };
  forget_input_peer: { args: { serviceInstanceId: string; }; result: RuntimeSnapshot };
  forget_paired_device: { args: { deviceId: string; }; result: null };
  frontend_log: { args: { entry: FrontendLogEntry; }; result: null };
  get_action_output: { args: { actionId: string; }; result: ActionOutputSnapshot };
  get_action_presets: { args: { }; result: Array<ActionPresetView> };
  get_app_settings: { args: { }; result: AppSettings };
  get_application_bundle_path: { args: { }; result: string };
  get_application_drag_icon_path: { args: { }; result: string };
  get_bootstrap_state: { args: { }; result: BootstrapState };
  get_clipboard_window_pinned: { args: { }; result: boolean };
  get_gesture_debug_report: { args: { }; result: GestureDebugReport };
  get_gesture_debug_snapshot: { args: { }; result: GestureDebugSnapshot };
  get_input_runtime_snapshot: { args: { }; result: RuntimeSnapshot };
  get_installed_app_icon: { args: { path: string; }; result: string | null };
  get_local_file_shares: { args: { }; result: Array<LocalSharedDirectory> };
  get_log_status: { args: { }; result: LogStatus };
  get_mcp_client_config: { args: { clientId: string; }; result: McpClientConfig };
  get_mcp_config: { args: { }; result: McpConfigView };
  get_print_job_activity: { args: { }; result: PrintJobActivitySnapshot };
  get_printer_sharing_state: { args: { }; result: PrinterSharingSnapshot };
  get_remote_file_state: { args: { }; result: RemoteFileState };
  get_remote_file_thumbnail: { args: { modifiedAtMs: number; peerId: string; relativePath: string; shareId: string; }; result: string | null };
  get_runtime_modules: { args: { }; result: Array<ModuleStatus> };
  get_sound_mute_until: { args: { }; result: number | null };
  get_transfer_state: { args: { }; result: TransferSnapshot };
  get_web_gateway_status: { args: { }; result: WebGatewayStatus };
  hide_clipboard_window: { args: { }; result: null };
  hide_tray_transfer_panel: { args: { }; result: null };
  import_actions_text: { args: { text: string; }; result: ImportActionsResponse };
  inspect_transfer_files: { args: { paths: Array<string>; }; result: Array<TransferDraftFile> };
  install_action_preset: { args: { presetId: string; }; result: BootstrapState };
  install_app_update: { args: { }; result: null };
  install_remote_printer: { args: { shareId: string; sourceDeviceId: string; }; result: PrinterSharingSnapshot };
  list_automation_activities: { args: { automationId: string | null; limit: number; }; result: Array<AutomationActivity> };
  list_automations: { args: { }; result: Array<AutomationDefinition> };
  list_installed_apps: { args: { refresh: boolean; }; result: Array<InstalledAppView> };
  list_mcp_clients: { args: { }; result: Array<McpClientView> };
  list_mcp_configuration_changes: { args: { limit: number; }; result: Array<ConfigurationChange> };
  list_notifications: { args: { includeRead: boolean; limit: number; }; result: Array<NotificationView> };
  list_remote_directory: { args: { cursor: string | null; peerId: string; relativePath: string; search: string | null; shareId: string; sortDirection: RemoteFileSortDirection; sortKey: RemoteFileSortKey; }; result: RemoteFileDirectoryPage };
  list_remote_file_shares: { args: { peerId: string; }; result: Array<RemoteFileShare> };
  list_remote_file_transfers: { args: { }; result: Array<RemoteFileTransferSession> };
  list_system_folders: { args: { }; result: Array<SystemFolder> };
  list_system_share_requests: { args: { }; result: Array<SystemShareRequest> };
  mark_notification_read: { args: { notificationId: string; }; result: Array<NotificationView> };
  observe_print_jobs: { args: { enabled: boolean; }; result: null };
  open_accessibility_system_settings: { args: { }; result: null };
  open_automation_screen_permission: { args: { }; result: null };
  open_full_disk_access_settings: { args: { }; result: null };
  open_input_permission_settings: { args: { }; result: null };
  open_log_directory: { args: { }; result: null };
  open_remote_entry: { args: { peerId: string; relativePath: string; shareId: string; }; result: RemoteFileOpenResult };
  open_sniptra_settings: { args: { }; result: string };
  open_system_folder: { args: { id: string; }; result: null };
  open_system_folder_recovery: { args: { }; result: null };
  open_transfer_receive_directory: { args: { }; result: null };
  open_tray_transfer_history: { args: { }; result: null };
  open_web_gateway_url: { args: { url: string; }; result: null };
  pause_transfer: { args: { transferId: string; }; result: null };
  permission_guide_ready: { args: { }; result: null };
  pick_automation_path: { args: { directory: boolean; }; result: string | null };
  pick_remote_upload: { args: { folder: boolean; peerId: string; relativePath: string; shareId: string; }; result: number };
  pick_transfer_files: { args: { }; result: Array<string> };
  prepare_remote_drag: { args: { peerId: string; relativePath: string; shareId: string; }; result: RemoteFileDragPreparation };
  preview_automation_schedule: { args: { trigger: AutomationTrigger; }; result: string | null };
  preview_input_workspace: { args: { configuration: WorkspaceConfiguration; }; result: WorkspaceConfiguration };
  preview_sound: { args: { event: SoundEvent; }; result: null };
  publish_printer: { args: { localPrinterId: string; }; result: PrinterSharingSnapshot };
  quit_app: { args: { }; result: null };
  refresh_input_permission: { args: { }; result: InputPermissionState };
  refresh_printer_sharing_state: { args: { }; result: PrinterSharingSnapshot };
  refresh_transfer_devices: { args: { }; result: TransferSnapshot };
  release_input_control: { args: { }; result: RuntimeSnapshot };
  remove_remote_file_share: { args: { shareId: string; }; result: Array<LocalSharedDirectory> };
  remove_remote_printer: { args: { bindingId: string; }; result: PrinterSharingSnapshot };
  remove_system_folder: { args: { id: string; }; result: null };
  rename_remote_entry: { args: { newName: string; peerId: string; relativePath: string; shareId: string; }; result: RemoteFileEntry };
  reset_sound_preferences: { args: { }; result: AppSettings };
  respond_pairing: { args: { accepted: boolean; approvedGrantIds: Array<string> | null; }; result: null };
  respond_transfer: { args: { accepted: boolean; automaticReceive: boolean; transferId: string; }; result: null };
  resume_transfer: { args: { transferId: string; }; result: null };
  revoke_mcp_client: { args: { clientId: string; }; result: null };
  revoke_web_sessions: { args: { shareId: string | null; }; result: number };
  rotate_mcp_client_token: { args: { clientId: string; }; result: McpClientView };
  run_automation: { args: { automationId: string; }; result: string };
  save_action: { args: { action: QuickAction; }; result: BootstrapState };
  save_automation: { args: { definition: AutomationDefinition; }; result: AutomationDefinition };
  save_input_workspace: { args: { configuration: WorkspaceConfiguration; }; result: RuntimeSnapshot };
  send_system_share_transfer: { args: { paths: Array<string>; peerId: string; requestIds: Array<string>; }; result: string };
  send_transfer: { args: { paths: Array<string>; peerId: string; }; result: string };
  set_automation_enabled: { args: { automationId: string; enabled: boolean; }; result: null };
  set_clipboard_context_menu_open: { args: { open: boolean; }; result: null };
  set_clipboard_window_pinned: { args: { pinned: boolean; }; result: null };
  set_detailed_logging: { args: { enabled: boolean; }; result: LogStatus };
  set_device_auto_connect: { args: { deviceId: string; enabled: boolean; }; result: null };
  set_input_sharing_enabled: { args: { enabled: boolean; }; result: RuntimeSnapshot };
  set_mcp_client_permissions: { args: { clientId: string; permissions: McpPermissions; }; result: null };
  set_privacy_enabled: { args: { enabled: boolean; }; result: PrivacySnapshot };
  set_remote_file_share_web_policy: { args: { newPassword: string | null; policy: WebSharePolicyUpdate; shareId: string; }; result: LocalSharedDirectory };
  set_remote_file_share_writable: { args: { shareId: string; writable: boolean; }; result: LocalSharedDirectory };
  set_sound_temporary_mute: { args: { muted: boolean; }; result: number | null };
  set_transfer_receive_policy: { args: { automatic: boolean; peerId: string; }; result: TransferSnapshot };
  show_test_system_notification: { args: { }; result: string };
  start_clipboard_window_drag: { args: { }; result: null };
  start_gesture_debug_capture: { args: { label: string; suppress: boolean; }; result: GestureDebugSnapshot };
  start_permission_guide_window_drag: { args: { }; result: null };
  start_remote_download_entries: { args: { peerId: string; relativePaths: Array<string>; shareId: string; }; result: Array<RemoteFileTransferSession> | null };
  start_remote_file_promise_drag: { args: { kind: RemoteFileKind; peerId: string; relativePath: string; shareId: string; size: number; }; result: null };
  start_remote_upload: { args: { folder: boolean; peerId: string; relativePath: string; shareId: string; }; result: RemoteFileTransferSession | null };
  start_remote_upload_paths: { args: { paths: Array<string>; peerId: string; relativePath: string; shareId: string; }; result: RemoteFileTransferSession };
  start_screenshot_capture: { args: { }; result: string };
  stop_gesture_debug: { args: { }; result: GestureDebugSnapshot };
  stop_remote_edit: { args: { peerId: string; relativePath: string; shareId: string; }; result: null };
  submit_system_share_request: { args: { peerId: string; requestId: string; transferId: string; }; result: null };
  suspend_printer_share: { args: { shareId: string; }; result: PrinterSharingSnapshot };
  take_input_control: { args: { }; result: RuntimeSnapshot };
  take_pending_transfer_request: { args: { }; result: string | null };
  take_pending_tray_navigation: { args: { }; result: string | null };
  take_tray_transfer_drop: { args: { }; result: DropSnapshot };
  test_automation: { args: { definition: AutomationDefinition; event: AutomationEvent | null; }; result: string };
  test_input_edge: { args: { configuration: WorkspaceConfiguration | null; request: EdgeTestRequest; }; result: EdgeTestResult };
  trigger_gesture_debug: { args: { request: GestureDebugTrigger; }; result: GestureDebugSnapshot };
  update_app_settings: { args: { patch: AppSettingsPatch; }; result: AppSettings };
  update_privacy_settings: { args: { settings: PrivacySettings; }; result: PrivacySnapshot };
  update_tray_transfer_panel: { args: { hasContent: boolean; height: number; }; result: null };
  upload_remote_paths: { args: { paths: Array<string>; peerId: string; relativePath: string; shareId: string; }; result: number };
  validate_action_shortcut: { args: { actionId: string; shortcut: string; }; result: null };
  web_gateway_qr_code: { args: { url: string; }; result: string };
}

export interface EventMap {
  "action-output": ActionOutputSnapshot;
  "app-settings-changed": AppSettings;
  "app-update-checked": AppUpdateCheckResult;
  "app-update-progress": AppUpdateProgress;
  "arc-input-state": RuntimeSnapshot;
  "automation-activity": AutomationActivity;
  "automation-configuration": null;
  "clipboard-changed": null;
  "clipboard-continuous-paste-error": string;
  "clipboard-continuous-paste-progress": ContinuousPasteProgress;
  "clipboard-ocr-changed": number;
  "clipboard-window-hidden": null;
  "clipboard-window-pin-changed": boolean;
  "clipboard-window-shown": null;
  "desktop-runtime": DesktopRuntimeState;
  "desktop-state": BootstrapState;
  "input-metrics": InputMetricsView | null;
  "notification-count": number;
  "notifications-changed": null;
  "open-system-share-request": SystemShareRequest;
  "open-transfer-request": string;
  "print-job-activity": PrintJobActivitySnapshot;
  "print-job-activity-error": IpcError;
  "privacy-state": PrivacySnapshot;
  "runtime-modules": Array<ModuleStatus>;
  "sound-mute-changed": number | null;
  "transfer-progress": TransferProgress;
  "transfer-state": TransferSnapshot;
  "tray-navigation-pending": null;
  "tray-transfer-drop-changed": null;
  "web-gateway-status": WebGatewayStatus;
}
