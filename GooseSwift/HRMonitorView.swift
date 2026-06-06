import SwiftUI
import UIKit

struct HRMonitorView: View {
  @Environment(GooseAppModel.self) private var model

  var body: some View {
    HRMonitorContentView(ble: model.ble)
      .environment(model)
  }
}

private struct HRMonitorContentView: View {
  @Environment(GooseAppModel.self) private var model
  var ble: GooseBLEClient
  @State private var selectedDevice: GooseDiscoveredDevice?
  @State private var connectingDeviceID: UUID?

  private var connectedDeviceName: String {
    model.ble.hrMonitorManager.connectedDeviceName ?? "HR Monitor"
  }

  var body: some View {
    ZStack {
      deviceScreenBackground.ignoresSafeArea()
      ScrollView {
        VStack(alignment: .leading, spacing: 0) {
          headerView
            .padding(.bottom, 32)
          bodyView
        }
        .padding(.horizontal, 24)
        .padding(.top, 36)
        .padding(.bottom, 48)
      }
    }
    .navigationTitle("HR Monitor")
    .navigationBarTitleDisplayMode(.inline)
    .toolbarBackground(.hidden, for: .navigationBar)
    .tint(devicePrimaryText)
    .onAppear {
      guard ble.hrConnectionState != "connected" else { return }
      ble.startHRMonitorScan()
    }
    .onDisappear {
      ble.stopHRMonitorScan()
    }
    .onChange(of: ble.hrConnectionState) { _, newValue in
      if newValue == "connected" || newValue == "disconnected" {
        connectingDeviceID = nil
      }
    }
    .sheet(item: $selectedDevice) { device in
      HRMonitorDeviceSheet(device: device) {
        connectingDeviceID = device.id
        selectedDevice = nil
        ble.connectHRMonitor(device)
      }
      .presentationDetents([.height(220)])
      .presentationDragIndicator(.visible)
    }
  }

  @ViewBuilder
  private var headerView: some View {
    switch ble.hrConnectionState {
    case "connected":
      HRMonitorHeader(
        statusText: "CONNECTED",
        statusColor: connectedGreen,
        deviceDisplayName: connectedDeviceName
      )
    case "connecting":
      HRMonitorHeader(
        statusText: "CONNECTING",
        statusColor: secondaryText,
        deviceDisplayName: "HR Monitor"
      )
    default:
      if ble.hrBluetoothState == "unauthorized" {
        HRMonitorHeader(
          statusText: "NOT AUTHORISED",
          statusColor: disconnectedRed,
          deviceDisplayName: "HR Monitor"
        )
      } else if ble.hrBluetoothState == "poweredOff" || ble.hrBluetoothState == "unsupported" {
        HRMonitorHeader(
          statusText: "BLUETOOTH OFF",
          statusColor: disconnectedRed,
          deviceDisplayName: "HR Monitor"
        )
      } else {
        HRMonitorHeader(
          statusText: "SCANNING",
          statusColor: secondaryText,
          deviceDisplayName: "HR Monitor"
        )
      }
    }
  }

  private var hrBluetoothUnavailable: Bool {
    ble.hrBluetoothState == "poweredOff" || ble.hrBluetoothState == "unauthorized" || ble.hrBluetoothState == "unsupported"
  }

  @ViewBuilder
  private var bodyView: some View {
    if hrBluetoothUnavailable {
      bluetoothUnavailableBody
    } else if ble.hrConnectionState == "connected" {
      HRMonitorConnectedPanel(ble: ble, disconnectAction: {
        ble.disconnectHRMonitor()
      })
      .animation(.easeOut(duration: 0.16), value: ble.hrConnectionState == "connected")
    } else {
      HRMonitorScanList(
        ble: ble,
        connectingDeviceID: connectingDeviceID,
        onSelectDevice: { device in
          selectedDevice = device
        }
      )
      .animation(.easeOut(duration: 0.16), value: ble.hrConnectionState == "connected")
    }
  }

  @ViewBuilder
  private var bluetoothUnavailableBody: some View {
    let copy: String = ble.hrBluetoothState == "unauthorized"
      ? "Bluetooth access is required. Open Settings to allow access."
      : "Enable Bluetooth to scan for HR monitors."
    Text(copy)
      .font(deviceBodyFont)
      .foregroundStyle(mutedText)
      .accessibilityLabel("Bluetooth is off. Enable Bluetooth to scan for HR monitors.")
  }
}

private struct HRMonitorHeader: View {
  let statusText: String
  let statusColor: Color
  let deviceDisplayName: String

  var body: some View {
    HStack(alignment: .bottom, spacing: 16) {
      VStack(alignment: .leading, spacing: 8) {
        Text(statusText)
          .font(deviceLabelFont)
          .foregroundStyle(statusColor)
          .accessibilityHidden(true)
        Text(deviceDisplayName.uppercased())
          .font(deviceBodyFont.weight(.black))
          .foregroundStyle(devicePrimaryText)
      }
      .frame(maxWidth: .infinity, alignment: .leading)
    }
  }
}

private struct HRMonitorScanList: View {
  var ble: GooseBLEClient
  let connectingDeviceID: UUID?
  let onSelectDevice: (GooseDiscoveredDevice) -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      Text("DISCOVERED")
        .font(deviceLabelFont)
        .foregroundStyle(secondaryText)

      if ble.discoveredHRDevices.isEmpty {
        Text("No HR monitors found. Make sure your device is nearby and powered on.")
          .font(deviceBodyFont)
          .foregroundStyle(mutedText)
      } else {
        VStack(spacing: 0) {
          ForEach(ble.discoveredHRDevices) { device in
            HRMonitorDeviceRow(
              device: device,
              isConnecting: connectingDeviceID == device.id
            )
            .contentShape(Rectangle())
            .onTapGesture {
              onSelectDevice(device)
            }
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

private struct HRMonitorDeviceRow: View {
  let device: GooseDiscoveredDevice
  let isConnecting: Bool

  var body: some View {
    HStack(spacing: 12) {
      VStack(alignment: .leading, spacing: 4) {
        Text(device.name)
          .font(deviceBodyFont.weight(.black))
          .foregroundStyle(devicePrimaryText)
        Text("\(device.rssi) dBm")
          .font(.system(size: 12, weight: .bold))
          .foregroundStyle(mutedText)
      }
      Spacer(minLength: 16)
      if isConnecting {
        ProgressView()
          .scaleEffect(0.8)
          .tint(secondaryText)
          .accessibilityLabel("Connecting to \(device.name)")
      }
    }
    .padding(.vertical, 12)
    .accessibilityLabel("\(device.name), \(device.rssi) dBm")
  }
}

private struct HRMonitorConnectedPanel: View {
  var ble: GooseBLEClient
  let disconnectAction: () -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 24) {
      VStack(alignment: .leading, spacing: 8) {
        Text("HEART RATE")
          .font(deviceLabelFont)
          .foregroundStyle(secondaryText)
        HStack(alignment: .bottom, spacing: 4) {
          let bpmText = ble.liveHeartRateBPM.map(String.init) ?? "--"
          Text(bpmText)
            .font(.system(size: 52, weight: .black))
            .foregroundStyle(devicePrimaryText)
            .accessibilityLabel(
              ble.liveHeartRateBPM.map { "\($0) beats per minute" } ?? "Heart rate unavailable"
            )
          Text("BPM")
            .font(deviceBodyFont)
            .foregroundStyle(secondaryText)
            .padding(.bottom, 8)
        }
      }

      if ble.hrReconnectState != "idle" {
        Text(ble.hrReconnectState.localizedHRReconnectState)
          .font(deviceLabelFont)
          .foregroundStyle(disconnectedRed)
      }

      Button("Disconnect", role: .destructive) {
        disconnectAction()
      }
      .frame(maxWidth: .infinity, minHeight: 46)
      .foregroundStyle(disconnectedRed)
      .background(controlBackground, in: RoundedRectangle(cornerRadius: 8))
      .accessibilityLabel("Disconnect HR monitor")
    }
  }
}

private struct HRMonitorDeviceSheet: View {
  let device: GooseDiscoveredDevice
  let onConnect: () -> Void
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    VStack(spacing: 20) {
      VStack(alignment: .leading, spacing: 8) {
        Text(device.name.uppercased())
          .font(.system(size: 17, weight: .black))
          .foregroundStyle(devicePrimaryText)
        Text("\(device.rssi) dBm")
          .font(deviceLabelFont)
          .foregroundStyle(secondaryText)
      }
      .frame(maxWidth: .infinity, alignment: .leading)

      Button("Connect") {
        onConnect()
        dismiss()
      }
      .frame(maxWidth: .infinity, minHeight: 50)
      .font(.system(size: 17, weight: .black))
      .foregroundStyle(.white)
      .background(connectedGreen, in: RoundedRectangle(cornerRadius: 12))
      .accessibilityLabel("Connect to \(device.name)")
    }
    .padding(24)
    .background(deviceScreenBackground)
  }
}

// MARK: - Visual tokens (file-scope private lets — each file declares its own copies per project convention)

private let deviceScreenBackground = GooseTheme.appBackground
private let devicePrimaryText = Color(uiColor: .label)
private let controlBackground = Color(uiColor: UIColor { traits in
  traits.userInterfaceStyle == .dark
    ? UIColor(red: 0.12, green: 0.16, blue: 0.18, alpha: 1)
    : .secondarySystemGroupedBackground
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
private let deviceLabelFont = Font.system(size: 15, weight: .black, design: .default)
private let deviceBodyFont = Font.system(size: 17, weight: .bold, design: .default)
