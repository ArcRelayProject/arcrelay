export type ActionType = import('./ipc/generated').ActionType;

export type QuickAction = import('./ipc/generated').QuickAction;

export type ActionView = import('./ipc/generated').ActionView;

export type ActionPreset = import('./ipc/generated').ActionPresetView;

export interface ActionIconOption {
  id: string;
  label: string;
  svg: string;
}

export type InstalledApp = import('./ipc/generated').InstalledAppView;

export type PrinterStatus = import('./ipc/generated').PrinterStatus;

export type LocalPrinter = import('./ipc/generated').LocalPrinter;

export type PrinterShare = import('./ipc/generated').PrinterShare;

export type PrinterSharingSnapshot = import('./ipc/generated').PrinterSharingSnapshot;

export type PrintJobState = import('./ipc/generated').PrintJobState;

export type ReceivedPrintJob = import('./ipc/generated').ReceivedPrintJobView;

export type SentPrintJob = import('./ipc/generated').OutgoingPrintJob;

export type PrintJobActivitySnapshot = import('./ipc/generated').PrintJobActivitySnapshot;

export type RemoteQueueBinding = import('./ipc/generated').RemoteQueueBinding;

export type PublishedPrinter = import('./ipc/generated').PublishedPrinter;

export type RemotePrinter = import('./ipc/generated').RemotePrinter;

export type AppLanguage = import('./ipc/generated').LanguagePreference;

export type PrivacyMaskStyle = import('./ipc/generated').MaskStyle;

export type ProtectedApp = import('./ipc/generated').ProtectedApp;

export type PrivacySettings = import('./ipc/generated').PrivacySettings;

export type ProtectedWindowView = import('./ipc/generated').ProtectedWindowView;

export type PrivacySnapshot = import('./ipc/generated').PrivacySnapshot;

export type ImportActionsResponse = import('./ipc/generated').ImportActionsResponse;

export type ConnectedDevice = import('./ipc/generated').ConnectedDeviceView;

export type InputMetrics = import('./ipc/generated').InputMetricsView;

export type PendingPairing = import('./ipc/generated').PendingPairingView;

export type OutgoingPairing = import('./ipc/generated').OutgoingPairingView;

export type NearbyDesktop = import('./ipc/generated').NearbyDesktopView;

export type BootstrapState = import('./ipc/generated').BootstrapState;

export type ThemePreference = import('./ipc/generated').ThemePreference;
export type LanguagePreference = import('./ipc/generated').LanguagePreference;
export type ClipboardSortPreference = import('./ipc/generated').ClipboardSortPreference;
export type ScreenshotFormat = import('./ipc/generated').ScreenshotFormat;
export type WebAccessMode = import('./ipc/generated').WebAccessMode;

export type WebGatewaySettings = import('./ipc/generated').WebGatewaySettings;

export type WebGatewayStatus = import('./ipc/generated').WebGatewayStatus;

export type NotificationPreferences = import('./ipc/generated').NotificationPreferences;

export type AppSettings = import('./ipc/generated').AppSettings;

export type AppUpdateMetadata = import('./ipc/generated').AppUpdateMetadata;

export type AppUpdateCheckResult = import('./ipc/generated').AppUpdateCheckResult;

export type AppUpdateProgressPhase = import('./ipc/generated').AppUpdateProgressPhase;
export type AppUpdateProgress = import('./ipc/generated').AppUpdateProgress;

export type NotificationKind = import('./ipc/generated').NotificationKind;
export type NotificationDeliveryState = "unread" | "read" | "waitingForDevice";

export type NotificationView = import('./ipc/generated').NotificationView;

export type McpConfig = import('./ipc/generated').McpConfigView;

export type ActionOutputEvent = import('./ipc/generated').ActionOutputSnapshot;

export type TransferDirection = import('./ipc/generated').TransferDirection;
export type TransferStatus = import('./ipc/generated').TransferStatus;
export type TransferFileKind = import('./ipc/generated').FileKind;

export type TransferPeer = import('./ipc/generated').NearbyPeer;

export type TransferFileView = import('./ipc/generated').TransferFileView;
export type TransferDraftFile = import('./ipc/generated').TransferDraftFile;

export type TransferView = import('./ipc/generated').TransferView;

export type TransferSnapshot = import('./ipc/generated').TransferSnapshot;
export type SystemShareSource = import('./ipc/generated').SystemShareSource;
export type SystemShareItem = import('./ipc/generated').SystemShareItem;
export type SystemShareRequest = import('./ipc/generated').SystemShareRequest;

export type RemoteFileDevice = import('./ipc/generated').RemoteFileDeviceView;

export type RemoteFileShare = import('./ipc/generated').RemoteFileShare;

export type LocalSharedDirectory = import('./ipc/generated').LocalSharedDirectory;

export type RemoteFileKind = import('./ipc/generated').RemoteFileKind;

export type RemoteFileEntry = import('./ipc/generated').RemoteFileEntry;

export type RemoteFileDirectoryPage = import('./ipc/generated').RemoteFileDirectoryPage;

export type RemoteFileState = import('./ipc/generated').RemoteFileState;

export type RemoteFileOpenResult = import('./ipc/generated').RemoteFileOpenResult;

export type RemoteFileTransferDirection = "upload" | "download";
export type RemoteFileTransferStatus = "transferring" | "completed" | "failed";

export type RemoteFileTransferSession = import('./ipc/generated').RemoteFileTransferSession;

export type RemoteFileDragPreparation = import('./ipc/generated').RemoteFileDragPreparation;

export type InputPlatformCapabilities = import('./ipc/generated').PlatformCapabilities;

export type DeskRect = import('./ipc/generated').DeskRectUm;

export type DisplaySurface = import('./ipc/generated').DisplaySurface;

export type Edge = import('./ipc/generated').Edge;

export type Portal = import('./ipc/generated').Portal;

export type WorkspaceLayout = import('./ipc/generated').WorkspaceLayout;

export type KeyboardMappingRule = import('./ipc/generated').KeyboardMappingRule;

export type KeyboardProfile = import('./ipc/generated').KeyboardProfile;

export type WorkspaceConfiguration = import('./ipc/generated').WorkspaceConfiguration;

export type InputOsFamily = import('./ipc/generated').OsFamily;

export type DiagnosticRecord = import('./ipc/generated').DiagnosticRecord;

export type LogStatus = import('./ipc/generated').LogStatus;

export type DiagnosticBundleInfo = import('./ipc/generated').DiagnosticBundleInfo;

export type NearbyInputPeer = import('./ipc/generated').NearbyPeerView;

export type InputRuntimeSnapshot = import('./ipc/generated').RuntimeSnapshot;

export type EdgeTestResult = import('./ipc/generated').EdgeTestResult;

// Short aliases retained inside the input-sharing feature tree.
export type PlatformCapabilities = import('./ipc/generated').PlatformCapabilities;
export type NearbyPeer = NearbyInputPeer;
export type RuntimeSnapshot = import('./ipc/generated').RuntimeSnapshot;

export type TransferProgress = import('./ipc/generated').TransferProgress;
