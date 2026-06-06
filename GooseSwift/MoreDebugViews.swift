import SwiftUI

struct MoreDebugView: View {
  @Environment(GooseAppModel.self) private var model
  @EnvironmentObject private var packetMonitor: PacketMonitorModel
  @ObservedObject var store: MoreDataStore
  @AppStorage(OnboardingStorage.onboardingComplete) private var onboardingComplete = false
  @AppStorage(OnboardingStorage.onboardingRedoRequested) private var onboardingRedoRequested = false
  @State private var showDestructiveConfirmation = false

  var body: some View {
    List {
      Section("Rust And Parser") {
        MoreInfoRow(title: "Rust Bridge/Core", value: store.coreVersionStatus, systemImage: "shippingbox", status: store.coreVersionStatus.hasPrefix("Rust core") ? .ready : .pending)
        MoreInfoRow(title: "Frame Parse", value: store.frameParseStatus, systemImage: "curlybraces.square", status: store.frameParseStatus.hasPrefix("Parsed") ? .ready : .pending)
        MoreInfoRow(title: "CRC", value: store.frameCRCStatus, systemImage: "checkmark.seal", status: .pending)
        MoreInfoRow(title: "Payload", value: store.framePayloadStatus, systemImage: "doc.text.magnifyingglass", status: .pending)
        MoreInfoRow(title: "Warnings", value: store.frameWarningsStatus, systemImage: "exclamationmark.triangle", status: store.frameWarningsStatus == "No warnings" ? .ready : .stale)
        MoreInfoRow(title: "Timeline", value: store.frameTimelineStatus, systemImage: "timeline.selection", status: .pending)
        Button {
          store.runFrameParseProbe()
        } label: {
          Label("Run Parser Probe", systemImage: "play.circle")
        }
      }

      Section("Debug Session") {
        MoreInfoRow(title: "WebSocket", value: store.debugWebSocketStatus, systemImage: "network", status: store.debugWebSocketStatus.contains("started") ? .ready : .pending)
        MoreInfoRow(title: "Next Action", value: store.debugNextAction, systemImage: "arrow.forward.circle", status: .pending)
        Button {
          store.startDebugSession()
        } label: {
          Label("Start Debug Session", systemImage: "play.circle")
        }
        Button {
          store.refreshDebugSnapshot()
        } label: {
          Label("Refresh Snapshot", systemImage: "arrow.clockwise")
        }
      }

      Section("Health Packet Capture") {
        MoreInfoRow(
          title: "Connection",
          value: "\(model.ble.connectionState) | \(model.ble.activeDeviceName)",
          systemImage: "sensor.tag.radiowaves.forward",
          status: model.ble.connectionState == "ready" ? .ready : .blocked
        )
        MoreInfoRow(
          title: "Session",
          value: model.healthPacketCaptureStatus.localizedCaptureStatus,
          systemImage: "record.circle",
          status: self.healthPacketCaptureStatus
        )
        MoreInfoRow(
          title: "Targets",
          value: model.healthPacketCaptureTargetSummary.localizedCaptureTargetSummary,
          systemImage: "scope",
          status: model.healthPacketCaptureFamilyRows.isEmpty ? .pending : .ready
        )
        MoreInfoRow(
          title: "Last Packet",
          value: model.healthPacketCaptureLastPacketSummary,
          systemImage: "waveform.path.ecg.rectangle",
          status: model.healthPacketCaptureLastPacketSummary == "No packets captured" ? .pending : .ready
        )
        MoreInfoRow(
          title: "Live Data",
          value: packetMonitor.liveDeviceDataSummary,
          systemImage: "dot.radiowaves.left.and.right",
          status: packetMonitor.recentDeviceSignalPoints.isEmpty ? .pending : .ready
        )
        MoreInfoRow(
          title: "Historical",
          value: "\(model.ble.historicalSyncStatus) | packets \(model.ble.historicalPacketCount)",
          systemImage: "arrow.triangle.2.circlepath",
          status: model.ble.isHistoricalSyncing ? .pending : (model.ble.lastHistoricalSyncCompletedAt == nil ? .pending : .ready)
        )
        MoreInfoRow(
          title: "RR Watch",
          value: model.respiratoryPacketWatchStatus,
          systemImage: "lungs",
          status: self.respiratoryPacketWatchStatus
        )
        MoreActionRow(
          title: model.healthPacketCaptureSessionID == nil ? "Start Walk Capture" : "Stop Capture",
          detail: model.healthPacketCaptureSessionID == nil ? "Starts a 30 minute WHOOP movement, HR, GPS, and activity candidate capture" : model.healthPacketCaptureTargetSummary,
          systemImage: model.healthPacketCaptureSessionID == nil ? "figure.walk.circle" : "stop.circle",
          status: self.healthPacketCaptureActionStatus,
          disabled: model.healthPacketCaptureSessionID == nil && model.ble.connectionState != "ready"
        ) {
          if model.healthPacketCaptureSessionID == nil {
            model.startHealthPacketCapture()
          } else {
            model.stopHealthPacketCapture()
          }
        }
        MoreActionRow(
          title: "Start Physiology Capture",
          detail: "Full-rate K10/K11/R17/R21/K25/K26 streams into the capture DB",
          systemImage: "waveform.path.ecg.rectangle",
          status: self.healthPacketCaptureActionStatus,
          disabled: model.healthPacketCaptureSessionID != nil || model.ble.connectionState != "ready"
        ) {
          model.startPhysiologyPacketCapture()
        }
        MoreActionRow(
          title: "Start Temperature Capture",
          detail: "Event 17 plus K18/K24 history",
          systemImage: "thermometer.medium",
          status: self.temperatureCaptureActionStatus,
          disabled: model.healthPacketCaptureSessionID != nil
            || model.ble.connectionState != "ready"
            || (!model.ble.canSyncHistorical && !model.ble.isHistoricalSyncing)
        ) {
          model.startTemperaturePacketCapture()
        }
        MoreActionRow(
          title: model.respiratoryPacketWatchActive ? "Stop RR Packet Watch" : "Watch K18 RR Packets",
          detail: model.respiratoryPacketWatchStatus,
          systemImage: "lungs",
          status: self.respiratoryPacketWatchStatus,
          disabled: !model.respiratoryPacketWatchActive && model.ble.connectionState != "ready"
        ) {
          if model.respiratoryPacketWatchActive {
            model.stopRespiratoryPacketWatch()
          } else {
            model.startRespiratoryPacketWatch()
          }
        }
        if model.healthPacketCaptureFamilyRows.isEmpty {
          MoreInfoRow(
            title: "Families",
            value: "No decoded packet families in this capture yet",
            systemImage: "list.bullet.rectangle",
            status: .pending
          )
        } else {
          ForEach(model.healthPacketCaptureFamilyRows.prefix(10)) { family in
            MoreInfoRow(
              title: "\(family.title) x\(family.count)",
              value: family.detail,
              systemImage: self.healthPacketFamilyIcon(family),
              status: self.healthPacketFamilyStatus(family)
            )
          }
        }
      }

      Section("WHOOP Movement Test") {
        MoreInfoRow(
          title: "Connection",
          value: "\(model.ble.connectionState) | \(model.ble.activeDeviceName)",
          systemImage: "sensor.tag.radiowaves.forward",
          status: model.ble.connectionState == "ready" ? .ready : .blocked
        )
        MoreInfoRow(
          title: "Last Packet",
          value: packetMonitor.movementPacketStatus,
          systemImage: "waveform.path.ecg",
          status: packetMonitor.movementPacketStatus == "No movement packets" ? .pending : .ready
        )
        MoreInfoRow(
          title: "Detector",
          value: model.activityDetectionStatus.localizedActivityDetectionStatus,
          systemImage: "figure.run.circle",
          status: activityDetectorStatus
        )
        MoreActionRow(
          title: model.movementPacketValidationIsRunning ? "Listening For Movement" : "Run Movement Packet Test",
          detail: model.movementPacketValidationStatus,
          systemImage: "dot.radiowaves.left.and.right",
          status: movementPacketTestStatus,
          disabled: model.movementPacketValidationIsRunning
        ) {
          model.startMovementPacketValidationTest()
        }
      }

      Section("WHOOP Event Signals") {
        MoreInfoRow(
          title: "Latest Event",
          value: packetMonitor.latestWhoopEventStatus,
          systemImage: "waveform.path",
          status: packetMonitor.latestWhoopEventStatus == "No WHOOP events" ? .pending : .ready
        )
        MoreInfoRow(
          title: "Skin Temp Candidate",
          value: packetMonitor.latestSkinTemperatureCandidateStatus,
          systemImage: "thermometer",
          status: packetMonitor.latestSkinTemperatureCandidateStatus == "No skin temperature events" ? .pending : .stale
        )
        MoreInfoRow(
          title: "Latest Data Packet",
          value: packetMonitor.latestWhoopDataPacketStatus,
          systemImage: "waveform.path.ecg.rectangle",
          status: packetMonitor.latestWhoopDataPacketStatus == "No WHOOP data packets" ? .pending : .ready
        )
        MoreInfoRow(
          title: "Capture",
          value: "\(model.ble.physiologyCaptureStatus) | \(model.ble.lastPhysiologyCommandSummary)",
          systemImage: "dot.radiowaves.left.and.right",
          status: model.ble.physiologyCaptureStatus == "Not started" ? .pending : .stale
        )
        MoreInfoRow(
          title: "High Frequency Sync",
          value: "\(model.ble.highFrequencyHistorySyncDisplaySummary) | \(model.ble.lastHighFrequencyHistorySyncResponse)",
          systemImage: "bolt.horizontal",
          status: model.ble.highFrequencyHistorySyncActive ? .ready : .pending
        )
        MoreInfoRow(
          title: "History Temp",
          value: packetMonitor.latestHistoryTemperatureCandidateStatus,
          systemImage: "thermometer.medium",
          status: packetMonitor.latestHistoryTemperatureCandidateStatus == "No history temperature packets" ? .pending : .stale
        )
        MoreInfoRow(
          title: "History RR",
          value: packetMonitor.latestRespiratoryRateCandidateStatus,
          systemImage: "lungs",
          status: packetMonitor.latestRespiratoryRateCandidateStatus == "No respiratory rate candidates" ? .pending : .stale
        )
        MoreInfoRow(
          title: "Pulse Info",
          value: packetMonitor.latestPulseInformationPacketStatus,
          systemImage: "lungs",
          status: packetMonitor.latestPulseInformationPacketStatus == "No pulse information packets" ? .pending : .stale
        )
        MoreInfoRow(
          title: "Optical",
          value: packetMonitor.latestOpticalPacketStatus,
          systemImage: "waveform",
          status: packetMonitor.latestOpticalPacketStatus == "No optical packets" ? .pending : .stale
        )
        MoreInfoRow(
          title: "Raw/Research K20",
          value: packetMonitor.latestRawResearchPacketStatus,
          systemImage: "waveform.path.ecg",
          status: packetMonitor.latestRawResearchPacketStatus == "No raw/research packets" ? .pending : .ready
        )
        MoreInfoRow(
          title: "Realtime Status K2",
          value: packetMonitor.latestRealtimeStatusPacketStatus,
          systemImage: "dot.radiowaves.left.and.right",
          status: packetMonitor.latestRealtimeStatusPacketStatus == "No realtime status packets" ? .pending : .ready
        )
        if !packetMonitor.recentDeviceSignalPoints.isEmpty {
          ForEach(packetMonitor.recentDeviceSignalPoints.prefix(8)) { point in
            MoreInfoRow(
              title: "\(point.family) | \(point.value)",
              value: "\(point.capturedAt.formatted(date: .omitted, time: .standard)) | \(point.detail)",
              systemImage: self.deviceSignalIcon(point.family),
              status: .ready
            )
          }
        }
        MoreActionRow(
          title: "Start Movement + HR Capture",
          detail: "Requests live HR plus K10/K11 movement streams",
          systemImage: "play.circle",
          status: model.ble.connectionState == "ready" ? .pending : .blocked,
          disabled: model.ble.connectionState != "ready"
        ) {
          model.startMovementHeartRateCapture()
        }
        MoreActionRow(
          title: "Stop Movement + HR Capture",
          detail: "Turns live HR plus K10/K11 streams off",
          systemImage: "stop.circle",
          status: model.ble.connectionState == "ready" ? .pending : .blocked,
          disabled: model.ble.connectionState != "ready"
        ) {
          model.stopMovementHeartRateCapture()
        }
        MoreActionRow(
          title: model.ble.highFrequencyHistorySyncActive ? "Exit High Frequency Sync" : "Enter High Frequency Sync",
          detail: "WHOOP Smart Alarm history-sync mode: 180s interval for 2h",
          systemImage: "bolt.horizontal",
          status: model.ble.canWriteHighFrequencyHistorySync ? .pending : .blocked,
          disabled: !model.ble.canWriteHighFrequencyHistorySync
        ) {
          if model.ble.highFrequencyHistorySyncActive {
            model.exitHighFrequencyHistorySync()
          } else {
            model.enterHighFrequencyHistorySync()
          }
        }
      }

      Section("Research BT Commands") {
        MoreInfoRow(
          title: "Connection",
          value: "\(model.ble.connectionState) | \(model.ble.activeDeviceName)",
          systemImage: "sensor.tag.radiowaves.forward",
          status: model.ble.connectionState == "ready" ? .ready : .blocked
        )
        MoreInfoRow(
          title: "Last Result",
          value: model.ble.debugCommandStatus,
          systemImage: "terminal",
          status: self.debugCommandStatusKind
        )
        MoreInfoRow(
          title: "Remote Calls",
          value: "gooseswift://debug-command/<id>?payload=<hex>",
          systemImage: "link",
          status: .pending
        )
        ForEach(model.ble.debugResearchCommands) { command in
          if command.canSendFromButton {
            MoreActionRow(
              title: "Send \(command.title)",
              detail: self.debugCommandDetail(command),
              systemImage: self.debugCommandIcon(command),
              status: self.debugCommandActionStatus(command),
              disabled: model.ble.connectionState != "ready"
            ) {
              _ = model.ble.sendDebugResearchCommand(id: command.id)
            }
          } else {
            MoreInfoRow(
              title: command.title,
              value: "\(self.debugCommandDetail(command)) | \(command.remoteURLExample)",
              systemImage: self.debugCommandIcon(command),
              status: .unavailable
            )
          }
        }
        if model.ble.debugCommandResponses.isEmpty {
          MoreInfoRow(
            title: "Responses",
            value: "No debug command responses yet",
            systemImage: "list.bullet.rectangle",
            status: .pending
          )
        } else {
          ForEach(Array(model.ble.debugCommandResponses.prefix(12))) { response in
            MoreInfoRow(
              title: response.title,
              value: self.debugCommandResponseDetail(response),
              systemImage: response.status == "ok" ? "checkmark.circle" : "exclamationmark.triangle",
              status: response.status == "ok" ? .ready : .stale
            )
          }
        }
      }

      Section("Diagnostics") {
        MoreInfoRow(title: "UI Coverage", value: store.uiCoverageStatus, systemImage: "rectangle.3.group", status: .pending)
        MoreInfoRow(title: "Deferred Surfaces", value: store.deferredSurfaceStatus, systemImage: "rectangle.badge.plus", status: .pending)
        MoreInfoRow(title: "Property Suite", value: store.propertySuiteStatus, systemImage: "checklist", status: .pending)
        MoreInfoRow(title: "Perf Budget", value: store.perfBudgetStatus, systemImage: "speedometer", status: .pending)
        Button {
          store.runUICoverageAudit()
        } label: {
          Label("Run UI Coverage", systemImage: "rectangle.3.group")
        }
        Button {
          store.runPropertySuite()
        } label: {
          Label("Run Property Suite", systemImage: "checklist")
        }
        Button {
          store.runPerfBudget()
        } label: {
          Label("Run Perf Budget", systemImage: "speedometer")
        }
      }

      Section("Command Evidence") {
        MoreInfoRow(title: "Evidence Import", value: store.commandEvidenceImportStatus, systemImage: "doc.text.magnifyingglass", status: .unavailable)
        MoreInfoRow(title: "Gate Sweep", value: store.commandGateSweepStatus, systemImage: "checkmark.shield", status: .pending)
        MoreInfoRow(title: "Capture Plan", value: store.commandCapturePlanStatus, systemImage: "scope", status: store.validationStatusKind(store.commandCapturePlanStatus))
        Button {
          store.loadCommandDefinitions()
        } label: {
          Label("Load Command Definitions", systemImage: "list.bullet.rectangle")
        }
        Button {
          store.runCaptureArrivalPlan()
        } label: {
          Label("Run Capture Arrival Plan", systemImage: "scope")
        }
      }

      Section("Command Shortcuts") {
        ForEach(store.commandGroups) { group in
          MoreCommandGroupRow(group: group)
        }
      }

      Section("Protected Controls") {
        Button {
          showDestructiveConfirmation = true
        } label: {
          Label("Destructive Commands Locked", systemImage: "lock.shield")
        }
        MoreInfoRow(title: "Gate", value: store.destructiveGateStatus, systemImage: "lock", status: .blocked)
      }

#if DEBUG
      Section("Developer") {
        Button {
          model.ble.previewHelloWorldToast()
        } label: {
          Label("Hello World Toast", systemImage: "bell.badge")
        }

        Button {
          model.recordUIAction("ui.debug.redo_onboarding")
          OnboardingProfilePersistence.requestRedoFromDefaults()
          model.onboardingComplete = false
          onboardingComplete = false
          onboardingRedoRequested = true
        } label: {
          Label("Re-do Onboarding", systemImage: "arrow.counterclockwise.circle")
        }
      }
#endif
    }
    .gooseListBackground()
    .navigationTitle("Debug")
    .onAppear {
      model.recordUIAction("page.opened", detail: "More Debug")
      store.refreshBridgeStatus(model: model)
    }
    .alert("Destructive commands are locked", isPresented: $showDestructiveConfirmation) {
      Button("Keep Locked", role: .cancel) {
        store.showDestructiveGate()
      }
    } message: {
      Text("This surface records the gate only. No haptics, firmware, config, or reboot command is sent from this tap.")
    }
  }

  private var movementPacketTestStatus: MoreStatusKind {
    if model.movementPacketValidationIsRunning {
      return .pending
    }
    if model.movementPacketValidationStatus.hasPrefix("Passed") {
      return .ready
    }
    if model.movementPacketValidationStatus.hasPrefix("Failed") || model.movementPacketValidationStatus.hasPrefix("Connect WHOOP") {
      return .blocked
    }
    return .pending
  }

  private var activityDetectorStatus: MoreStatusKind {
    if model.activityDetectionStatus.contains("Candidate") || model.activityDetectionStatus.contains("Movement") {
      return .ready
    }
    return packetMonitor.movementPacketStatus == "No movement packets" ? .pending : .ready
  }

  private var healthPacketCaptureStatus: MoreStatusKind {
    if model.healthPacketCaptureSessionID != nil {
      return .pending
    }
    if model.healthPacketCaptureStatus.hasPrefix("Stopped") {
      return .ready
    }
    if model.healthPacketCaptureStatus.contains("failed") || model.healthPacketCaptureStatus.hasPrefix("Connect WHOOP") {
      return .blocked
    }
    return .pending
  }

  private var healthPacketCaptureActionStatus: MoreStatusKind {
    if model.healthPacketCaptureSessionID != nil {
      return .pending
    }
    return model.ble.connectionState == "ready" ? .pending : .blocked
  }

  private var temperatureCaptureActionStatus: MoreStatusKind {
    if model.healthPacketCaptureSessionID != nil {
      return .blocked
    }
    if model.ble.connectionState != "ready" {
      return .blocked
    }
    return model.ble.canSyncHistorical || model.ble.isHistoricalSyncing ? .pending : .stale
  }

  private var respiratoryPacketWatchStatus: MoreStatusKind {
    if model.respiratoryPacketWatchActive {
      return .pending
    }
    if model.respiratoryPacketWatchStatus.hasPrefix("Found K18") {
      return .ready
    }
    if model.respiratoryPacketWatchStatus.hasPrefix("Connect WHOOP") {
      return .blocked
    }
    if model.respiratoryPacketWatchStatus.hasPrefix("Timed out") {
      return .stale
    }
    return model.ble.connectionState == "ready" ? .pending : .blocked
  }

  private var debugCommandStatusKind: MoreStatusKind {
    if model.ble.debugCommandStatus.contains("SUCCESS") || model.ble.debugCommandStatus.contains("ok:") {
      return .ready
    }
    if model.ble.debugCommandStatus.contains("blocked")
        || model.ble.debugCommandStatus.contains("Unknown")
        || model.ble.debugCommandStatus.contains("failed")
        || model.ble.debugCommandStatus.contains("timeout") {
      return .stale
    }
    return model.ble.connectionState == "ready" ? .pending : .blocked
  }

  private func debugCommandActionStatus(_ command: GooseDebugCommandDefinition) -> MoreStatusKind {
    if model.ble.connectionState != "ready" {
      return .blocked
    }
    return command.risk == "read" ? .pending : .stale
  }

  private func debugCommandDetail(_ command: GooseDebugCommandDefinition) -> String {
    "id \(command.id) | cmd \(command.commandNumber) | \(command.payloadHint) | \(command.risk)"
  }

  private func debugCommandResponseDetail(_ response: GooseDebugCommandResponse) -> String {
    let body = response.responseBodyHex.isEmpty
      ? "no body"
      : "body \(String(response.responseBodyHex.prefix(96)))"
    let payload = response.responsePayloadHex.isEmpty
      ? "no payload"
      : "payload \(String(response.responsePayloadHex.prefix(64)))"
    return "\(response.status) | \(response.result) | seq \(response.sequence) | \(body) | \(payload) | src \(response.source)"
  }

  private func debugCommandIcon(_ command: GooseDebugCommandDefinition) -> String {
    switch command.family {
    case "battery":
      return "battery.100"
    case "optical":
      return "waveform.path.ecg"
    case "movement":
      return "figure.walk.motion"
    case "config":
      return "slider.horizontal.3"
    default:
      return "antenna.radiowaves.left.and.right"
    }
  }

  private func healthPacketFamilyStatus(_ family: HealthPacketCaptureFamily) -> MoreStatusKind {
    switch family.status {
    case .target:
      return .ready
    case .expected:
      return .pending
    case .unresolved:
      return .stale
    case .unknown:
      return .blocked
    }
  }

  private func healthPacketFamilyIcon(_ family: HealthPacketCaptureFamily) -> String {
    switch family.status {
    case .target:
      return "scope"
    case .expected:
      return "waveform.path.ecg"
    case .unresolved:
      return "questionmark.diamond"
    case .unknown:
      return "questionmark.circle"
    }
  }

  private func deviceSignalIcon(_ family: String) -> String {
    switch family {
    case "HR":
      return "heart"
    case "Motion", "R21 IMU":
      return "figure.walk.motion"
    case "K2":
      return "dot.radiowaves.left.and.right"
    case "K20", "K11":
      return "waveform.path.ecg"
    case "Optical":
      return "waveform.path.ecg"
    case "Pulse":
      return "lungs"
    case "Skin Temp":
      return "thermometer.medium"
    default:
      return "waveform.path"
    }
  }
}
