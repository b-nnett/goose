import Foundation
import HealthKit

struct HealthKitProfileAutofill {
  let weightGrams: Int?
  let sourceSummary: String

  static let empty = HealthKitProfileAutofill(
    weightGrams: nil,
    sourceSummary: "No weight samples found"
  )

  var hasValues: Bool {
    weightGrams != nil
  }
}

struct HealthKitProfileImportResult {
  let status: String
  let autofill: HealthKitProfileAutofill
}

enum HealthKitProfileImporter {
  static var readTypes: Set<HKObjectType> {
    var types = Set<HKObjectType>()
    if let bodyMassType = HKObjectType.quantityType(forIdentifier: .bodyMass) {
      types.insert(bodyMassType)
    }
    return types
  }

  static func requestProfileAccess() async -> HealthKitProfileImportResult {
    await requestAccess()
  }

  private static func requestAccess() async -> HealthKitProfileImportResult {
    guard HKHealthStore.isHealthDataAvailable() else {
      return HealthKitProfileImportResult(status: "Unavailable on this device", autofill: .empty)
    }

    let store = HKHealthStore()
    do {
      try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
        store.requestAuthorization(toShare: Set<HKSampleType>(), read: Self.readTypes) { success, error in
          if let error {
            continuation.resume(throwing: error)
          } else if success {
            continuation.resume()
          } else {
            continuation.resume(throwing: HealthKitProfileImporterError.authorizationDenied)
          }
        }
      }
      let autofill = await latestMeasurements(store: store)
      return HealthKitProfileImportResult(
        status: autofill.hasValues ? "Requested in Health; \(autofill.sourceSummary)" : "Requested in Health",
        autofill: autofill
      )
    } catch {
      return HealthKitProfileImportResult(status: "Failed: \(error.localizedDescription)", autofill: .empty)
    }
  }

  static func latestMeasurements(store: HKHealthStore = HKHealthStore()) async -> HealthKitProfileAutofill {
    guard HKHealthStore.isHealthDataAvailable() else {
      return .empty
    }

    async let weightSample = latestQuantitySample(
      store: store,
      type: HKObjectType.quantityType(forIdentifier: .bodyMass)
    )

    let latestWeightSample = await weightSample

    let weight = latestWeightSample.flatMap { sample -> Int? in
      let kilograms = sample.quantity.doubleValue(for: HKUnit.gramUnit(with: .kilo))
      guard kilograms > 0 else {
        return nil
      }
      return Int((kilograms * 1000).rounded())
    }

    var filled: [String] = []
    if weight != nil {
      filled.append("weight")
    }

    return HealthKitProfileAutofill(
      weightGrams: weight,
      sourceSummary: filled.isEmpty ? "No weight samples found" : "Filled \(filled.joined(separator: " and ")) from Apple Health"
    )
  }

  private static func latestQuantitySample(store: HKHealthStore, type: HKQuantityType?) async -> HKQuantitySample? {
    guard let type else {
      return nil
    }
    return await withCheckedContinuation { continuation in
      let sort = NSSortDescriptor(key: HKSampleSortIdentifierEndDate, ascending: false)
      let query = HKSampleQuery(sampleType: type, predicate: nil, limit: 1, sortDescriptors: [sort]) { _, samples, _ in
        continuation.resume(returning: samples?.first as? HKQuantitySample)
      }
      store.execute(query)
    }
  }
}

enum HealthKitProfileImporterError: LocalizedError {
  case authorizationDenied

  var errorDescription: String? {
    "Health access was not allowed."
  }
}

enum HealthKitSleepImporter {
  enum ImportResult {
    case success(PrimarySleepDetail)
    case noData(String)
    case denied(String)
    case unavailable
  }

  static func importMostRecentSleep() async -> ImportResult {
    guard HKHealthStore.isHealthDataAvailable() else {
      return .unavailable
    }
    let store = HKHealthStore()
    guard let sleepType = HKObjectType.categoryType(forIdentifier: .sleepAnalysis) else {
      return .noData("Sleep analysis type unavailable")
    }
    do {
      try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
        store.requestAuthorization(toShare: [], read: [sleepType]) { ok, err in
          if let err { cont.resume(throwing: err) }
          else if ok { cont.resume() }
          else { cont.resume(throwing: HealthKitProfileImporterError.authorizationDenied) }
        }
      }
    } catch {
      return .denied(error.localizedDescription)
    }
    let samples = await fetchSleepSamples(store: store)
    guard !samples.isEmpty else {
      return .noData("No sleep samples found in Apple Health")
    }
    guard let detail = buildSleepDetail(from: samples) else {
      return .noData("Could not parse sleep samples")
    }
    return .success(detail)
  }

  private static func fetchSleepSamples(store: HKHealthStore) async -> [HKCategorySample] {
    await withCheckedContinuation { cont in
      guard let sleepType = HKObjectType.categoryType(forIdentifier: .sleepAnalysis) else {
        cont.resume(returning: [])
        return
      }
      let cutoff = Calendar.current.date(byAdding: .day, value: -2, to: Date()) ?? Date()
      let predicate = HKQuery.predicateForSamples(withStart: cutoff, end: Date(), options: .strictStartDate)
      let sort = NSSortDescriptor(key: HKSampleSortIdentifierStartDate, ascending: true)
      let query = HKSampleQuery(sampleType: sleepType, predicate: predicate, limit: 500, sortDescriptors: [sort]) { _, samples, _ in
        cont.resume(returning: (samples as? [HKCategorySample]) ?? [])
      }
      store.execute(query)
    }
  }

  private static func buildSleepDetail(from samples: [HKCategorySample]) -> PrimarySleepDetail? {
    // Group samples into sessions; gaps > 90 min = new session. Use the longest.
    let asleep = samples.filter { $0.value != HKCategoryValueSleepAnalysis.awake.rawValue }
    guard !asleep.isEmpty else { return nil }

    var bestSession: [HKCategorySample] = []
    var current: [HKCategorySample] = [asleep[0]]
    for sample in asleep.dropFirst() {
      let gap = sample.startDate.timeIntervalSince(current.last!.endDate)
      if gap < 90 * 60 {
        current.append(sample)
      } else {
        if current.sessionDuration > bestSession.sessionDuration { bestSession = current }
        current = [sample]
      }
    }
    if current.sessionDuration > bestSession.sessionDuration { bestSession = current }

    guard !bestSession.isEmpty,
          let start = bestSession.first?.startDate,
          let end = bestSession.last?.endDate else { return nil }

    let asleepMinutes = bestSession.sessionDuration / 60
    let timeInBedMinutes = end.timeIntervalSince(start) / 60
    let stages = bestSession.compactMap { stageSegment(from: $0) }
    let idSuffix = "\(Int(start.timeIntervalSince1970))"

    return PrimarySleepDetail(
      id: "hk-sleep-\(idSuffix)",
      dateLabel: formatDate(start),
      startLabel: formatTime(start),
      endLabel: formatTime(end),
      durationText: HealthDataStore.minutesText(asleepMinutes),
      timeInBedText: HealthDataStore.minutesText(timeInBedMinutes),
      scoreText: "--",
      qualityText: qualityLabel(totalMinutes: asleepMinutes),
      source: .local("apple.health.sleep"),
      stages: stages
    )
  }

  private static func stageSegment(from sample: HKCategorySample) -> HealthSleepStageSegment? {
    let durationMinutes = sample.endDate.timeIntervalSince(sample.startDate) / 60
    guard durationMinutes > 0 else { return nil }
    let stageName: String
    switch HKCategoryValueSleepAnalysis(rawValue: sample.value) {
    case .asleepDeep: stageName = "deep"
    case .asleepREM: stageName = "rem"
    case .asleepCore, .asleepUnspecified: stageName = "light"
    case .inBed: stageName = "in bed"
    default: stageName = "light"
    }
    return HealthSleepStageSegment(
      id: "\(sample.uuid)",
      stage: stageName,
      startLabel: formatTime(sample.startDate),
      endLabel: formatTime(sample.endDate),
      durationMinutes: durationMinutes,
      confidence: nil,
      source: .local("apple.health.sleep")
    )
  }

  private static func formatDate(_ date: Date) -> String {
    let f = DateFormatter()
    f.dateStyle = .medium
    f.timeStyle = .none
    return f.string(from: date)
  }

  private static func formatTime(_ date: Date) -> String {
    let f = DateFormatter()
    f.dateStyle = .none
    f.timeStyle = .short
    return f.string(from: date)
  }

  private static func qualityLabel(totalMinutes: Double) -> String {
    switch totalMinutes {
    case ..<300: return "Poor"
    case 300..<360: return "Fair"
    case 360..<480: return "Good"
    default: return "Optimal"
    }
  }
}

private extension Array where Element == HKCategorySample {
  var sessionDuration: TimeInterval {
    reduce(0) { $0 + $1.endDate.timeIntervalSince($1.startDate) }
  }
}
