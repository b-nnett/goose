import CoreBluetooth
import Foundation
import Observation
import OSLog


@Observable final class GooseBLEClient: NSObject, @unchecked Sendable {
  var bluetoothState = "not requested"
  var connectionState = "disconnected"
  var isScanning = false
  var discoveredDevices: [GooseDiscoveredDevice] = []
  var liveHeartRateBPM: Int?
  var liveHeartRateSource = "waiting"
  var liveHeartRateUpdatedAt: Date?
  var restingHeartRateEstimateBPM: Double?
  var restingHeartRateEstimateSampleCount = 0
  var restingHeartRateEstimateSource = "waiting"
  var restingHeartRateEstimateUpdatedAt: Date?
  var liveHRVRMSSD: Double?
  var liveHRVRRIntervalCount = 0
  var liveHRVSource = "waiting"
  var liveHRVUpdatedAt: Date?
  var liveHRVRMSSDSampleCount = 0
  var reconnectState = "idle"
  var hrReconnectState = "idle"
  var discoveredHRDevices: [GooseDiscoveredDevice] = []
  var hrConnectionState: String = "disconnected"
  var hrBluetoothState: String = "unknown"
  var rememberedDeviceDescription = "none"
  var activeDeviceName = "WHOOP"
  var activeDeviceIdentifier: UUID?
  var selectedDeviceID: UUID?
  var connectedPeripheralUUID: String?
  var connectedAt: Date?
  var lastSyncAt: Date?
  var batteryLevelPercent: Int?
  var batteryUpdatedAt: Date?
  var batteryIsCharging: Bool?
  var batteryPowerStatus = "Unknown"
  var firmwareVersion: String?
  var modelNumber: String?
  var hardwareRevision: String?
  var softwareRevision: String?
  var manufacturerName: String?
  var historicalPacketCount = 0
  var lastHistoricalSyncCompletedAt: Date?
  var lastHistoricalRangeCommandStatus = "No GET_DATA_RANGE response"
  var alarmCommandStatus = "No alarm command sent"
  var lastAlarmCommandFrameHex = ""
  var lastAlarmResponseSummary = "No alarm response yet"
  var lastAlarmResponsePayloadHex = ""
  var lastAlarmEventSummary = "No alarm event yet"
  var lastAlarmEventPayloadHex = ""
  var lastAlarmScheduledAt: Date?
  var lastAlarmID: Int?
  var physiologyCaptureStatus = "Not started"
  var lastPhysiologyCommandSummary = "No physiology stream command sent"
  var highFrequencyHistorySyncStatus = "Off"
  var highFrequencyHistorySyncActive = false
  var highFrequencyHistorySyncExpiresAt: Date?
  var lastHighFrequencyHistorySyncResponse = "No high-frequency sync response yet"
  var lastHighFrequencyHistorySyncEvent = "No high-frequency sync event yet"
  var strapClockDate: Date?
  var strapClockOffsetSeconds: TimeInterval?
  var strapClockUpdatedAt: Date?
  var strapClockStatus = "Not read"
  var lastClockCommandFrameHex = ""
  var lastClockResponsePayloadHex = ""
  var syncToast: GooseSyncToast?
  var lastSyncFailure: GooseSyncFailure?
  var syncFailureSheet: GooseSyncFailure?
  var debugCommandStatus = "No debug command sent"
  var debugCommandResponses: [GooseDebugCommandResponse] = []
  var debugCommandSnapshotPath = "No debug command snapshot"

  var onNotification: ((GooseNotificationEvent) -> Void)?
  var onRawNotification: ((GooseNotificationEvent) -> Void)?
  var onRawNotificationWithContext: ((GooseNotificationEvent, GooseBLENotificationContext) -> Void)?
  var onCommandWrite: ((GooseCommandWriteEvent) -> Void)?
  var onLiveHeartRate: ((Int, String, Date) -> Void)?
  var onHRVSample: ((Double, Int, String, Date) -> Void)?
  var onHRSpike: ((Int, String) -> Void)?
  var onConnectionStateChange: ((String) -> Void)?
  var onHRConnectionStateChange: ((String) -> Void)?
  var onHistoricalSyncProgress: ((GooseHistoricalSyncProgress) -> Void)?
  var onHistoricalRangeTelemetry: ((GooseHistoricalRangeTelemetry) -> Void)?
  var onMessage: ((GooseMessage) -> Void)?

  let logger = Logger(subsystem: "com.goose.swift", category: "ble")
  let coreBluetoothQueue = DispatchQueue(label: "com.goose.swift.corebluetooth", qos: .utility)
  let realtimeVitalsQueue = DispatchQueue(label: "com.goose.swift.realtime-vitals", qos: .userInitiated)
  let diagnosticLogQueue = DispatchQueue(label: "com.goose.swift.diagnostic-log", qos: .utility)
  let bleUIStateAggregator = BLEUIStateAggregator(publishInterval: GooseBLEClient.bleUIStatePublishInterval)
  let messageStore = GooseMessageStore(
    maximumMessages: GooseBLEClient.maximumDisplayedMessages,
    flushInterval: GooseBLEClient.displayedMessageFlushInterval
  )
  let hrMonitorManager = GooseBLEHRMonitorManager()
  let bondingManager = GooseBLEBondingManager()
  let historicalManager = GooseBLEHistoricalManager()

  // Proxy computed properties — all historical read call sites across extensions use these.
  var isHistoricalSyncing: Bool { historicalManager.isHistoricalSyncing }
  var historicalSyncStatus: String { historicalManager.historicalSyncStatus }
  var historicalSyncRunID: UUID { historicalManager.historicalSyncRunID }
  let notificationContextLock = NSLock()
  var notificationContextActiveDeviceName = "WHOOP"
  var notificationContextConnectionState = "disconnected"
  static let displayedMessageFlushInterval: TimeInterval = 0.5
  static let maximumDisplayedMessages = 300
  static let bleUIStatePublishInterval: TimeInterval = 1.0
  static let diagnosticLogProtection: FileProtectionType = .completeUntilFirstUserAuthentication
  static let diagnosticLogSetupWarningLock = NSLock()
  static var diagnosticLogSetupWarnings: [String] = []
  let defaults = UserDefaults.standard
  let autoStartPhysiologyCaptureOnReady: Bool = {
    let processInfo = ProcessInfo.processInfo
    return processInfo.arguments.contains("--goose-start-physiology-capture")
      || processInfo.environment["GOOSE_START_PHYSIOLOGY_CAPTURE"] == "1"
  }()
  let autoHistoricalSyncOnReady: Bool = {
    let processInfo = ProcessInfo.processInfo
    return processInfo.arguments.contains("--goose-auto-historical-sync")
      || processInfo.environment["GOOSE_AUTO_HISTORICAL_SYNC"] == "1"
  }()
  let diagnosticLoggingEnabled: Bool = {
    let processInfo = ProcessInfo.processInfo
    if processInfo.arguments.contains("--goose-disable-diagnostics")
      || processInfo.environment["GOOSE_DISABLE_DIAGNOSTICS"] == "1"
      || processInfo.environment["GOOSE_DIAGNOSTIC_LOGGING"] == "0" {
      return false
    }
    return processInfo.arguments.contains("--goose-enable-diagnostics")
      || processInfo.environment["GOOSE_ENABLE_DIAGNOSTICS"] == "1"
      || processInfo.environment["GOOSE_DIAGNOSTIC_LOGGING"] == "1"
  }()
  let prioritizeLiveCaptureOnReady: Bool = {
    let processInfo = ProcessInfo.processInfo
    return processInfo.arguments.contains("--goose-start-physiology-capture")
      || processInfo.arguments.contains("--goose-start-health-packet-capture")
      || processInfo.arguments.contains("--goose-start-temperature-packet-capture")
      || processInfo.environment["GOOSE_START_PHYSIOLOGY_CAPTURE"] == "1"
      || processInfo.environment["GOOSE_START_HEALTH_PACKET_CAPTURE"] == "1"
      || processInfo.environment["GOOSE_START_TEMPERATURE_PACKET_CAPTURE"] == "1"
  }()
  let autoSendDebugSkinTemperatureCommand: Bool = {
    let processInfo = ProcessInfo.processInfo
    return processInfo.arguments.contains("--goose-send-debug-skin-temp-command")
      || processInfo.environment["GOOSE_SEND_DEBUG_SKIN_TEMP_COMMAND"] == "1"
  }()
  let forceDebugMenuWrite: Bool = {
    let processInfo = ProcessInfo.processInfo
    return processInfo.arguments.contains("--goose-force-debug-menu-write")
      || processInfo.environment["GOOSE_FORCE_DEBUG_MENU_WRITE"] == "1"
  }()
  let consoleCaptureStatusEnabled: Bool = {
    let processInfo = ProcessInfo.processInfo
    return processInfo.arguments.contains("--goose-console-capture-status")
      || processInfo.environment["GOOSE_CONSOLE_CAPTURE_STATUS"] == "1"
  }()
  let debugSkinTemperatureCommandPayload: Data = {
    let processInfo = ProcessInfo.processInfo
    if let hex = processInfo.environment["GOOSE_DEBUG_MENU_COMMAND_HEX"],
       let data = Data(hexString: hex),
       !data.isEmpty {
      return data
    }
    if let text = processInfo.environment["GOOSE_DEBUG_MENU_COMMAND"],
       let data = text.data(using: .utf8),
       !data.isEmpty {
      return data
    }
    return Data([0x73, 0x0a])
  }()
  let diagnosticLogURL: URL? = {
    let processInfo = ProcessInfo.processInfo
    let loggingEnabled = processInfo.arguments.contains("--goose-enable-diagnostics")
      || processInfo.environment["GOOSE_ENABLE_DIAGNOSTICS"] == "1"
      || processInfo.environment["GOOSE_DIAGNOSTIC_LOGGING"] == "1"
    let loggingDisabled = processInfo.arguments.contains("--goose-disable-diagnostics")
      || processInfo.environment["GOOSE_DISABLE_DIAGNOSTICS"] == "1"
      || processInfo.environment["GOOSE_DIAGNOSTIC_LOGGING"] == "0"
    guard loggingEnabled, !loggingDisabled else {
      return nil
    }
    guard let directory = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
      GooseBLEClient.recordDiagnosticLogSetupWarning("goose-ble.log setup failed: Application Support directory unavailable")
      return nil
    }
    let gooseDirectory = directory.appendingPathComponent("GooseSwift", isDirectory: true)
    let url = gooseDirectory.appendingPathComponent("goose-ble.log")
    do {
      try GooseBLEClient.prepareDiagnosticLogDirectory(gooseDirectory)
      if FileManager.default.fileExists(atPath: url.path) {
        try FileManager.default.removeItem(at: url)
      }
    } catch {
      GooseBLEClient.recordDiagnosticLogSetupWarning("goose-ble.log setup failed: \(String(describing: error))")
      return nil
    }
    return url
  }()
  let diagnosticLogMirrorURL: URL? = {
    let processInfo = ProcessInfo.processInfo
    let mirrorEnabled = processInfo.arguments.contains("--goose-afc-diagnostic-mirror")
      || processInfo.environment["GOOSE_AFC_DIAGNOSTIC_MIRROR"] == "1"
    guard mirrorEnabled else {
      return nil
    }
    guard let directory = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first else {
      GooseBLEClient.recordDiagnosticLogSetupWarning("goose-ble-live.log mirror setup failed: Documents directory unavailable")
      return nil
    }
    let gooseDirectory = directory.appendingPathComponent("GooseSwift", isDirectory: true)
    let url = gooseDirectory.appendingPathComponent("goose-ble-live.log")
    do {
      try GooseBLEClient.prepareDiagnosticLogFile(at: url, directory: gooseDirectory)
    } catch {
      GooseBLEClient.recordDiagnosticLogSetupWarning("goose-ble-live.log mirror setup failed: \(String(describing: error))")
      return nil
    }
    return url
  }()
  let overnightSideChannelLogURL: URL? = {
    guard let directory = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first else {
      GooseBLEClient.recordDiagnosticLogSetupWarning("goose-ble-live.log setup failed: Documents directory unavailable")
      return nil
    }
    let gooseDirectory = directory.appendingPathComponent("GooseSwift", isDirectory: true)
    let url = gooseDirectory.appendingPathComponent("goose-ble-live.log")
    do {
      try GooseBLEClient.prepareDiagnosticLogFile(at: url, directory: gooseDirectory)
      return url
    } catch {
      GooseBLEClient.recordDiagnosticLogSetupWarning("goose-ble-live.log setup failed: \(String(describing: error))")
      return nil
    }
  }()
  var central: CBCentralManager?
  var peripherals: [UUID: CBPeripheral] = [:]
  var whoopCandidateIDs = Set<UUID>()
  var activePeripheral: CBPeripheral?
  var messages: [GooseMessage] {
    messageStore.messages
  }
  var commandCharacteristic: CBCharacteristic?
  /// Only mutated and read on the main thread. CoreBluetooth delegates land
  /// on `coreBluetoothQueue` but every entry point bounces to main via
  /// `dispatchCoreBluetoothDelegateToMainIfNeeded` before touching this
  /// property; UI callers (SwiftUI buttons, @MainActor app model paths)
  /// are already on main.
  var activeDeviceGeneration: WhoopGeneration = .gen5
  var activeDescriptor: WearableDescriptor?
  var debugMenuCharacteristic: CBCharacteristic?
  var batteryLevelCharacteristic: CBCharacteristic?
  var batteryLevelStatusCharacteristic: CBCharacteristic?
  var batteryCharacteristicDiscoveryPending = false
  var lastBatteryLevelSample: (percent: Int, capturedAt: Date)?
  var inferredBatteryChargingUntil: Date?
  var rememberedDeviceID: UUID?
  var rememberedDeviceName: String?
  var rememberedDeviceValidated = false
  var autoReconnectTargetID: UUID?
  var reconnectBackoff = ReconnectBackoff()
  var reconnectWorkItem: DispatchWorkItem?
  var reconnectGeneration: Int = 0
  var startupReconnectAttempted = false
  var pendingConnectionReason: String?
  var pendingAutomaticHistoricalSyncReason: String?
  var clientHelloSentForCurrentConnection = false
  var readySyncWorkItem: DispatchWorkItem?
  var syncClearWorkItem: DispatchWorkItem?
  let historicalDirectWriteBridge = GooseRustBridge()
  var historicalDirectWriteDatabasePath: String = ""
  var lastHeartRateLogAt: Date?
  var lastHeartRateLogBPM: Int?
  var lastHeartRateLogSource = ""
  var lastHeartRatePublishedAt = Date.distantPast
  var lastHeartRatePublishedBPM: Int?
  var lastHeartRatePublishedSource = "waiting"
  var lastHeartRateCallbackAt = Date.distantPast
  var lastHeartRateCallbackSource = ""
  var lastNotificationSyncPublishedAt = Date.distantPast
  var notificationSideEffectSkipCount = 0
  var notificationSideEffectSkipBytes = 0
  var lastNotificationSideEffectSkipLoggedAt = Date.distantPast
  var restingHeartRateWindowBPM: [Int] = []
  var lastRestingHeartRateEstimateBPM: Double?
  var lastRestingHeartRateEstimatePublishedAt = Date.distantPast
  var rrIntervalWindowMS: [Double] = []
  var rrIntervalChunkMS: [Double] = []
  var rrIntervalChunkStartedAt: Date?
  var hrvRMSSDSamples: [(rmssd: Double, rrIntervalCount: Int)] = []
  var lastPublishedHRVRMSSD: Double?
  var lastHRVPublishedAt = Date.distantPast
  var lastHRVLogAt: Date?
  var autoStartedPhysiologyCapture = false
  var autoConnectForPhysiologyCapture = false
  var nextSensorCommandSequence: UInt8 = 180
  var pendingAlarmCommand: PendingAlarmCommand?
  var alarmCommandTimeoutWorkItem: DispatchWorkItem?
  var nextAlarmCommandSequence: UInt8 = 64
  var pendingClockCommand: PendingClockCommand?
  var clockCommandTimeoutWorkItem: DispatchWorkItem?
  var nextClockCommandSequence: UInt8 = 96
  var pendingDebugCommands: [UInt8: PendingDebugCommand] = [:]
  var debugCommandTimeoutWorkItems: [UInt8: DispatchWorkItem] = [:]
  var nextDebugCommandSequence: UInt8 = 120
  var highFrequencyHistorySyncRequestedExpiry: Date?
  var debugSkinTemperatureCommandSent = false
  var debugSkinTemperatureCommandWorkItem: DispatchWorkItem?

  enum DefaultsKey {
    static let rememberedDeviceID = "goose.swift.rememberedDeviceID"
    static let rememberedDeviceName = "goose.swift.rememberedDeviceName"
    static let deviceUUIDMap = "goose.swift.device_uuid_map"
    static let rememberedDeviceValidated = "goose.swift.rememberedDeviceValidatedWhoop"
    static let lastBatteryPercent = "goose.swift.lastBatteryPercent"
    static let lastBatteryCapturedAt = "goose.swift.lastBatteryCapturedAt"
    static let inferredBatteryChargingUntil = "goose.swift.inferredBatteryChargingUntil"
    static let restingHeartRateEstimateBPM = "goose.swift.restingHeartRateEstimateBPM"
    static let restingHeartRateEstimateSampleCount = "goose.swift.restingHeartRateEstimateSampleCount"
    static let restingHeartRateEstimateUpdatedAt = "goose.swift.restingHeartRateEstimateUpdatedAt"
    static let restingHeartRateEstimateSource = "goose.swift.restingHeartRateEstimateSource"
    static let liveHRVRMSSD = "goose.swift.liveHRVRMSSD"
    static let liveHRVRRIntervalCount = "goose.swift.liveHRVRRIntervalCount"
    static let liveHRVRMSSDSampleCount = "goose.swift.liveHRVRMSSDSampleCount"
    static let liveHRVUpdatedAt = "goose.swift.liveHRVUpdatedAt"
    static let liveHRVSource = "goose.swift.liveHRVSource"
    static let debugHistoricalRangeStatus = "goose.swift.debug.historicalRangeStatus"
  }

  static let restorationIdentifier = "com.goose.swift.central"
  static let heartRatePublishInterval: TimeInterval = 1
  static let heartRateCallbackInterval: TimeInterval = 0.1
  static let notificationSyncPublishInterval: TimeInterval = 1
  static let notificationSideEffectSkipLogInterval: TimeInterval = 30
  static let notificationSideEffectSkipLogStride = 250
  static let restingHeartRateWindowSize = 300
  static let restingHeartRateMinimumSampleCount = 12
  static let restingHeartRateEstimatePublishInterval: TimeInterval = 60
  static let hrvRRIntervalWindowSize = 120
  static let hrvRRIntervalChunkSize = 30
  static let hrvMinimumRRIntervalsPerChunk = 10
  static let hrvChunkMaxAge: TimeInterval = 60
  static let hrvRMSSDAverageWindowSize = 12
  static let hrvEstimatePublishInterval: TimeInterval = 60
  static let historicalFrameFlushBatchSize = 32
  static let historicalPacketCountPublishInterval: TimeInterval = 1
  static let historicalProgressCallbackInterval: TimeInterval = 1
  static let strapClockAutoSyncThresholdSeconds: TimeInterval = 5
  static let diagnosticLogFormatter: ISO8601DateFormatter = {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return formatter
  }()
  static let diagnosticLogFormatterLock = NSLock()
  static let alarmTimeFormatter: DateFormatter = {
    let formatter = DateFormatter()
    formatter.dateStyle = .none
    formatter.timeStyle = .short
    return formatter
  }()

  let whoopServices = [
    CBUUID(string: "fd4b0001-cce1-4033-93ce-002d5875f58a"),
    CBUUID(string: "61080001-8d6d-82b8-614a-1c8cb0f8dcc6"),
  ]

  let commandCharacteristicIDs = [
    CBUUID(string: "fd4b0002-cce1-4033-93ce-002d5875f58a"),
    CBUUID(string: "61080002-8d6d-82b8-614a-1c8cb0f8dcc6"),
  ]

  let notificationCharacteristicIDs = [
    CBUUID(string: "fd4b0003-cce1-4033-93ce-002d5875f58a"),
    CBUUID(string: "fd4b0004-cce1-4033-93ce-002d5875f58a"),
    CBUUID(string: "fd4b0005-cce1-4033-93ce-002d5875f58a"),
    CBUUID(string: "fd4b0007-cce1-4033-93ce-002d5875f58a"),
    CBUUID(string: "61080003-8d6d-82b8-614a-1c8cb0f8dcc6"),
    CBUUID(string: "61080004-8d6d-82b8-614a-1c8cb0f8dcc6"),
    CBUUID(string: "61080005-8d6d-82b8-614a-1c8cb0f8dcc6"),
    CBUUID(string: "61080007-8d6d-82b8-614a-1c8cb0f8dcc6"),
  ]

  let debugMenuCharacteristicIDs = [
    CBUUID(string: "fd4b0007-cce1-4033-93ce-002d5875f58a"),
    CBUUID(string: "61080007-8d6d-82b8-614a-1c8cb0f8dcc6"),
  ]

  let standardHeartRateServiceID = CBUUID(string: "180D")
  let standardHeartRateMeasurementID = CBUUID(string: "2A37")
  let batteryServiceID = CBUUID(string: "180F")
  let batteryLevelCharacteristicID = CBUUID(string: "2A19")
  let batteryLevelStatusCharacteristicID = CBUUID(string: "2BED")
  let deviceInformationServiceID = CBUUID(string: "180A")
  let modelNumberCharacteristicID = CBUUID(string: "2A24")
  let firmwareRevisionCharacteristicID = CBUUID(string: "2A26")
  let hardwareRevisionCharacteristicID = CBUUID(string: "2A27")
  let softwareRevisionCharacteristicID = CBUUID(string: "2A28")
  let manufacturerNameCharacteristicID = CBUUID(string: "2A29")

  var deviceInformationCharacteristicIDs: [CBUUID] {
    [
      modelNumberCharacteristicID,
      firmwareRevisionCharacteristicID,
      hardwareRevisionCharacteristicID,
      softwareRevisionCharacteristicID,
      manufacturerNameCharacteristicID,
    ]
  }

  var serviceDiscoveryIDs: [CBUUID] {
    whoopServices + [
      standardHeartRateServiceID,
      batteryServiceID,
      deviceInformationServiceID,
    ]
  }

  enum HistoricalCommandKind {
    case getDataRange
    case sendHistoricalData
    case historicalDataResult

    var commandNumber: UInt8 {
      switch self {
      case .getDataRange: 34
      case .sendHistoricalData: 22
      case .historicalDataResult: 23
      }
    }

    var payload: [UInt8] {
      switch self {
      case .getDataRange, .sendHistoricalData:
        []
      case .historicalDataResult:
        Self.defaultHistoricalDataResultPayload
      }
    }

    static let defaultHistoricalDataResultPayload: [UInt8] = [1, 0, 0, 0, 0, 0, 0, 0, 0]

    var name: String {
      switch self {
      case .getDataRange: "GET_DATA_RANGE"
      case .sendHistoricalData: "SEND_HISTORICAL_DATA"
      case .historicalDataResult: "HISTORICAL_DATA_RESULT"
      }
    }
  }

  struct PendingHistoricalCommand {
    let kind: HistoricalCommandKind
    let sequence: UInt8
  }

  enum ClockCommandKind {
    case get
    case set(Date)

    var commandNumber: UInt8 {
      switch self {
      case .get:
        return 11
      case .set:
        return 10
      }
    }

    var payload: [UInt8] {
      switch self {
      case .get:
        return []
      case .set(let date):
        let timestamp = GooseBLEClient.clockTimestampParts(for: date)
        var bytes: [UInt8] = []
        GooseBLEClient.appendUInt32LE(timestamp.seconds, to: &bytes)
        GooseBLEClient.appendUInt32LE(timestamp.subseconds, to: &bytes)
        return bytes
      }
    }

    var name: String {
      switch self {
      case .get:
        return "GET_CLOCK"
      case .set:
        return "SET_CLOCK"
      }
    }
  }

  struct PendingClockCommand {
    let kind: ClockCommandKind
    let sequence: UInt8
    let sentAt: Date
    let syncIfNeeded: Bool
  }

  struct SensorStreamCommandKind {
    let commandNumber: UInt8
    let payload: [UInt8]
    let name: String

    static func revisionBoolean(_ enabled: Bool) -> [UInt8] {
      [1, enabled ? 1 : 0]
    }

    private static func uint16LE(_ value: UInt16) -> [UInt8] {
      [
        UInt8(value & 0xff),
        UInt8((value >> 8) & 0xff),
      ]
    }

    static let startPhysiologyCapture = [
      SensorStreamCommandKind(commandNumber: 3, payload: [1], name: "TOGGLE_REALTIME_HR_ON"),
      SensorStreamCommandKind(commandNumber: 63, payload: [1], name: "SEND_R10_R11_REALTIME_ON"),
      SensorStreamCommandKind(commandNumber: 106, payload: revisionBoolean(true), name: "TOGGLE_IMU_MODE_ON"),
      SensorStreamCommandKind(commandNumber: 154, payload: revisionBoolean(true), name: "TOGGLE_PERSISTENT_R21_ON"),
      SensorStreamCommandKind(commandNumber: 107, payload: revisionBoolean(true), name: "ENABLE_OPTICAL_DATA_ON"),
      SensorStreamCommandKind(commandNumber: 108, payload: revisionBoolean(true), name: "TOGGLE_OPTICAL_MODE_ON"),
      SensorStreamCommandKind(commandNumber: 153, payload: revisionBoolean(true), name: "TOGGLE_PERSISTENT_R20_ON"),
    ]

    static let startMovementHeartRateCapture = [
      SensorStreamCommandKind(commandNumber: 3, payload: [1], name: "TOGGLE_REALTIME_HR_ON"),
      SensorStreamCommandKind(commandNumber: 63, payload: [1], name: "SEND_R10_R11_REALTIME_ON"),
      SensorStreamCommandKind(commandNumber: 106, payload: revisionBoolean(true), name: "TOGGLE_IMU_MODE_ON"),
    ]

    static let stopPhysiologyCapture = [
      SensorStreamCommandKind(commandNumber: 153, payload: revisionBoolean(false), name: "TOGGLE_PERSISTENT_R20_OFF"),
      SensorStreamCommandKind(commandNumber: 108, payload: revisionBoolean(false), name: "TOGGLE_OPTICAL_MODE_OFF"),
      SensorStreamCommandKind(commandNumber: 107, payload: revisionBoolean(false), name: "ENABLE_OPTICAL_DATA_OFF"),
      SensorStreamCommandKind(commandNumber: 154, payload: revisionBoolean(false), name: "TOGGLE_PERSISTENT_R21_OFF"),
      SensorStreamCommandKind(commandNumber: 106, payload: revisionBoolean(false), name: "TOGGLE_IMU_MODE_OFF"),
      SensorStreamCommandKind(commandNumber: 63, payload: [0], name: "SEND_R10_R11_REALTIME_OFF"),
      SensorStreamCommandKind(commandNumber: 3, payload: [0], name: "TOGGLE_REALTIME_HR_OFF"),
    ]

    static let stopMovementHeartRateCapture = [
      SensorStreamCommandKind(commandNumber: 106, payload: revisionBoolean(false), name: "TOGGLE_IMU_MODE_OFF"),
      SensorStreamCommandKind(commandNumber: 63, payload: [0], name: "SEND_R10_R11_REALTIME_OFF"),
      SensorStreamCommandKind(commandNumber: 3, payload: [0], name: "TOGGLE_REALTIME_HR_OFF"),
    ]

    static func enterHighFrequencyHistorySync(intervalSeconds: Int, durationSeconds: Int) -> SensorStreamCommandKind? {
      guard
        intervalSeconds > 0,
        durationSeconds > 0,
        let interval = UInt16(exactly: intervalSeconds),
        let duration = UInt16(exactly: durationSeconds)
      else {
        return nil
      }

      var payload: [UInt8] = [2]
      payload.append(contentsOf: uint16LE(interval))
      payload.append(contentsOf: uint16LE(duration))
      return SensorStreamCommandKind(
        commandNumber: 96,
        payload: payload,
        name: "ENTER_HIGH_FREQ_SYNC"
      )
    }

    static let exitHighFrequencyHistorySync = SensorStreamCommandKind(
      commandNumber: 97,
      payload: [],
      name: "EXIT_HIGH_FREQ_SYNC"
    )

    static let responseNames: [UInt8: String] = [
      3: "TOGGLE_REALTIME_HR",
      63: "SEND_R10_R11_REALTIME",
      96: "ENTER_HIGH_FREQ_SYNC",
      97: "EXIT_HIGH_FREQ_SYNC",
      106: "TOGGLE_IMU_MODE",
      107: "ENABLE_OPTICAL_DATA",
      108: "TOGGLE_OPTICAL_MODE",
      153: "TOGGLE_PERSISTENT_R20",
      154: "TOGGLE_PERSISTENT_R21",
    ]
  }

  struct AlarmHapticsPattern {
    let waveformEffects: [UInt8]
    let loopControl: UInt16
    let overallLoop: UInt8
    let durationSeconds: UInt8

    static let whoopDefault = AlarmHapticsPattern(
      waveformEffects: [47, 152, 0, 0, 0, 0, 0, 0],
      loopControl: 0,
      overallLoop: 7,
      durationSeconds: 30
    )

    var payloadBytes: [UInt8] {
      var bytes = Array(waveformEffects.prefix(8))
      if bytes.count < 8 {
        bytes.append(contentsOf: repeatElement(UInt8(0), count: 8 - bytes.count))
      }
      GooseBLEClient.appendUInt16LE(loopControl, to: &bytes)
      bytes.append(overallLoop)
      bytes.append(durationSeconds)
      return bytes
    }
  }

  enum AlarmCommandKind {
    case get(alarmID: UInt8)
    case set(alarmID: UInt8, date: Date, pattern: AlarmHapticsPattern)
    case run(alarmID: UInt8)
    case disableAll

    var commandNumber: UInt8 {
      switch self {
      case .set:
        return 66
      case .get:
        return 67
      case .run:
        return 68
      case .disableAll:
        return 69
      }
    }

    var name: String {
      switch self {
      case .set:
        return "SET_ALARM_TIME"
      case .get:
        return "GET_ALARM_TIME"
      case .run:
        return "RUN_ALARM"
      case .disableAll:
        return "DISABLE_ALARM"
      }
    }

    var alarmID: UInt8? {
      switch self {
      case .get(let alarmID), .set(let alarmID, _, _), .run(let alarmID):
        return alarmID
      case .disableAll:
        return nil
      }
    }

    var scheduledDate: Date? {
      switch self {
      case .set(_, let date, _):
        return date
      case .get, .run, .disableAll:
        return nil
      }
    }

    var payload: [UInt8] {
      switch self {
      case .get(let alarmID):
        return [4, alarmID]
      case .set(let alarmID, let date, let pattern):
        var bytes: [UInt8] = [4, alarmID]
        let timestamp = GooseBLEClient.alarmTimestampParts(for: date)
        GooseBLEClient.appendUInt32LE(timestamp.seconds, to: &bytes)
        GooseBLEClient.appendUInt16LE(timestamp.subseconds, to: &bytes)
        bytes.append(contentsOf: pattern.payloadBytes)
        return bytes
      case .run(let alarmID):
        return [2, alarmID]
      case .disableAll:
        return [2, 0xff]
      }
    }
  }

  struct PendingAlarmCommand {
    let kind: AlarmCommandKind
    let sequence: UInt8
  }

  enum HistoricalMetadataKind: UInt16 {
    case historyStart = 1
    case historyEnd = 2
    case historyComplete = 3

    var name: String {
      switch self {
      case .historyStart: "HistoryStart"
      case .historyEnd: "HistoryEnd"
      case .historyComplete: "HistoryComplete"
      }
    }
  }

  enum V5PacketType {
    static let command: UInt8 = 35
    static let commandResponse: UInt8 = 36
    static let puffinCommandResponse: UInt8 = 38
    static let event: UInt8 = 48
    static let historicalData: UInt8 = 47
    static let metadata: UInt8 = 49
    static let historicalIMUDataStream: UInt8 = 52
    static let puffinMetadata: UInt8 = 56
  }

  struct PendingDebugCommand {
    let id: String
    let title: String
    let commandNumber: UInt8
    let sequence: UInt8
    let requestedAt: Date
    let requestPayloadHex: String
    let requestFrameHex: String
    let source: String
  }

  static let debugResearchCommandDefinitions: [GooseDebugCommandDefinition] = [
    GooseDebugCommandDefinition(
      id: "get_body_location_and_status",
      title: "Body Location And Status",
      commandNumber: 84,
      family: "research",
      risk: "read",
      detail: "WHOOP APK render-only status command; likely body placement and strap state.",
      defaultPayloadHex: "",
      requiresPayloadHex: false,
      payloadHint: "no payload"
    ),
    GooseDebugCommandDefinition(
      id: "get_research_packet",
      title: "Research Packet",
      commandNumber: 132,
      family: "research",
      risk: "read",
      detail: "Generic research packet request from the APK command map.",
      defaultPayloadHex: "",
      requiresPayloadHex: false,
      payloadHint: "no payload"
    ),
    GooseDebugCommandDefinition(
      id: "get_extended_battery_info",
      title: "Extended Battery Info",
      commandNumber: 98,
      family: "battery",
      risk: "read",
      detail: "Extended battery state beyond the standard GATT level/status characteristics.",
      defaultPayloadHex: "",
      requiresPayloadHex: false,
      payloadHint: "no payload"
    ),
    GooseDebugCommandDefinition(
      id: "get_battery_pack_info",
      title: "Battery Pack Info",
      commandNumber: 151,
      family: "battery",
      risk: "read",
      detail: "APK parser nh0.l; payload revision 1.",
      defaultPayloadHex: "01",
      requiresPayloadHex: false,
      payloadHint: "revision 01"
    ),
    GooseDebugCommandDefinition(
      id: "get_led_drive",
      title: "LED Drive",
      commandNumber: 40,
      family: "optical",
      risk: "read",
      detail: "Optical LED drive configuration read.",
      defaultPayloadHex: "",
      requiresPayloadHex: false,
      payloadHint: "no payload"
    ),
    GooseDebugCommandDefinition(
      id: "get_tia_gain",
      title: "TIA Gain",
      commandNumber: 42,
      family: "optical",
      risk: "read",
      detail: "Optical transimpedance amplifier gain read.",
      defaultPayloadHex: "",
      requiresPayloadHex: false,
      payloadHint: "no payload"
    ),
    GooseDebugCommandDefinition(
      id: "get_bias_offset",
      title: "Bias Offset",
      commandNumber: 44,
      family: "optical",
      risk: "read",
      detail: "Optical bias offset configuration read.",
      defaultPayloadHex: "",
      requiresPayloadHex: false,
      payloadHint: "no payload"
    ),
    GooseDebugCommandDefinition(
      id: "get_device_config_value",
      title: "Device Config Value",
      commandNumber: 121,
      family: "config",
      risk: "keyed read",
      detail: "APK parser nh0.o. Accepts a 32-byte key; app prefixes revision 01.",
      defaultPayloadHex: nil,
      requiresPayloadHex: true,
      payloadHint: "64 hex key, or 66 hex revision+key"
    ),
    GooseDebugCommandDefinition(
      id: "get_feature_flag_value",
      title: "Feature Flag Value",
      commandNumber: 128,
      family: "config",
      risk: "keyed read",
      detail: "APK parser nh0.p. Accepts a 32-byte key; app prefixes revision 01.",
      defaultPayloadHex: nil,
      requiresPayloadHex: true,
      payloadHint: "64 hex key, or 66 hex revision+key"
    ),
    GooseDebugCommandDefinition(
      id: "toggle_imu_mode_historical",
      title: "Toggle IMU Mode Historical",
      commandNumber: 105,
      family: "movement",
      risk: "state change",
      detail: "Historical IMU mode toggle. Remote-only; requires an explicit payload.",
      defaultPayloadHex: nil,
      requiresPayloadHex: true,
      payloadHint: "explicit payload hex required"
    ),
  ]

  var canScan: Bool {
    central?.state == .poweredOn
  }

  var canConnect: Bool {
    canScan && !discoveredDevices.isEmpty && activePeripheral == nil
  }

  var canSendHello: Bool {
    activePeripheral != nil && commandCharacteristic != nil && connectionState == "ready"
  }

  var canSyncHistorical: Bool {
    canSendHello && !isHistoricalSyncing && supportsHistoricalSync
  }

  var canWriteHighFrequencyHistorySync: Bool {
    canSendHello && !isHistoricalSyncing && supportsSensorCommands
  }

  var debugResearchCommands: [GooseDebugCommandDefinition] {
    Self.debugResearchCommandDefinitions
  }

  var canWriteAlarm: Bool {
    canSendHello && !isHistoricalSyncing && supportsAlarmCommands && pendingAlarmCommand == nil
  }

  var canSyncClock: Bool {
    canSendHello
      && !isHistoricalSyncing
      && supportsClockCommands
      && pendingClockCommand == nil
      && pendingAlarmCommand == nil
  }

  var strapClockAutoSyncThresholdDisplay: String {
    "\(Int(Self.strapClockAutoSyncThresholdSeconds.rounded()))s"
  }

  var alarmDisplaySummary: String {
    if let lastAlarmScheduledAt {
      let time = Self.alarmTimeFormatter.string(from: lastAlarmScheduledAt)
      let slot = lastAlarmID.map { "slot \($0)" } ?? "WHOOP"
      return "\(time) | \(slot)"
    }
    return alarmCommandStatus
  }

  var alarmWriteSupportSummary: String {
    if activePeripheral == nil {
      return "Connect WHOOP first"
    }
    guard let commandCharacteristic else {
      return "Waiting for command characteristic"
    }
    if isHistoricalSyncing {
      return "Wait for historical sync to finish"
    }
    if pendingAlarmCommand != nil {
      return "Alarm command in flight"
    }
    if !supportsAlarmCommands {
      return "Alarm writes need command characteristic; active \(commandCharacteristic.uuid.uuidString)"
    }
    if !canSendHello {
      return "WHOOP connection is not ready"
    }
    return "Ready to write alarm to WHOOP"
  }

  var highFrequencyHistorySyncDisplaySummary: String {
    if highFrequencyHistorySyncActive, let expiresAt = highFrequencyHistorySyncExpiresAt {
      let time = Self.alarmTimeFormatter.string(from: expiresAt)
      return "Active until \(time)"
    }
    return highFrequencyHistorySyncStatus
  }

  var batteryChargeDisplayStatus: String {
    if batteryIsCharging == true {
      return "Charging"
    }
    if batteryIsCharging == false {
      return "Not charging"
    }
    return "Unknown"
  }

  var batterySettingsSummary: String {
    guard let batteryLevelPercent else {
      return "Unknown"
    }
    let status = batteryChargeDisplayStatus
    if batteryPowerStatus == "Unknown" || batteryPowerStatus == status {
      return "\(batteryLevelPercent)% | \(status)"
    }
    return "\(batteryLevelPercent)% | \(status) | \(batteryPowerStatus)"
  }

  var canReconnectRemembered: Bool {
    central?.state == .poweredOn && activePeripheral == nil && rememberedDeviceID != nil
  }

  var hasRememberedDevice: Bool {
    rememberedDeviceID != nil
  }

  var isReconnecting: Bool {
    reconnectState.contains("reconnecting")
  }

  var reconnectFailed: Bool {
    reconnectState.hasPrefix("failed")
  }

  var hrIsReconnecting: Bool {
    hrReconnectState.contains("reconnecting")
  }

  var hrReconnectFailed: Bool {
    hrReconnectState.hasPrefix("failed")
  }

  func updateHRReconnectState(_ value: String) {
    DispatchQueue.main.async { [weak self] in
      self?.hrReconnectState = value
    }
  }

  func stopHRReconnect() {
    hrMonitorManager.hrStopReconnect()
  }

  func retryHRReconnect() {
    hrMonitorManager.hrRetryReconnect()
  }

  init(startCentral: Bool = true) {
    super.init()
    bleUIStateAggregator.onSnapshot = { [weak self] snapshot in
      DispatchQueue.main.async {
        self?.applyBLEUIStateSnapshot(snapshot)
      }
    }
    bondingManager.onBondingStateChange = { [weak self] newState in
      guard let self else { return }
      self.updateConnectionState(newState.connectionStateString)
    }
    historicalManager.onSyncStateChange = { [weak self] _ in
      // @Observable tracks historicalManager.isHistoricalSyncing via the proxy var;
      // the callback exists for future explicit observers if needed.
    }
    historicalManager.onSyncCompleted = { [weak self] completedAt in
      guard let self else { return }
      Task { @MainActor in
        self.lastHistoricalSyncCompletedAt = completedAt
      }
    }
    historicalManager.onPacketCountChange = { [weak self] count in
      guard let self else { return }
      Task { @MainActor in
        self.historicalPacketCount = count
      }
    }
    loadRememberedDevice()
    loadPersistedBatterySample()
    loadPersistedRestingHeartRateEstimate()
    loadPersistedHRVSample()
    record(source: "app", title: "ble.init", body: "startCentral=\(startCentral)")
    record(
      source: "app",
      title: "physiology_capture.launch_config",
      body: "physiologyAutoStart=\(autoStartPhysiologyCaptureOnReady) prioritizeLive=\(prioritizeLiveCaptureOnReady) autoHistoricalSync=\(autoHistoricalSyncOnReady) debugSkinTemp=\(autoSendDebugSkinTemperatureCommand) args=\(ProcessInfo.processInfo.arguments.joined(separator: " "))"
    )
    if startCentral {
      if Self.canCreateCentralWithoutPrompt || prioritizeLiveCaptureOnReady || autoSendDebugSkinTemperatureCommand {
        ensureCentral()
      } else {
        updateBluetoothState()
        record(source: "ble", title: "central.create.deferred", body: Self.authorizationStateDescription)
      }
    }
    writeDebugCommandSnapshot()
  }

}
