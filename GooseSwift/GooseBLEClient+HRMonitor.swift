import CoreBluetooth
import Foundation
import OSLog


final class GooseBLEHRMonitorManager: NSObject, CBCentralManagerDelegate, CBPeripheralDelegate {
  var central: CBCentralManager?
  var discoveredHRDevices: [GooseDiscoveredDevice] = []
  var hrPeripheral: CBPeripheral?
  var hrConnectionState: String = "disconnected"
  var connectedDeviceName: String?
  weak var owner: GooseBLEClient?

  // Reconnect backoff state — all mutations must run on callbackQueue (the BLE queue).
  var reconnectBackoff = ReconnectBackoff()
  var callbackQueue: DispatchQueue?
  var hrReconnectWorkItem: DispatchWorkItem?
  var hrReconnectGeneration: Int = 0
  var pendingHRPeripheral: CBPeripheral?

  func start(queue: DispatchQueue) {
    guard central == nil else { return }
    callbackQueue = queue
    central = CBCentralManager(
      delegate: self,
      queue: queue,
      options: [CBCentralManagerOptionRestoreIdentifierKey: "com.goose.swift.hr-monitor"]
    )
  }

  private func cancelHRReconnectCycle() {
    hrReconnectWorkItem?.cancel()
    hrReconnectWorkItem = nil
    hrReconnectGeneration += 1
  }

  private func scheduleNextHRReconnect() {
    guard let delay = reconnectBackoff.nextDelay() else {
      owner?.updateHRReconnectState("failed after 10 attempts")
      reconnectBackoff.reset()
      return
    }
    guard pendingHRPeripheral != nil else {
      owner?.updateHRReconnectState("idle")
      return
    }
    owner?.updateHRReconnectState(reconnectBackoff.statusString)
    let generation = hrReconnectGeneration
    let item = DispatchWorkItem { [weak self] in
      guard let self,
            self.hrReconnectGeneration == generation,
            let peripheral = self.pendingHRPeripheral
      else { return }
      self.central?.connect(peripheral, options: nil)
    }
    hrReconnectWorkItem = item
    callbackQueue?.asyncAfter(deadline: .now() + delay, execute: item)
  }

  func hrStopReconnect() {
    callbackQueue?.async { [weak self] in
      guard let self else { return }
      self.cancelHRReconnectCycle()
      self.reconnectBackoff.reset()
      self.owner?.updateHRReconnectState("idle")
    }
  }

  func hrRetryReconnect() {
    callbackQueue?.async { [weak self] in
      guard let self else { return }
      guard self.pendingHRPeripheral != nil else {
        self.owner?.updateHRReconnectState("idle")
        return
      }
      self.cancelHRReconnectCycle()
      self.reconnectBackoff.reset()
      self.scheduleNextHRReconnect()
    }
  }

  func startScan() {
    central?.scanForPeripherals(
      withServices: [CBUUID(string: "180D")],
      options: [CBCentralManagerScanOptionAllowDuplicatesKey: false]
    )
  }

  func stopScan() {
    central?.stopScan()
  }

  func connect(_ device: GooseDiscoveredDevice) {
    guard let peripheral = central?.retrievePeripherals(withIdentifiers: [device.id]).first else {
      return
    }
    connectedDeviceName = device.name
    hrConnectionState = "connecting"
    DispatchQueue.main.async { [weak self] in
      self?.owner?.hrConnectionState = "connecting"
    }
    central?.connect(peripheral, options: nil)
  }

  // MARK: - CBCentralManagerDelegate

  func centralManagerDidUpdateState(_ central: CBCentralManager) {
    let stateStr: String
    switch central.state {
    case .poweredOn: stateStr = "poweredOn"
    case .poweredOff: stateStr = "poweredOff"
    case .unauthorized: stateStr = "unauthorized"
    case .unsupported: stateStr = "unsupported"
    case .resetting: stateStr = "resetting"
    default: stateStr = "unknown"
    }
    DispatchQueue.main.async { [weak self] in
      self?.owner?.hrBluetoothState = stateStr
    }
  }

  func centralManager(
    _ central: CBCentralManager,
    willRestoreState dict: [String: Any]
  ) {
    // State restoration not required for manual-only HR monitor connections
  }

  func centralManager(
    _ central: CBCentralManager,
    didDiscover peripheral: CBPeripheral,
    advertisementData: [String: Any],
    rssi RSSI: NSNumber
  ) {
    var rawName = peripheral.name
      ?? (advertisementData[CBAdvertisementDataLocalNameKey] as? String)
      ?? "unknown_hr_monitor"
    rawName = rawName.trimmingCharacters(in: .whitespacesAndNewlines)
    if rawName.isEmpty { rawName = "unknown_hr_monitor" }
    let sanitizedName = String(rawName.prefix(64))

    let device = GooseDiscoveredDevice(
      id: peripheral.identifier,
      name: sanitizedName,
      rssi: RSSI.intValue,
      generation: "hr_monitor"
    )

    if let index = discoveredHRDevices.firstIndex(where: { $0.id == device.id }) {
      discoveredHRDevices[index] = device
    } else {
      discoveredHRDevices.append(device)
    }
    discoveredHRDevices.sort { $0.rssi > $1.rssi }

    DispatchQueue.main.async { [weak self] in
      self?.owner?.discoveredHRDevices = self?.discoveredHRDevices ?? []
    }
  }

  func centralManager(_ central: CBCentralManager, didConnect peripheral: CBPeripheral) {
    hrConnectionState = "connected"
    hrPeripheral = peripheral
    peripheral.delegate = self
    peripheral.discoverServices([CBUUID(string: "180D")])
    cancelHRReconnectCycle()
    reconnectBackoff.reset()
    pendingHRPeripheral = nil
    owner?.updateHRReconnectState("idle")
    DispatchQueue.main.async { [weak self] in
      let previous = self?.owner?.hrConnectionState
      self?.owner?.hrConnectionState = "connected"
      if previous != "connected" {
        self?.owner?.onHRConnectionStateChange?("connected")
      }
    }
  }

  func centralManager(
    _ central: CBCentralManager,
    didDisconnectPeripheral peripheral: CBPeripheral,
    error: Error?
  ) {
    let disconnectedPeripheral = peripheral
    hrConnectionState = "disconnected"
    hrPeripheral = nil
    pendingHRPeripheral = disconnectedPeripheral
    scheduleNextHRReconnect()
    DispatchQueue.main.async { [weak self] in
      let previous = self?.owner?.hrConnectionState
      self?.owner?.hrConnectionState = "disconnected"
      if previous != "disconnected" {
        self?.owner?.onHRConnectionStateChange?("disconnected")
      }
    }
  }

  func centralManager(
    _ central: CBCentralManager,
    didFailToConnect peripheral: CBPeripheral,
    error: Error?
  ) {
    hrConnectionState = "disconnected"
    hrPeripheral = nil
    DispatchQueue.main.async { [weak self] in
      let previous = self?.owner?.hrConnectionState
      self?.owner?.hrConnectionState = "disconnected"
      if previous != "disconnected" {
        self?.owner?.onHRConnectionStateChange?("disconnected")
      }
    }
  }

  // MARK: - CBPeripheralDelegate

  func peripheral(_ peripheral: CBPeripheral, didDiscoverServices error: Error?) {
    guard error == nil, let services = peripheral.services else { return }
    for service in services where service.uuid == CBUUID(string: "180D") {
      peripheral.discoverCharacteristics([CBUUID(string: "2A37")], for: service)
    }
  }

  func peripheral(
    _ peripheral: CBPeripheral,
    didDiscoverCharacteristicsFor service: CBService,
    error: Error?
  ) {
    guard error == nil, let characteristics = service.characteristics else { return }
    for characteristic in characteristics where characteristic.uuid == CBUUID(string: "2A37") {
      peripheral.setNotifyValue(true, for: characteristic)
    }
  }

  func peripheral(
    _ peripheral: CBPeripheral,
    didUpdateValueFor characteristic: CBCharacteristic,
    error: Error?
  ) {
    guard error == nil, characteristic.uuid == CBUUID(string: "2A37") else { return }
    let capturedAt = Date()
    let value = characteristic.value ?? Data()

    // This callback runs on the background CoreBluetooth queue (CBCentralManager was created
    // with that queue). Deliver directly on this queue — do NOT hop to @MainActor or
    // DispatchQueue.main (review MEDIUM-3: HR notifications arrive at high frequency).
    let event = GooseNotificationEvent(
      deviceID: peripheral.identifier,
      serviceUUID: "180D",
      characteristicUUID: "2A37",
      value: value,
      capturedAt: capturedAt
    )
    owner?.onNotification?(event)

    // For live HR display, use the existing method which performs its own main-thread hop
    owner?.handleStandardHeartRate(value, characteristic: characteristic, capturedAt: capturedAt)
  }
}


extension GooseBLEClient {
  func startHRMonitorScan() {
    hrMonitorManager.owner = self
    hrMonitorManager.start(queue: coreBluetoothQueue)
    hrMonitorManager.startScan()
    record(source: "ble.hr_monitor", title: "scan.start")
  }

  func stopHRMonitorScan() {
    hrMonitorManager.stopScan()
    record(source: "ble.hr_monitor", title: "scan.stop")
  }

  func connectHRMonitor(_ device: GooseDiscoveredDevice) {
    hrMonitorManager.connect(device)
    record(source: "ble.hr_monitor", title: "connect.requested", body: device.name)
  }

  func disconnectHRMonitor() {
    hrMonitorManager.stopScan()
    hrMonitorManager.hrStopReconnect()
    if let peripheral = hrMonitorManager.hrPeripheral {
      hrMonitorManager.central?.cancelPeripheralConnection(peripheral)
    }
    hrMonitorManager.hrConnectionState = "disconnected"
    hrMonitorManager.connectedDeviceName = nil
    hrMonitorManager.pendingHRPeripheral = nil
    DispatchQueue.main.async { [weak self] in
      self?.hrConnectionState = "disconnected"
    }
    record(source: "ble.hr_monitor", title: "disconnect.requested")
  }
}
