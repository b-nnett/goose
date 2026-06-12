import Foundation
import CryptoKit
import SwiftUI
import UIKit

#if canImport(HealthKit)
import HealthKit
#endif

@MainActor
final class MoreDataStore: ObservableObject {
  @Published var routeStatus = MoreRouteStatus(
    profile: .pending,
    device: .pending,
    hrMonitor: .pending,
    connectionLab: .pending,
    capture: .pending,
    localStore: .pending,
    healthSync: .pending,
    rawExport: .pending,
    algorithms: .ready,
    debug: .pending,
    privacy: .pending,
    remoteServer: .pending,
    support: .pending,
    about: .ready,
    developer: .pending,
    breathe: .ready,
    intervalTimer: .ready,
    metricExplorer: .ready
  )

  @Published var databasePath: String
  @Published var storageStatus = "Not checked"
  @Published var storageNextAction = "Run Check after Goose has created the local database"
  @Published var schemaVersion = "Unknown"

  @Published var captureSessionID: String?
  @Published var captureSessionStartedAt: Date?
  @Published var captureFrameCount = 0
  @Published var captureStatus = "No capture session"
  @Published var liveCaptureStatus = "Connect a device to mirror notifications into capture"
  @Published var captureImportStatus = "Waiting for a document picker bridge"
  @Published var commandEvidenceStatus = "Waiting for a command evidence file"
  @Published var emulatorLogStatus = "Waiting for an emulator log"
  @Published var localFrameMatchStatus = "Waiting for imported frames"
  @Published var validatedCommandStatus = "Waiting for command validation samples"
  @Published var recentCaptureSessions: [String] = []

  @Published var healthBackfillStart: String
  @Published var healthBackfillEnd: String
  @Published var selectedHealthFamilies: Set<String> = []
  @Published var healthAdapterStatus = "Not refreshed"
  @Published var healthAuthorizationStatus = "Not requested in More"
  @Published var existingGooseRecordsStatus = "No local database checked"
  @Published var importedSleepHistoryStatus = "No imported sleep history loaded"
  @Published var healthSyncReports: [String] = ["No dry run yet"]

  @Published var rawExportStart: String
  @Published var rawExportEnd: String
  @Published var rawCaptureSessions = ""
  @Published var rawPacketTypes = ""
  @Published var rawSensorSignals = ""
  @Published var rawMetricFamilies = "heart_rate,hrv,activity"
  @Published var rawAlgorithmIDs = ""
  @Published var rawAlgorithmVersions = ""
  @Published var includeRawBytes = true
  @Published var selectedRawFamilies: Set<String> = ["raw_evidence", "decoded_frames", "packet_timeline", "sensor_samples", "metric_features", "metric_outputs", "algorithm_runs", "local_health_metrics", "sqlite"]
  @Published var rawExportStatus = "No export yet"
  @Published var rawExportInProgress = false
  @Published var rawBundlePath = "No bundle"
  @Published var rawZipPath = "No zip"
  @Published var rawZipURL: URL?
  @Published var rawRowCounts = "No rows"
  @Published var rawValidationManifestStatus = "No validation manifest"
  @Published var rawValidationManifestURL: URL?
  @Published var rawValidationReviewStatus = "No validation review"
  @Published var rawValidationReviewURL: URL?
  @Published var rawValidationRunbookStatus = "No validation runbook"
  @Published var rawValidationRunbookURL: URL?
  @Published var rawBundleValidation = "Not validated"
  @Published var rawZipValidation = "Not validated"
  @Published var privacyLintStatus = "Not linted"
  @Published var sanitizedPrivacyStatus = "No sanitized copy"
  @Published var localExportStatus = "No local export"
  @Published var localExportInProgress = false
  @Published var localExportURL: URL?
  @Published var localExportManifestURL: URL?

  @Published var algorithmPreferenceStatus = "Local selection only"

  @Published var coreVersionStatus = "Rust bridge not checked"
  @Published var frameParseStatus = "No parser probe run"
  @Published var frameCRCStatus = "CRC pending"
  @Published var framePayloadStatus = "Payload pending"
  @Published var frameWarningsStatus = "Warnings pending"
  @Published var frameTimelineStatus = "Timeline pending"
  @Published var debugWebSocketStatus = "Not started"
  @Published var debugNextAction = "Start a local debug session"
  @Published var uiCoverageStatus = "No audit run"
  @Published var deferredSurfaceStatus = "Deferred surfaces unknown"
  @Published var propertySuiteStatus = "No property suite run"
  @Published var perfBudgetStatus = "No perf budget run"
  @Published var commandEvidenceImportStatus = "No command evidence imported"
  @Published var commandGateSweepStatus = "No gate sweep run"
  @Published var commandCapturePlanStatus = "No capture plan generated"
  @Published var commandGroups: [MoreCommandGroup] = MoreCommandGroup.defaults
  @Published var destructiveGateStatus = "Locked"

  @Published var supportBundlePath: String
  @Published var logExportStatus = "Logs remain in the app event stream"
  @Published var deletionStatus = "Deletion bridge not wired"

  let bridge = GooseRustBridge()
  let outputDirectory: String

  struct RawExportArtifactValidationResult {
    let bundleValidation: String
    let zipValidation: String
    let privacyLint: String
    let sanitizedPrivacy: String
  }

  struct RawValidationSidecarResult {
    let manifestStatus: String
    let manifestURL: URL?
    let reviewStatus: String
    let reviewURL: URL?
    let runbookStatus: String
    let runbookURL: URL?
  }
  var debugSessionID = "swift-more-\(UUID().uuidString)"

  init(databasePath: String? = nil) {
    let appDirectory = MoreDataStore.applicationDirectory()
    let documentsDirectory = MoreDataStore.documentsApplicationDirectory()
    self.databasePath = databasePath ?? appDirectory.appendingPathComponent("goose.sqlite").path
    outputDirectory = documentsDirectory.appendingPathComponent("Exports", isDirectory: true).path
    supportBundlePath = documentsDirectory.appendingPathComponent("Support", isDirectory: true).path

    let now = Date()
    let start = Self.fullExportStart
    let end = now.moreISO8601String()
    healthBackfillStart = start
    healthBackfillEnd = end
    rawExportStart = start
    rawExportEnd = end
  }

  func refreshRouteStatus(ble: GooseBLEClient, model: GooseAppModel) {
    let newStatus = MoreRouteStatus(
      profile: OnboardingProfileSnapshot().hasRequiredDetails ? .ready : .pending,
      device: ble.connectionState == "ready" ? .ready : .pending,
      hrMonitor: ble.hrConnectionState == "connected" ? .ready : .pending,
      connectionLab: model.helloSummary.hasPrefix("GET_HELLO") ? .ready : .pending,
      capture: captureSessionID == nil ? .pending : .ready,
      localStore: databaseExists ? .ready : .unavailable,
      healthSync: healthSyncBackfillWindowIssueSummary() == nil ? .pending : .blocked,
      rawExport: rawExportWindowIssueSummary() == nil ? (databaseExists ? .pending : .unavailable) : .blocked,
      algorithms: .ready,
      debug: coreVersionStatus.hasPrefix("Rust core") ? .ready : .pending,
      privacy: privacyLintStatus == "Not linted" ? .pending : .ready,
      remoteServer: (UserDefaults.standard.string(forKey: RemoteServerStorage.serverURL)?.isEmpty ?? true) ? .pending : .ready,
      support: .pending,
      about: .ready,
      developer: .pending,
      breathe: .ready,
      intervalTimer: .ready,
      metricExplorer: .ready
    )
    routeStatus = newStatus
  }

  func refreshBridgeStatus(model: GooseAppModel) {
    coreVersionStatus = model.rustStatus
    guard schemaVersion == "Unknown" || coreVersionStatus == "Rust bridge not checked" else {
      return
    }
    do {
      let value = try bridge.request(method: "core.version")
      let version = value["core_version"] as? String ?? "unknown"
      let schema = value["storage_schema_version"].map(Self.stringValue) ?? "unknown"
      coreVersionStatus = "Rust core \(version)"
      schemaVersion = schema
    } catch {
      coreVersionStatus = "Rust bridge unavailable"
    }
  }

  func refreshHealthAdapter() {
#if canImport(HealthKit)
    healthAdapterStatus = HKHealthStore.isHealthDataAvailable() ? "Apple Health profile autofill available" : "Apple Health unavailable on this device"
#else
    healthAdapterStatus = "HealthKit framework unavailable"
#endif
    healthAuthorizationStatus = "Reads body mass only for profile autofill"
  }

  func refreshRecentCaptureSessions() {
    let nowMs = Self.unixMilliseconds(Date())
    let thirtyDaysMs: Int64 = 30 * 24 * 60 * 60 * 1_000
    do {
      let value = try bridge.request(
        method: "capture.list_sessions",
        args: [
          "database_path": databasePath,
          "start_unix_ms": nowMs - thirtyDaysMs,
          "end_unix_ms": nowMs,
        ]
      )
      recentCaptureSessions = Self.captureSessionSummaries(from: value)
    } catch {
      if recentCaptureSessions.isEmpty {
        recentCaptureSessions = ["No stored capture sessions"]
      }
    }
  }

  func captureSessionSummary() -> String {
    if let captureSessionID, let captureSessionStartedAt {
      return "Active \(captureSessionID.prefix(8)) since \(captureSessionStartedAt.formatted(date: .omitted, time: .shortened)); \(captureFrameCount) frames"
    }
    return captureStatus
  }

  func liveNotificationCaptureSummary(ble: GooseBLEClient) -> String {
    if ble.connectionState == "ready" {
      return "Ready; notifications are mirrored through GooseBLEClient.onNotification"
    }
    if ble.isScanning {
      return "Scanning; capture starts after connection"
    }
    return liveCaptureStatus
  }

  func startCapture(ble: GooseBLEClient) {
    guard captureSessionID == nil else {
      captureStatus = "Capture already active"
      return
    }

    let sessionID = "swift-capture-\(UUID().uuidString)"
    let now = Date()
    do {
      var args: [String: Any] = [
        "database_path": databasePath,
        "session_id": sessionID,
        "source": "ios_swift_more",
        "started_at_unix_ms": Self.unixMilliseconds(now),
        "device_model": ble.modelNumber ?? ble.activeDeviceName,
        "provenance": [
          "surface": "MoreCaptureView",
          "connection_state": ble.connectionState,
        ],
      ]
      if let activeDeviceID = ble.activeDeviceIdentifier?.uuidString {
        args["active_device_id"] = activeDeviceID
      }

      let value = try bridge.request(
        method: "capture.start_session",
        args: args
      )
      captureSessionID = sessionID
      captureSessionStartedAt = now
      captureFrameCount = 0
      captureStatus = "Started \(Self.shortBridgeSummary(value))"
      refreshRecentCaptureSessions()
    } catch {
      captureStatus = "Start failed: \(Self.errorSummary(error))"
    }
  }

  func stopCapture() {
    guard let sessionID = captureSessionID else {
      captureStatus = "No capture session to stop"
      return
    }

    do {
      let value = try bridge.request(
        method: "capture.finish_session",
        args: [
          "database_path": databasePath,
          "session_id": sessionID,
          "ended_at_unix_ms": Self.unixMilliseconds(Date()),
          "frame_count": captureFrameCount,
        ]
      )
      captureStatus = "Finished \(Self.shortBridgeSummary(value))"
      captureSessionID = nil
      captureSessionStartedAt = nil
      captureFrameCount = 0
      refreshRecentCaptureSessions()
    } catch {
      captureStatus = "Finish failed: \(Self.errorSummary(error))"
    }
  }

  func markFileActionUnavailable(_ kind: MoreFileActionKind) {
    switch kind {
    case .captureFile:
      captureImportStatus = "Document picker bridge is not wired in Swift yet"
    case .commandEvidence:
      commandEvidenceStatus = "Command evidence import requires a selected JSON file"
      commandEvidenceImportStatus = "Blocked until a file picker provides evidence JSON"
    case .emulatorLog:
      emulatorLogStatus = "Emulator log import requires a selected log file"
    case .localFrameMatch:
      localFrameMatchStatus = "Blocked until imported frames exist in the local store"
    case .validatedCommand:
      validatedCommandStatus = "Blocked until command validation records are imported"
    }
  }

  func storageCheckStatusSummary() -> String {
    storageStatus
  }

  func storageCheckNextActionSummary() -> String {
    storageNextAction
  }

  var databaseExists: Bool {
    FileManager.default.fileExists(atPath: databasePath)
  }

  func runStorageCheck() {
    guard databaseExists else {
      storageStatus = "Unavailable; no database at path"
      storageNextAction = "Start capture or run another bridge flow that creates goose.sqlite"
      existingGooseRecordsStatus = "No Goose records"
      return
    }

    do {
      let value = try bridge.request(
        method: "storage.check",
        args: [
          "database_path": databasePath,
          "self_test": true,
        ]
      )
      storageStatus = Self.passSummary(value, fallback: Self.shortBridgeSummary(value))
      storageNextAction = Self.nextActionSummary(value, fallback: "Review any failed checks before exporting")
      if let schema = value["schema_version"].map(Self.stringValue) {
        schemaVersion = schema
      }
      existingGooseRecordsStatus = Self.recordCountSummary(value)
    } catch {
      storageStatus = "Check failed: \(Self.errorSummary(error))"
      storageNextAction = "Inspect the local database path and rerun Check"
    }
  }

  func healthSyncBackfillWindowSummary() -> String {
    "\(healthBackfillStart) to \(healthBackfillEnd)"
  }

  func healthSyncBackfillWindowIssueSummary() -> String? {
    guard let start = Self.parseISO8601(healthBackfillStart) else {
      return "Start must be ISO-8601"
    }
    guard let end = Self.parseISO8601(healthBackfillEnd) else {
      return "End must be ISO-8601"
    }
    guard start < end else {
      return "Start must be before end"
    }
    return nil
  }

  func healthSyncMetricFamilySummary() -> String {
    "No metric families: Apple Health is profile-only"
  }

  func healthSyncMetricSourceSummary(_ family: String) -> String {
    switch family {
    case "weight": "Apple Health bodyMass profile autofill"
    default: "No source registered"
    }
  }

  func unavailableHealthSyncMetricSummary() -> String {
    Self.unavailableHealthFamilies.joined(separator: ", ")
  }

  func setHealthFamily(_ family: String, enabled: Bool) {
    if enabled {
      selectedHealthFamilies.insert(family)
    } else {
      selectedHealthFamilies.remove(family)
    }
  }

  var canRunAppleHealthDryRun: Bool {
    false
  }

  func runAppleHealthDryRun() {
    healthSyncReports = ["Apple Health metric sync disabled; Goose metrics must come from WHOOP packets or local estimates."]
  }

  func markHealthConnectUnavailable() {
    healthSyncReports = ["Health Connect dry run is unavailable in the iOS Swift target"]
  }

  func rawExportWindowSummary() -> String {
    "\(rawExportStart) to \(rawExportEnd)"
  }

  func rawExportWindowIssueSummary() -> String? {
    guard let start = Self.parseISO8601(rawExportStart) else {
      return "Start must be ISO-8601"
    }
    guard let end = Self.parseISO8601(rawExportEnd) else {
      return "End must be ISO-8601"
    }
    guard start < end else {
      return "Start must be before end"
    }
    return nil
  }

  func rawExportScopeSummary() -> String {
    if selectedRawFamilies.isEmpty {
      return "No data families selected"
    }
    return selectedRawFamilies.sorted().joined(separator: ", ")
  }

  func setRawFamily(_ family: String, enabled: Bool) {
    if enabled {
      selectedRawFamilies.insert(family)
    } else {
      selectedRawFamilies.remove(family)
    }
  }

  var canRunRawExport: Bool {
    databaseExists && rawExportWindowIssueSummary() == nil && !selectedRawFamilies.isEmpty
  }

  func runRawExport() {
    guard canRunRawExport else {
      rawExportStatus = rawExportWindowIssueSummary() ?? "No database or data family selected"
      return
    }

    guard !rawExportInProgress else {
      rawExportStatus = "Export already running"
      return
    }

    do {
      try FileManager.default.createDirectory(atPath: outputDirectory, withIntermediateDirectories: true)
    } catch {
      rawExportStatus = "Export failed: \(Self.errorSummary(error))"
      return
    }

    let zipPath = URL(fileURLWithPath: outputDirectory)
      .appendingPathComponent("goose-raw-export-\(Int(Date().timeIntervalSince1970)).zip")
      .path
    let args: [String: Any] = [
      "database_path": databasePath,
      "output_dir": outputDirectory,
      "zip_output_path": zipPath,
      "start": rawExportStart,
      "end": rawExportEnd,
      "app_version": Self.appVersion,
      "core_version": coreVersionStatus,
      "include_sqlite": selectedRawFamilies.contains("sqlite"),
      "data_families": Array(selectedRawFamilies).sorted(),
      "include_raw_bytes": includeRawBytes,
      "capture_session_ids": Self.csvValues(rawCaptureSessions),
      "packet_type_names": Self.csvValues(rawPacketTypes),
      "sensor_source_signals": Self.csvValues(rawSensorSignals),
      "metric_families": Self.csvValues(rawMetricFamilies),
      "algorithm_ids": Self.csvValues(rawAlgorithmIDs),
      "algorithm_versions": Self.csvValues(rawAlgorithmVersions),
    ]
    let validationManifestBaseArgs: [String: Any] = [
      "database_path": databasePath,
      "manifest_id": "local-health-\(Int(Date().timeIntervalSince1970))",
      "timezone": TimeZone.current.identifier,
      "start": rawExportStart,
      "end": rawExportEnd,
    ]

    rawExportInProgress = true
    rawExportStatus = "Saving export..."
    rawZipPath = zipPath
    rawZipURL = nil
    rawValidationManifestStatus = "Generating after export..."
    rawValidationManifestURL = nil
    rawValidationReviewStatus = "Reviewing after export..."
    rawValidationReviewURL = nil
    rawValidationRunbookStatus = "Generating after export..."
    rawValidationRunbookURL = nil
    rawBundleValidation = "Not validated"
    rawZipValidation = "Not validated"
    privacyLintStatus = "Not linted"
    sanitizedPrivacyStatus = "No sanitized copy"

    DispatchQueue.global(qos: .userInitiated).async {
      do {
        let bridge = GooseRustBridge()
        let value = try bridge.request(method: "export.raw_timeframe", args: args)
        let bundlePath = Self.firstString(value, keys: ["bundle_path", "output_dir", "path"]) ?? self.outputDirectory
        let finishedZipPath = Self.firstString(value, keys: ["zip_output_path", "zip_path"]) ?? zipPath
        let rowCounts = Self.rowCountSummary(value)
        let validationSidecars: RawValidationSidecarResult
        do {
          var validationManifestArgs = validationManifestBaseArgs
          validationManifestArgs["database_source_kind"] = "raw_export_directory"
          validationManifestArgs["window_source"] = "raw_export_manifest"
          validationManifestArgs["raw_export_bundle_path"] = bundlePath
          let manifest = try bridge.request(method: "validation.local_health_manifest_scaffold", args: validationManifestArgs)
          let review = try bridge.request(
            method: "validation.local_health_manifest_review",
            args: ["manifest": manifest]
          )
          let runbookMarkdown = try Self.rawValidationRunbookMarkdown(
            bridge: bridge,
            manifest: manifest
          )
          validationSidecars = try Self.writeRawValidationSidecars(
            manifest,
            review: review,
            reviewStatus: Self.rawValidationReviewSummary(review),
            runbookMarkdown: runbookMarkdown,
            bundlePath: bundlePath,
            outputDirectory: self.outputDirectory
          )
        } catch {
          let message = Self.errorSummary(error)
          validationSidecars = RawValidationSidecarResult(
            manifestStatus: "Manifest failed: \(message)",
            manifestURL: nil,
            reviewStatus: "Review failed: \(message)",
            reviewURL: nil,
            runbookStatus: "Runbook failed: \(message)",
            runbookURL: nil
          )
        }
        let artifactValidation = Self.validateRawExportArtifacts(
          bridge: bridge,
          bundlePath: bundlePath,
          zipPath: finishedZipPath
        )
        DispatchQueue.main.async {
          let status = Self.passSummary(value, fallback: "Export completed")
          self.rawExportInProgress = false
          self.rawExportStatus = status
          self.rawBundlePath = bundlePath
          self.rawZipPath = finishedZipPath
          self.rawZipURL = URL(fileURLWithPath: finishedZipPath)
          self.rawRowCounts = rowCounts
          self.rawValidationManifestStatus = validationSidecars.manifestStatus
          self.rawValidationManifestURL = validationSidecars.manifestURL
          self.rawValidationReviewStatus = validationSidecars.reviewStatus
          self.rawValidationReviewURL = validationSidecars.reviewURL
          self.rawValidationRunbookStatus = validationSidecars.runbookStatus
          self.rawValidationRunbookURL = validationSidecars.runbookURL
          self.rawBundleValidation = artifactValidation.bundleValidation
          self.rawZipValidation = artifactValidation.zipValidation
          self.privacyLintStatus = artifactValidation.privacyLint
          self.sanitizedPrivacyStatus = artifactValidation.sanitizedPrivacy
        }
      } catch {
        DispatchQueue.main.async {
          let message = Self.errorSummary(error)
          self.rawExportInProgress = false
          self.rawExportStatus = "Export failed: \(message)"
          self.rawValidationManifestStatus = "No validation manifest"
          self.rawValidationReviewStatus = "No validation review"
          self.rawValidationRunbookStatus = "No validation runbook"
        }
      }
    }
  }

}
