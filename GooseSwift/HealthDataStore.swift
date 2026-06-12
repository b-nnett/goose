import Darwin
import Foundation
import Observation
import SwiftUI
import UIKit

@MainActor @Observable
final class HealthDataStore {
  var algorithmDefinitions: [HealthAlgorithmDefinition]
  var referenceDefinitions: [HealthAlgorithmDefinition]
  var selectedAlgorithmByFamily: [String: String]
  var catalogStatus = "Metric catalog not loaded"
  var catalogSource = HealthDataSource.unavailable("metric registry not loaded")
  var packetInputStatus = "No run"
  var packetScoreStatus = "No run"
  var bandSleepImportStatus = "A aguardar sincronização"
  var externalSleepImportStatus = "External sleep imports disabled"
  var referenceRunStatusByFamily: [String: String] = [:]
  var primarySleepDetail: PrimarySleepDetail?

  // Apple Health fallback values — used when WHOOP packet data is unavailable
  var hkRestingHR: Double?
  var hkHRVSDNNMs: Double?
  var hkRespiratoryRate: Double?
  var hkSpO2Percent: Double?
  var hkSkinTempDeltaC: Double?
  var hkSteps: Int?
  var hkActiveKcal: Double?
  var hkWorkouts: [ActivityTimelineItem] = []
  var hkImportStatus = "Not imported"
  // 90-day history for WHOOP-style baseline recovery scoring
  var hkHRVHistory: [(sdnn: Double, date: Date)] = []
  var hkRHRHistory: [(bpm: Double, date: Date)] = []

  var calibrationTargetFamily = "recovery"
  var calibrationLabelsImported = false
  var calibrationRunComplete = false
  var calibrationResult: [String: Any]?
  var energyDailyPersistStatus = "Not persisted"
  var heartRateHourlyRanges: [HeartRateHourlyRange] = []
  var heartRateTimelineStatus = "No HR samples stored"

  let bridge = GooseRustBridge()
  let heartRateSeriesStore = HeartRateSeriesStore.shared
  var attemptedCatalogLoad = false
  var previewMissingData = false
  var packetInputReports: [String: [String: Any]] = [:]
  var packetScoreReports: [String: [String: Any]] = [:]
  var referenceComparisonReports: [String: [String: Any]] = [:]
  var packetInputRefreshTask: Task<Void, Error>?
  var packetInputRunID: UUID?
  var packetInputIsRunning = false
  var heartRateTimelineRefreshID: UUID?
  @ObservationIgnored nonisolated(unsafe) var heartRateSeriesUpdateObserver: NSObjectProtocol?
  var databasePath: String

  // Cache for the 7-day rolling average strain computation (moved from extension — stored
  // properties are not allowed inside Swift extensions).
  var sevenDayStrainCache: (value: Double?, computedAt: Date)?

  // Recovery V1 result from metrics.goose_recovery_v1 (personal-baseline EWMA score).
  // Stored here because Swift extensions cannot add stored properties to @Observable classes.
  var recoveryV1Result: RecoveryV1Result?

  // Readiness Engine result from metrics.goose_readiness_v1 (ACWR + Foster monotony).
  // Stored here because Swift extensions cannot add stored properties to @Observable classes.
  var readinessResult: ReadinessResult?

  // 4-class sleep staging result from metrics.sleep_staging (Cole-Kripke + cardiorespiratory).
  // Stored here because Swift extensions cannot add stored properties to @Observable classes.
  var sleepStagingResult: SleepStagingResult?

  // V24 biometrics result (SpO2, skin temp, resp) from biometrics.v24_between.
  // Always quality_flag="uncalibrated". Stored here (extensions cannot add stored properties).
  var v24BiometricsResult: V24BiometricsResult?

  // Exercise sessions from the last 7 days, sorted newest-first.
  // Stored here (extensions cannot add stored properties to @Observable classes).
  var exerciseSessions: [ExerciseSessionDisplayItem] = []

  // IMU-derived step count from K10 gravity zero-crossing (imu_step_count_v1).
  // Stored here (extensions cannot add stored properties to @Observable classes).
  var imuStepCountResult: IMUStepCountResult?

  // Trends 7-day series cache (DATA-03)
  // Stored here because Swift extensions cannot add stored properties to @Observable classes.
  var recoveryTrendPoints: [(date: String, value: Double)] = []
  var hrvTrendPoints: [(date: String, value: Double)] = []
  var strainTrendPoints: [(date: String, value: Double)] = []

  static let liveHRVRMSSDDefaultsKey = "goose.swift.liveHRVRMSSD"
  static let liveHRVRRIntervalCountDefaultsKey = "goose.swift.liveHRVRRIntervalCount"
  static let liveHRVRMSSDSampleCountDefaultsKey = "goose.swift.liveHRVRMSSDSampleCount"
  static let liveHRVUpdatedAtDefaultsKey = "goose.swift.liveHRVUpdatedAt"
  static let liveHRVSourceDefaultsKey = "goose.swift.liveHRVSource"
  static let restingHeartRateEstimateBPMDefaultsKey = "goose.swift.restingHeartRateEstimateBPM"
  static let restingHeartRateEstimateSampleCountDefaultsKey = "goose.swift.restingHeartRateEstimateSampleCount"
  static let restingHeartRateEstimateUpdatedAtDefaultsKey = "goose.swift.restingHeartRateEstimateUpdatedAt"
  static let restingHeartRateEstimateSourceDefaultsKey = "goose.swift.restingHeartRateEstimateSource"

  init() {
    algorithmDefinitions = []
    referenceDefinitions = []
    selectedAlgorithmByFamily = [:]
    primarySleepDetail = nil
    databasePath = HealthDataStore.defaultDatabasePath()
    Task { await self.refreshHeartRateTimeline() }
    heartRateSeriesUpdateObserver = NotificationCenter.default.addObserver(
      forName: HeartRateSeriesStore.didUpdateNotification,
      object: nil,
      queue: .main
    ) { [weak self] _ in
      Task { @MainActor [weak self] in
        await self?.refreshHeartRateTimeline()
      }
    }
  }

  deinit {
    if let heartRateSeriesUpdateObserver {
      NotificationCenter.default.removeObserver(heartRateSeriesUpdateObserver)
    }
  }

  nonisolated static func defaultDatabasePath() -> String {
    _sharedDatabasePath
  }

  private nonisolated static let _sharedDatabasePath: String = {
    let baseDirectory = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
      ?? FileManager.default.temporaryDirectory
    let directory = baseDirectory.appendingPathComponent("GooseSwift", isDirectory: true)
    try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    return directory.appendingPathComponent("goose.sqlite").path
  }()

  var usesSampleData: Bool {
    false
  }

  var localDataSupportsExport: Bool {
    !packetInputReports.isEmpty || !packetScoreReports.isEmpty || !referenceComparisonReports.isEmpty
  }

  var localHealthExportText: String {
    [
      "Goose Health Export",
      "Catalog: \(catalogStatus)",
      "Band sleep import: \(bandSleepImportStatus)",
      "HealthKit metric import: disabled; profile weight only",
      "Packet inputs: \(packetInputStatus)",
      "Packet scores: \(packetScoreStatus)",
      "Readiness: \(metricInputReadinessSummary())",
      "Sleep: \(sleepFeatureScoreSummary())",
      "Recovery: \(recoveryFeatureScoreSummary())",
      "Strain: \(strainFeatureScoreSummary())",
      "Stress: \(stressFeatureScoreSummary())",
    ].joined(separator: "\n")
  }

  func loadBridgeCatalogsIfNeeded() async {
    guard !attemptedCatalogLoad else {
      return
    }
    attemptedCatalogLoad = true
    await refreshBridgeCatalogs()
  }

  func refreshPacketInputsIfNeeded() {
    guard packetInputReports.isEmpty, packetInputStatus == "No run" else {
      return
    }
    Task { await self.runPacketInputs() }
  }

  func refreshHeartRateTimeline(for date: Date = Date()) async {
    let refreshID = UUID()
    heartRateTimelineRefreshID = refreshID
    let store = heartRateSeriesStore
    let snapshot = store.timelineSnapshot(forDayContaining: date)
    guard heartRateTimelineRefreshID == refreshID else {
      return
    }
    heartRateHourlyRanges = snapshot.ranges
    heartRateTimelineStatus = snapshot.status
  }

  func heartRateHourlyTimelineRows(maxRows: Int = 8) -> [HealthSummaryRow] {
    let ranges = Array(heartRateHourlyRanges.suffix(maxRows)).reversed()
    guard !ranges.isEmpty else {
      return []
    }

    return ranges.map { range in
      let hour = range.hourStart.formatted(.dateTime.hour(.twoDigits(amPM: .abbreviated)))
      return HealthSummaryRow(
        "HR \(hour)",
        value: "\(range.minBPM)-\(range.maxBPM) bpm | avg \(range.averageBPM) | \(range.sampleCount) samples",
        source: .live("BLE heart-rate sample store"),
        systemImage: "heart"
      )
    }
  }

  func refreshPacketInputsAfterCapture() {
    packetInputRefreshTask?.cancel()
    packetInputRefreshTask = Task { [weak self] in
      try await Task.sleep(for: .seconds(0.8))
      guard let self, !Task.isCancelled else { return }
      await self.runPacketInputs()
    }
  }

  func refreshBridgeCatalogs() async {
    catalogStatus = "Loading bridge catalog..."
    do {
      let algorithmsValue = try await bridge.requestValueAsync(method: "metrics.built_in_definitions")
      let referencesValue = try await bridge.requestValueAsync(method: "metrics.reference_definitions")
      let preferencesValue = try await bridge.requestValueAsync(method: "metrics.default_preferences")

      let parsedAlgorithms = Self.algorithmRows(from: algorithmsValue)
        .map { HealthAlgorithmDefinition(row: $0, source: .bridge("metrics.built_in_definitions")) }
      let parsedReferences = Self.algorithmRows(from: referencesValue)
        .map { HealthAlgorithmDefinition(row: $0, source: .bridge("metrics.reference_definitions")) }
      let parsedPreferences = Self.preferenceRows(from: preferencesValue)

      if !parsedAlgorithms.isEmpty {
        algorithmDefinitions = parsedAlgorithms
      }
      if !parsedReferences.isEmpty {
        referenceDefinitions = parsedReferences
      }
      if !parsedPreferences.isEmpty {
        selectedAlgorithmByFamily = parsedPreferences
      } else {
        selectedAlgorithmByFamily = Dictionary(
          uniqueKeysWithValues: algorithmDefinitions.map { ($0.family, $0.id) }
        )
      }
      catalogSource = .bridge("Rust metric registry")
      catalogStatus = "Bridge catalog loaded"
    } catch {
      let shortErr = Self.shortError(error)
      algorithmDefinitions = []
      referenceDefinitions = []
      selectedAlgorithmByFamily = [:]
      catalogSource = .unavailable("Rust catalog unavailable")
      catalogStatus = "Metric catalog unavailable: \(shortErr)"
    }
  }

  func selectAlgorithm(_ algorithmID: String, for family: String) {
    selectedAlgorithmByFamily[family] = algorithmID
  }

  // HALG-01: Named shims over existing catalog state — wired to bridge catalog loaded by loadBridgeCatalogsIfNeeded().
  var algorithmPreferences: [String: String] { selectedAlgorithmByFamily }
  var referenceAlgorithmDefinitions: [HealthAlgorithmDefinition] { referenceDefinitions }

  func runPacketInputs() async {
    guard !packetInputIsRunning else {
      packetInputStatus = "Packet-derived input extraction already running..."
      return
    }
    packetInputRefreshTask?.cancel()
    let runID = UUID()
    packetInputRunID = runID
    packetInputIsRunning = true
    let databasePath = databasePath
    packetInputStatus = "Extracting packet-derived inputs..."

    let result = await HealthDataStore.packetInputBridgeReports(databasePath: databasePath)
    guard packetInputRunID == runID else {
      return
    }
    packetInputIsRunning = false
    switch result {
    case .success(let reports):
      packetInputReports = reports
      packetInputStatus = "Bridge packet-derived inputs extracted"
    case .failure(let error):
      packetInputStatus = "Bridge input extraction blocked: \(HealthDataStore.shortError(error))"
    }
  }

  func markBandSleepSyncRequested(automatic: Bool, canSync: Bool, detail: String) {
    if canSync {
      bandSleepImportStatus = automatic ? "Auto-syncing band sleep packets..." : "Syncing band sleep packets..."
    } else {
      bandSleepImportStatus = "Band sync unavailable: \(detail)"
    }
  }

  func markBandSleepSyncFailed(_ detail: String) {
    bandSleepImportStatus = "Band sync failed: \(detail)"
  }

  func refreshSleepAfterBandSync(packetCount: Int) async {
    bandSleepImportStatus = "Band sync captured \(packetCount) packets | extracting sleep inputs..."
    await runPacketInputs()
    await runSleepScore()
    await runSleepStaging()
    bandSleepImportStatus = "Band sync captured \(packetCount) packets | \(packetScoreStatus)"
  }
}
