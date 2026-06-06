import SwiftUI
import UIKit

struct DeviceView: View {
  @Environment(GooseAppModel.self) private var model

  var body: some View {
    DeviceContentView(ble: model.ble)
      .environment(model)
  }
}

private enum DevicePanel {
  case status
  case advanced
}

private struct DeviceContentView: View {
  @Environment(GooseAppModel.self) private var model
  @EnvironmentObject private var packetMonitor: PacketMonitorModel
  var ble: GooseBLEClient
  @State private var selectedPanel: DevicePanel = .status

  var body: some View {
    ZStack {
      deviceScreenBackground.ignoresSafeArea()
      ScrollView {
        VStack(alignment: .leading, spacing: 0) {
          DeviceConnectionHeader(
            connected: deviceConnected,
            statusText: connectionHeadline,
            deviceName: ble.activeDeviceName,
            lastSync: lastSyncSummary,
            generation: model.connectedDeviceGeneration
          )
          .padding(.bottom, 30)

          DeviceStatusTabs(selectedPanel: $selectedPanel)
            .padding(.bottom, 46)

          if selectedPanel == .status {
            DeviceImageAndBattery(
              batteryPercent: ble.batteryLevelPercent,
              isCharging: ble.batteryIsCharging == true
            )
          } else {
            DeviceAdvancedPanel(model: model, packetMonitor: packetMonitor, ble: ble)
          }
        }
        .padding(.horizontal, 22)
        .padding(.top, 36)
        .padding(.bottom, 28)
      }
    }
    .navigationTitle("Device")
    .navigationBarTitleDisplayMode(.inline)
    .toolbarBackground(.hidden, for: .navigationBar)
    .tint(devicePrimaryText)
    .toolbar {
      ToolbarItem(placement: .topBarTrailing) {
        Button {
          ble.refreshBatteryLevel()
          ble.refreshDeviceInformation()
        } label: {
          Image(systemName: "battery.75percent")
        }
        .foregroundStyle(devicePrimaryText)
        .accessibilityLabel("Refresh Device")
      }
    }
    .onAppear {
      ble.refreshBatteryLevel()
      ble.refreshDeviceInformation()
    }
    .task {
      while !Task.isCancelled {
        ble.refreshBatteryLevel()
        try? await Task.sleep(for: .seconds(60))
      }
    }
  }

  private var deviceConnected: Bool {
    let state = ble.connectionState.lowercased()
    return state == "ready" || state == "connected" || state == "discovering"
  }

  private var connectionHeadline: String {
    let state = ble.connectionState.lowercased()
    if deviceConnected {
      return "CONNECTED"
    }
    if state == "connecting" {
      return "CONNECTING"
    }
    if ble.isScanning {
      return "SCANNING"
    }
    return "NOT CONNECTED"
  }

  private var lastSyncSummary: String {
    relativeSummary(for: ble.lastSyncAt) ?? "Not synced"
  }
}

private struct DeviceStatusTabs: View {
  @Binding var selectedPanel: DevicePanel

  var body: some View {
    HStack(spacing: 46) {
      DeviceTabButton(
        label: "STATUS",
        selected: selectedPanel == .status
      ) {
        withAnimation(.easeOut(duration: 0.16)) {
          selectedPanel = .status
        }
      }
      DeviceTabButton(
        label: "ADVANCED",
        selected: selectedPanel == .advanced
      ) {
        withAnimation(.easeOut(duration: 0.16)) {
          selectedPanel = .advanced
        }
      }
    }
  }
}

private struct DeviceTabButton: View {
  let label: String
  let selected: Bool
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      VStack(alignment: .leading, spacing: 10) {
        Text(label)
          .font(deviceLabelFont)
          .foregroundStyle(selected ? devicePrimaryText : mutedText)
        Rectangle()
          .fill(devicePrimaryText)
          .frame(width: selected ? underlineWidth : 0, height: 3)
      }
      .frame(width: label == "ADVANCED" ? 96 : 72, alignment: .leading)
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
  }

  private var underlineWidth: CGFloat {
    label == "ADVANCED" ? 76 : 52
  }
}

private struct DeviceImageAndBattery: View {
  let batteryPercent: Int?
  let isCharging: Bool

  var body: some View {
    GeometryReader { proxy in
      let imageWidth = min(max(proxy.size.width * 0.95, 290), 390)
      let percentFontSize = min(max(proxy.size.width * 0.155, 50), 62)
      ZStack(alignment: .topLeading) {
        Image("whoop_gen5_front")
          .resizable()
          .scaledToFit()
          .frame(width: imageWidth, height: 305)
          .offset(x: -imageWidth * 0.28, y: 36)
          .accessibilityLabel("WHOOP strap")

        HStack(alignment: .bottom, spacing: 18) {
          HStack(alignment: .bottom, spacing: 0) {
            Text(batteryText)
              .font(.system(size: percentFontSize, weight: .black, design: .default))
              .foregroundStyle(devicePrimaryText)
              .lineLimit(1)
              .minimumScaleFactor(0.7)
            Text("%")
              .font(.system(size: percentFontSize * 0.42, weight: .black, design: .default))
              .foregroundStyle(devicePrimaryText)
              .padding(.bottom, percentFontSize * 0.08)
          }
          BatteryRail(percent: batteryPercent, isCharging: isCharging)
        }
        .frame(maxWidth: proxy.size.width, alignment: .trailing)
        .padding(.top, 190)
      }
      .frame(width: proxy.size.width, height: 350, alignment: .topLeading)
    }
    .frame(height: 350)
  }

  private var batteryText: String {
    guard let batteryPercent else {
      return "--"
    }
    return "\(batteryPercent)"
  }
}

private struct DeviceConnectionHeader: View {
  let connected: Bool
  let statusText: String
  let deviceName: String
  let lastSync: String
  let generation: String?  // nil when disconnected

  var body: some View {
    HStack(alignment: .bottom, spacing: 16) {
      VStack(alignment: .leading, spacing: 7) {
        Text(statusText)
          .font(deviceLabelFont)
          .foregroundStyle(connected ? connectedGreen : disconnectedRed)
          .lineLimit(1)
        Text(deviceName.uppercased())
          .font(.system(size: 26, weight: .black, design: .default))
          .foregroundStyle(devicePrimaryText)
          .lineLimit(2)
          .minimumScaleFactor(0.78)
        if let gen = generation, gen != "unknown" {
          Text("Gen \(gen.prefix(1))")
            .font(deviceLabelFont)
            .foregroundStyle(secondaryText)
        }
      }
      .frame(maxWidth: .infinity, alignment: .leading)

      VStack(alignment: .trailing, spacing: 7) {
        Text("LAST SYNC")
          .font(deviceLabelFont)
          .foregroundStyle(secondaryText)
        HStack(spacing: 8) {
          Text(lastSync)
            .font(deviceBodyFont.weight(.black))
            .foregroundStyle(devicePrimaryText)
            .lineLimit(1)
            .minimumScaleFactor(0.75)
          Image(systemName: "icloud")
            .font(.system(size: 24, weight: .regular))
            .foregroundStyle(secondaryText)
        }
      }
    }
  }
}

private struct BatteryRail: View {
  let percent: Int?
  let isCharging: Bool
  @State private var chargingPulse = false

  var body: some View {
    ZStack(alignment: .bottom) {
      RoundedRectangle(cornerRadius: 8, style: .continuous)
        .fill(deviceRailBackground)
        .frame(width: 10, height: 138)
      RoundedRectangle(cornerRadius: 8, style: .continuous)
        .fill(fillStyle)
        .frame(width: 10, height: 138 * CGFloat(value))
        .opacity(isCharging ? (chargingPulse ? 1 : 0.62) : 1)
        .shadow(color: isCharging ? batteryYellow.opacity(chargingPulse ? 0.7 : 0.18) : .clear, radius: chargingPulse ? 10 : 2)
      if isCharging {
        Image(systemName: "bolt.fill")
          .font(.system(size: 15, weight: .black))
          .foregroundStyle(batteryYellow)
          .shadow(color: batteryYellow.opacity(0.55), radius: chargingPulse ? 8 : 2)
          .scaleEffect(chargingPulse ? 1.12 : 0.92)
          .offset(y: -150)
          .accessibilityHidden(true)
      }
    }
    .frame(width: 12, height: 138)
    .onAppear {
      chargingPulse = isCharging
    }
    .onChange(of: isCharging) { _, charging in
      chargingPulse = charging
    }
    .animation(
      isCharging ? .easeInOut(duration: 0.9).repeatForever(autoreverses: true) : .default,
      value: chargingPulse
    )
  }

  private var value: Double {
    Double(min(max(percent ?? 0, 0), 100)) / 100
  }

  private var fillStyle: LinearGradient {
    LinearGradient(
      colors: isCharging
        ? [batteryYellow, Color(red: 0.74, green: 1.0, blue: 0.56), batteryYellow]
        : [batteryYellow, batteryYellow],
      startPoint: .bottom,
      endPoint: .top
    )
  }
}

private struct DeviceAdvancedPanel: View {
  @EnvironmentObject private var messageStore: GooseMessageStore
  var model: GooseAppModel
  @ObservedObject var packetMonitor: PacketMonitorModel
  var ble: GooseBLEClient

  var body: some View {
    VStack(alignment: .leading, spacing: 22) {
      DeviceDetailStack {
        DeviceFactRow(systemName: "gearshape", label: "Firmware", value: firmwareSummary)
        DeviceFactRow(systemName: "battery.25percent", label: "Battery", value: batterySummary)
        DeviceFactRow(systemName: ble.batteryIsCharging == true ? "bolt.fill" : "powerplug", label: "Charging", value: ble.batteryChargeDisplayStatus)
        DeviceFactRow(systemName: "arrow.2.circlepath", label: "Last sync", value: relativeSummary(for: ble.lastSyncAt) ?? "Not synced")
        DeviceFactRow(systemName: "clock.arrow.circlepath", label: "Strap clock", value: clockSummary)
      }

      DeviceFactRow(systemName: "iphone", label: "Model", value: modelSummary)

      DeviceDetailStack {
        DeviceFactRow(systemName: "heart", label: "Live HR", value: heartRateSummary)
        DeviceFactRow(systemName: "dot.radiowaves.left.and.right", label: "Connection", value: ble.connectionState.localizedConnectionState)
        DeviceFactRow(systemName: "arrow.triangle.2.circlepath", label: "Historical sync", value: ble.historicalSyncStatus.localizedHistoricalSyncStatus)
        DeviceFactRow(systemName: "bolt.horizontal", label: "High freq", value: ble.highFrequencyHistorySyncDisplaySummary)
        DeviceFactRow(systemName: "lungs", label: "RR packets", value: model.respiratoryPacketWatchStatus)
        DeviceFactRow(systemName: "cpu", label: "Rust", value: model.rustStatus)
        DeviceFactRow(systemName: "waveform.path.ecg", label: "Last frame", value: packetMonitor.lastParsedFrameSummary)
      }

      DeviceActionGrid(model: model, ble: ble)
      DiscoveredDeviceList(ble: ble)
      EventLogPreview(messages: Array(messageStore.messages.prefix(5)))
    }
    .onAppear(perform: refreshClockIfPossible)
    .onChange(of: ble.connectionState) { _, _ in
      refreshClockIfPossible()
    }
  }

  private var firmwareSummary: String {
    ble.firmwareVersion ?? ble.softwareRevision ?? "Unknown"
  }

  private var batterySummary: String {
    guard let battery = ble.batteryLevelPercent else {
      return "Unknown"
    }
    let status = ble.batteryPowerStatus == "Unknown" ? "" : " | \(ble.batteryPowerStatus.localizedBatteryPowerStatus)"
    if let updatedAt = ble.batteryUpdatedAt,
       Date().timeIntervalSince(updatedAt) > 3600,
       let relative = relativeSummary(for: updatedAt) {
      return "\(battery)%\(status) [\(relative)]"
    }
    return "\(battery)%\(status)"
  }

  private var modelSummary: String {
    if let modelNumber = ble.modelNumber {
      return modelNumber
    }
    if let hardwareRevision = ble.hardwareRevision {
      return "Hardware \(hardwareRevision)"
    }
    return ble.activeDeviceName
  }

  private var heartRateSummary: String {
    guard let bpm = ble.liveHeartRateBPM else {
      return ble.liveHeartRateSource.capitalized
    }
    if let updatedAt = ble.liveHeartRateUpdatedAt,
       let relative = relativeSummary(for: updatedAt) {
      return "\(bpm) bpm \(relative)"
    }
    return "\(bpm) bpm"
  }

  private var clockSummary: String {
    guard let offset = ble.strapClockOffsetSeconds else {
      return ble.strapClockStatus.localizedStrapClockStatus
    }
    let drift = formattedClockOffset(offset)
    if let updatedAt = ble.strapClockUpdatedAt,
       let relative = relativeSummary(for: updatedAt) {
      return "\(drift) | \(ble.strapClockStatus.localizedStrapClockStatus) | \(relative)"
    }
    return "\(drift) | \(ble.strapClockStatus.localizedStrapClockStatus)"
  }

  private func refreshClockIfPossible() {
    guard ble.canSyncClock else {
      return
    }
    ble.readStrapClock(syncIfNeeded: true)
  }

  private func formattedClockOffset(_ offset: TimeInterval) -> String {
    let rounded = Int(offset.rounded())
    if rounded == 0 {
      return "0s"
    }
    let sign = rounded > 0 ? "+" : "-"
    return "\(sign)\(abs(rounded))s"
  }
}

private struct DeviceDetailStack<Content: View>: View {
  let content: Content

  init(@ViewBuilder content: () -> Content) {
    self.content = content()
  }

  var body: some View {
    VStack(spacing: 0) {
      content
    }
  }
}

private struct DeviceFactRow: View {
  let systemName: String
  let label: String
  let value: String

  var body: some View {
    HStack(spacing: 12) {
      Image(systemName: systemName)
        .font(.system(size: 20, weight: .semibold))
        .foregroundStyle(secondaryText)
        .frame(width: 24)
      Text(label)
        .font(advancedBodyFont)
        .foregroundStyle(secondaryText)
        .lineLimit(1)
      Spacer(minLength: 16)
      Text(value)
        .font(advancedBodyFont)
        .foregroundStyle(devicePrimaryText)
        .lineLimit(1)
        .minimumScaleFactor(0.72)
        .multilineTextAlignment(.trailing)
    }
    .padding(.vertical, 16)
    .overlay(alignment: .bottom) {
      Rectangle()
        .fill(dividerColor)
        .frame(height: 1)
    }
  }
}

private struct DeviceActionGrid: View {
  var model: GooseAppModel
  var ble: GooseBLEClient

  private let columns = [
    GridItem(.flexible(), spacing: 10),
    GridItem(.flexible(), spacing: 10),
  ]

  var body: some View {
    LazyVGrid(columns: columns, spacing: 10) {
      DeviceActionButton(title: "Bluetooth", systemName: "antenna.radiowaves.left.and.right") {
        ble.requestBluetooth()
      }
      DeviceActionButton(title: ble.isScanning ? "Stop Scan" : "Scan", systemName: "dot.radiowaves.left.and.right") {
        ble.isScanning ? ble.stopScan() : ble.startScan()
      }
      .disabled(!ble.canScan)

      DeviceActionButton(title: "Connect", systemName: "link") {
        ble.connectSelected()
      }
      .disabled(!ble.canConnect)

      DeviceActionButton(title: "Reconnect", systemName: "arrow.clockwise") {
        ble.reconnectRemembered()
      }
      .disabled(!ble.canReconnectRemembered)

      DeviceActionButton(title: ble.isHistoricalSyncing ? "Syncing" : "Sync", systemName: "arrow.triangle.2.circlepath") {
        ble.syncHistoricalPackets()
      }
      .disabled(!ble.canSyncHistorical)

      DeviceActionButton(title: ble.highFrequencyHistorySyncActive ? "Exit HF" : "High Freq", systemName: "bolt.horizontal") {
        if ble.highFrequencyHistorySyncActive {
          ble.exitHighFrequencyHistorySync()
        } else {
          ble.enterHighFrequencyHistorySync()
        }
      }
      .disabled(!ble.canWriteHighFrequencyHistorySync)

      DeviceActionButton(title: model.respiratoryPacketWatchActive ? "Stop RR" : "Watch RR", systemName: "lungs") {
        if model.respiratoryPacketWatchActive {
          model.stopRespiratoryPacketWatch()
        } else {
          model.startRespiratoryPacketWatch()
        }
      }
      .disabled(!model.respiratoryPacketWatchActive && ble.connectionState != "ready")

      DeviceActionButton(title: "Hello", systemName: "paperplane") {
        ble.sendClientHello()
      }
      .disabled(!ble.canSendHello)

      DeviceActionButton(title: "Clock", systemName: "clock.arrow.circlepath") {
        ble.readStrapClock(syncIfNeeded: true)
      }
      .disabled(!ble.canSyncClock)

      DeviceActionButton(title: "Forget", systemName: "trash", role: .destructive) {
        ble.forgetRememberedDevice()
      }
      .disabled(!ble.hasRememberedDevice)
    }
  }
}

private struct DeviceActionButton: View {
  let title: String
  let systemName: String
  var role: ButtonRole?
  let action: () -> Void

  var body: some View {
    Button(role: role, action: action) {
      HStack(spacing: 8) {
        Image(systemName: systemName)
          .font(.system(size: 15, weight: .bold))
        Text(title)
          .font(.system(size: 15, weight: .black, design: .default))
          .lineLimit(1)
          .minimumScaleFactor(0.78)
      }
      .frame(maxWidth: .infinity, minHeight: 46)
      .padding(.horizontal, 10)
      .foregroundStyle(role == .destructive ? disconnectedRed : devicePrimaryText)
      .background(controlBackground, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
    }
    .buttonStyle(.plain)
    .opacity(isDisabled ? 0.45 : 1)
  }

  @Environment(\.isEnabled) private var isEnabled

  private var isDisabled: Bool {
    !isEnabled
  }
}

private func generationMajorVersion(_ generation: String) -> String {
  // "4.0" -> "4", "5.0" -> "5", "unknown" -> "?"
  generation == "unknown" ? "?" : String(generation.prefix(1))
}

private struct DiscoveredDeviceList: View {
  var ble: GooseBLEClient

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      Text("DISCOVERED")
        .font(deviceLabelFont)
        .foregroundStyle(secondaryText)
      if ble.discoveredDevices.isEmpty {
        Text("No devices yet")
          .font(deviceBodyFont)
          .foregroundStyle(mutedText)
          .frame(maxWidth: .infinity, alignment: .leading)
      } else {
        VStack(spacing: 0) {
          ForEach(ble.discoveredDevices) { device in
            Button {
              ble.select(device)
            } label: {
              HStack(spacing: 12) {
                VStack(alignment: .leading, spacing: 4) {
                  Text(device.name)
                    .font(deviceBodyFont.weight(.black))
                    .foregroundStyle(devicePrimaryText)
                    .lineLimit(1)
                  Text("Gen \(generationMajorVersion(device.generation)) · \(device.rssi) dBm")
                    .font(.system(size: 12, weight: .semibold, design: .default))
                    .foregroundStyle(mutedText)
                    .lineLimit(1)
                }
                Spacer()
              }
              .padding(.vertical, 13)
              .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .overlay(alignment: .bottom) {
              Rectangle()
                .fill(dividerColor)
                .frame(height: 1)
            }
          }
        }
      }
    }
  }
}

private struct EventLogPreview: View {
  let messages: [GooseMessage]

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      Text("EVENTS")
        .font(deviceLabelFont)
        .foregroundStyle(secondaryText)
      if messages.isEmpty {
        Text("No events yet")
          .font(deviceBodyFont)
          .foregroundStyle(mutedText)
      } else {
        VStack(spacing: 0) {
          ForEach(messages) { message in
            VStack(alignment: .leading, spacing: 5) {
              HStack(spacing: 8) {
                Text(message.timestamp, style: .time)
                Text(message.level.rawValue.uppercased())
                Text(message.source)
              }
              .font(.system(size: 12, weight: .bold, design: .default))
              .foregroundStyle(mutedText)

              Text(message.title)
                .font(.system(size: 15, weight: .black, design: .default))
                .foregroundStyle(devicePrimaryText)
                .lineLimit(1)

              if !message.body.isEmpty {
                Text(message.body)
                  .font(.system(size: 12, weight: .semibold, design: .monospaced))
                  .foregroundStyle(secondaryText)
                  .lineLimit(2)
              }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.vertical, 12)
            .overlay(alignment: .bottom) {
              Rectangle()
                .fill(dividerColor)
                .frame(height: 1)
            }
          }
        }
      }
    }
  }
}

private func relativeSummary(for date: Date?) -> String? {
  guard let date else {
    return nil
  }
  if abs(date.timeIntervalSinceNow) < 10 {
    return "Now"
  }
  let formatter = RelativeDateTimeFormatter()
  formatter.unitsStyle = .short
  return formatter.localizedString(for: date, relativeTo: Date()).capitalized
}

private let deviceScreenBackground = GooseTheme.appBackground
private let devicePrimaryText = Color(uiColor: .label)
private let controlBackground = Color(uiColor: UIColor { traits in
  traits.userInterfaceStyle == .dark
    ? UIColor(red: 0.12, green: 0.16, blue: 0.18, alpha: 1)
    : .secondarySystemGroupedBackground
})
private let deviceRailBackground = Color(uiColor: UIColor { traits in
  traits.userInterfaceStyle == .dark
    ? UIColor(red: 0.23, green: 0.25, blue: 0.27, alpha: 1)
    : .systemGray4
})
private let dividerColor = Color(uiColor: UIColor { traits in
  traits.userInterfaceStyle == .dark
    ? UIColor(red: 0.19, green: 0.22, blue: 0.25, alpha: 1)
    : .separator
})
private let secondaryText = Color(uiColor: UIColor { traits in
  traits.userInterfaceStyle == .dark
    ? UIColor(red: 0.63, green: 0.65, blue: 0.67, alpha: 1)
    : .secondaryLabel
})
private let mutedText = Color(uiColor: UIColor { traits in
  traits.userInterfaceStyle == .dark
    ? UIColor(red: 0.56, green: 0.58, blue: 0.60, alpha: 1)
    : .tertiaryLabel
})
private let connectedGreen = Color(red: 0.42, green: 0.84, blue: 0.30)
private let disconnectedRed = Color(red: 1.0, green: 0.27, blue: 0.23)
private let batteryYellow = Color(red: 1.0, green: 0.89, blue: 0.36)
private let deviceLabelFont = Font.system(size: 15, weight: .black, design: .default)
private let deviceBodyFont = Font.system(size: 17, weight: .bold, design: .default)
private let advancedBodyFont = Font.system(size: 17, weight: .regular, design: .default)
