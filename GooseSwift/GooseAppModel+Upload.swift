import Foundation


extension GooseAppModel {

  // One-shot guard: ensures health check runs at most once per app session.
  // Stored as a nonisolated var on a private wrapper to work around extension
  // stored-property restrictions; access is always on @MainActor.
  private static var _didRunHealthCheck = false

  func configureUploadService() {
    uploadService.onStatusUpdate = { [weak self] status in
      // Called on @MainActor via DispatchQueue.main.async in GooseUploadService
      self?.lastUploadAt = status.lastUploadTimestamp
      self?.pendingBatchCount = status.pendingBatchCount
      self?.lastSyncedCount = status.lastSyncedCount
    }
  }

  func triggerManualUpload() {
    let sinceTimestamp = lastUploadAt ?? Date().addingTimeInterval(-24 * 3600)

    // WHOOP upload: derive device type from the active descriptor's command characteristic prefix.
    // Gen4 uses a 61080002- prefix; Gen5 uses fd4b0002-. Fall back to GOOSE when no descriptor is set.
    if let whoopID = ble.activeDeviceIdentifier {
      let whoopType: String
      if let desc = ble.activeDescriptor {
        whoopType = desc.commandCharacteristicPrefix.hasPrefix("610800") ? "GEN4" : "GOOSE"
      } else {
        whoopType = "GOOSE"
      }
      uploadService.upload(deviceID: whoopID, deviceType: whoopType, sinceTimestamp: sinceTimestamp)
    }

    // HR monitor upload: trigger when an HR monitor is connected, using the sanitized device name.
    // The upload service default case tags this payload with device_class: "HR_MONITOR".
    let hrManager = ble.hrMonitorManager
    if hrManager.hrConnectionState != "disconnected", let hrPeripheral = hrManager.hrPeripheral {
      let hrDeviceType = hrManager.connectedDeviceName ?? "unknown_hr_monitor"
      uploadService.upload(
        deviceID: hrPeripheral.identifier,
        deviceType: hrDeviceType,
        sinceTimestamp: sinceTimestamp
      )
    }
  }

  func triggerUpload(for result: CaptureFrameWriteResult, deviceEvent: GooseNotificationEvent) {
    guard result.pass, result.errorDescription == nil else { return }
    // sinceTimestamp: 30 seconds ago covers the batch window generously
    let sinceTimestamp = Date().addingTimeInterval(-30)
    uploadService.upload(
      deviceID: deviceEvent.deviceID,
      deviceType: deviceEvent.rustDeviceType,
      sinceTimestamp: sinceTimestamp
    )
  }

  // Explicit health check — always runs regardless of session state.
  // Called after user saves server settings.
  func checkServerHealth() {
    let serverURLString = UserDefaults.standard.string(forKey: RemoteServerStorage.serverURL) ?? ""
    guard !serverURLString.isEmpty else { return }
    GooseAppModel._didRunHealthCheck = true
    Task { @MainActor in self.serverReachable = nil }
    runHealthCheck(serverURLString: serverURLString)
  }

  // Runs the GET /healthz check once per app session when upload is enabled
  // and a server URL is configured. Result is published via serverReachable.
  func triggerHealthCheckIfNeeded() {
    guard !GooseAppModel._didRunHealthCheck else { return }
    let uploadEnabled = UserDefaults.standard.bool(forKey: RemoteServerStorage.uploadEnabled)
    let serverURLString = UserDefaults.standard.string(forKey: RemoteServerStorage.serverURL) ?? ""
    guard uploadEnabled, !serverURLString.isEmpty else { return }
    GooseAppModel._didRunHealthCheck = true
    runHealthCheck(serverURLString: serverURLString)
  }

  private func runHealthCheck(serverURLString: String) {
    DispatchQueue.global(qos: .utility).async { [weak self] in
      guard let self else { return }
      guard let url = URL(string: serverURLString + "/healthz") else {
        Task { @MainActor in self.serverReachable = false }
        return
      }
      var request = URLRequest(url: url)
      request.timeoutInterval = 5
      let semaphore = DispatchSemaphore(value: 0)
      var isReachable = false
      var taskError: String?
      URLSession.shared.dataTask(with: request) { _, response, error in
        if let error {
          taskError = error.localizedDescription
        }
        isReachable = (response as? HTTPURLResponse)?.statusCode == 200
        semaphore.signal()
      }.resume()
      semaphore.wait()
      let logBody = taskError.map { "error=\($0)" } ?? "reachable=\(isReachable)"
      let logTitle = taskError != nil ? "healthz.error" : "healthz"
      Task { @MainActor [weak self] in
        self?.ble.record(level: .debug, source: "upload.health", title: logTitle, body: logBody)
      }

      Task { @MainActor in self.serverReachable = isReachable }
    }
  }
}
