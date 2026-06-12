import Foundation


extension GooseAppModel {
  // Called by the GooseNetworkMonitor callback whenever connectivity changes.
  // When connectivity returns and there is a deferred upload, clears the pending flag
  // and the visible error state before triggering the upload.
  func handleReachabilityChange(_ reachable: Bool) {
    guard reachable, hasPendingUploadAfterReconnect else { return }
    hasPendingUploadAfterReconnect = false
    uploadErrorState = nil
    triggerManualUpload()
  }

  func handleAppLifecycleChange(_ phase: String) {
    ble.record(source: "app.lifecycle", title: "scene_phase", body: phase)
    if phase == "active" || phase == "foreground" {
      purgeLegacyOvernightGuardDirectory()
      triggerHealthCheckIfNeeded()
      triggerForegroundBLESync()
    }
  }

  // One-shot best-effort cleanup of the legacy overnight session directory left on disk
  // by devices that ran the now-removed overnight guard feature (D-03).
  // Idempotent: a UserDefaults flag prevents repeat filesystem work after the first run.
  // Silent on missing path: try? is a no-op when the directory never existed (safe on all devices).
  private func purgeLegacyOvernightGuardDirectory() {
    let defaults = UserDefaults.standard
    guard !defaults.bool(forKey: "goose.swift.legacyOvernightDirectoryPurged") else {
      return
    }
    let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
      ?? FileManager.default.temporaryDirectory
    let url = documents
      .appendingPathComponent("GooseSwift", isDirectory: true)
      .appendingPathComponent("OvernightGuard", isDirectory: true)
    ble.record(source: "app.lifecycle", title: "overnight.purge", body: url.path)
    try? FileManager.default.removeItem(at: url)
    defaults.set(true, forKey: "goose.swift.legacyOvernightDirectoryPurged")
  }

  func completeOnboarding() {
    onboardingComplete = true
    ble.record(source: "ui", title: "onboarding.complete")
  }

  func recordUIAction(_ title: String, detail: String = "") {
    ble.record(source: "ui", title: title, body: detail)
  }

  @discardableResult
  func handleDebugCommandDeepLink(_ url: URL) -> Bool {
    guard ["gooseswift", "goose"].contains(url.scheme?.lowercased() ?? ""),
          url.host == "debug-command" else {
      return false
    }

    let components = URLComponents(url: url, resolvingAgainstBaseURL: false)
    let queryItems = components?.queryItems ?? []
    let commandID = url.pathComponents.dropFirst().first
      ?? queryItems.first(where: { $0.name == "id" || $0.name == "command" })?.value
      ?? ""
    let payloadHex = queryItems.first(where: { $0.name == "payload" || $0.name == "hex" })?.value
    guard !commandID.isEmpty else {
      ble.record(level: .warn, source: "ble.debug_command", title: "deep_link.invalid", body: url.absoluteString)
      return true
    }

    let normalizedID = commandID.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    guard let command = ble.debugResearchCommands.first(where: { $0.id == normalizedID }) else {
      ble.setDebugCommandStatus("Unknown debug command: \(commandID)")
      ble.record(level: .warn, source: "ble.debug_command", title: "deep_link.unknown", body: commandID)
      return true
    }
    guard command.allowsRemoteInvocation else {
      ble.setDebugCommandStatus("\(command.title) blocked from external deep link")
      ble.record(
        level: .warn,
        source: "ble.debug_command",
        title: "deep_link.blocked",
        body: "\(command.id) risk=\(command.risk)"
      )
      return true
    }

    ble.record(source: "ui", title: "debug_command.deep_link", body: "\(command.id) payload=\(payloadHex ?? "nil")")
    _ = ble.sendDebugResearchCommand(id: command.id, payloadHex: payloadHex, source: "deep_link")
    return true
  }

  func refreshHeartRateHourlyRanges(for date: Date = Date()) {
    heartRateSamplePipeline.refreshHeartRateTimeline(for: date)
  }

  func applyHeartRateTimelineSnapshot(_ snapshot: HeartRateTimelineSnapshot) {
    if snapshot.ranges != heartRateHourlyRanges {
      heartRateHourlyRanges = snapshot.ranges
    }
    if snapshot.status != heartRateStorageStatus {
      heartRateStorageStatus = snapshot.status
    }
  }

  func handleHRConnectionStateChange(_ state: String) {
    switch state {
    case "connected":
      ble.record(source: "health.packet_capture", title: "hr_monitor.auto_start", body: state)
      startHRMonitorCapture(source: "auto.hr_monitor_connected")
    case "disconnected":
      ble.record(source: "health.packet_capture", title: "hr_monitor.auto_stop", body: state)
      stopHRMonitorCapture(reason: "hr_monitor_disconnected")
    default:
      break
    }
  }

  func handleBLEConnectionStateChange(_ state: String) {
    if state == "ready" {
      connectedDeviceGeneration = ble.discoveredDevices
        .first(where: { $0.id == ble.activeDeviceIdentifier })?.generation
      captureFrameWriteQueue.activeDeviceID = ble.activeDeviceIdentifier?.uuidString
      if let uuid = ble.connectedPeripheralUUID {
        captureFrameWriteQueue.currentDeviceUUID = uuid
        let deviceModel = ble.activeDeviceName
        var map = UserDefaults.standard.data(forKey: GooseBLEClient.DefaultsKey.deviceUUIDMap)
          .flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: String] } ?? [:]
        map[uuid] = deviceModel
        if let data = try? JSONSerialization.data(withJSONObject: map) {
          UserDefaults.standard.set(data, forKey: GooseBLEClient.DefaultsKey.deviceUUIDMap)
        }
      }
    } else {
      // Clear on all non-ready states (connecting, discovering, connect timeout, disconnected, etc.)
      // to prevent a stale generation label from the previous connection showing during reconnection.
      connectedDeviceGeneration = nil
      captureFrameWriteQueue.activeDeviceID = nil
      captureFrameWriteQueue.currentDeviceUUID = nil
      alarmIsArmed = false  // HAP-03: armed alarm is unreliable if strap disconnects
    }

    guard state == "ready" else {
      passiveActivityCaptureWorkItem?.cancel()
      return
    }
    schedulePassiveActivityCapture(reason: "ble_ready")
    scheduleAutoStartRespiratoryPacketWatchIfNeeded()
    if ble.canSyncClock {
      ble.writeClockCommand(.get, syncIfNeeded: true)
      ble.record(source: "ble.clock", title: "clock.auto_sync.triggered", body: "state=ready")
    }
    maybeScheduleMorningSleepSync()
  }

  func schedulePassiveActivityCapture(reason: String) {
    guard !autoStartHealthPacketCaptureOnReady,
          !autoStartTemperaturePacketCaptureOnReady,
          !autoStartPhysiologyPacketCaptureOnReady,
          !autoStartRespiratoryPacketWatchOnReady,
          activeHealthPacketCapture == nil else {
      return
    }
    passiveActivityCaptureWorkItem?.cancel()
    let workItem = DispatchWorkItem { [weak self] in
      Task { @MainActor in
        self?.attemptStartPassiveActivityCapture(reason: reason)
      }
    }
    passiveActivityCaptureWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + 2, execute: workItem)
  }

  func attemptStartPassiveActivityCapture(reason: String) {
    passiveActivityCaptureWorkItem?.cancel()
    passiveActivityCaptureWorkItem = nil
    guard ble.connectionState == "ready",
          activeHealthPacketCapture == nil,
          !autoStartPhysiologyPacketCaptureOnReady,
          !activitySession.isActive else {
      return
    }
    ble.record(source: "activity.detect", title: "passive_capture.auto_start", body: reason)
    startHealthPacketCapture(duration: Self.passiveActivityCaptureDuration, source: "auto.passive_activity_detection")
  }

  func startMovementPacketValidationTest(timeout: TimeInterval = 45) {
    ble.record(source: "ui.debug", title: "movement_packet_test.start")
    guard ble.connectionState == "ready" else {
      movementPacketValidationStatus = "Connect WHOOP first. Current state: \(ble.connectionState)"
      movementPacketValidationIsRunning = false
      ble.record(level: .warn, source: "activity.detect", title: "movement_packet_test.blocked", body: ble.connectionState)
      return
    }

    movementPacketValidationTimeoutWorkItem?.cancel()
    movementPacketValidation = MovementPacketValidation(startedAt: Date(), timeout: timeout)
    movementPacketValidationIsRunning = true
    movementPacketValidationStatus = "Listening for real WHOOP movement packets"
    ble.record(source: "activity.detect", title: "movement_packet_test.listening", body: "timeout=\(Int(timeout.rounded()))s")

    let workItem = DispatchWorkItem { [weak self] in
      Task { @MainActor in
        self?.finishMovementPacketValidationTimedOut()
      }
    }
    movementPacketValidationTimeoutWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + timeout, execute: workItem)
  }

  func startPhysiologySignalCapture() {
    ble.startPhysiologySignalCapture()
  }

  func stopPhysiologySignalCapture() {
    ble.stopPhysiologySignalCapture()
  }

  func startMovementHeartRateCapture() {
    ble.startMovementHeartRateCapture()
  }

  func stopMovementHeartRateCapture() {
    ble.stopMovementHeartRateCapture()
  }

  func enterHighFrequencyHistorySync() {
    ble.enterHighFrequencyHistorySync()
  }

  func exitHighFrequencyHistorySync() {
    ble.exitHighFrequencyHistorySync()
  }

}
