import Darwin
import Foundation
import SwiftUI
import UIKit

struct SleepDataBridgeSection: View {
  var store: HealthDataStore
  var ble: GooseBLEClient

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HealthSectionTitle("Sleep Data")
      VStack(spacing: 8) {
        HealthInfoRow(row: HealthSummaryRow("Band history", value: "\(ble.historicalSyncStatus.localizedHistoricalSyncStatus) | \(packetText)", source: .live("WHOOP historical sync"), systemImage: "antenna.radiowaves.left.and.right"))
        HealthInfoRow(row: HealthSummaryRow("Band sleep import", value: store.bandSleepImportStatus, source: .bridge("band historical packets"), systemImage: "square.stack.3d.up"))
        HealthInfoRow(row: HealthSummaryRow("Goose sleep score", value: store.sleepFeatureScoreSummary(), source: store.packetScoreSource("metrics.sleep_score_from_features"), systemImage: "bed.double"))
      }
      HStack(spacing: 10) {
        Button {
          store.markBandSleepSyncRequested(
            automatic: false,
            canSync: ble.canSyncHistorical,
            detail: ble.historicalSyncStatus
          )
          if ble.canSyncHistorical {
            ble.syncHistoricalPackets(rangeFirst: true)
          }
        } label: {
          Label("Sync from band", systemImage: "arrow.down.circle")
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.bordered)
        .disabled(!ble.canSyncHistorical)

        Button {
          store.refreshSleepAfterBandSync(packetCount: ble.historicalPacketCount)
        } label: {
          Label("Refresh Score", systemImage: "chart.xyaxis.line")
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.borderedProminent)
      }
    }
  }

  private var packetText: String {
    ble.historicalPacketCount == 1 ? "1 packet" : "\(ble.historicalPacketCount) packets"
  }
}

enum SleepAlarmConfirmation: Identifiable {
  case set(Date, Int)
  case run(Int)
  case disable

  var id: String {
    switch self {
    case .set(let date, let alarmID):
      return "set-\(alarmID)-\(date.timeIntervalSince1970)"
    case .run(let alarmID):
      return "run-\(alarmID)"
    case .disable:
      return "disable"
    }
  }

  var title: String {
    switch self {
    case .set:
      return "Save Alarm to Band"
    case .run:
      return "Run Band Alarm Now"
    case .disable:
      return "Disable Band Alarms"
    }
  }

  var message: String {
    switch self {
    case .set(let date, _):
      return "Save the alarm for \(date.formatted(date: .abbreviated, time: .shortened)) to the connected band."
    case .run:
      return "Trigger the alarm haptic on the connected band now."
    case .disable:
      return "Disable all alarms on the connected band."
    }
  }
}

struct SleepAlarmBridgeSection: View {
  var ble: GooseBLEClient
  @State private var alarmTime = defaultWakeTime()
  @State private var pendingConfirmation: SleepAlarmConfirmation?
  private let alarmID = 1

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HealthSectionTitle("WHOOP Alarm")
      VStack(spacing: 8) {
        HealthInfoRow(row: HealthSummaryRow("Write support", value: ble.alarmWriteSupportSummary, source: alarmSource, systemImage: "antenna.radiowaves.left.and.right"))
        HealthInfoRow(row: HealthSummaryRow("Last alarm state", value: ble.alarmDisplaySummary, source: alarmSource, systemImage: "bell"))
        HealthInfoRow(row: HealthSummaryRow("Last response", value: ble.lastAlarmResponseSummary, source: .bridge("WHOOP command response"), systemImage: "checkmark.seal"))
        HealthInfoRow(row: HealthSummaryRow("Last event", value: ble.lastAlarmEventSummary, source: .bridge("WHOOP event stream"), systemImage: "waveform.path.ecg"))
        if !ble.lastAlarmCommandFrameHex.isEmpty {
          HealthInfoRow(row: HealthSummaryRow("Last write frame", value: String(ble.lastAlarmCommandFrameHex.prefix(38)), source: .bridge("V5 command frame"), systemImage: "doc.text.magnifyingglass"))
        }
        if !ble.lastAlarmResponsePayloadHex.isEmpty {
          HealthInfoRow(row: HealthSummaryRow("Response hex", value: String(ble.lastAlarmResponsePayloadHex.prefix(38)), source: .bridge("WHOOP command response"), systemImage: "number"))
        }
        if !ble.lastAlarmEventPayloadHex.isEmpty {
          HealthInfoRow(row: HealthSummaryRow("Event hex", value: String(ble.lastAlarmEventPayloadHex.prefix(38)), source: .bridge("WHOOP event stream"), systemImage: "number"))
        }
      }

      VStack(alignment: .leading, spacing: 10) {
        DatePicker("Wake time", selection: $alarmTime, displayedComponents: .hourAndMinute)
          .datePickerStyle(.compact)
      }
      .font(.subheadline.weight(.semibold))
      .padding(12)
      .healthCardSurface()

      HStack(spacing: 10) {
        Button {
          pendingConfirmation = .set(nextWakeDate, alarmID)
        } label: {
          Label("Set Alarm", systemImage: "bell.badge")
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.borderedProminent)
        .disabled(!ble.canWriteAlarm)

        Button {
          pendingConfirmation = .run(alarmID)
        } label: {
          Label("Run Now", systemImage: "waveform")
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.bordered)
        .disabled(!ble.canWriteAlarm)
      }

      Button(role: .destructive) {
        pendingConfirmation = .disable
      } label: {
        Label("Disable WHOOP Alarms", systemImage: "bell.slash")
          .frame(maxWidth: .infinity)
      }
      .buttonStyle(.bordered)
      .disabled(!ble.canWriteAlarm)
    }
    .alert(item: $pendingConfirmation) { confirmation in
      switch confirmation {
      case .set(let date, let alarmID):
        return Alert(
          title: Text(confirmation.title),
          message: Text(confirmation.message),
          primaryButton: .default(Text("Save to Band")) {
            ble.setWhoopAlarm(at: date, alarmID: alarmID)
          },
          secondaryButton: .cancel()
        )
      case .run(let alarmID):
        return Alert(
          title: Text(confirmation.title),
          message: Text(confirmation.message),
          primaryButton: .default(Text("Run Now")) {
            ble.runWhoopAlarmNow(alarmID: alarmID)
          },
          secondaryButton: .cancel()
        )
      case .disable:
        return Alert(
          title: Text(confirmation.title),
          message: Text(confirmation.message),
          primaryButton: .destructive(Text("Disable on Band")) {
            ble.disableWhoopAlarms()
          },
          secondaryButton: .cancel()
        )
      }
    }
  }

  private var nextWakeDate: Date {
    let calendar = Calendar.current
    let components = calendar.dateComponents([.hour, .minute], from: alarmTime)
    let now = Date()
    guard let next = calendar.nextDate(
      after: now,
      matching: components,
      matchingPolicy: .nextTime,
      repeatedTimePolicy: .first,
      direction: .forward
    ) else {
      return now.addingTimeInterval(60)
    }
    return next <= now.addingTimeInterval(10)
      ? (calendar.date(byAdding: .day, value: 1, to: next) ?? now.addingTimeInterval(60))
      : next
  }

  private var alarmSource: HealthDataSource {
    if ble.lastAlarmScheduledAt != nil {
      return .live("WHOOP alarm event")
    }
    if ble.canWriteAlarm {
      return .live("GooseBLEClient alarm write")
    }
    return .unavailable(ble.alarmWriteSupportSummary)
  }

  private static func defaultWakeTime() -> Date {
    Calendar.current.date(bySettingHour: 7, minute: 0, second: 0, of: Date()) ?? Date()
  }
}

