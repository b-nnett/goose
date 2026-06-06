import Foundation
import OSLog

private let logger = Logger(subsystem: "com.goose.swift", category: "upload")

struct GooseUploadStatus {
  let lastUploadTimestamp: Date?
  let pendingBatchCount: Int
  let lastSyncedCount: Int?
}

final class GooseUploadService: @unchecked Sendable {
  private let rust = GooseRustBridge()
  private let databasePath: String
  private let session: URLSession

  // Protected by Swift's cooperative thread pool — only mutated from upload tasks
  private var lastUploadTimestamp: Date?
  private var pendingBatchCount: Int = 0
  private var lastSyncedCount: Int?

  var onStatusUpdate: (@MainActor (GooseUploadStatus) -> Void)?

  init(databasePath: String) {
    self.databasePath = databasePath
    let config = URLSessionConfiguration.ephemeral
    config.timeoutIntervalForRequest = 15
    self.session = URLSession(configuration: config)
  }

  func upload(deviceID: UUID, deviceType: String, sinceTimestamp: Date) {
    pendingBatchCount += 1
    Task.detached(priority: .utility) { [weak self] in
      await self?.performUpload(deviceID: deviceID, deviceType: deviceType, sinceTimestamp: sinceTimestamp)
    }
  }

  private func performUpload(deviceID: UUID, deviceType: String, sinceTimestamp: Date) async {
    guard UserDefaults.standard.bool(forKey: RemoteServerStorage.uploadEnabled) else {
      pendingBatchCount = max(0, pendingBatchCount - 1)
      return
    }
    let rawURL = UserDefaults.standard.string(forKey: RemoteServerStorage.serverURL) ?? ""
    guard !rawURL.isEmpty, let baseURL = URL(string: rawURL) else {
      pendingBatchCount = max(0, pendingBatchCount - 1)
      return
    }
    guard let token = (try? RemoteServerKeychain.loadToken()) ?? nil, !token.isEmpty else {
      pendingBatchCount = max(0, pendingBatchCount - 1)
      return
    }

    // Fetch recent decoded streams from Rust bridge (synchronous — runs on detached task thread)
    let streamsResult: [String: Any]
    do {
      streamsResult = try rust.request(
        method: "upload.get_recent_decoded_streams",
        args: [
          "database_path": databasePath,
          "device_id": deviceID.uuidString,
          "since_ts": sinceTimestamp.timeIntervalSince1970,
        ]
      )
    } catch {
      logger.debug("upload.get_recent_decoded_streams failed: \(error)")
      pendingBatchCount = max(0, pendingBatchCount - 1)
      return
    }

    let hr = streamsResult["hr"] as? [Any] ?? []
    let rr = streamsResult["rr"] as? [Any] ?? []
    let events = streamsResult["events"] as? [Any] ?? []
    let battery = streamsResult["battery"] as? [Any] ?? []
    let spo2 = streamsResult["spo2"] as? [Any] ?? []
    let skinTemp = streamsResult["skin_temp"] as? [Any] ?? []
    let resp = streamsResult["resp"] as? [Any] ?? []
    let gravity = streamsResult["gravity"] as? [Any] ?? []

    let hasData = !hr.isEmpty || !rr.isEmpty || !events.isEmpty || !battery.isEmpty
      || !spo2.isEmpty || !skinTemp.isEmpty || !resp.isEmpty || !gravity.isEmpty
    guard hasData else {
      pendingBatchCount = max(0, pendingBatchCount - 1)
      return
    }

    let streams: [String: Any] = [
      "hr": hr, "rr": rr, "events": events, "battery": battery,
      "spo2": spo2, "skin_temp": skinTemp, "resp": resp, "gravity": gravity,
    ]
    let payload = buildUploadPayload(deviceID: deviceID, deviceType: deviceType, streams: streams)

    guard let body = try? JSONSerialization.data(withJSONObject: payload) else {
      pendingBatchCount = max(0, pendingBatchCount - 1)
      return
    }

    var request = URLRequest(url: baseURL.appendingPathComponent("v1/ingest-decoded"))
    request.httpMethod = "POST"
    request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
    request.setValue("application/json", forHTTPHeaderField: "Content-Type")
    request.httpBody = body

    // Retry with async backoff — no thread blocking
    let delays: [UInt64] = [1_000_000_000, 2_000_000_000, 4_000_000_000]
    var uploadSucceeded = false
    var syncedCount: Int?
    for attempt in 0..<3 {
      if attempt > 0 {
        try? await Task.sleep(nanoseconds: delays[attempt - 1])
      }
      if let count = await performRequest(request) {
        uploadSucceeded = true
        syncedCount = count
        break
      }
    }

    if uploadSucceeded {
      lastUploadTimestamp = Date()
      lastSyncedCount = syncedCount
    } else {
      logger.debug("upload failed after 3 attempts — discarding batch silently")
    }
    pendingBatchCount = max(0, pendingBatchCount - 1)
    publishStatus()
  }

  private func performRequest(_ request: URLRequest) async -> Int? {
    guard let (data, response) = try? await session.data(for: request) else {
      logger.debug("upload request error")
      return nil
    }
    guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
      if let http = response as? HTTPURLResponse {
        logger.debug("upload server error: \(http.statusCode)")
      }
      return nil
    }
    if let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
       let upserted = json["upserted"] as? [String: Int] {
      return upserted.values.reduce(0, +)
    }
    return 0
  }

  // Pure payload builder — no async, no URLSession, no Rust bridge access.
  // Internal so GooseSwiftTests (@testable import GooseSwift) can call it directly.
  // WHOOP Gen4/Gen5 use device_generation with no device_class key.
  // HR monitors (default case) use device_type (sanitized BLE name) + device_class: "HR_MONITOR"
  // so the server can distinguish wearable class from model name (review HIGH-1, HIGH-3).
  func buildUploadPayload(
    deviceID: UUID,
    deviceType: String,
    streams: [String: Any]
  ) -> [String: Any] {
    let device: [String: Any] = ["id": deviceID.uuidString, "mac": NSNull(), "name": NSNull()]
    switch deviceType {
    case "GEN4":
      return [
        "device": device,
        "streams": streams,
        "device_generation": "4.0",
      ]
    case "GOOSE":
      return [
        "device": device,
        "streams": streams,
        "device_generation": "5.0",
      ]
    default:
      // device_type carries the model/name (pre-sanitized BLE advertised name),
      // device_class carries the wearable class so the server can distinguish class from model.
      return [
        "device": device,
        "streams": streams,
        "device_type": deviceType,
        "device_class": "HR_MONITOR",
      ]
    }
  }

  private func publishStatus() {
    let status = GooseUploadStatus(
      lastUploadTimestamp: lastUploadTimestamp,
      pendingBatchCount: pendingBatchCount,
      lastSyncedCount: lastSyncedCount
    )
    Task { @MainActor [weak self] in
      self?.onStatusUpdate?(status)
    }
  }
}
