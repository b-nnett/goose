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
  var serverReachable: Bool? = nil
  private(set) var isNetworkReachable: Bool = true
  private(set) var hrSpikeCount: Int = 0
  var liveWorkoutStrain: Double = 0
  var scheduledAlarmTime: Date? = nil    // HAP-03
  var alarmIsArmed: Bool = false         // HAP-03
  var apnsDeviceToken: String? = nil
  var uploadErrorState: String? = nil
  var hasPendingUploadAfterReconnect: Bool = false
  var lastUploadAt: Date? = nil
  var pendingBatchCount: Int = 0
  var lastSyncedCount: Int? = nil
  var syncPendingRowCount: Int = 0
  var serverImportInProgress: Bool = false
  var serverImportLastFrameCount: Int? = nil
  var connectionTestRunning: Bool = false
  var connectionTestResult: String? = nil
  // SYNC-04: owned by the @MainActor GooseAppModel; only read and written on the main actor.
  // Set from connect/disconnect handling in GooseAppModel+Lifecycle.swift.
  // BLE-thread code must not touch this property directly.
  var connectedDeviceGeneration: String? = nil

  // Set by AppShellView to run the metric-extract pipeline on HealthDataStore
  // after a sync completes. Kept as a callback because HealthDataStore is
  // owned by the view layer, not GooseAppModel.
  var onHistoricalSyncCompleted: (() -> Void)?
  // Set by AppShellView alongside onHistoricalSyncCompleted so GooseAppModel
  // extensions can call HealthDataStore methods (e.g. markBandSleepSyncFailed)
  // without creating a strong reference cycle. HealthDataStore is owned by the
  // view layer; this weak reference lets SleepSync coordinate with it.
  weak var healthStore: HealthDataStore?

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
    maxBatchRows: GooseAppModel.captureFrameWriteBatchMaxRows,
    coalesceDelay: GooseAppModel.captureFrameWriteCoalesceDelay
  )
  let uploadService = GooseUploadService(databasePath: HealthDataStore.defaultDatabasePath())
  let networkMonitor = GooseNetworkMonitor()
  let captureFrameEnqueueAggregator = CaptureFrameEnqueueAggregator(
    publishInterval: GooseAppModel.packetUIStatePublishInterval
  )
  let overnightSQLiteMirror = OvernightSQLiteMirrorQueue(databasePath: HealthDataStore.defaultDatabasePath())
  let passiveActivityDetectionPipeline = PassiveActivityDetectionPipeline()
  let strainAccumulator = GooseStrainAccumulator()
  var activeActivityPersistence: ActiveActivityPersistence?
  var activeActivityOwnsCaptureSession = false
  var activityRequestedHighFrequencyHistorySync = false
  var activeHealthPacketCapture: ActiveHealthPacketCapture?
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
  // Stored property so @Observable tracks changes and SwiftUI views invalidate correctly.
  // Kept in sync with GooseBLEBondingManager via the onBondingStateChange callback in init().
  var bondingState: GooseBLEBondingState = .notStarted
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
  static let captureFrameWriteCoalesceDelay: TimeInterval = 0.05
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
    ble.onNotification = { [weak self] event in
      Task { @MainActor [weak self] in
        self?.handleNotification(event)
      }
    }
    ble.historicalDirectWriteDatabasePath = HealthDataStore.defaultDatabasePath()
    ble.onLiveHeartRate = { [weak self] bpm, source, capturedAt in
      heartRateSamplePipeline.recordHeartRateSample(bpm: bpm, source: source, capturedAt: capturedAt)
      Task { [weak self] in
        guard let self, self.activeActivityPersistence != nil else { return }
        await self.strainAccumulator.ingest(bpm: bpm, date: capturedAt)
        if let load = await self.strainAccumulator.pollIfReady(now: capturedAt) {
          Task { @MainActor [weak self] in self?.liveWorkoutStrain = load }
        }
      }
    }
    ble.onHRVSample = { rmssdMS, rrIntervalCount, source, capturedAt in
      heartRateSamplePipeline.recordHRVSample(
        rmssdMS: rmssdMS,
        rrIntervalCount: rrIntervalCount,
        source: source,
        capturedAt: capturedAt
      )
    }
    ble.onHRSpike = { [weak self] _, _ in
      Task { @MainActor in self?.hrSpikeCount += 1 }
    }
    // Override the default onBondingStateChange set in GooseBLEClient.init() so we can
    // (a) keep driving connectionState via updateConnectionState, and
    // (b) mirror the bonding state into the @Observable stored property so SwiftUI
    //     views reading model.bondingState are properly invalidated on each transition.
    ble.bondingManager.onBondingStateChange = { [weak self] newState in
      guard let self else { return }
      self.ble.updateConnectionState(newState.connectionStateString)
      self.bondingState = newState
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
    // SYNC-01: weak self prevents a GooseAppModel↔GooseBLEClient retain cycle;
    // this closure is intentionally weak (per upstream PR #26 review).
    ble.onHistoricalSyncProgress = { [weak self] progress in
      Task { @MainActor in
        self?.handleHistoricalSyncProgress(progress)
      }
    }
    configureUploadService()
    networkMonitor.onReachabilityChange = { [weak self] reachable in
      Task { @MainActor in
        self?.isNetworkReachable = reachable
        self?.handleReachabilityChange(reachable)
      }
    }
    networkMonitor.start()
    ble.record(source: "app.network", title: "monitor.start")
    refreshHeartRateHourlyRanges()
    ble.record(source: "app", title: "model.init")
    prepareClientHello()
    cleanupOrphanedActivityCaptureSessions()
    refreshActivityTimeline()
    scheduleAutoStartHealthPacketCaptureIfNeeded()
    scheduleAutoStartRespiratoryPacketWatchIfNeeded()
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
