import SwiftUI

struct ConnectionView: View {
  @Environment(GooseAppModel.self) private var model

  var body: some View {
    ConnectionContentView(ble: model.ble)
      .environment(model)
  }
}

private struct ConnectionContentView: View {
  @Environment(GooseAppModel.self) private var model
  @EnvironmentObject private var messageStore: GooseMessageStore
  var ble: GooseBLEClient

  var body: some View {
    List {
      Section("Status") {
        LabeledContent("Bluetooth", value: ble.bluetoothState.localizedBluetoothState)
        LabeledContent("Connection", value: ble.connectionState.localizedConnectionState)
        // Reconnect row shows "reconnecting (attempt N/10)" during backoff (Task 1).
        LabeledContent("Reconnect", value: ble.reconnectState.localizedReconnectState)
        LabeledContent("HR Reconnect", value: ble.hrReconnectState.localizedHRReconnectState)
        LabeledContent("Historical", value: historicalSyncValue)
        LabeledContent("Remembered", value: ble.rememberedDeviceDescription)
        LabeledContent("Live HR", value: liveHeartRateValue)
        LabeledContent("Rust", value: model.rustStatus)
        LabeledContent("Hello", value: model.helloSummary)
      }

      Section("Actions") {
        Button("Request Bluetooth") {
          ble.requestBluetooth()
        }
        Button(ble.isScanning ? "Stop Scan" : "Scan") {
          ble.isScanning ? ble.stopScan() : ble.startScan()
        }
        .disabled(!ble.canScan)

        Button("Connect Selected") {
          ble.connectSelected()
        }
        .disabled(!ble.canConnect)

        Button("Reconnect Remembered") {
          ble.reconnectRemembered()
        }
        .disabled(!ble.canReconnectRemembered)

        if ble.isReconnecting {
          Button("Stop Reconnecting") {
            ble.stopReconnect()
          }
        }

        if ble.reconnectFailed {
          Button("Try Again") {
            ble.retryReconnect()
          }
          Text("Reconnection failed after 10 attempts. Tap \"Try Again\" to restart.")
            .font(.caption)
            .foregroundStyle(.secondary)
        }

        if ble.hrIsReconnecting {
          Button("Stop HR Reconnect") {
            ble.stopHRReconnect()
          }
        }

        if ble.hrReconnectFailed {
          Button("Retry HR Reconnect") {
            ble.retryHRReconnect()
          }
          Text("HR monitor reconnection failed after 10 attempts. Tap \"Retry HR Reconnect\" to restart.")
            .font(.caption)
            .foregroundStyle(.secondary)
        }

        Button("Send Client Hello") {
          ble.sendClientHello()
        }
        .disabled(!ble.canSendHello)

        Button(ble.isHistoricalSyncing ? "Syncing Historical Packets" : "Request Historical Packets") {
          ble.syncHistoricalPackets()
        }
        .disabled(!ble.canSyncHistorical)

        Button("Forget Remembered Device", role: .destructive) {
          ble.forgetRememberedDevice()
        }
        .disabled(!ble.hasRememberedDevice)
      }

      Section("Discovered") {
        if ble.discoveredDevices.isEmpty {
          Text("No devices yet")
            .foregroundStyle(.secondary)
        } else {
          ForEach(ble.discoveredDevices) { device in
            Button {
              ble.select(device)
            } label: {
              HStack {
                VStack(alignment: .leading) {
                  Text(device.name)
                  Text("Gen \(device.generation == "unknown" ? "?" : String(device.generation.prefix(1))) · \(device.rssi) dBm")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                }
                Spacer()
                Text("\(device.rssi)")
                  .foregroundStyle(.secondary)
              }
            }
          }
        }
      }

      Section("Client Hello") {
        Text(GooseHello.clientHelloFrameHex)
          .font(.system(.footnote, design: .monospaced))
          .textSelection(.enabled)
      }

      Section("Event Log") {
        ForEach(messageStore.messages) { message in
          VStack(alignment: .leading, spacing: 4) {
            HStack {
              Text(message.timestamp, style: .time)
                .font(.caption)
                .foregroundStyle(.secondary)
              Text(message.level.rawValue.uppercased())
                .font(.caption.weight(.semibold))
                .foregroundStyle(message.level == .error ? .red : .secondary)
              Text(message.source)
                .font(.caption)
                .foregroundStyle(.secondary)
            }
            Text(message.title)
              .font(.subheadline.weight(.semibold))
            Text(message.body)
              .font(.system(.caption, design: .monospaced))
              .foregroundStyle(.secondary)
              .textSelection(.enabled)
          }
        }
      }
    }
    .gooseListBackground()
    .navigationTitle("Connect")
  }

  private var liveHeartRateValue: String {
    guard let bpm = ble.liveHeartRateBPM else {
      return ble.liveHeartRateSource
    }
    if let updatedAt = ble.liveHeartRateUpdatedAt {
      return "\(bpm) bpm via \(ble.liveHeartRateSource) @ \(updatedAt.formatted(date: .omitted, time: .standard))"
    }
    return "\(bpm) bpm via \(ble.liveHeartRateSource)"
  }

  private var historicalSyncValue: String {
    let packetCount = ble.historicalPacketCount
    let packets = "\(packetCount) \(packetCount == 1 ? "packet" : "packets")"
    if ble.isHistoricalSyncing {
      return "\(ble.historicalSyncStatus.localizedHistoricalSyncStatus) | \(packets)"
    }
    if let completedAt = ble.lastHistoricalSyncCompletedAt {
      return "\(ble.historicalSyncStatus.localizedHistoricalSyncStatus) | \(packets) @ \(completedAt.formatted(date: .omitted, time: .standard))"
    }
    return "\(ble.historicalSyncStatus.localizedHistoricalSyncStatus) | \(packets)"
  }
}
