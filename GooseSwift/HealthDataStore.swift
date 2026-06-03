import Darwin
import Foundation
import SwiftUI
import UIKit

@MainActor
final class HealthDataStore: ObservableObject {
  @Published var algorithmDefinitions: [HealthAlgorithmDefinition]
  @Published var referenceDefinitions: [HealthAlgorithmDefinition]
  @Published var selectedAlgorithmByFamily: [String: String]
  @Published var catalogStatus = "Metric catalog not loaded"
  @Published var catalogSource = HealthDataSource.unavailable("metric registry not loaded")
  @Published var packetInputStatus = "No run"
  @Published var packetScoreStatus = "No run"
  @Published var bandSleepImportStatus = "No band sync yet"
  @Published var externalSleepImportStatus = "External sleep imports disabled"
  @Published var referenceRunStatusByFamily: [String: String] = [:]
  @Published var primarySleepDetail: PrimarySleepDetail?

  // Apple Health fallback values — used when WHOOP packet data is unavailable
  @Published var hkRestingHR: Double?
  @Published var hkHRVRmssdMs: Double?
  @Published var hkRespiratoryRate: Double?
  @Published var hkSpO2Percent: Double?
  @Published var hkSkinTempDeltaC: Double?
  @Published var hkSteps: Int?
  @Published var hkActiveKcal: Double?
  @Published var hkWorkouts: [ActivityTimelineItem] = []
  @Published var hkImportStatus = "Not imported"
  // 90-day history for WHOOP-style baseline recovery scoring
  var hkHRVHistory: [(sdnn: Double, date: Date)] = []
  var hkRHRHistory: [(bpm: Double, date: Date)] = []

  @Published var calibrationTargetFamily = "recovery"
  @Published var calibrationLabelsImported = false
  @Published var calibrationRunComplete = false
  @Published var heartRateHourlyRanges: [HeartRateHourlyRange] = []
  @Published var heartRateTimelineStatus = "No HR samples stored"

  let bridge = GooseRustBridge()
  let heartRateSeriesStore = HeartRateSeriesStore.shared
  var attemptedCatalogLoad = false
  var previewMissingData = false
  var packetInputReports: [String: [String: Any]] = [:]
  var packetScoreReports: [String: [String: Any]] = [:]
  var referenceComparisonReports: [String: [String: Any]] = [:]
  var packetInputRefreshWorkItem: DispatchWorkItem?
  var packetInputRunID: UUID?
  var packetInputIsRunning = false
  var heartRateTimelineRefreshID: UUID?
  var heartRateSeriesUpdateObserver: NSObjectProtocol?
  let packetInputQueue = DispatchQueue(label: "com.goose.swift.health.packet-inputs", qos: .utility)
  let heartRateTimelineQueue = DispatchQueue(label: "com.goose.swift.health.heart-rate-timeline", qos: .utility)
  lazy var databasePath = HealthDataStore.defaultDatabasePath()

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
    refreshHeartRateTimeline()
    heartRateSeriesUpdateObserver = NotificationCenter.default.addObserver(
      forName: HeartRateSeriesStore.didUpdateNotification,
      object: nil,
      queue: .main
    ) { [weak self] _ in
      Task { @MainActor in
        self?.refreshHeartRateTimeline()
      }
    }
  }

  deinit {
    if let heartRateSeriesUpdateObserver {
      NotificationCenter.default.removeObserver(heartRateSeriesUpdateObserver)
    }
  }

  static func defaultDatabasePath() -> String {
    let baseDirectory = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
      ?? FileManager.default.temporaryDirectory
    let directory = baseDirectory.appendingPathComponent("GooseSwift", isDirectory: true)
    try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    return directory.appendingPathComponent("goose.sqlite").path
  }

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

  func loadBridgeCatalogsIfNeeded() {
    guard !attemptedCatalogLoad else {
      return
    }
    attemptedCatalogLoad = true
    refreshBridgeCatalogs()
  }

  func refreshPacketInputsIfNeeded() {
    guard packetInputReports.isEmpty, packetInputStatus == "No run" else {
      return
    }
    runPacketInputs()
  }

  func refreshHeartRateTimeline(for date: Date = Date()) {
    let refreshID = UUID()
    heartRateTimelineRefreshID = refreshID
    let store = heartRateSeriesStore
    heartRateTimelineQueue.async { [weak self] in
      let snapshot = store.timelineSnapshot(forDayContaining: date)
      Task { @MainActor in
        guard let self,
              self.heartRateTimelineRefreshID == refreshID else {
          return
        }
        self.heartRateHourlyRanges = snapshot.ranges
        self.heartRateTimelineStatus = snapshot.status
      }
    }
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
    packetInputRefreshWorkItem?.cancel()
    let workItem = DispatchWorkItem { [weak self] in
      self?.runPacketInputs()
    }
    packetInputRefreshWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.8, execute: workItem)
  }

  func refreshBridgeCatalogs() {
    do {
      let algorithmsValue = try bridge.requestValue(method: "metrics.built_in_definitions")
      let referencesValue = try bridge.requestValue(method: "metrics.reference_definitions")
      let preferencesValue = try bridge.requestValue(method: "metrics.default_preferences")

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
      algorithmDefinitions = []
      referenceDefinitions = []
      selectedAlgorithmByFamily = [:]
      catalogSource = .unavailable("Rust catalog unavailable")
      catalogStatus = "Metric catalog unavailable: \(Self.shortError(error))"
    }
  }

  func selectAlgorithm(_ algorithmID: String, for family: String) {
    selectedAlgorithmByFamily[family] = algorithmID
  }

  func runPacketInputs(completion: (() -> Void)? = nil) {
    guard !packetInputIsRunning else {
      packetInputStatus = "Packet-derived input extraction already running..."
      completion?()
      return
    }
    packetInputRefreshWorkItem?.cancel()
    let runID = UUID()
    packetInputRunID = runID
    packetInputIsRunning = true
    let databasePath = databasePath
    packetInputStatus = "Extracting packet-derived inputs..."

    packetInputQueue.async { [weak self] in
      let result = HealthDataStore.packetInputBridgeReports(databasePath: databasePath)
      DispatchQueue.main.async { [weak self] in
        guard let self, self.packetInputRunID == runID else {
          return
        }
        self.packetInputIsRunning = false
        switch result {
        case .success(let reports):
          self.packetInputReports = reports
          self.packetInputStatus = "Bridge packet-derived inputs extracted"
        case .failure(let error):
          self.packetInputStatus = "Bridge input extraction blocked: \(HealthDataStore.shortError(error))"
        }
        completion?()
      }
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

  func refreshSleepAfterBandSync(packetCount: Int) {
    bandSleepImportStatus = "Band sync captured \(packetCount) packets | extracting sleep inputs..."
    runPacketInputs { [weak self] in
      guard let self else {
        return
      }
      self.runSleepScore()
      self.bandSleepImportStatus = "Band sync captured \(packetCount) packets | \(self.packetScoreStatus)"
    }
  }
}
