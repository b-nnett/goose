import Foundation
import Observation
import UIKit


@MainActor @Observable
final class GooseAppModel {
  var onboardingComplete = false
  var rustStatus = "Rust bridge not checked"
  var helloSummary = "Client hello not prepared"
  var packetImportRevision = 0
  var packetImportStatus = "No packet import"
  var activityPersistenceStatus = "No activity stored"
  var homeActivityTimelineItems: [ActivityTimelineItem] = []
  var homeActivityTimelineStatus = "Activity timeline not loaded"
  var activityDetectionStatus = "Watching for movement packets"
  var movementPacketValidationStatus = "Not run"
  var movementPacketValidationIsRunning = false
  var heartRateHourlyRanges: [HeartRateHourlyRange] = []
  var heartRateStorageStatus = "No HR samples stored"
  var healthPacketCaptureSessionID: String?
  var healthPacketCaptureStatus = "No health packet capture"
  var healthPacketCaptureStartedAt: Date?
  var healthPacketCaptureFrameCount = 0
  var healthPacketCaptureTargetSummary = "No health packet capture"
  var healthPacketCaptureLastPacketSummary = "No packets captured"
  var healthPacketCaptureFamilyRows: [HealthPacketCaptureFamily] = []
  var respiratoryPacketWatchActive = false
  var respiratoryPacketWatchStatus = "Not watching K18 respiratory history"
  var overnightGuardActive = false
  var overnightGuardStatus = "Not started"
  var overnightGuardReadinessStatus = "pending"
  var overnightGuardReadinessSummary = "Not sleep-ready | connect WHOOP and start Overnight Guard"
  var overnightGuardRawNotificationCount = 0
  var overnightGuardRangePollCount = 0
  var overnightGuardRangeTelemetryCount = 0
  var overnightGuardSuccessfulRangePollCount = 0
  var overnightGuardCommandWriteCount = 0
  var overnightGuardEventLogCount = 0
  var overnightGuardTargetSummary = OvernightGuardTargetCounts().summary
  var overnightGuardHistoricalOrderSummary = OvernightGuardHistoricalOrderEvidence().summary
  var overnightGuardLastPacketSummary = "No raw notifications"
  var overnightGuardSpoolPath = "No overnight spool"
  var overnightGuardSpoolSizeSummary = "No overnight spool size"
  var overnightGuardSQLiteMirrorSummary = "SQLite mirror not started"
  var overnightGuardPowerSummary = "Power not checked"
  var overnightGuardWatchdogSummary = "Watchdog not checked"
  var overnightGuardWarning = "Keep the official WHOOP app closed until Goose final sync/export finishes."
  var overnightGuardExportStatus = "No overnight export"
  var overnightGuardExportInProgress = false
  var overnightGuardExportURL: URL?
  var overnightGuardExportManifestURL: URL?
  var overnightGuardExportManifestError: String?
  var overnightGuardCanExportLastSession = false
  var serverReachable: Bool? = nil
  var lastUploadAt: Date? = nil
  var pendingBatchCount: Int = 0
  var lastSyncedCount: Int? = nil
  var connectedDeviceGeneration: String? = nil

  let ble: GooseBLEClient
  let packetMonitor = PacketMonitorModel()
  let activitySession = ActivitySessionModel()
  let activityLocationTracker = ActivityLocationTracker()
  let rust = GooseRustBridge()
  let notificationFrameParser = NotificationFrameParser()
  let notificationIngestQueue = DispatchQueue(label: "com.goose.swift.notification-ingest", qos: .utility)
  let notificationIngestStateLock = NSLock()
  let notificationParseQueue = DispatchQueue(label: "com.goose.swift.notification-parse", qos: .utility)
  let notificationParseStateLock = NSLock()
  let captureFrameRowBuildQueue = DispatchQueue(label: "com.goose.swift.capture-frame-row-build", qos: .utility)
  let rustStartupQueue = DispatchQueue(label: "com.goose.swift.rust-startup", qos: .utility)
  let activityTimelineRefreshQueue = DispatchQueue(label: "com.goose.swift.activity-timeline-refresh", qos: .utility)
  let captureStatusSnapshotWriteQueue = DispatchQueue(label: "com.goose.swift.capture-status-snapshot", qos: .utility)
  let heartRateSamplePipeline = HeartRateSamplePipeline(
    timelinePublishInterval: GooseAppModel.heartRateHourlyRangePublishInterval
  )
  let packetUIStateAggregator = PacketUIStateAggregator(
    publishInterval: GooseAppModel.packetUIStatePublishInterval,
    maximumPendingDeviceSignalPoints: GooseAppModel.maxRecentDeviceSignalPoints
  )
  let whoopDataSignalPipeline: WhoopDataSignalPipeline
  let healthPacketCaptureFamilyAggregator = HealthPacketCaptureFamilyAggregator(
    publishInterval: GooseAppModel.healthPacketCaptureUIUpdateInterval
  )
  let captureFrameWriteQueue = CaptureFrameWriteQueue(
    databasePath: HealthDataStore.defaultDatabasePath(),
    maxQueuedRows: GooseAppModel.captureFrameWriteQueueMaxRows,
    maxBatchRows: GooseAppModel.captureFrameWriteBatchMaxRows
  )
  let uploadService = GooseUploadService(databasePath: HealthDataStore.defaultDatabasePath())
  let captureFrameEnqueueAggregator = CaptureFrameEnqueueAggregator(
    publishInterval: GooseAppModel.packetUIStatePublishInterval
  )
  let overnightSQLiteMirror = OvernightSQLiteMirrorQueue(databasePath: HealthDataStore.defaultDatabasePath())
  let passiveActivityDetectionPipeline = PassiveActivityDetectionPipeline()
  var activeActivityPersistence: ActiveActivityPersistence?
  var activeActivityOwnsCaptureSession = false
  var activityRequestedHighFrequencyHistorySync = false
  var activeHealthPacketCapture: ActiveHealthPacketCapture?
  let overnightRawSpool = OvernightRawNotificationSpool()
  var overnightGuardSession: OvernightGuardSession?
  var overnightGuardHeartbeatWorkItem: DispatchWorkItem?
  var overnightGuardRangePollWorkItem: DispatchWorkItem?
  var overnightGuardFinalSyncDrainWorkItem: DispatchWorkItem?
  var overnightGuardFinalSyncPending = false
  var overnightGuardCriticalBackgroundTaskID: UIBackgroundTaskIdentifier = .invalid
  var overnightGuardCriticalBackgroundTaskReason: String?
  var overnightGuardStartedHealthCapture = false
  var overnightGuardTargetCounts = OvernightGuardTargetCounts()
  var overnightGuardHistoricalOrder = OvernightGuardHistoricalOrderEvidence()
  var overnightGuardPowerWarning: String?
  var overnightGuardWatchdogWarning: String?
  var overnightGuardRawSpoolWarning: String?
  var overnightGuardBLELogWarning: String?
  var overnightGuardSQLiteMirrorWarning: String?
  var overnightGuardWroteInitialRawNotificationStatus = false
  var overnightGuardWroteInitialSQLiteMirrorStatus = false
  var overnightGuardLastRawStaleWarningAt = Date.distantPast
  var overnightGuardLastRangeSuccessWarningAt = Date.distantPast
  var overnightGuardLastTargetMissingWarningAt = Date.distantPast
  var activityDetectionIdleWorkItem: DispatchWorkItem?
  var movementPacketValidation = MovementPacketValidation()
  var movementPacketValidationTimeoutWorkItem: DispatchWorkItem?
  var packetImportRevisionWorkItem: DispatchWorkItem?
  var healthPacketCaptureTimeoutWorkItem: DispatchWorkItem?
  var healthPacketCaptureStreamRetryWorkItem: DispatchWorkItem?
  var healthPacketCaptureUIUpdateWorkItem: DispatchWorkItem?
  var respiratoryPacketWatchTimeoutWorkItem: DispatchWorkItem?
  var autoStartRespiratoryPacketWatchWorkItem: DispatchWorkItem?
  var temperatureHistorySyncWorkItem: DispatchWorkItem?
  var healthPacketCaptureStreamRetryAttempt = 0
  var autoStartHealthPacketCaptureWorkItem: DispatchWorkItem?
  var autoStartHealthPacketCaptureAttempt = 0
  var autoStartRespiratoryPacketWatchAttempt = 0
  var passiveActivityCaptureWorkItem: DispatchWorkItem?
  var healthPacketCaptureFamilyRowsByID: [String: HealthPacketCaptureFamily] = [:]
  var lastParsedFrameSummary: String { packetMonitor.lastParsedFrameSummary }
  var movementPacketStatus: String { packetMonitor.movementPacketStatus }
  var latestWhoopEventStatus: String { packetMonitor.latestWhoopEventStatus }
  var latestSkinTemperatureCandidateStatus: String { packetMonitor.latestSkinTemperatureCandidateStatus }
  var latestWhoopDataPacketStatus: String { packetMonitor.latestWhoopDataPacketStatus }
  var latestHistoryTemperatureCandidateStatus: String { packetMonitor.latestHistoryTemperatureCandidateStatus }
  var latestRespiratoryRateCandidateStatus: String { packetMonitor.latestRespiratoryRateCandidateStatus }
  var latestPulseInformationPacketStatus: String { packetMonitor.latestPulseInformationPacketStatus }
  var latestOpticalPacketStatus: String { packetMonitor.latestOpticalPacketStatus }
  var latestRawResearchPacketStatus: String { packetMonitor.latestRawResearchPacketStatus }
  var latestRealtimeStatusPacketStatus: String { packetMonitor.latestRealtimeStatusPacketStatus }
  var performancePipelineStatus: String { packetMonitor.performancePipelineStatus }
  var liveDeviceDataSummary: String { packetMonitor.liveDeviceDataSummary }
  var recentDeviceSignalPoints: [DeviceSignalPoint] { packetMonitor.recentDeviceSignalPoints }
  var pendingHealthPacketCaptureLastPacketSummary: String?
  var pendingPacketImportStatus: String?
  var lastPacketImportRevisionPublishedAt = Date.distantPast
  var lastHealthPacketCaptureUIUpdatedAt = Date.distantPast
  var lastHealthPacketCaptureSummaryLoggedAt = Date.distantPast
  var lastParsedFrameSummaryUpdatedAt = Date.distantPast
  var lastRestingHeartRateFrameWriteAt = Date.distantPast
  var lastMovementPacketStatusUpdatedAt = Date.distantPast
  var lastMovementPacketLoggedAt = Date.distantPast
  var lastMovementPacketLoggedMoving: Bool?
  var passiveActivityPacketCount = 0
  var movementPacketLogCount = 0
  var deviceSignalCountsByFamily: [String: Int] = [:]
  var notificationIngestQueueDepth = 0
  var notificationIngestQueueHighWatermark = 0
  var notificationParseQueueDepth = 0
  var notificationParseQueueHighWatermark = 0
  let captureFrameRowBuildStateLock = NSLock()
  @ObservationIgnored nonisolated(unsafe) var captureFrameRowBuildQueueDepth = 0
  @ObservationIgnored nonisolated(unsafe) var captureFrameRowBuildQueueHighWatermark = 0
  let pipelinePerformanceLogLock = NSLock()
  var lastPipelinePerformanceLoggedAt = Date.distantPast
  var respiratoryPacketWatchK18Count = 0
  var respiratoryPacketWatchK24Count = 0
  var respiratoryPacketWatchStartedAt: Date?
  var lastWhoopEventLoggedAt = Date.distantPast
  var lastWhoopEventStatusUpdatedAt = Date.distantPast
  var activityTimelineRefreshGeneration = 0
  var skippedNotificationDiagnostics = SkippedNotificationDiagnostics()
  @ObservationIgnored nonisolated(unsafe) var frameReassemblyBuffers: [String: Data] = [:]
  var lastNotificationEvent: GooseNotificationEvent?
  let autoStartHealthPacketCaptureOnReady: Bool = {
    let processInfo = ProcessInfo.processInfo
    return processInfo.arguments.contains("--goose-start-health-packet-capture")
      || processInfo.environment["GOOSE_START_HEALTH_PACKET_CAPTURE"] == "1"
  }()
  let autoStartTemperaturePacketCaptureOnReady: Bool = {
    let processInfo = ProcessInfo.processInfo
    return processInfo.arguments.contains("--goose-start-temperature-packet-capture")
      || processInfo.environment["GOOSE_START_TEMPERATURE_PACKET_CAPTURE"] == "1"
  }()
  let autoStartPhysiologyPacketCaptureOnReady: Bool = {
    let processInfo = ProcessInfo.processInfo
    return processInfo.arguments.contains("--goose-start-physiology-packet-capture")
      || processInfo.environment["GOOSE_START_PHYSIOLOGY_PACKET_CAPTURE"] == "1"
  }()
  let autoStartRespiratoryPacketWatchOnReady: Bool = {
    let processInfo = ProcessInfo.processInfo
    return processInfo.arguments.contains("--goose-start-respiratory-packet-watch")
      || processInfo.environment["GOOSE_START_RESPIRATORY_PACKET_WATCH"] == "1"
  }()
  let autoStartHealthPacketCaptureDuration: TimeInterval = GooseAppModel.durationFromEnvironment(
    envVar: "GOOSE_HEALTH_PACKET_CAPTURE_DURATION_SECONDS",
    cliPrefix: "--goose-health-packet-capture-duration=",
    fallback: 30 * 60
  )
  let autoStartTemperaturePacketCaptureDuration: TimeInterval = GooseAppModel.durationFromEnvironment(
    envVar: "GOOSE_TEMPERATURE_PACKET_CAPTURE_DURATION_SECONDS",
    cliPrefix: "--goose-temperature-packet-capture-duration=",
    fallback: 10 * 60
  )
  let autoStartPhysiologyPacketCaptureDuration: TimeInterval = GooseAppModel.durationFromEnvironment(
    envVar: "GOOSE_PHYSIOLOGY_PACKET_CAPTURE_DURATION_SECONDS",
    cliPrefix: "--goose-physiology-packet-capture-duration=",
    fallback: 30 * 60
  )
  let autoStartRespiratoryPacketWatchDuration: TimeInterval = GooseAppModel.durationFromEnvironment(
    envVar: "GOOSE_RESPIRATORY_PACKET_WATCH_DURATION_SECONDS",
    cliPrefix: "--goose-respiratory-packet-watch-duration=",
    fallback: 10 * 60
  )
  let autoSyncHistoryDuringPhysiologyCapture: Bool = {
    let processInfo = ProcessInfo.processInfo
    return processInfo.arguments.contains("--goose-sync-history-during-physiology-capture")
      || processInfo.environment["GOOSE_SYNC_HISTORY_DURING_PHYSIOLOGY_CAPTURE"] == "1"
  }()
  let captureStatusSnapshotURL: URL? = {
    let processInfo = ProcessInfo.processInfo
    let enabled = processInfo.arguments.contains("--goose-afc-capture-status")
      || processInfo.environment["GOOSE_AFC_CAPTURE_STATUS"] == "1"
    guard enabled else {
      return nil
    }
    guard let directory = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first else {
      return nil
    }
    let gooseDirectory = directory.appendingPathComponent("GooseSwift", isDirectory: true)
    try? FileManager.default.createDirectory(at: gooseDirectory, withIntermediateDirectories: true)
    return gooseDirectory.appendingPathComponent("capture-status.txt")
  }()
  static let captureTimestampFormatter: ISO8601DateFormatter = {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return formatter
  }()
  nonisolated static let maximumBufferedFrameBytes = 64 * 1024
  static let packetImportRevisionInterval: TimeInterval = 5
  static let healthPacketCaptureUIUpdateInterval: TimeInterval = 1
  static let healthPacketCaptureSummaryLogInterval: TimeInterval = 10
  static let parsedFrameSummaryUpdateInterval: TimeInterval = 1
  static let heartRateHourlyRangePublishInterval: TimeInterval = 1
  static let packetUIStatePublishInterval: TimeInterval = 0.2
  static let restingHeartRateFrameWriteInterval: TimeInterval = 0.1
  static let captureFrameWriteQueueMaxRows = 2048
  static let captureFrameWriteBatchMaxRows = 128
  static let passiveActivityCaptureDuration: TimeInterval = 12 * 60 * 60
  static let movementPacketStatusInterval: TimeInterval = 1
  static let movementPacketLogInterval: TimeInterval = 5
  static let whoopDataSignalLogInterval: TimeInterval = 10
  static let pipelinePerformanceLogInterval: TimeInterval = 5
  static let whoopEventStatusInterval: TimeInterval = 1
  static let whoopDataSignalStatusInterval: TimeInterval = 1
  static let whoopDataSignalPipelineMaxSamples = 256
  static let maxRecentDeviceSignalPoints = 32
  static let deviceSignalPointInterval: TimeInterval = 0.75
  static let overnightGuardDuration: TimeInterval = 12 * 60 * 60
  static let overnightGuardHeartbeatInterval: TimeInterval = 60
  static let overnightGuardRangePollInterval: TimeInterval = 15 * 60
  static let overnightGuardRangeBlockedRetryInterval: TimeInterval = 30
  static let overnightGuardRangeFailureRetryInterval: TimeInterval = 2 * 60
  static let overnightGuardFinalSyncDrainInterval: TimeInterval = 8
  static let overnightGuardRawStaleWarningInterval: TimeInterval = 5 * 60
  static let overnightGuardRangeSuccessWarningDelay: TimeInterval = 2 * 60
  static let overnightGuardTargetMissingWarningDelay: TimeInterval = 30 * 60
  static let overnightGuardWarningRepeatInterval: TimeInterval = 15 * 60

  init(startBLE: Bool = true) {
    ble = GooseBLEClient(startCentral: startBLE)
    whoopDataSignalPipeline = WhoopDataSignalPipeline(
      ble: ble,
      packetUIStateAggregator: packetUIStateAggregator,
      statusInterval: Self.whoopDataSignalStatusInterval,
      logInterval: Self.whoopDataSignalLogInterval,
      deviceSignalPointInterval: Self.deviceSignalPointInterval,
      maxQueuedSamples: Self.whoopDataSignalPipelineMaxSamples
    )
    let heartRateSamplePipeline = self.heartRateSamplePipeline
    heartRateSamplePipeline.onHeartRateTimelineSnapshot = { [weak self] snapshot in
      Task { @MainActor in
        self?.applyHeartRateTimelineSnapshot(snapshot)
      }
    }
    packetUIStateAggregator.onSnapshot = { [weak self] snapshot in
      Task { @MainActor in
        self?.applyPacketUIStateSnapshot(snapshot)
      }
    }
    whoopDataSignalPipeline.onStatus = { [weak self] status in
      Task { @MainActor in
        self?.publishPipelinePerformanceStatus(status)
      }
    }
    healthPacketCaptureFamilyAggregator.onSnapshot = { [weak self] snapshot in
      Task { @MainActor in
        self?.applyHealthPacketCaptureFamilySnapshot(snapshot)
      }
    }
    healthPacketCaptureFamilyAggregator.onStatus = { [weak self] status in
      Task { @MainActor in
        self?.publishPipelinePerformanceStatus(status)
      }
    }
    captureFrameEnqueueAggregator.onSnapshot = { [weak self] snapshot in
      Task { @MainActor in
        self?.applyCaptureFrameEnqueueSnapshot(snapshot)
      }
    }
    passiveActivityDetectionPipeline.onEvents = { [weak self] events in
      Task { @MainActor in
        self?.applyActivityDetectionEvents(events)
      }
    }
    passiveActivityDetectionPipeline.onStatus = { [weak self] status in
      Task { @MainActor in
        self?.publishPipelinePerformanceStatus(status)
      }
    }
    ble.onRawNotificationWithContext = { [weak self] event, context in
      self?.persistOvernightRawNotificationBeforeInterpretation(
        event,
        activeDeviceName: context.activeDeviceName,
        connectionState: context.connectionState
      )
    }
    ble.onCommandWrite = { [weak self, weak ble] event in
      self?.persistOvernightCommandWrite(
        event,
        activeDeviceName: ble?.activeDeviceName ?? "WHOOP",
        connectionState: ble?.connectionState ?? "unknown"
      )
    }
    ble.onNotification = { [weak self] event in
      Task { @MainActor [weak self] in
        self?.handleNotification(event)
      }
    }
    ble.onLiveHeartRate = { bpm, source, capturedAt in
      heartRateSamplePipeline.recordHeartRateSample(bpm: bpm, source: source, capturedAt: capturedAt)
    }
    ble.onHRVSample = { rmssdMS, rrIntervalCount, source, capturedAt in
      heartRateSamplePipeline.recordHRVSample(
        rmssdMS: rmssdMS,
        rrIntervalCount: rrIntervalCount,
        source: source,
        capturedAt: capturedAt
      )
    }
    ble.onConnectionStateChange = { [weak self] state in
      Task { @MainActor in
        self?.handleBLEConnectionStateChange(state)
      }
    }
    ble.onHRConnectionStateChange = { [weak self] state in
      Task { @MainActor in
        self?.handleHRConnectionStateChange(state)
      }
    }
    ble.onHistoricalSyncProgress = { [weak self] progress in
      Task { @MainActor in
        self?.handleHistoricalSyncProgress(progress)
      }
    }
    ble.onHistoricalRangeTelemetry = { [weak self] telemetry in
      self?.persistOvernightHistoricalRangeTelemetry(telemetry)
    }
    ble.onMessage = { [weak self] message in
      self?.persistOvernightEventLog(message)
    }
    configureUploadService()
    refreshHeartRateHourlyRanges()
    ble.record(source: "app", title: "model.init")
    prepareClientHello()
    cleanupOrphanedActivityCaptureSessions()
    refreshActivityTimeline()
    scheduleAutoStartHealthPacketCaptureIfNeeded()
    scheduleAutoStartRespiratoryPacketWatchIfNeeded()
    recoverUncleanOvernightGuardSessionIfNeeded()
    // FIX-05 (D-09a): trigger storage compaction at launch from a background queue (Pitfall 6).
    DispatchQueue.global(qos: .utility).async { [weak self] in
      self?.runStorageCompactionIfNeeded()
    }
  }

  @MainActor deinit {
    activityDetectionIdleWorkItem?.cancel()
    movementPacketValidationTimeoutWorkItem?.cancel()
    packetImportRevisionWorkItem?.cancel()
    healthPacketCaptureTimeoutWorkItem?.cancel()
    healthPacketCaptureStreamRetryWorkItem?.cancel()
    healthPacketCaptureUIUpdateWorkItem?.cancel()
    respiratoryPacketWatchTimeoutWorkItem?.cancel()
    autoStartRespiratoryPacketWatchWorkItem?.cancel()
    temperatureHistorySyncWorkItem?.cancel()
    autoStartHealthPacketCaptureWorkItem?.cancel()
    passiveActivityCaptureWorkItem?.cancel()
    overnightGuardHeartbeatWorkItem?.cancel()
    overnightGuardRangePollWorkItem?.cancel()
    overnightGuardFinalSyncDrainWorkItem?.cancel()
    if overnightGuardCriticalBackgroundTaskID != .invalid {
      let backgroundTaskID = overnightGuardCriticalBackgroundTaskID
      Task { @MainActor in
        UIApplication.shared.endBackgroundTask(backgroundTaskID)
      }
    }
    if overnightRawSpool.isActive {
      _ = overnightRawSpool.suspendActive(reason: "model_deinit")
    } else {
      _ = overnightRawSpool.finish(status: "model_deinit")
    }
  }

  private nonisolated func runStorageCompactionIfNeeded() {
    // nonisolated: called from DispatchQueue.global background queue (Pitfall 6).
    // Uses a local GooseRustBridge() — the Rust side is stateless across instances.
    let localRust = GooseRustBridge()
    guard let report = try? localRust.request(
      method: "storage.compact_raw_evidence",
      args: [
        "database_path": HealthDataStore.defaultDatabasePath(),
        "limit_bytes": 25_165_824,
      ]
    ) else { return }

    let compactedRows = (report["compacted_rows"] as? Int) ?? 0
    let freedBytes = (report["freed_bytes"] as? Int) ?? 0
    // D-10: log only when compaction actually happened; silent otherwise.
    if compactedRows > 0 {
      let mbFreed = String(format: "%.1f", Double(freedBytes) / 1_048_576)
      DispatchQueue.main.async { [weak self] in
        self?.ble.record(source: "storage", title: "compact", body: "\(compactedRows) rows, \(mbFreed) MB freed")
      }
    }
  }

  private nonisolated static func durationFromEnvironment(
    envVar: String,
    cliPrefix: String,
    fallback: TimeInterval
  ) -> TimeInterval {
    let processInfo = ProcessInfo.processInfo
    if let value = processInfo.environment[envVar],
       let seconds = Double(value),
       seconds > 0 {
      return seconds
    }
    if let argument = processInfo.arguments.first(where: { $0.hasPrefix(cliPrefix) }),
       let seconds = Double(argument.dropFirst(cliPrefix.count)),
       seconds > 0 {
      return seconds
    }
    return fallback
  }

}
