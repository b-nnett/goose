import Foundation
import HealthKit

struct HealthKitFullImportResult {
  var sleepDetail: PrimarySleepDetail?
  var restingHR: Double?
  var hrvRmssdMs: Double?
  var respiratoryRate: Double?
  var spO2Percent: Double?
  var skinTempDeltaC: Double?
  var steps: Int?
  var activeKcal: Double?
  var hrSamples: [(bpm: Int, date: Date)]
  var hrvSamples: [(rmssdMs: Double, date: Date)]
  // 90-day history for baseline computation
  var hrvHistory: [(sdnn: Double, date: Date)]
  var rhrHistory: [(bpm: Double, date: Date)]
  var workouts: [ActivityTimelineItem]
  var errors: [String]

  static let empty = HealthKitFullImportResult(
    hrSamples: [], hrvSamples: [], hrvHistory: [], rhrHistory: [], workouts: [], errors: []
  )
}

enum HealthKitFullImporter {
  private static let lookback: TimeInterval = 7 * 24 * 60 * 60
  private static let baselineLookback: TimeInterval = 90 * 24 * 60 * 60

  static func importAll() async -> HealthKitFullImportResult {
    guard HKHealthStore.isHealthDataAvailable() else {
      return HealthKitFullImportResult(
        hrSamples: [], hrvSamples: [], hrvHistory: [], rhrHistory: [], workouts: [],
        errors: ["HealthKit unavailable on this device"]
      )
    }
    let store = HKHealthStore()
    let types = readTypes()
    do {
      try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
        store.requestAuthorization(toShare: [], read: types) { ok, err in
          if let err { cont.resume(throwing: err) }
          else { cont.resume() }
        }
      }
    } catch {
      return HealthKitFullImportResult(
        hrSamples: [], hrvSamples: [], hrvHistory: [], rhrHistory: [], workouts: [],
        errors: ["Authorization failed: \(error.localizedDescription)"]
      )
    }

    async let sleepResult = querySleep(store: store)
    async let restingHRResult = queryLatestQuantity(store: store, id: .restingHeartRate)
    async let hrvResult = queryLatestQuantity(store: store, id: .heartRateVariabilitySDNN)
    async let respResult = queryLatestQuantity(store: store, id: .respiratoryRate)
    async let spO2Result = queryLatestQuantity(store: store, id: .oxygenSaturation)
    async let skinTempResult = querySkinTemp(store: store)
    async let stepsResult = queryTodaySum(store: store, id: .stepCount)
    async let kcalResult = queryTodaySum(store: store, id: .activeEnergyBurned)
    async let hrSamplesResult = queryHRSamples(store: store)
    async let workoutsResult = queryWorkouts(store: store)
    async let hrvHistoryResult = queryQuantityHistory(store: store, id: .heartRateVariabilitySDNN, unit: .secondUnit(with: .milli), days: 90)
    async let rhrHistoryResult = queryQuantityHistory(store: store, id: .restingHeartRate, unit: HKUnit(from: "count/min"), days: 90)

    var errors: [String] = []
    let sleepDetail = await sleepResult
    let restingHR = await restingHRResult
    let hrv = await hrvResult
    let resp = await respResult
    let spO2 = await spO2Result
    let skinTemp = await skinTempResult
    let steps = await stepsResult
    let kcal = await kcalResult
    let hrSamples = await hrSamplesResult
    let workouts = await workoutsResult
    let hrvHistory = await hrvHistoryResult
    let rhrHistory = await rhrHistoryResult

    if sleepDetail == nil { errors.append("No sleep data") }

    return HealthKitFullImportResult(
      sleepDetail: sleepDetail,
      restingHR: restingHR,
      hrvRmssdMs: hrv,
      respiratoryRate: resp,
      spO2Percent: spO2.map { $0 * 100 },
      skinTempDeltaC: skinTemp,
      steps: steps.map { Int($0) },
      activeKcal: kcal,
      hrSamples: hrSamples,
      hrvSamples: [],
      hrvHistory: hrvHistory.map { (sdnn: $0.value, date: $0.date) },
      rhrHistory: rhrHistory.map { (bpm: $0.value, date: $0.date) },
      workouts: workouts,
      errors: errors
    )
  }

  // MARK: - Read Types

  private static func readTypes() -> Set<HKObjectType> {
    var types = Set<HKObjectType>()
    let quantityIDs: [HKQuantityTypeIdentifier] = [
      .restingHeartRate,
      .heartRateVariabilitySDNN,
      .respiratoryRate,
      .oxygenSaturation,
      .heartRate,
      .stepCount,
      .activeEnergyBurned,
    ]
    for id in quantityIDs {
      if let t = HKObjectType.quantityType(forIdentifier: id) { types.insert(t) }
    }
    if let t = HKObjectType.categoryType(forIdentifier: .sleepAnalysis) { types.insert(t) }
    if #available(iOS 16.0, *) {
      if let t = HKObjectType.quantityType(forIdentifier: .appleSleepingWristTemperature) { types.insert(t) }
    }
    types.insert(HKObjectType.workoutType())
    return types
  }

  // MARK: - Sleep

  private static func querySleep(store: HKHealthStore) async -> PrimarySleepDetail? {
    guard let sleepType = HKObjectType.categoryType(forIdentifier: .sleepAnalysis) else { return nil }
    let cutoff = Date().addingTimeInterval(-2 * 24 * 60 * 60)
    let pred = HKQuery.predicateForSamples(withStart: cutoff, end: Date(), options: .strictStartDate)
    let sort = NSSortDescriptor(key: HKSampleSortIdentifierStartDate, ascending: true)
    let samples: [HKCategorySample] = await withCheckedContinuation { cont in
      let q = HKSampleQuery(sampleType: sleepType, predicate: pred, limit: 500, sortDescriptors: [sort]) { _, s, _ in
        cont.resume(returning: (s as? [HKCategorySample]) ?? [])
      }
      store.execute(q)
    }
    return buildSleepDetail(from: samples)
  }

  private static func buildSleepDetail(from samples: [HKCategorySample]) -> PrimarySleepDetail? {
    let asleep = samples.filter { $0.value != HKCategoryValueSleepAnalysis.awake.rawValue }
    guard !asleep.isEmpty else { return nil }
    var bestSession: [HKCategorySample] = []
    var current: [HKCategorySample] = [asleep[0]]
    for sample in asleep.dropFirst() {
      if sample.startDate.timeIntervalSince(current.last!.endDate) < 90 * 60 {
        current.append(sample)
      } else {
        if current.totalDuration > bestSession.totalDuration { bestSession = current }
        current = [sample]
      }
    }
    if current.totalDuration > bestSession.totalDuration { bestSession = current }
    guard let start = bestSession.first?.startDate, let end = bestSession.last?.endDate else { return nil }
    let asleepMinutes = bestSession.totalDuration / 60
    let timeInBedMinutes = end.timeIntervalSince(start) / 60
    let stages = bestSession.compactMap { makeSleepStage(from: $0) }
    return PrimarySleepDetail(
      id: "hk-sleep-\(Int(start.timeIntervalSince1970))",
      dateLabel: shortDate(start),
      startLabel: shortTime(start),
      endLabel: shortTime(end),
      durationText: HealthDataStore.minutesText(asleepMinutes),
      timeInBedText: HealthDataStore.minutesText(timeInBedMinutes),
      scoreText: "--",
      qualityText: sleepQuality(asleepMinutes),
      source: .local("apple.health.sleep"),
      stages: stages
    )
  }

  private static func makeSleepStage(from sample: HKCategorySample) -> HealthSleepStageSegment? {
    let minutes = sample.endDate.timeIntervalSince(sample.startDate) / 60
    guard minutes > 0 else { return nil }
    let name: String
    switch HKCategoryValueSleepAnalysis(rawValue: sample.value) {
    case .asleepDeep: name = "deep"
    case .asleepREM: name = "rem"
    case .asleepCore, .asleepUnspecified: name = "light"
    case .inBed: name = "in bed"
    default: name = "light"
    }
    return HealthSleepStageSegment(
      id: "\(sample.uuid)",
      stage: name,
      startLabel: shortTime(sample.startDate),
      endLabel: shortTime(sample.endDate),
      durationMinutes: minutes,
      confidence: nil,
      source: .local("apple.health.sleep")
    )
  }

  private static func sleepQuality(_ minutes: Double) -> String {
    switch minutes {
    case ..<300: return "Poor"
    case 300..<360: return "Fair"
    case 360..<480: return "Good"
    default: return "Optimal"
    }
  }

  // MARK: - Latest Quantity Samples

  private static func queryLatestQuantity(store: HKHealthStore, id: HKQuantityTypeIdentifier) async -> Double? {
    guard let type = HKObjectType.quantityType(forIdentifier: id) else { return nil }
    return await withCheckedContinuation { cont in
      let sort = NSSortDescriptor(key: HKSampleSortIdentifierEndDate, ascending: false)
      let q = HKSampleQuery(sampleType: type, predicate: nil, limit: 1, sortDescriptors: [sort]) { _, samples, _ in
        let sample = samples?.first as? HKQuantitySample
        let value: Double? = sample.map { s in
          switch id {
          case .restingHeartRate: return s.quantity.doubleValue(for: .init(from: "count/min"))
          case .heartRateVariabilitySDNN: return s.quantity.doubleValue(for: .secondUnit(with: .milli))
          case .respiratoryRate: return s.quantity.doubleValue(for: .init(from: "count/min"))
          case .oxygenSaturation: return s.quantity.doubleValue(for: .percent())
          default: return nil
          }
        } ?? nil
        cont.resume(returning: value)
      }
      store.execute(q)
    }
  }

  private static func querySkinTemp(store: HKHealthStore) async -> Double? {
    guard #available(iOS 16.0, *),
          let type = HKObjectType.quantityType(forIdentifier: .appleSleepingWristTemperature) else { return nil }
    return await withCheckedContinuation { cont in
      let sort = NSSortDescriptor(key: HKSampleSortIdentifierEndDate, ascending: false)
      let q = HKSampleQuery(sampleType: type, predicate: nil, limit: 1, sortDescriptors: [sort]) { _, samples, _ in
        let value = (samples?.first as? HKQuantitySample)?.quantity.doubleValue(for: .degreeCelsius())
        cont.resume(returning: value)
      }
      store.execute(q)
    }
  }

  // MARK: - Today Sums

  private static func queryTodaySum(store: HKHealthStore, id: HKQuantityTypeIdentifier) async -> Double? {
    guard let type = HKObjectType.quantityType(forIdentifier: id) else { return nil }
    let cal = Calendar.current
    let start = cal.startOfDay(for: Date())
    let pred = HKQuery.predicateForSamples(withStart: start, end: Date(), options: .strictStartDate)
    let unit: HKUnit = id == .stepCount ? .count() : .kilocalorie()
    return await withCheckedContinuation { cont in
      let q = HKStatisticsQuery(quantityType: type, quantitySamplePredicate: pred, options: .cumulativeSum) { _, stats, _ in
        cont.resume(returning: stats?.sumQuantity()?.doubleValue(for: unit))
      }
      store.execute(q)
    }
  }

  // MARK: - Heart Rate Samples

  private static func queryHRSamples(store: HKHealthStore) async -> [(bpm: Int, date: Date)] {
    guard let type = HKObjectType.quantityType(forIdentifier: .heartRate) else { return [] }
    let cutoff = Date().addingTimeInterval(-lookback)
    let pred = HKQuery.predicateForSamples(withStart: cutoff, end: Date(), options: .strictStartDate)
    let sort = NSSortDescriptor(key: HKSampleSortIdentifierStartDate, ascending: true)
    return await withCheckedContinuation { cont in
      let q = HKSampleQuery(sampleType: type, predicate: pred, limit: 10_000, sortDescriptors: [sort]) { _, samples, _ in
        let unit = HKUnit(from: "count/min")
        let pts = (samples as? [HKQuantitySample] ?? []).map { s in
          (bpm: Int(s.quantity.doubleValue(for: unit).rounded()), date: s.startDate)
        }
        cont.resume(returning: pts)
      }
      store.execute(q)
    }
  }

  // MARK: - Workouts

  private static func queryWorkouts(store: HKHealthStore) async -> [ActivityTimelineItem] {
    let cutoff = Date().addingTimeInterval(-lookback)
    let pred = HKQuery.predicateForSamples(withStart: cutoff, end: Date(), options: .strictStartDate)
    let sort = NSSortDescriptor(key: HKSampleSortIdentifierStartDate, ascending: false)
    let workouts: [HKWorkout] = await withCheckedContinuation { cont in
      let q = HKSampleQuery(sampleType: HKObjectType.workoutType(), predicate: pred, limit: 50, sortDescriptors: [sort]) { _, samples, _ in
        cont.resume(returning: (samples as? [HKWorkout]) ?? [])
      }
      store.execute(q)
    }
    return workouts.map { w in
      let hr = w.statistics(for: HKObjectType.quantityType(forIdentifier: .heartRate)!)?
        .averageQuantity()?.doubleValue(for: HKUnit(from: "count/min"))
      let dist = w.statistics(for: HKObjectType.quantityType(forIdentifier: .distanceWalkingRunning)!)?
        .sumQuantity()?.doubleValue(for: .meter())
      return ActivityTimelineItem(
        id: w.uuid.uuidString,
        startedAt: w.startDate,
        title: w.workoutActivityType.displayName,
        activityType: w.workoutActivityType.displayName,
        syncStatus: "apple.health",
        durationSeconds: w.duration,
        distanceMeters: dist,
        averageHeartRate: hr.map { Int($0.rounded()) }
      )
    }
  }

  // MARK: - History queries (for baseline)

  private static func queryQuantityHistory(
    store: HKHealthStore,
    id: HKQuantityTypeIdentifier,
    unit: HKUnit,
    days: Int
  ) async -> [(value: Double, date: Date)] {
    guard let type = HKObjectType.quantityType(forIdentifier: id) else { return [] }
    let cutoff = Date().addingTimeInterval(-Double(days) * 24 * 60 * 60)
    let pred = HKQuery.predicateForSamples(withStart: cutoff, end: Date(), options: .strictStartDate)
    let sort = NSSortDescriptor(key: HKSampleSortIdentifierStartDate, ascending: true)
    return await withCheckedContinuation { cont in
      let q = HKSampleQuery(sampleType: type, predicate: pred, limit: days * 3, sortDescriptors: [sort]) { _, samples, _ in
        let pts = (samples as? [HKQuantitySample] ?? []).map { s in
          (value: s.quantity.doubleValue(for: unit), date: s.startDate)
        }
        cont.resume(returning: pts)
      }
      store.execute(q)
    }
  }

  // MARK: - Formatting helpers

  private static func shortDate(_ date: Date) -> String {
    let f = DateFormatter()
    f.dateStyle = .medium; f.timeStyle = .none
    return f.string(from: date)
  }

  private static func shortTime(_ date: Date) -> String {
    let f = DateFormatter()
    f.dateStyle = .none; f.timeStyle = .short
    return f.string(from: date)
  }
}

// MARK: - HKWorkoutActivityType display name

private extension HKWorkoutActivityType {
  var displayName: String {
    switch self {
    case .running: return "Run"
    case .cycling: return "Ride"
    case .swimming: return "Swim"
    case .walking: return "Walk"
    case .hiking: return "Hike"
    case .yoga: return "Yoga"
    case .functionalStrengthTraining, .traditionalStrengthTraining: return "Strength"
    case .highIntensityIntervalTraining: return "HIIT"
    case .soccer: return "Soccer"
    case .basketball: return "Basketball"
    case .tennis: return "Tennis"
    case .rowing: return "Row"
    case .elliptical: return "Elliptical"
    case .stairClimbing: return "Stairs"
    case .crossTraining: return "Cross Training"
    case .dance: return "Dance"
    case .pilates: return "Pilates"
    case .downhillSkiing: return "Ski"
    case .snowboarding: return "Snowboard"
    case .surfingSports: return "Surf"
    case .golf: return "Golf"
    case .climbing: return "Climb"
    case .boxing: return "Box"
    case .martialArts: return "Martial Arts"
    case .jumpRope: return "Jump Rope"
    case .pickleball: return "Pickleball"
    default: return "Workout"
    }
  }
}

private extension [HKCategorySample] {
  var totalDuration: TimeInterval {
    reduce(0) { $0 + $1.endDate.timeIntervalSince($1.startDate) }
  }
}
