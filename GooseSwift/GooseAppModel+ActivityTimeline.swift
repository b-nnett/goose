import Foundation
import UIKit


extension GooseAppModel {
  func intString(_ value: Any?) -> String {
    intValue(value).map(String.init) ?? "?"
  }

  func intValue(_ value: Any?) -> Int? {
    if let int = value as? Int {
      return int
    }
    if let number = value as? NSNumber {
      return number.intValue
    }
    if let string = value as? String {
      return Int(string)
    }
    return nil
  }

  nonisolated static func intString(_ value: Any?) -> String {
    intValue(value).map(String.init) ?? "?"
  }

  nonisolated static func intValue(_ value: Any?) -> Int? {
    if let value = value as? Int {
      return value
    }
    if let value = value as? Int64 {
      return Int(value)
    }
    if let value = value as? Double {
      return Int(value)
    }
    if let value = value as? String {
      return Int(value)
    }
    return nil
  }

  func doubleValue(_ value: Any?) -> Double? {
    if let double = value as? Double {
      return double
    }
    if let number = value as? NSNumber {
      return number.doubleValue
    }
    if let string = value as? String {
      return Double(string)
    }
    return nil
  }

  func int64Value(_ value: Any?) -> Int64? {
    if let int64 = value as? Int64 {
      return int64
    }
    if let int = value as? Int {
      return Int64(int)
    }
    if let number = value as? NSNumber {
      return number.int64Value
    }
    if let string = value as? String {
      return Int64(string)
    }
    return nil
  }

  func normalizedZoneDurations(
    _ zoneDurations: [Int: TimeInterval],
    targetDuration: TimeInterval,
    fallbackHeartRate: Int?
  ) -> [Int: TimeInterval] {
    let boundedTarget = max(targetDuration, 0)
    var normalized: [Int: TimeInterval] = [:]
    for zoneID in 1...5 {
      normalized[zoneID] = max(zoneDurations[zoneID, default: 0], 0)
    }

    let total = normalized.values.reduce(0, +)
    if total > boundedTarget + 1, total > 0 {
      let scale = boundedTarget / total
      for zoneID in 1...5 {
        normalized[zoneID] = normalized[zoneID, default: 0] * scale
      }
      return normalized
    }

    if total < boundedTarget - 1, let fallbackHeartRate {
      let fallbackZone = HeartRateZone.zoneID(for: fallbackHeartRate)
      normalized[fallbackZone, default: 0] += boundedTarget - total
    }
    return normalized
  }

  func cleanupOrphanedActivityCaptureSessions(now: Date = Date()) {
    let start = now.addingTimeInterval(-7 * 24 * 60 * 60)
    let startMs = unixMilliseconds(start)
    let nowMs = unixMilliseconds(now)
    let endMs = unixMilliseconds(now.addingTimeInterval(60))
    let cutoffMs = unixMilliseconds(now.addingTimeInterval(-60))
    let databasePath = HealthDataStore.defaultDatabasePath()

    rustStartupQueue.async { [weak self] in
      let result: Result<Int, Error>
      do {
        let rust = GooseRustBridge()
        let report = try rust.request(
          method: "capture.list_sessions",
          args: [
            "database_path": databasePath,
            "start_unix_ms": startMs,
            "end_unix_ms": endMs,
          ]
        )
        let sessions = report["sessions"] as? [[String: Any]] ?? []
        var repaired = 0
        for session in sessions {
          guard
            let sessionID = session["session_id"] as? String,
            sessionID.contains(".activity."),
            (session["status"] as? String) == "active",
            Self.timelineInt64Value(session["frame_count"]) == 0,
            let startedMs = Self.timelineInt64Value(session["started_at_unix_ms"]),
            startedMs <= cutoffMs
          else {
            continue
          }

          _ = try rust.request(
            method: "capture.finish_session",
            args: [
              "database_path": databasePath,
              "session_id": sessionID,
              "ended_at_unix_ms": max(nowMs, startedMs + 1),
              "frame_count": 0,
            ]
          )
          repaired += 1
        }

        result = .success(repaired)
      } catch {
        result = .failure(error)
      }

      DispatchQueue.main.async { [weak self] in
        guard let self else {
          return
        }
        switch result {
        case .success(let repaired):
          if repaired > 0 {
            self.ble.record(source: "activity.capture", title: "orphan_zero_frame.repaired", body: "sessions=\(repaired)")
          }
        case .failure(let error):
          self.ble.record(level: .warn, source: "activity.capture", title: "orphan_zero_frame.repair_failed", body: String(describing: error))
        }
      }
    }
  }

  func activityTimelineItem(from session: [String: Any]) -> ActivityTimelineItem? {
    guard
      let sessionID = session["session_id"] as? String,
      let startMs = activityStartMilliseconds(from: session)
    else {
      return nil
    }

    let metrics = activityMetricsByName(sessionID: sessionID)
    let activityType = session["activity_type"] as? String ?? "activity"
    let customLabel = session["custom_label"] as? String
    let externalName = session["external_activity_type_name"] as? String
    let syncStatus = session["sync_status"] as? String ?? ""
    let baseTitle = nonEmpty(externalName) ?? nonEmpty(customLabel) ?? activityType.replacingOccurrences(of: "_", with: " ").capitalized
    let title = syncStatus == "candidate" ? "Candidate \(baseTitle)" : baseTitle
    let sessionDurationMs = int64Value(session["duration_ms"]) ?? activityEndMilliseconds(from: session).map { max(0, $0 - startMs) }
    let durationSeconds = doubleValue(metrics["duration"]?["value"]) ?? Double(sessionDurationMs ?? 0) / 1000
    let distanceMeters = doubleValue(metrics["distance"]?["value"])
    let averageHeartRate = doubleValue(metrics["average_hr"]?["value"]).map { Int($0.rounded()) }

    guard Self.activityTimelineSessionIsDisplaySafe(session, metrics: Array(metrics.values)) else {
      return nil
    }

    return ActivityTimelineItem(
      id: sessionID,
      startedAt: Date(timeIntervalSince1970: Double(startMs) / 1000),
      title: title,
      activityType: activityType,
      syncStatus: syncStatus,
      durationSeconds: durationSeconds,
      distanceMeters: distanceMeters,
      averageHeartRate: averageHeartRate
    )
  }

  func activityStartMilliseconds(from session: [String: Any]) -> Int64? {
    int64Value(session["start_time_unix_ms"])
      ?? int64Value(session["started_at_unix_ms"])
      ?? int64Value(session["start_unix_ms"])
  }

  func activityEndMilliseconds(from session: [String: Any]) -> Int64? {
    int64Value(session["end_time_unix_ms"])
      ?? int64Value(session["ended_at_unix_ms"])
      ?? int64Value(session["end_unix_ms"])
  }

  func activityMetricsByName(sessionID: String) -> [String: [String: Any]] {
    do {
      let report = try rust.request(
        method: "activity.list_metrics",
        args: [
          "database_path": HealthDataStore.defaultDatabasePath(),
          "activity_session_id": sessionID,
        ]
      )
      let metrics = report["metrics"] as? [[String: Any]] ?? []
      return Dictionary(
        uniqueKeysWithValues: metrics.compactMap { metric in
          guard let name = metric["metric_name"] as? String else {
            return nil
          }
          return (name, metric)
        }
      )
    } catch {
      ble.record(level: .warn, source: "activity.timeline", title: "metric.load.failed", body: "\(sessionID) \(String(describing: error))")
      return [:]
    }
  }

  nonisolated static func activityTimelineRefreshResult(
    sessions: [[String: Any]],
    dayStart: Date,
    dayEnd: Date,
    metricsBySession: [String: [[String: Any]]]
  ) -> ActivityTimelineRefreshResult {
    var skippedCounts: [String: Int] = [:]
    let displaySafeSessions = sessions.filter { session in
      guard let sessionID = session["session_id"] as? String else {
        skippedCounts["missing_session_id", default: 0] += 1
        return false
      }
      guard activityTimelineSessionIsDisplaySafe(session, metrics: metricsBySession[sessionID] ?? []) else {
        skippedCounts["platform_import", default: 0] += 1
        return false
      }
      return true
    }
    let candidateCount = displaySafeSessions.filter { ($0["sync_status"] as? String) == "candidate" }.count
    let visibleSessions = displaySafeSessions.filter { session in
      guard let startMs = timelineActivityStartMilliseconds(from: session) else {
        skippedCounts["missing_start", default: 0] += 1
        return false
      }
      let startedAt = Date(timeIntervalSince1970: Double(startMs) / 1000)
      guard startedAt >= dayStart && startedAt < dayEnd else {
        skippedCounts["outside_day", default: 0] += 1
        return false
      }
      return true
    }

    var items: [ActivityTimelineItem] = []
    for session in visibleSessions {
      guard let sessionID = session["session_id"] as? String else {
        skippedCounts["missing_session_id", default: 0] += 1
        continue
      }

      let metrics: [String: [String: Any]] = Dictionary(
        uniqueKeysWithValues: (metricsBySession[sessionID] ?? []).compactMap { metric -> (String, [String: Any])? in
          guard let name = metric["metric_name"] as? String else {
            return nil
          }
          return (name, metric)
        }
      )

      if let item = activityTimelineItem(from: session, metrics: metrics) {
        items.append(item)
      } else {
        skippedCounts["unmapped", default: 0] += 1
      }
    }

    let sortedItems = items.sorted { $0.startedAt < $1.startedAt }
    let skippedText = skippedCounts
      .sorted { $0.key < $1.key }
      .map { "\($0.key)=\($0.value)" }
      .joined(separator: ", ")
    let candidateText = candidateCount > 0 ? " | candidates \(candidateCount)" : ""
    let status = sortedItems.isEmpty
      ? "No activities today | raw \(sessions.count)\(skippedText.isEmpty ? "" : " | \(skippedText)")"
      : "\(sortedItems.count) activities today | raw \(sessions.count)\(candidateText)\(skippedText.isEmpty ? "" : " | skipped \(skippedText)")"

    return ActivityTimelineRefreshResult(items: sortedItems, status: status)
  }

  nonisolated static func activityTimelineItem(
    from session: [String: Any],
    metrics: [String: [String: Any]]
  ) -> ActivityTimelineItem? {
    guard
      let sessionID = session["session_id"] as? String,
      let startMs = timelineActivityStartMilliseconds(from: session)
    else {
      return nil
    }

    let activityType = session["activity_type"] as? String ?? "activity"
    let customLabel = session["custom_label"] as? String
    let externalName = session["external_activity_type_name"] as? String
    let syncStatus = session["sync_status"] as? String ?? ""
    let baseTitle = timelineNonEmpty(externalName) ?? timelineNonEmpty(customLabel) ?? activityType.replacingOccurrences(of: "_", with: " ").capitalized
    let title = syncStatus == "candidate" ? "Candidate \(baseTitle)" : baseTitle
    let sessionDurationMs = timelineInt64Value(session["duration_ms"])
      ?? timelineActivityEndMilliseconds(from: session).map { max(0, $0 - startMs) }
    let durationSeconds = timelineDoubleValue(metrics["duration"]?["value"]) ?? Double(sessionDurationMs ?? 0) / 1000
    let distanceMeters = timelineDoubleValue(metrics["distance"]?["value"])
    let averageHeartRate = timelineDoubleValue(metrics["average_hr"]?["value"]).map { Int($0.rounded()) }

    return ActivityTimelineItem(
      id: sessionID,
      startedAt: Date(timeIntervalSince1970: Double(startMs) / 1000),
      title: title,
      activityType: activityType,
      syncStatus: syncStatus,
      durationSeconds: durationSeconds,
      distanceMeters: distanceMeters,
      averageHeartRate: averageHeartRate
    )
  }

  nonisolated static func activityTimelineSessionIsDisplaySafe(
    _ session: [String: Any],
    metrics: [[String: Any]]
  ) -> Bool {
    if activityTimelineValueContainsPlatformSourceMarker(session) {
      return false
    }
    return !metrics.contains { activityTimelineValueContainsPlatformSourceMarker($0) }
  }

  nonisolated static func activityTimelineMetricsByName(
    sessionID: String,
    databasePath: String,
    rust: GooseRustBridge
  ) throws -> [String: [String: Any]] {
    let report = try rust.request(
      method: "activity.list_metrics",
      args: [
        "database_path": databasePath,
        "activity_session_id": sessionID,
      ]
    )
    let metrics = report["metrics"] as? [[String: Any]] ?? []
    return Dictionary(
      uniqueKeysWithValues: metrics.compactMap { metric in
        guard let name = metric["metric_name"] as? String else {
          return nil
        }
        return (name, metric)
      }
    )
  }

  nonisolated static func timelineActivityStartMilliseconds(from session: [String: Any]) -> Int64? {
    timelineInt64Value(session["start_time_unix_ms"])
      ?? timelineInt64Value(session["started_at_unix_ms"])
      ?? timelineInt64Value(session["start_unix_ms"])
  }

  nonisolated static func timelineActivityEndMilliseconds(from session: [String: Any]) -> Int64? {
    timelineInt64Value(session["end_time_unix_ms"])
      ?? timelineInt64Value(session["ended_at_unix_ms"])
      ?? timelineInt64Value(session["end_unix_ms"])
  }

  nonisolated static func timelineNonEmpty(_ value: String?) -> String? {
    guard let value = value?.trimmingCharacters(in: .whitespacesAndNewlines), !value.isEmpty else {
      return nil
    }
    return value
  }

  nonisolated static func timelineDoubleValue(_ value: Any?) -> Double? {
    switch value {
    case let number as Double:
      return number
    case let number as Float:
      return Double(number)
    case let number as Int:
      return Double(number)
    case let number as Int64:
      return Double(number)
    case let number as NSNumber:
      return number.doubleValue
    case let string as String:
      return Double(string)
    default:
      return nil
    }
  }

  nonisolated static func timelineInt64Value(_ value: Any?) -> Int64? {
    switch value {
    case let number as Int64:
      return number
    case let number as Int:
      return Int64(number)
    case let number as Double:
      return Int64(number)
    case let number as NSNumber:
      return number.int64Value
    case let string as String:
      return Int64(string)
    default:
      return nil
    }
  }

  nonisolated static func activityTimelineValueContainsPlatformSourceMarker(_ value: Any?) -> Bool {
    guard let value else {
      return false
    }
    if let text = value as? String {
      if activityTimelineJSONStringContainsPlatformSourceMarker(text) {
        return true
      }
      return activityTimelineTextContainsPlatformSourceMarker(text)
    }
    if let dictionary = value as? [String: Any] {
      return dictionary.contains { element in
        let key = element.key
        let child = element.value
        return activityTimelineTextContainsPlatformSourceMarker(key)
          || activityTimelineValueContainsPlatformSourceMarker(child)
      }
    }
    if let array = value as? [Any] {
      return array.contains { activityTimelineValueContainsPlatformSourceMarker($0) }
    }
    return false
  }

  nonisolated static func activityTimelineJSONStringContainsPlatformSourceMarker(_ text: String) -> Bool {
    guard let data = text.data(using: .utf8),
          let value = try? JSONSerialization.jsonObject(with: data) else {
      return false
    }
    return activityTimelineValueContainsPlatformSourceMarker(value)
  }

  nonisolated static func activityTimelineTextContainsPlatformSourceMarker(_ text: String) -> Bool {
    let normalized = text
      .lowercased()
      .unicodeScalars
      .map { CharacterSet.alphanumerics.contains($0) ? Character($0) : "_" }
      .reduce(into: "") { result, character in
        if character == "_" {
          if result.last != "_" {
            result.append(character)
          }
        } else {
          result.append(character)
        }
      }
      .trimmingCharacters(in: CharacterSet(charactersIn: "_"))
    return normalized.contains("healthkit")
      || normalized.contains("health_connect")
      || normalized.contains("apple_health")
      || normalized.contains("platform_import")
      || normalized.contains("imported_platform")
      || normalized.contains("hksample")
  }

  func nonEmpty(_ value: String?) -> String? {
    guard let value = value?.trimmingCharacters(in: .whitespacesAndNewlines), !value.isEmpty else {
      return nil
    }
    return value
  }

  func attachActivityMetric(
    sessionID: String,
    name: String,
    value: Double,
    unit: String,
    startMs: Int64,
    endMs: Int64,
    source: String
  ) {
    attachActivityMetrics([
      activityMetricPayload(
        sessionID: sessionID,
        name: name,
        value: value,
        unit: unit,
        startMs: startMs,
        endMs: endMs,
        source: source
      )
    ])
  }

  func appendActivityMetric(
    _ metrics: inout [[String: Any]],
    sessionID: String,
    name: String,
    value: Double,
    unit: String,
    startMs: Int64,
    endMs: Int64,
    source: String
  ) {
    metrics.append(
      activityMetricPayload(
        sessionID: sessionID,
        name: name,
        value: value,
        unit: unit,
        startMs: startMs,
        endMs: endMs,
        source: source
      )
    )
  }

  func activityMetricPayload(
    sessionID: String,
    name: String,
    value: Double,
    unit: String,
    startMs: Int64,
    endMs: Int64,
    source: String
  ) -> [String: Any] {
    [
      "metric_id": "\(sessionID).\(name)",
      "activity_session_id": sessionID,
      "metric_name": name,
      "value": value,
      "unit": unit,
      "start_time_unix_ms": startMs,
      "end_time_unix_ms": endMs,
      "quality_flags": [],
      "provenance": [
        "source": source,
      ],
    ]
  }

  func attachActivityMetrics(_ metrics: [[String: Any]]) {
    guard !metrics.isEmpty else {
      return
    }
    do {
      let report = try rust.request(
        method: "activity.attach_metrics",
        args: [
          "database_path": HealthDataStore.defaultDatabasePath(),
          "include_metrics": false,
          "metrics": metrics,
        ]
      )
      let inserted = intValue(report["inserted"]) ?? 0
      let existing = intValue(report["existing"]) ?? 0
      ble.record(
        level: .debug,
        source: "rust",
        title: "activity.metrics.store.ok",
        body: "inserted=\(inserted) existing=\(existing) requested=\(metrics.count)"
      )
    } catch {
      ble.record(level: .warn, source: "rust", title: "activity.metrics.store.failed", body: "count=\(metrics.count): \(String(describing: error))")
    }
  }

  func rustActivityType(for activity: ActivityKind) -> String {
    switch activity {
    case .run:
      return "running"
    case .indoorRun:
      return "treadmill_running"
    case .walk:
      return "walking"
    case .indoorWalk:
      return "indoor_walking"
    case .hike:
      return "hiking"
    case .roadRide, .mountainBike:
      return "cycling"
    case .indoorRide:
      return "spinning"
    case .elliptical:
      return "elliptical"
    case .stairStepper:
      return "stair_stepper"
    case .soccer:
      return "team_sport"
    case .strength, .functionalTraining:
      return "strength"
    case .hiit:
      return "hiit"
    case .yoga, .pilates, .barre:
      return "yoga"
    case .row:
      return "rowing"
    case .poolSwim:
      return "swimming"
    }
  }

  func activityExternalName(for activity: ActivityKind) -> String {
    switch activity {
    case .run:
      return "Outdoor Run"
    case .indoorRun:
      return "Indoor Run"
    case .walk:
      return "Outdoor Walk"
    case .indoorWalk:
      return "Indoor Walk"
    case .hike:
      return "Hiking"
    case .roadRide:
      return "Outdoor Cycle"
    case .mountainBike:
      return "Mountain Biking"
    case .soccer:
      return "Soccer"
    case .strength:
      return "Traditional Strength Training"
    case .hiit:
      return "High Intensity Interval Training"
    case .yoga:
      return "Yoga"
    case .row:
      return "Rowing"
    case .indoorRide:
      return "Indoor Cycle"
    case .elliptical:
      return "Elliptical"
    case .stairStepper:
      return "Stair Stepper"
    case .pilates:
      return "Pilates"
    case .barre:
      return "Barre"
    case .functionalTraining:
      return "Functional Training"
    case .poolSwim:
      return "Pool Swim"
    }
  }

  func unixMilliseconds(_ date: Date) -> Int64 {
    Int64((date.timeIntervalSince1970 * 1000).rounded())
  }

  static func overnightGuardDirectoryURL(sessionID: String) -> URL {
    overnightGuardRootDirectoryURL()
      .appendingPathComponent(sessionID, isDirectory: true)
  }

  nonisolated static func overnightGuardRootDirectoryURL() -> URL {
    let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
      ?? FileManager.default.temporaryDirectory
    return documents
      .appendingPathComponent("GooseSwift", isDirectory: true)
      .appendingPathComponent("OvernightGuard", isDirectory: true)
  }

  func formatPersistedDistance(_ meters: Double) -> String {
    if meters >= 1000 {
      return String(format: "%.2fkm", meters / 1000)
    }
    return "\(Int(max(meters, 0).rounded()))m"
  }
}
