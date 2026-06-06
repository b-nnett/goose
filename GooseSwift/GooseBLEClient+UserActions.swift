import CoreBluetooth
import Foundation
import OSLog


extension GooseBLEClient {
  func requestBluetooth() {
    record(source: "ui", title: "request_bluetooth")
    ensureCentral()
    updateBluetoothState()
  }

  func startScan() {
    record(source: "ui", title: "scan.start.requested")
    startScan(reason: "manual", clearDiscovered: true)
  }

  func stopScan() {
    record(source: "ui", title: "scan.stop.requested")
    stopScan(reason: "manual")
  }

  func reconnectRemembered() {
    record(source: "ui", title: "reconnect_remembered.requested")
    ensureCentral()
    attemptAutomaticReconnect(reason: "manual")
  }

  func forgetRememberedDevice() {
    clearRememberedDevice(reason: "manual", source: "ui")
  }

  @discardableResult
  func sendDebugResearchCommand(
    id: String,
    payloadHex: String? = nil,
    source: String = "ui.debug"
  ) -> Bool {
    let normalizedID = id.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    guard let definition = Self.debugResearchCommandDefinitions.first(where: { $0.id == normalizedID }) else {
      setDebugCommandStatus("Unknown debug command: \(id)")
      record(level: .warn, source: "ble.debug_command", title: "command.unknown", body: id)
      return false
    }
    guard !isHistoricalSyncing else {
      setDebugCommandStatus("\(definition.title) blocked during historical sync")
      record(level: .warn, source: "ble.debug_command", title: "command.blocked", body: debugCommandStatus)
      return false
    }
    guard let activePeripheral, let commandCharacteristic else {
      setDebugCommandStatus("\(definition.title) needs active WHOOP command characteristic")
      record(level: .warn, source: "ble.debug_command", title: "command.blocked", body: debugCommandStatus)
      return false
    }
    guard connectionState == "ready" else {
      setDebugCommandStatus("\(definition.title) needs ready connection; current state \(connectionState)")
      record(level: .warn, source: "ble.debug_command", title: "command.blocked", body: debugCommandStatus)
      return false
    }
    guard supportsSensorCommands else {
      setDebugCommandStatus("\(definition.title) needs command characteristic")
      record(level: .warn, source: "ble.debug_command", title: "command.blocked", body: commandCharacteristic.uuid.uuidString)
      return false
    }
    guard let writeType = writeType(for: commandCharacteristic) else {
      setDebugCommandStatus("\(definition.title) blocked: command characteristic is not writable")
      record(level: .warn, source: "ble.debug_command", title: "command.blocked", body: commandCharacteristic.uuid.uuidString)
      return false
    }
    guard let payload = debugCommandPayload(for: definition, payloadHex: payloadHex) else {
      setDebugCommandStatus("\(definition.title) needs \(definition.payloadHint)")
      record(
        level: .warn,
        source: "ble.debug_command",
        title: "payload.invalid",
        body: "\(definition.id) supplied=\(payloadHex ?? "nil") hint=\(definition.payloadHint)"
      )
      return false
    }

    let sequence = nextDebugSequence()
    let frame = Self.buildV5CommandFrame(
      sequence: sequence,
      command: definition.commandNumber,
      data: payload
    )
    let pending = PendingDebugCommand(
      id: definition.id,
      title: definition.title,
      commandNumber: definition.commandNumber,
      sequence: sequence,
      requestedAt: Date(),
      requestPayloadHex: Data(payload).hexString,
      requestFrameHex: frame.hexString,
      source: source
    )
    pendingDebugCommands[sequence] = pending
    scheduleDebugCommandTimeout(pending)
    setDebugCommandStatus("\(definition.title) sent seq \(sequence)")
    activePeripheral.writeValue(frame, for: commandCharacteristic, type: writeType)
    emitCommandWrite(
      source: "ble.debug_command",
      commandName: definition.id,
      commandNumber: definition.commandNumber,
      sequence: sequence,
      payload: Data(payload),
      frame: frame,
      peripheral: activePeripheral,
      characteristic: commandCharacteristic,
      writeType: writeType
    )
    record(
      source: "ble.debug_command",
      title: "command.sent",
      body: "\(definition.id) seq=\(sequence) command=\(definition.commandNumber) payload=\(Data(payload).hexString) source=\(source) writeType=\(writeTypeName(writeType)) frame=\(frame.hexString)"
    )
    return true
  }

  func startScan(reason: String, clearDiscovered: Bool) {
    ensureCentral()
    guard let central, central.state == .poweredOn else {
      bluetoothState = "bluetooth unavailable"
      record(level: .warn, source: "ble", title: "scan.start.blocked", body: bluetoothState)
      return
    }
    if clearDiscovered {
      discoveredDevices = []
      peripherals = [:]
      whoopCandidateIDs.removeAll()
      selectedDeviceID = nil
    }
    isScanning = true
    central.scanForPeripherals(
      withServices: whoopServices,
      options: [CBCentralManagerScanOptionAllowDuplicatesKey: false]
    )
    record(source: "ble", title: "scan.started", body: "reason=\(reason) services=\(uuidList(whoopServices))")
  }

  func stopScan(reason: String) {
    central?.stopScan()
    isScanning = false
    record(source: "ble", title: "scan.stopped", body: "reason=\(reason)")
  }

  func select(_ device: GooseDiscoveredDevice) {
    selectedDeviceID = device.id
    record(source: "ui", title: "device.selected", body: "\(device.name) \(device.id.uuidString)")
  }

  func connectSelected() {
    record(source: "ui", title: "connect.requested")
    ensureCentral()
    guard let central, central.state == .poweredOn else {
      updateConnectionState("bluetooth unavailable")
      record(level: .warn, source: "ble", title: "connect.blocked", body: connectionState)
      return
    }
    let deviceID = selectedDeviceID ?? discoveredDevices.first?.id
    guard let deviceID, let peripheral = peripherals[deviceID] else {
      updateConnectionState("no device selected")
      record(level: .warn, source: "ble", title: "connect.blocked", body: connectionState)
      return
    }
    stopScan(reason: "connect_selected")
    connect(peripheral, reason: "manual")
  }

  func sendClientHello() {
    record(source: "ui", title: "hello.send.requested")
    sendClientHello(reason: "manual", force: true)
  }

  func sendClientHelloIfNeeded(reason: String) {
    sendClientHello(reason: reason, force: false)
  }

  func sendClientHello(reason: String, force: Bool) {
    if clientHelloSentForCurrentConnection && !force {
      record(level: .debug, source: "ble", title: "hello.skipped", body: "already sent reason=\(reason)")
      return
    }
    guard
      let activePeripheral,
      let commandCharacteristic,
      !GooseHello.clientHelloFrame.isEmpty
    else {
      updateConnectionState("hello blocked")
      record(level: .warn, source: "ble", title: "hello.blocked", body: "missing active peripheral or command characteristic")
      return
    }

    let writeType: CBCharacteristicWriteType
    if commandCharacteristic.properties.contains(.write) {
      writeType = .withResponse
    } else if commandCharacteristic.properties.contains(.writeWithoutResponse) {
      writeType = .withoutResponse
    } else {
      updateConnectionState("hello blocked")
      record(level: .warn, source: "ble", title: "hello.blocked", body: "Command characteristic is not writable")
      return
    }

    activePeripheral.writeValue(
      GooseHello.clientHelloFrame,
      for: commandCharacteristic,
      type: writeType
    )
    emitCommandWrite(
      source: "ble",
      commandName: "CLIENT_HELLO",
      commandNumber: nil,
      sequence: nil,
      payload: Data(),
      frame: GooseHello.clientHelloFrame,
      peripheral: activePeripheral,
      characteristic: commandCharacteristic,
      writeType: writeType
    )
    clientHelloSentForCurrentConnection = true
    record(
      source: "ble",
      title: "hello.sent",
      body: "reason=\(reason) \(commandCharacteristic.uuid.uuidString) \(writeTypeName(writeType)) \(GooseHello.clientHelloFrameHex)"
    )
  }

  func syncHistoricalPackets(rangeFirst: Bool = false) {
    record(source: "ui", title: "historical_sync.requested", body: "range_first=\(rangeFirst)")
    beginHistoricalSync(
      trigger: rangeFirst ? "manual_range_first" : "manual",
      automatic: false,
      firstCommandOverride: rangeFirst ? .getDataRange : nil
    )
  }

  func syncHistoricalPacketsPreservingUnreadQueue(rangeFirst: Bool = false) {
    record(source: "ui", title: "historical_sync_preserve.requested", body: "range_first=\(rangeFirst) ack=disabled")
    beginHistoricalSync(
      trigger: rangeFirst ? "manual_range_first_preserve" : "manual_preserve",
      automatic: false,
      firstCommandOverride: rangeFirst ? .getDataRange : nil,
      acknowledgeHistoricalDataResult: false
    )
  }

  func pollHistoricalRange(source: String = "ui") {
    record(source: source, title: "historical_range_poll.requested")
    beginHistoricalSync(
      trigger: "\(source)_range_poll",
      automatic: false,
      firstCommandOverride: .getDataRange,
      rangeOnly: true
    )
  }

  func readStrapClock(syncIfNeeded: Bool = true) {
    record(source: "ui.clock", title: "clock.read.requested", body: "sync_if_needed=\(syncIfNeeded)")
    writeClockCommand(.get, syncIfNeeded: syncIfNeeded)
  }

  func startPhysiologySignalCapture() {
    record(source: "ui.debug", title: "physiology_capture.start.requested")
    writeSensorStreamCommands(
      SensorStreamCommandKind.startPhysiologyCapture,
      requestedStatus: "Starting physiology capture"
    )
  }

  func startMovementHeartRateCapture() {
    record(source: "ui.debug", title: "movement_hr_capture.start.requested")
    writeSensorStreamCommands(
      SensorStreamCommandKind.startMovementHeartRateCapture,
      requestedStatus: "Starting movement + HR capture"
    )
  }

  func stopMovementHeartRateCapture() {
    record(source: "ui.debug", title: "movement_hr_capture.stop.requested")
    writeSensorStreamCommands(
      SensorStreamCommandKind.stopMovementHeartRateCapture,
      requestedStatus: "Stopping movement + HR capture"
    )
  }

  func stopPhysiologySignalCapture() {
    record(source: "ui.debug", title: "physiology_capture.stop.requested")
    writeSensorStreamCommands(
      SensorStreamCommandKind.stopPhysiologyCapture,
      requestedStatus: "Stopping physiology capture"
    )
  }

  func enterHighFrequencyHistorySync(intervalSeconds: Int = 180, durationSeconds: Int = 7_200) {
    record(
      source: "ui.debug",
      title: "high_frequency_sync.enter.requested",
      body: "interval=\(intervalSeconds)s duration=\(durationSeconds)s"
    )
    guard let command = SensorStreamCommandKind.enterHighFrequencyHistorySync(
      intervalSeconds: intervalSeconds,
      durationSeconds: durationSeconds
    ) else {
      highFrequencyHistorySyncStatus = "Invalid interval or duration"
      record(level: .warn, source: "ble.high_frequency_sync", title: "command.invalid", body: highFrequencyHistorySyncStatus)
      return
    }

    guard canWriteHighFrequencyHistorySync else {
      highFrequencyHistorySyncStatus = "Needs ready V5 connection"
      record(level: .warn, source: "ble.high_frequency_sync", title: "command.blocked", body: highFrequencyHistorySyncStatus)
      return
    }

    let requestedExpiry = Date().addingTimeInterval(TimeInterval(durationSeconds))
    highFrequencyHistorySyncRequestedExpiry = requestedExpiry
    highFrequencyHistorySyncStatus = "Starting high-frequency history sync"
    highFrequencyHistorySyncExpiresAt = nil
    lastHighFrequencyHistorySyncResponse = "Waiting for ENTER_HIGH_FREQ_SYNC response"
    writeSensorStreamCommands(
      [command],
      requestedStatus: "Sending high-frequency history sync command",
      updatePhysiologyStatus: false
    )
  }

  func exitHighFrequencyHistorySync() {
    record(source: "ui.debug", title: "high_frequency_sync.exit.requested")
    guard canWriteHighFrequencyHistorySync else {
      highFrequencyHistorySyncStatus = "Needs ready V5 connection"
      record(level: .warn, source: "ble.high_frequency_sync", title: "command.blocked", body: highFrequencyHistorySyncStatus)
      return
    }

    highFrequencyHistorySyncStatus = "Stopping high-frequency history sync"
    lastHighFrequencyHistorySyncResponse = "Waiting for EXIT_HIGH_FREQ_SYNC response"
    writeSensorStreamCommands(
      [SensorStreamCommandKind.exitHighFrequencyHistorySync],
      requestedStatus: "Sending high-frequency history sync stop",
      updatePhysiologyStatus: false
    )
  }

  func queryWhoopAlarm(alarmID: Int = 1) {
    record(source: "ui.alarm", title: "alarm.query.requested", body: "alarmID=\(alarmID)")
    guard let alarmID = validatedAlarmID(alarmID) else {
      return
    }
    writeAlarmCommand(.get(alarmID: alarmID))
  }

  func setWhoopAlarm(at localWakeTime: Date, alarmID: Int = 1) {
    let targetDate = Self.nextFutureAlarmDate(from: localWakeTime)
    record(
      source: "ui.alarm",
      title: "alarm.set.requested",
      body: "alarmID=\(alarmID) target=\(targetDate.formatted(date: .abbreviated, time: .standard))"
    )
    guard let alarmID = validatedAlarmID(alarmID) else {
      return
    }
    writeAlarmCommand(.set(alarmID: alarmID, date: targetDate, pattern: .whoopDefault))
  }

  func runWhoopAlarmNow(alarmID: Int = 1) {
    record(source: "ui.alarm", title: "alarm.run.requested", body: "alarmID=\(alarmID)")
    guard let alarmID = validatedAlarmID(alarmID) else {
      return
    }
    writeAlarmCommand(.run(alarmID: alarmID))
  }

  func disableWhoopAlarms() {
    record(source: "ui.alarm", title: "alarm.disable.requested", body: "all")
    writeAlarmCommand(.disableAll)
  }

#if DEBUG
  func previewHelloWorldToast() {
    record(source: "ui.debug", title: "toast.preview.requested", body: "Hello World")
    publishSyncToast(phase: .synced, titleOverride: "Hello World", detail: "Toast preview", clearAfter: 2.2)
  }
#endif

  func refreshDeviceInformation() {
    record(source: "ui", title: "device_info.refresh.requested")
    guard let activePeripheral else {
      record(level: .warn, source: "ble.metadata", title: "device_info.refresh.blocked", body: "no active peripheral")
      return
    }
    guard activePeripheral.state == .connected else {
      record(level: .warn, source: "ble.metadata", title: "device_info.refresh.blocked", body: "peripheral state \(activePeripheral.state.rawValue)")
      return
    }

    activePeripheral.delegate = self
    if let deviceInformationService = activePeripheral.services?.first(where: { $0.uuid == deviceInformationServiceID }) {
      guard let characteristics = deviceInformationService.characteristics,
            !characteristics.isEmpty else {
        record(
          source: "ble.metadata",
          title: "device_info.discover_characteristic.requested",
          body: uuidList(deviceInformationCharacteristicIDs)
        )
        activePeripheral.discoverCharacteristics(deviceInformationCharacteristicIDs, for: deviceInformationService)
        return
      }

      let readableCharacteristics = characteristics.filter { deviceInformationCharacteristicIDs.contains($0.uuid) }
      for characteristic in readableCharacteristics {
        readStandardValueIfPossible(activePeripheral, characteristic, reason: "view_appear.device_info")
      }

      let missingCharacteristicIDs = deviceInformationCharacteristicIDs.filter { expectedID in
        !characteristics.contains(where: { $0.uuid == expectedID })
      }
      if !missingCharacteristicIDs.isEmpty {
        record(
          source: "ble.metadata",
          title: "device_info.discover_characteristic.requested",
          body: uuidList(missingCharacteristicIDs)
        )
        activePeripheral.discoverCharacteristics(missingCharacteristicIDs, for: deviceInformationService)
      }

      if readableCharacteristics.isEmpty && missingCharacteristicIDs.isEmpty {
        record(level: .warn, source: "ble.metadata", title: "device_info.refresh.empty")
      }
      return
    }

    record(source: "ble.metadata", title: "device_info.discover_service.requested", body: deviceInformationServiceID.uuidString)
    activePeripheral.discoverServices(serviceDiscoveryIDs)
  }

  func refreshBatteryLevel() {
    record(source: "ui", title: "battery.refresh.requested")
    guard let activePeripheral else {
      record(level: .warn, source: "ble.metadata", title: "battery.refresh.blocked", body: "no active peripheral")
      return
    }

    activePeripheral.delegate = self
    if let batteryLevelCharacteristic {
      readStandardValueIfPossible(activePeripheral, batteryLevelCharacteristic, reason: "view_appear")
    }
    if let batteryLevelStatusCharacteristic {
      readStandardValueIfPossible(activePeripheral, batteryLevelStatusCharacteristic, reason: "view_appear")
    }
    if batteryLevelCharacteristic != nil && batteryLevelStatusCharacteristic != nil {
      return
    }

    if let batteryService = activePeripheral.services?.first(where: { $0.uuid == batteryServiceID }) {
      var missingCharacteristicIDs: [CBUUID] = []
      if let characteristic = batteryService.characteristics?.first(where: { $0.uuid == batteryLevelCharacteristicID }) {
        batteryLevelCharacteristic = characteristic
        readStandardValueIfPossible(activePeripheral, characteristic, reason: "view_appear.cached_service")
      } else {
        missingCharacteristicIDs.append(batteryLevelCharacteristicID)
      }
      if let characteristic = batteryService.characteristics?.first(where: { $0.uuid == batteryLevelStatusCharacteristicID }) {
        batteryLevelStatusCharacteristic = characteristic
        readStandardValueIfPossible(activePeripheral, characteristic, reason: "view_appear.cached_service")
      } else {
        missingCharacteristicIDs.append(batteryLevelStatusCharacteristicID)
      }
      if !missingCharacteristicIDs.isEmpty {
        guard !batteryCharacteristicDiscoveryPending else {
          record(source: "ble.metadata", title: "battery.discover_characteristic.skipped", body: "discovery_in_progress")
          return
        }
        batteryCharacteristicDiscoveryPending = true
        record(source: "ble.metadata", title: "battery.discover_characteristic.requested", body: uuidList(missingCharacteristicIDs))
        activePeripheral.discoverCharacteristics(missingCharacteristicIDs, for: batteryService)
      }
      return
    }

    record(source: "ble.metadata", title: "battery.discover_service.requested", body: batteryServiceID.uuidString)
    activePeripheral.discoverServices([batteryServiceID])
  }

}
