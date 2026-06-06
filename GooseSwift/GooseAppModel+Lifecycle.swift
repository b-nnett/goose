import Foundation
import UIKit


extension GooseAppModel {
  func handleAppLifecycleChange(_ phase: String) {
    let power = Self.currentOvernightPowerState()
    ble.record(source: "app.lifecycle", title: "scene_phase", body: "\(phase) | \(power.summary)")
    guard overnightGuardActive else {
      return
    }

    applyOvernightPowerState(power)
    if phase == "background" || phase == "inactive" {
      overnightGuardStatus = "Recording overnight guard | app \(phase)"
      let snapshot = overnightRawSpool.synchronizeActive(reason: "scene_phase_\(phase)")
      overnightGuardRawNotificationCount = snapshot.notificationCount
      overnightGuardRangeTelemetryCount = snapshot.historicalRangePollCount
      overnightGuardCommandWriteCount = snapshot.commandWriteCount
      overnightGuardEventLogCount = snapshot.eventLogCount
      overnightGuardSpoolSizeSummary = Self.overnightSpoolSizeSummary(snapshot)
      if let rawURL = snapshot.rawNotificationsURL {
        overnightGuardSpoolPath = rawURL.path
      }
      if snapshot.lastError != nil {
        applyOvernightRawSpoolWarning(
          from: snapshot,
          reason: "lifecycle_spool_\(phase)",
          warningStatus: "Recording overnight guard | app \(phase) | flush warning"
        )
      }
      ble.record(source: "overnight.guard", title: "lifecycle.flush", body: "phase=\(phase) raw=\(snapshot.notificationCount) range=\(snapshot.historicalRangePollCount) commands=\(snapshot.commandWriteCount) events=\(snapshot.eventLogCount)")
    } else if phase == "active" || phase == "foreground" {
      resumeOvernightGuardStreamsIfReady(reason: "scene_phase_\(phase)")
      triggerHealthCheckIfNeeded()
    }
    writeOvernightGuardStatus(reason: "scene_phase_\(phase)")
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
    heartRateHourlyRanges = snapshot.ranges
    heartRateStorageStatus = snapshot.status
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
    } else {
      // Clear on all non-ready states (connecting, discovering, connect timeout, disconnected, etc.)
      // to prevent a stale generation label from the previous connection showing during reconnection.
      connectedDeviceGeneration = nil
      captureFrameWriteQueue.activeDeviceID = nil
    }

    if overnightGuardActive {
      if state == "ready" {
        resumeOvernightGuardStreamsIfReady(reason: "ble_ready")
        if ble.canSyncClock {
          ble.writeClockCommand(.get, syncIfNeeded: true)
          ble.record(source: "ble.clock", title: "clock.auto_sync.triggered", body: "state=ready overnight_guard=true")
        }
      } else {
        passiveActivityCaptureWorkItem?.cancel()
        overnightGuardStatus = "Recording overnight guard | connection \(state)"
        refreshOvernightReadiness(reason: "ble_\(state)", record: true)
        writeOvernightGuardStatus(reason: "ble_\(state)")
      }
      return
    }

    guard state == "ready" else {
      passiveActivityCaptureWorkItem?.cancel()
      refreshOvernightReadiness(reason: "ble_\(state)")
      return
    }
    refreshOvernightReadiness(reason: "ble_ready")
    schedulePassiveActivityCapture(reason: "ble_ready")
    scheduleAutoStartRespiratoryPacketWatchIfNeeded()
    if ble.canSyncClock {
      ble.writeClockCommand(.get, syncIfNeeded: true)
      ble.record(source: "ble.clock", title: "clock.auto_sync.triggered", body: "state=ready")
    }
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

  func beginOvernightGuardCriticalBackgroundTask(reason: String) {
    guard overnightGuardCriticalBackgroundTaskID == .invalid else {
      ble.record(
        source: "overnight.guard",
        title: "background_task.already_active",
        body: "active_reason=\(overnightGuardCriticalBackgroundTaskReason ?? "unknown") requested_reason=\(reason)"
      )
      return
    }

    let taskName = "Goose Overnight \(reason)"
    let taskID = UIApplication.shared.beginBackgroundTask(withName: taskName) { [weak self] in
      Task { @MainActor [weak self] in
        self?.expireOvernightGuardCriticalBackgroundTask()
      }
    }
    if taskID == .invalid {
      overnightGuardCriticalBackgroundTaskReason = nil
      ble.record(level: .warn, source: "overnight.guard", title: "background_task.denied", body: "reason=\(reason)")
      writeOvernightGuardStatus(reason: "background_task_denied")
      return
    }

    overnightGuardCriticalBackgroundTaskID = taskID
    overnightGuardCriticalBackgroundTaskReason = reason
    ble.record(source: "overnight.guard", title: "background_task.started", body: "reason=\(reason)")
    writeOvernightGuardStatus(reason: "background_task_started")
  }

  func expireOvernightGuardCriticalBackgroundTask() {
    let reason = overnightGuardCriticalBackgroundTaskReason ?? "unknown"
    ble.record(level: .warn, source: "overnight.guard", title: "background_task.expired", body: "reason=\(reason)")
    overnightGuardStatus = "Background time expired during \(reason); keep Goose foregrounded if possible"
    endOvernightGuardCriticalBackgroundTask(reason: "expired_\(reason)")
    writeOvernightGuardStatus(reason: "background_task_expired")
  }

  func endOvernightGuardCriticalBackgroundTask(reason: String) {
    let taskID = overnightGuardCriticalBackgroundTaskID
    guard taskID != .invalid else {
      return
    }
    let activeReason = overnightGuardCriticalBackgroundTaskReason ?? "unknown"
    overnightGuardCriticalBackgroundTaskID = .invalid
    overnightGuardCriticalBackgroundTaskReason = nil
    UIApplication.shared.endBackgroundTask(taskID)
    ble.record(source: "overnight.guard", title: "background_task.ended", body: "active_reason=\(activeReason) reason=\(reason)")
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
