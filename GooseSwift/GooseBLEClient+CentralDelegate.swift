import CoreBluetooth
import Foundation
import OSLog

extension GooseBLEClient: CBCentralManagerDelegate {
  func centralManager(
    _ central: CBCentralManager,
    willRestoreState dict: [String: Any]
  ) {
    if dispatchCoreBluetoothDelegateToMainIfNeeded({ [weak self] in
      self?.centralManager(central, willRestoreState: dict)
    }) {
      return
    }

    let restored = dict[CBCentralManagerRestoredStatePeripheralsKey] as? [CBPeripheral] ?? []
    record(source: "ble", title: "central.restore_state", body: "peripherals=\(restored.count)")
    guard let peripheral = restored.first else {
      updateReconnectState("restore empty")
      return
    }
    guard let evidence = whoopIdentityEvidence(for: peripheral) else {
      updateReconnectState("restore ignored non-WHOOP")
      rejectNonWhoopPeripheral(peripheral, reason: "restore_without_whoop_evidence", disconnect: true)
      return
    }
    whoopCandidateIDs.insert(peripheral.identifier)
    peripherals[peripheral.identifier] = peripheral
    selectedDeviceID = peripheral.identifier
    activePeripheral = peripheral
    peripheral.delegate = self
    rememberPeripheral(peripheral, evidence: evidence)
    if autoHistoricalSyncOnReady && !prioritizeLiveCaptureOnReady {
      pendingAutomaticHistoricalSyncReason = "restore"
    } else {
      pendingAutomaticHistoricalSyncReason = nil
      record(
        source: "ble.sync",
        title: "historical_sync.auto_skipped",
        body: "reason=restore autoHistoricalSync=\(autoHistoricalSyncOnReady) prioritizeLive=\(prioritizeLiveCaptureOnReady)"
      )
    }
    updateReconnectState("restored")
    switch peripheral.state {
    case .connected:
      let now = Date()
      connectedAt = now
      lastSyncAt = now
      updateConnectionState("discovering")
      peripheral.discoverServices(serviceDiscoveryIDs)
      processCachedServicesIfAvailable(peripheral, reason: "restore.connected")
    case .connecting:
      updateConnectionState("connecting")
    case .disconnected, .disconnecting:
      if central.state == .poweredOn {
        connect(peripheral, reason: "restore")
      }
    @unknown default:
      if central.state == .poweredOn {
        connect(peripheral, reason: "restore")
      }
    }
  }

  func centralManagerDidUpdateState(_ central: CBCentralManager) {
    if dispatchCoreBluetoothDelegateToMainIfNeeded({ [weak self] in
      self?.centralManagerDidUpdateState(central)
    }) {
      return
    }

    updateBluetoothState()
    if central.state == .poweredOn {
      if !startupReconnectAttempted {
        startupReconnectAttempted = true
        attemptAutomaticReconnect(reason: "startup")
      }
    } else {
      isScanning = false
      if isHistoricalSyncing {
        failHistoricalSync("Bluetooth became unavailable during historical sync. State: \(bluetoothState).")
      }
      updateConnectionState("disconnected")
      updateReconnectState("waiting for bluetooth")
      connectedAt = nil
    }
  }

  func centralManager(
    _ central: CBCentralManager,
    didDiscover peripheral: CBPeripheral,
    advertisementData: [String: Any],
    rssi RSSI: NSNumber
  ) {
    if dispatchCoreBluetoothDelegateToMainIfNeeded({ [weak self] in
      self?.centralManager(central, didDiscover: peripheral, advertisementData: advertisementData, rssi: RSSI)
    }) {
      return
    }

    let advertisedName = advertisementData[CBAdvertisementDataLocalNameKey] as? String
    let advertisedServices = advertisedServiceUUIDs(from: advertisementData)
    guard let evidence = whoopIdentityEvidence(
      for: peripheral,
      fallbackName: advertisedName,
      advertisedServices: advertisedServices,
      allowRememberedValidation: false
    ) else {
      rejectNonWhoopPeripheral(peripheral, reason: "scan_without_whoop_evidence", fallbackName: advertisedName)
      return
    }

    whoopCandidateIDs.insert(peripheral.identifier)
    peripherals[peripheral.identifier] = peripheral
    let name = Self.sanitizedWhoopDisplayName(peripheral.name ?? advertisedName ?? "WHOOP strap")
    let serviceUUIDs = advertisedServices
      .map(\.uuidString)
      .joined(separator: ",")
    let device = GooseDiscoveredDevice(
      id: peripheral.identifier,
      name: name,
      rssi: RSSI.intValue
    )

    discoveredDevices.removeAll { $0.id == device.id }
    discoveredDevices.append(device)
    discoveredDevices.sort { $0.rssi > $1.rssi }
    selectedDeviceID = selectedDeviceID ?? device.id
    record(
      source: "ble",
      title: "device.discovered",
      body: "\(name) id=\(device.id.uuidString) rssi=\(device.rssi) services=\(serviceUUIDs) evidence=\(evidence)"
    )

    if autoConnectForPhysiologyCapture && activePeripheral == nil {
      record(source: "ble.sensor", title: "physiology_capture.scan.match", body: "\(peripheral.identifier.uuidString) evidence=\(evidence)")
      autoConnectForPhysiologyCapture = false
      stopScan(reason: "auto_physiology_capture_match")
      connect(peripheral, reason: "auto_physiology_scan")
      return
    }

    if autoReconnectTargetID == peripheral.identifier || shouldAutoConnectDiscoveredWhoop(peripheral) {
      record(source: "ble", title: "reconnect.scan_match", body: "\(peripheral.identifier.uuidString) evidence=\(evidence)")
      autoReconnectTargetID = nil
      stopScan(reason: "auto_reconnect_whoop_match")
      let connectReason = autoStartPhysiologyCaptureOnReady
        ? "auto_physiology_scan_remembered"
        : "auto.scan"
      connect(peripheral, reason: connectReason)
    }
  }

  func centralManager(_ central: CBCentralManager, didConnect peripheral: CBPeripheral) {
    if dispatchCoreBluetoothDelegateToMainIfNeeded({ [weak self] in
      self?.centralManager(central, didConnect: peripheral)
    }) {
      return
    }

    let fallbackName = discoveredDevices.first { $0.id == peripheral.identifier }?.name
    guard let evidence = whoopIdentityEvidence(for: peripheral, fallbackName: fallbackName) else {
      pendingConnectionReason = nil
      autoReconnectInFlight = false
      autoReconnectTargetID = nil
      rejectNonWhoopPeripheral(peripheral, reason: "connected_without_whoop_evidence", fallbackName: fallbackName, disconnect: true)
      return
    }

    whoopCandidateIDs.insert(peripheral.identifier)
    activePeripheral = peripheral
    peripheral.delegate = self
    clientHelloSentForCurrentConnection = false
    autoReconnectInFlight = false
    autoReconnectTargetID = nil
    resetReconnectBackoff()
    let reason = pendingConnectionReason ?? "unknown"
    pendingConnectionReason = nil
    if !prioritizeLiveCaptureOnReady,
       reason == "manual" || reason.hasPrefix("auto.") || reason == "restore" {
      if autoHistoricalSyncOnReady {
        pendingAutomaticHistoricalSyncReason = reason
      } else {
        pendingAutomaticHistoricalSyncReason = nil
        record(
          source: "ble.sync",
          title: "historical_sync.auto_skipped",
          body: "reason=\(reason) autoHistoricalSync=false"
        )
      }
    }
    rememberPeripheral(
      peripheral,
      fallbackName: fallbackName,
      evidence: evidence
    )
    let now = Date()
    connectedAt = now
    lastSyncAt = now
    updateConnectionState("discovering")
    updateReconnectState("connected")
    record(source: "ble", title: "connect.succeeded", body: "\(peripheral.name ?? fallbackName ?? "WHOOP") \(peripheral.identifier.uuidString) evidence=\(evidence)")
    peripheral.discoverServices(serviceDiscoveryIDs)
    processCachedServicesIfAvailable(peripheral, reason: "connect.\(reason)")
  }

  func centralManager(
    _ central: CBCentralManager,
    didFailToConnect peripheral: CBPeripheral,
    error: Error?
  ) {
    if dispatchCoreBluetoothDelegateToMainIfNeeded({ [weak self] in
      self?.centralManager(central, didFailToConnect: peripheral, error: error)
    }) {
      return
    }

    autoReconnectInFlight = false
    autoConnectForPhysiologyCapture = false
    pendingConnectionReason = nil
    updateConnectionState("connect failed")
    record(level: .error, source: "ble", title: "connect.failed", body: error?.localizedDescription ?? "unknown")
    if rememberedDeviceID == peripheral.identifier {
      scheduleReconnectWithBackoff(peripheral, reason: "auto.connect_failed")
    } else {
      updateReconnectState("connect failed")
    }
  }

  func centralManager(
    _ central: CBCentralManager,
    didDisconnectPeripheral peripheral: CBPeripheral,
    error: Error?
  ) {
    if dispatchCoreBluetoothDelegateToMainIfNeeded({ [weak self] in
      self?.centralManager(central, didDisconnectPeripheral: peripheral, error: error)
    }) {
      return
    }

    let shouldReconnect = rememberedDeviceID == peripheral.identifier
    autoReconnectInFlight = false
    autoConnectForPhysiologyCapture = false
    autoStartedPhysiologyCapture = false
    readySyncWorkItem?.cancel()
    if isHistoricalSyncing {
      failHistoricalSync("WHOOP disconnected during historical sync. \(error?.localizedDescription ?? "No CoreBluetooth error was provided.")")
    }
    if pendingAlarmCommand != nil {
      failAlarmCommand("WHOOP disconnected during alarm command. \(error?.localizedDescription ?? "No CoreBluetooth error was provided.")")
    }
    if pendingClockCommand != nil {
      failClockCommand("WHOOP disconnected during clock command. \(error?.localizedDescription ?? "No CoreBluetooth error was provided.")")
    }
    if !pendingDebugCommands.isEmpty {
      failAllDebugCommands("WHOOP disconnected during debug command. \(error?.localizedDescription ?? "No CoreBluetooth error was provided.")")
    }
    updateConnectionState(error?.localizedDescription ?? "disconnected")
    record(
      level: error == nil ? .info : .warn,
      source: "ble",
      title: "disconnect",
      body: error?.localizedDescription ?? peripheral.identifier.uuidString
    )
    activePeripheral = nil
    commandCharacteristic = nil
    batteryLevelCharacteristic = nil
    batteryLevelStatusCharacteristic = nil
    clientHelloSentForCurrentConnection = false
    connectedAt = nil
    if shouldReconnect {
      let reconnectReason = prioritizeLiveCaptureOnReady ? "auto_live_capture_disconnect" : "auto.disconnect"
      if autoHistoricalSyncOnReady && !prioritizeLiveCaptureOnReady {
        pendingAutomaticHistoricalSyncReason = reconnectReason
      } else {
        pendingAutomaticHistoricalSyncReason = nil
        record(
          source: "ble.sync",
          title: "historical_sync.auto_skipped",
          body: "reason=\(reconnectReason) autoHistoricalSync=\(autoHistoricalSyncOnReady) prioritizeLive=\(prioritizeLiveCaptureOnReady)"
        )
      }
      scheduleReconnectWithBackoff(peripheral, reason: reconnectReason)
    }
  }
}
