import SwiftUI

enum MoreFileActionKind {
  case captureFile
  case commandEvidence
  case emulatorLog
  case localFrameMatch
  case validatedCommand
}

struct MoreCommandGroup: Identifiable {
  let id: String
  let title: String
  let status: MoreStatusKind
  let commands: [String]

  static let defaults = [
    MoreCommandGroup(id: "identity", title: "Identity", status: .ready, commands: ["GET_HELLO", "GET_DEVICE_INFO"]),
    MoreCommandGroup(id: "battery", title: "Battery", status: .ready, commands: ["READ_BATTERY"]),
    MoreCommandGroup(id: "historical_sync", title: "Historical Sync", status: .pending, commands: ["GET_DATA_RANGE", "SEND_HISTORICAL_DATA"]),
    MoreCommandGroup(id: "haptics", title: "Haptics", status: .blocked, commands: ["SET_ALARM"]),
    MoreCommandGroup(id: "sensors", title: "Sensors", status: .pending, commands: ["READ_SENSORS"]),
    MoreCommandGroup(id: "config", title: "Config", status: .blocked, commands: ["WRITE_CONFIG"]),
    MoreCommandGroup(id: "firmware", title: "Firmware", status: .blocked, commands: ["FIRMWARE_UPDATE"]),
    MoreCommandGroup(id: "reboot", title: "Reboot", status: .blocked, commands: ["REBOOT"]),
  ]

  static func groups(from value: Any) -> [MoreCommandGroup] {
    let dictionaries: [[String: Any]]
    if let rows = value as? [[String: Any]] {
      dictionaries = rows
    } else if let dict = value as? [String: Any], let rows = dict["commands"] as? [[String: Any]] {
      dictionaries = rows
    } else {
      return defaults
    }

    let grouped = Dictionary(grouping: dictionaries) { row -> String in
      (row["group"] as? String)
        ?? (row["category"] as? String)
        ?? (row["family"] as? String)
        ?? "other"
    }

    let rows = grouped.keys.sorted().map { key in
      let commands = grouped[key, default: []].compactMap { row in
        (row["command"] as? String) ?? (row["name"] as? String)
      }
      let status: MoreStatusKind = key == "identity" || key == "battery" ? .ready : .pending
      return MoreCommandGroup(id: key, title: key.replacingOccurrences(of: "_", with: " ").capitalized, status: status, commands: commands.isEmpty ? ["Definitions loaded"] : commands)
    }
    return rows.isEmpty ? defaults : rows
  }
}

struct MoreCaptureView: View {
  @Environment(GooseAppModel.self) private var model
  @EnvironmentObject private var messageStore: GooseMessageStore
  @ObservedObject var store: MoreDataStore

  var body: some View {
    List {
      Section("Session") {
        MoreInfoRow(title: "Capture Session", value: store.captureSessionSummary(), systemImage: "record.circle", status: store.captureSessionID == nil ? .pending : .ready)
        MoreInfoRow(title: "Live Notifications", value: store.liveNotificationCaptureSummary(ble: model.ble), systemImage: "dot.radiowaves.left.and.right", status: model.ble.connectionState == "ready" ? .ready : .pending)
        MoreInfoRow(title: "Selected Device", value: selectedDeviceSummary, systemImage: "sensor.tag.radiowaves.forward", status: selectedDeviceStatus)
        Button {
          if store.captureSessionID == nil {
            store.startCapture(ble: model.ble)
          } else {
            store.stopCapture()
          }
        } label: {
          Label(store.captureSessionID == nil ? "Start Capture" : "Stop Capture", systemImage: store.captureSessionID == nil ? "record.circle" : "stop.circle")
        }
      }

      Section("Overnight Guard") {
        MoreInfoRow(
          title: "Status",
          value: model.overnightGuardStatus.localizedOvernightGuardStatus,
          systemImage: "moon",
          status: overnightGuardStatus
        )
        MoreInfoRow(
          title: "Sleep Readiness",
          value: model.overnightGuardReadinessSummary,
          systemImage: "bed.double",
          status: overnightGuardReadinessStatus
        )
        MoreInfoRow(
          title: "Raw Notifications",
          value: "\(model.overnightGuardRawNotificationCount) | \(model.overnightGuardLastPacketSummary)",
          systemImage: "externaldrive",
          status: model.overnightGuardRawNotificationCount > 0 ? .ready : .pending
        )
        MoreInfoRow(
          title: "Range Polls",
          value: "polls \(model.overnightGuardRangePollCount) | success \(model.overnightGuardSuccessfulRangePollCount) / responses \(model.overnightGuardRangeTelemetryCount) | \(model.ble.lastHistoricalRangeCommandStatus)",
          systemImage: "arrow.triangle.2.circlepath",
          status: model.overnightGuardSuccessfulRangePollCount > 0 ? .ready : (model.overnightGuardRangeTelemetryCount > 0 ? .stale : .pending)
        )
        MoreInfoRow(
          title: "Command Writes",
          value: "\(model.overnightGuardCommandWriteCount) persisted writes",
          systemImage: "arrow.up.doc",
          status: model.overnightGuardCommandWriteCount > 0 ? .ready : .pending
        )
        MoreInfoRow(
          title: "Targets",
          value: model.overnightGuardTargetSummary,
          systemImage: "scope",
          status: model.overnightGuardTargetSummary.contains("K18 0 | K24 0 | K25 0 | K26 0 | packet47 0 | event17 0 | event29 0 | metadata49 0 | metadata56 0") ? .pending : .ready
        )
        MoreInfoRow(
          title: "Historical Order",
          value: model.overnightGuardHistoricalOrderSummary,
          systemImage: "timeline.selection",
          status: model.overnightGuardHistoricalOrderSummary.hasPrefix("no packet47") ? .pending : .ready
        )
        MoreInfoRow(
          title: "Spool",
          value: "\(model.overnightGuardSpoolSizeSummary) | \(model.overnightGuardSpoolPath)",
          systemImage: "folder",
          status: model.overnightGuardSpoolPath == "No overnight spool" ? .pending : .ready
        )
        MoreInfoRow(
          title: "SQLite Mirror",
          value: model.overnightGuardSQLiteMirrorSummary,
          systemImage: "externaldrive.badge.checkmark",
          status: overnightGuardSQLiteMirrorStatus
        )
        MoreInfoRow(
          title: "Power",
          value: model.overnightGuardPowerSummary,
          systemImage: "battery.100percent",
          status: model.overnightGuardPowerSummary.localizedCaseInsensitiveContains("Low Power ON") ? .stale : .ready
        )
        MoreInfoRow(
          title: "Watchdog",
          value: model.overnightGuardWatchdogSummary,
          systemImage: "checkmark.shield",
          status: model.overnightGuardWatchdogSummary.localizedCaseInsensitiveContains("warning") || model.overnightGuardWatchdogSummary.localizedCaseInsensitiveContains("No raw") ? .stale : .ready
        )
        MoreInfoRow(
          title: "Event Log",
          value: "\(model.overnightGuardEventLogCount) persisted events",
          systemImage: "list.bullet.rectangle",
          status: model.overnightGuardEventLogCount > 0 ? .ready : .pending
        )
        MoreInfoRow(
          title: "Final Export",
          value: model.overnightGuardExportStatus,
          systemImage: "square.and.arrow.up",
          status: overnightGuardExportStatus
        )
        MoreInfoRow(
          title: "WHOOP App",
          value: model.overnightGuardWarning,
          systemImage: "exclamationmark.triangle",
          status: .stale
        )
        HStack {
          Button {
            model.startOvernightGuard()
          } label: {
            Label("Start Guard", systemImage: "moon")
          }
          .disabled(model.overnightGuardActive || model.ble.connectionState != "ready")

          Button {
            model.requestOvernightGuardFinalSync()
          } label: {
            Label("Final Sync", systemImage: "arrow.triangle.2.circlepath")
          }
          .disabled(!model.overnightGuardActive || model.ble.isHistoricalSyncing)
        }
        if model.overnightGuardExportInProgress {
          ProgressView("Saving final sync bundle")
        }
        if let exportURL = model.overnightGuardExportURL {
          ShareLink(item: exportURL) {
            Label("AirDrop Final Bundle", systemImage: "square.and.arrow.up")
          }
        }
        if let exportManifestURL = model.overnightGuardExportManifestURL {
          ShareLink(item: exportManifestURL) {
            Label("AirDrop Export Manifest", systemImage: "list.bullet.rectangle")
          }
        }
        if model.overnightGuardCanExportLastSession {
          Button {
            model.exportLastOvernightGuardBundle()
          } label: {
            Label("Export Last Guard", systemImage: "externaldrive.badge.plus")
          }
          .disabled(model.overnightGuardActive || model.overnightGuardExportInProgress)
        }
        Button(role: .destructive) {
          model.stopOvernightGuard()
        } label: {
          Label("Stop Guard", systemImage: "stop.circle")
        }
        .disabled(!model.overnightGuardActive)
      }

      Section("Recent Notifications And Events") {
        if messageStore.messages.isEmpty {
          MoreInfoRow(title: "Events", value: "No BLE events yet", systemImage: "text.badge.plus", status: .pending)
        } else {
          ForEach(messageStore.messages.prefix(8)) { message in
            MoreInfoRow(title: message.title, value: "\(message.source) | \(message.body)", systemImage: icon(for: message.level), status: status(for: message.level))
          }
        }
      }

      Section("Imports And Matching") {
        MoreActionRow(title: "Import Capture File", detail: store.captureImportStatus, systemImage: "doc.badge.plus", status: .unavailable, disabled: true) {
          store.markFileActionUnavailable(.captureFile)
        }
        MoreActionRow(title: "Import Command Evidence", detail: store.commandEvidenceStatus, systemImage: "doc.text.magnifyingglass", status: .unavailable, disabled: true) {
          store.markFileActionUnavailable(.commandEvidence)
        }
        MoreActionRow(title: "Import Emulator Log", detail: store.emulatorLogStatus, systemImage: "terminal", status: .unavailable, disabled: true) {
          store.markFileActionUnavailable(.emulatorLog)
        }
        MoreActionRow(title: "Local Frame Match", detail: store.localFrameMatchStatus, systemImage: "scope", status: .blocked, disabled: true) {
          store.markFileActionUnavailable(.localFrameMatch)
        }
        MoreActionRow(title: "Validated Sample/Read Command", detail: store.validatedCommandStatus, systemImage: "checkmark.shield", status: .blocked, disabled: true) {
          store.markFileActionUnavailable(.validatedCommand)
        }
      }

      Section("Recent Capture Sessions") {
        ForEach(store.recentCaptureSessions, id: \.self) { session in
          MoreInfoRow(title: "Session", value: session, systemImage: "clock.arrow.circlepath", status: session.hasPrefix("No ") ? .pending : .ready)
        }
      }
    }
    .gooseListBackground()
    .navigationTitle("Capture")
    .onAppear {
      model.recordUIAction("page.opened", detail: "More Capture")
      store.refreshRecentCaptureSessions()
    }
  }

  private var selectedDeviceSummary: String {
    if let selected = model.ble.discoveredDevices.first(where: { $0.id == model.ble.selectedDeviceID }) {
      return "\(selected.name) RSSI \(selected.rssi)"
    }
    return model.ble.activeDeviceName
  }

  private var selectedDeviceStatus: MoreStatusKind {
    model.ble.selectedDeviceID == nil && model.ble.connectionState != "ready" ? .pending : .ready
  }

  private var overnightGuardStatus: MoreStatusKind {
    if model.overnightGuardActive {
      return .ready
    }
    if model.overnightGuardStatus.localizedCaseInsensitiveContains("failed")
      || model.overnightGuardStatus.localizedCaseInsensitiveContains("blocked") {
      return .blocked
    }
    if model.overnightGuardStatus.hasPrefix("Stopped") {
      return .stale
    }
    return .pending
  }

  private var overnightGuardReadinessStatus: MoreStatusKind {
    switch model.overnightGuardReadinessStatus {
    case "ready":
      return .ready
    case "blocked":
      return .blocked
    case "unavailable":
      return .unavailable
    case "stale":
      return .stale
    default:
      return .pending
    }
  }

  private var overnightGuardSQLiteMirrorStatus: MoreStatusKind {
    let summary = model.overnightGuardSQLiteMirrorSummary
    if summary.localizedCaseInsensitiveContains("warning") {
      return .stale
    }
    if summary == "SQLite mirror not started" || summary == "SQLite mirror waiting for first flush" {
      return .pending
    }
    if summary.localizedCaseInsensitiveContains("dropped 0") {
      return .ready
    }
    return summary.localizedCaseInsensitiveContains("dropped") ? .stale : .ready
  }

  private var overnightGuardExportStatus: MoreStatusKind {
    if model.overnightGuardExportInProgress {
      return .pending
    }
    if model.overnightGuardExportStatus.localizedCaseInsensitiveContains("failed")
      || model.overnightGuardExportStatus.localizedCaseInsensitiveContains("issue")
      || model.overnightGuardExportStatus.localizedCaseInsensitiveContains("missing") {
      return .stale
    }
    return model.overnightGuardExportURL == nil ? .pending : .ready
  }

  private func icon(for level: GooseLogLevel) -> String {
    switch level {
    case .debug: "terminal"
    case .info: "info.circle"
    case .warn: "exclamationmark.triangle"
    case .error: "xmark.octagon"
    }
  }

  private func status(for level: GooseLogLevel) -> MoreStatusKind {
    switch level {
    case .debug, .info: .ready
    case .warn: .stale
    case .error: .blocked
    }
  }
}

struct MoreLocalStoreView: View {
  @ObservedObject var store: MoreDataStore

  var body: some View {
    List {
      Section("SQLite") {
        MoreInfoRow(title: "Path", value: store.databasePath, systemImage: "externaldrive", status: store.databaseExists ? .ready : .unavailable)
        MoreInfoRow(title: "Storage Check", value: store.storageCheckStatusSummary(), systemImage: "checkmark.seal", status: store.databaseExists ? .pending : .unavailable)
        MoreInfoRow(title: "Schema Version", value: store.schemaVersion, systemImage: "number", status: store.schemaVersion == "Unknown" ? .pending : .ready)
        MoreInfoRow(title: "Next Action", value: store.storageCheckNextActionSummary(), systemImage: "arrow.forward.circle", status: store.databaseExists ? .pending : .blocked)
        Button {
          store.runStorageCheck()
        } label: {
          Label("Check", systemImage: "checkmark.circle")
        }
        .disabled(!store.databaseExists)
      }

      if !store.databaseExists {
        Section("Empty State") {
          MoreInfoRow(title: "No Database Yet", value: "The app has not created goose.sqlite at this path. Capture or bridge-backed flows can create it.", systemImage: "tray", status: .unavailable)
        }
      }
    }
    .gooseListBackground()
    .navigationTitle("Local Store")
  }
}

struct MoreHealthSyncView: View {
  @ObservedObject var store: MoreDataStore

  var body: some View {
    List {
      Section("Backfill Window") {
        MoreInfoRow(title: "Window", value: store.healthSyncBackfillWindowSummary(), systemImage: "calendar", status: store.healthSyncBackfillWindowIssueSummary() == nil ? .ready : .blocked)
        MoreInfoRow(title: "Validation", value: store.healthSyncBackfillWindowIssueSummary() ?? "Window is valid", systemImage: "checkmark.seal", status: store.healthSyncBackfillWindowIssueSummary() == nil ? .ready : .blocked)
        TextField("Start", text: $store.healthBackfillStart)
          .textInputAutocapitalization(.never)
          .keyboardType(.numbersAndPunctuation)
        TextField("End", text: $store.healthBackfillEnd)
          .textInputAutocapitalization(.never)
          .keyboardType(.numbersAndPunctuation)
      }

      Section("Metric Families") {
        MoreInfoRow(title: "Selected", value: store.healthSyncMetricFamilySummary(), systemImage: "list.bullet.rectangle", status: .unavailable)
        ForEach(MoreDataStore.healthFamilies, id: \.self) { family in
          Toggle(family, isOn: Binding(
            get: { store.selectedHealthFamilies.contains(family) },
            set: { store.setHealthFamily(family, enabled: $0) }
          ))
        }
      }

      Section("Sources") {
        ForEach(MoreDataStore.healthFamilies, id: \.self) { family in
          MoreInfoRow(title: family, value: store.healthSyncMetricSourceSummary(family), systemImage: "waveform.path.ecg", status: store.selectedHealthFamilies.contains(family) ? .ready : .pending)
        }
        MoreInfoRow(title: "Unavailable", value: store.unavailableHealthSyncMetricSummary(), systemImage: "minus.circle", status: .unavailable)
      }

      Section("Adapter") {
        MoreInfoRow(title: "Health Adapter", value: store.healthAdapterStatus, systemImage: "heart.text.square", status: adapterStatusKind)
        MoreInfoRow(title: "Authorization", value: store.healthAuthorizationStatus, systemImage: "lock.shield", status: .ready)
        MoreInfoRow(title: "Existing Goose Records", value: store.existingGooseRecordsStatus, systemImage: "externaldrive", status: .pending)
        MoreInfoRow(title: "Imported Sleep History", value: store.importedSleepHistoryStatus, systemImage: "bed.double", status: .pending)
        Button {
          store.refreshHealthAdapter()
        } label: {
          Label("Refresh Health Adapter", systemImage: "arrow.clockwise")
        }
      }

      Section("Dry Runs") {
        Button {
          store.runAppleHealthDryRun()
        } label: {
          Label("Metric Sync Disabled", systemImage: "minus.circle")
        }
        .disabled(!store.canRunAppleHealthDryRun)

        Button {
          store.markHealthConnectUnavailable()
        } label: {
          Label("Health Connect Dry Run", systemImage: "smartphone")
        }
        .disabled(true)

        ForEach(store.healthSyncReports, id: \.self) { report in
          MoreInfoRow(title: "Report", value: report, systemImage: "doc.text", status: report.contains("failed") ? .blocked : .pending)
        }
      }
    }
    .gooseListBackground()
    .navigationTitle("Apple Health Profile")
    .onAppear {
      store.refreshHealthAdapter()
    }
  }

  private var adapterStatusKind: MoreStatusKind {
    store.healthAdapterStatus.contains("available") ? .ready : .unavailable
  }
}
