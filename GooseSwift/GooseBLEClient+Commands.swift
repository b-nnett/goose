import CoreBluetooth
import Foundation
import OSLog


extension GooseBLEClient {
  func ensureCentral() {
    if central == nil {
      record(source: "ble", title: "central.create")
      central = CBCentralManager(
        delegate: self,
        queue: coreBluetoothQueue,
        options: [
          CBCentralManagerOptionRestoreIdentifierKey: Self.restorationIdentifier,
        ]
      )
    }
  }

  func dispatchCoreBluetoothDelegateToMainIfNeeded(_ work: @escaping () -> Void) -> Bool {
    guard !Thread.isMainThread else {
      return false
    }
    DispatchQueue.main.async(execute: work)
    return true
  }

  static var canCreateCentralWithoutPrompt: Bool {
    switch CBManager.authorization {
    case .allowedAlways:
      return true
    case .notDetermined, .denied, .restricted:
      return false
    @unknown default:
      return false
    }
  }

  static var authorizationStateDescription: String {
    switch CBManager.authorization {
    case .allowedAlways:
      return "allowed"
    case .notDetermined:
      return "not determined"
    case .denied:
      return "denied"
    case .restricted:
      return "restricted"
    @unknown default:
      return "unknown"
    }
  }

  func updateBluetoothState() {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.updateBluetoothState() }
      return
    }
    let previous = bluetoothState
    switch central?.state {
    case .poweredOn:
      bluetoothState = "powered on"
    case .poweredOff:
      bluetoothState = "powered off"
    case .unauthorized:
      bluetoothState = "unauthorized"
    case .unsupported:
      bluetoothState = "unsupported"
    case .resetting:
      bluetoothState = "resetting"
    case .unknown:
      bluetoothState = "unknown"
    case nil:
      switch CBManager.authorization {
      case .allowedAlways, .notDetermined:
        bluetoothState = "not requested"
      case .denied, .restricted:
        bluetoothState = "unauthorized"
      @unknown default:
        bluetoothState = "unknown"
      }
    @unknown default:
      bluetoothState = "unknown"
    }
    if previous != bluetoothState {
      record(source: "ble", title: "bluetooth.state", body: bluetoothState)
    }
  }

  func writeOSLog(_ message: GooseMessage) {
    let line = "\(message.source) \(message.title) \(message.body)"
    switch message.level {
    case .debug:
      logger.debug("\(line, privacy: .public)")
    case .info:
      logger.info("\(line, privacy: .public)")
    case .warn:
      logger.warning("\(line, privacy: .public)")
    case .error:
      logger.error("\(line, privacy: .public)")
    }
  }

  func updateConnectionState(_ value: String) {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.updateConnectionState(value) }
      return
    }
    let previous = connectionState
    connectionState = value
    updateNotificationContext(connectionState: value)
    if previous != value {
      record(source: "ble", title: "connection.state", body: value)
      onConnectionStateChange?(value)
    }
  }

  func updateActiveDeviceName(_ value: String) {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.updateActiveDeviceName(value) }
      return
    }
    activeDeviceName = value
    updateNotificationContext(activeDeviceName: value)
  }

  func updateNotificationContext(
    activeDeviceName: String? = nil,
    connectionState: String? = nil
  ) {
    notificationContextLock.lock()
    if let activeDeviceName {
      notificationContextActiveDeviceName = activeDeviceName
    }
    if let connectionState {
      notificationContextConnectionState = connectionState
    }
    notificationContextLock.unlock()
  }

  func notificationContextSnapshot() -> GooseBLENotificationContext {
    notificationContextLock.lock()
    let snapshot = GooseBLENotificationContext(
      activeDeviceName: notificationContextActiveDeviceName,
      connectionState: notificationContextConnectionState
    )
    notificationContextLock.unlock()
    return snapshot
  }

  func updateReconnectState(_ value: String) {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.updateReconnectState(value) }
      return
    }
    let previous = reconnectState
    reconnectState = value
    if previous != value {
      record(source: "ble", title: "reconnect.state", body: value)
    }
  }

  /// Returns true when the active command characteristic is confirmed for the connected wearable generation.
  /// If activeDescriptor is nil (e.g. cached-restore path set commandCharacteristic before
  /// processDiscoveredCharacteristics ran), falls back to UUID membership in commandCharacteristicIDs
  /// and logs a warning so the race is visible in diagnostics.
  private func commandCharacteristicIsValid(_ characteristic: CBCharacteristic) -> Bool {
    if let desc = activeDescriptor {
      return desc.isCommandCharacteristic(characteristic)
    }
    // activeDescriptor is unexpectedly nil while commandCharacteristic is set; allow the known
    // command UUIDs to pass so commands are not silently blocked during the setup race window.
    record(
      level: .warn,
      source: "ble",
      title: "command_characteristic.descriptor_nil",
      body: "commandCharacteristic=\(characteristic.uuid.uuidString) falling back to UUID membership check"
    )
    return commandCharacteristicIDs.contains(characteristic.uuid)
  }

  var supportsHistoricalSync: Bool {
    commandCharacteristic.map { commandCharacteristicIsValid($0) } == true
  }

  var supportsAlarmCommands: Bool {
    commandCharacteristic.map { commandCharacteristicIsValid($0) } == true
  }

  var supportsClockCommands: Bool {
    commandCharacteristic.map { commandCharacteristicIsValid($0) } == true
  }

  var supportsSensorCommands: Bool {
    commandCharacteristic.map { commandCharacteristicIsValid($0) } == true
  }

  func shouldUseCommandCharacteristic(_ characteristic: CBCharacteristic) -> Bool {
    guard commandCharacteristicIDs.contains(characteristic.uuid) else {
      return false
    }
    guard let current = commandCharacteristic else {
      return true
    }
    // Prefer command characteristic matching active descriptor; if no descriptor, accept first found
    if let desc = activeDescriptor {
      return desc.isCommandCharacteristic(characteristic) && !desc.isCommandCharacteristic(current)
    }
    return false
  }

  func validatedAlarmID(_ rawValue: Int) -> UInt8? {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in _ = self?.validatedAlarmID(rawValue) }
      return nil
    }
    guard (0...255).contains(rawValue) else {
      alarmCommandStatus = "Alarm ID must be 0-255"
      record(level: .warn, source: "ble.alarm", title: "alarm.id.invalid", body: "\(rawValue)")
      return nil
    }
    return UInt8(rawValue)
  }

  func writeClockCommand(_ kind: ClockCommandKind, syncIfNeeded: Bool) {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.writeClockCommand(kind, syncIfNeeded: syncIfNeeded) }
      return
    }
    guard !isHistoricalSyncing else {
      failClockCommand("Clock command blocked during historical sync.")
      return
    }
    guard pendingClockCommand == nil else {
      strapClockStatus = "Clock command already in flight"
      record(level: .warn, source: "ble.clock", title: "clock.write.blocked", body: strapClockStatus)
      return
    }
    guard pendingAlarmCommand == nil else {
      strapClockStatus = "Clock command blocked by alarm command"
      record(level: .warn, source: "ble.clock", title: "clock.write.blocked", body: strapClockStatus)
      return
    }
    guard let activePeripheral, let commandCharacteristic else {
      failClockCommand("Clock command needs an active WHOOP command characteristic.")
      return
    }
    guard connectionState == "ready" else {
      failClockCommand("Clock command needs ready connection; current state \(connectionState).")
      return
    }
    guard supportsClockCommands else {
      failClockCommand("Clock command needs command characteristic. Active command characteristic: \(commandCharacteristic.uuid.uuidString).")
      return
    }
    guard let writeType = writeType(for: commandCharacteristic) else {
      failClockCommand("Clock command blocked: command characteristic is not writable.")
      return
    }

    let sequence = nextClockSequence()
    let frame = Self.buildV5CommandFrame(
      sequence: sequence,
      command: kind.commandNumber,
      data: kind.payload
    )
    pendingClockCommand = PendingClockCommand(
      kind: kind,
      sequence: sequence,
      sentAt: Date(),
      syncIfNeeded: syncIfNeeded
    )
    scheduleClockCommandTimeout(kind: kind, sequence: sequence)
    lastClockCommandFrameHex = frame.hexString
    lastClockResponsePayloadHex = ""
    switch kind {
    case .get:
      strapClockStatus = syncIfNeeded
        ? "Reading clock; auto-sync >\(strapClockAutoSyncThresholdDisplay)"
        : "Reading clock"
    case .set:
    strapClockStatus = "Syncing clock"
    }
    activePeripheral.writeValue(frame, for: commandCharacteristic, type: writeType)
    emitCommandWrite(
      source: "ble.clock",
      commandName: kind.name,
      commandNumber: kind.commandNumber,
      sequence: sequence,
      payload: Data(kind.payload),
      frame: frame,
      peripheral: activePeripheral,
      characteristic: commandCharacteristic,
      writeType: writeType
    )
    record(
      source: "ble.clock",
      title: "clock.command.sent",
      body: "\(kind.name) seq=\(sequence) command=\(kind.commandNumber) payload=\(Data(kind.payload).hexString) writeType=\(writeTypeName(writeType)) frame=\(frame.hexString)"
    )
  }

  func nextClockSequence() -> UInt8 {
    let sequence = nextClockCommandSequence
    nextClockCommandSequence = nextClockCommandSequence == UInt8.max ? 96 : nextClockCommandSequence + 1
    return sequence
  }

  func scheduleClockCommandTimeout(kind: ClockCommandKind, sequence: UInt8) {
    clockCommandTimeoutWorkItem?.cancel()
    let workItem = DispatchWorkItem { [weak self] in
      guard let self,
            let pending = self.pendingClockCommand,
            pending.kind.commandNumber == kind.commandNumber,
            pending.sequence == sequence else {
        return
      }
      self.failClockCommand("\(kind.name) timed out waiting for command response sequence \(sequence).")
    }
    clockCommandTimeoutWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + 8, execute: workItem)
  }

  func writeAlarmCommand(_ kind: AlarmCommandKind) {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.writeAlarmCommand(kind) }
      return
    }
    guard !isHistoricalSyncing else {
      alarmCommandStatus = "Alarm write blocked during historical sync"
      record(level: .warn, source: "ble.alarm", title: "alarm.write.blocked", body: alarmCommandStatus)
      return
    }
    guard pendingAlarmCommand == nil else {
      alarmCommandStatus = "Alarm write blocked: command already in flight"
      record(level: .warn, source: "ble.alarm", title: "alarm.write.blocked", body: alarmCommandStatus)
      return
    }
    guard let activePeripheral, let commandCharacteristic else {
      alarmCommandStatus = "Alarm write needs an active WHOOP command characteristic"
      record(level: .warn, source: "ble.alarm", title: "alarm.write.blocked", body: alarmCommandStatus)
      return
    }
    guard connectionState == "ready" else {
      alarmCommandStatus = "Alarm write needs ready connection; current state \(connectionState)"
      record(level: .warn, source: "ble.alarm", title: "alarm.write.blocked", body: alarmCommandStatus)
      return
    }
    guard supportsAlarmCommands else {
      alarmCommandStatus = "Alarm writes need command characteristic"
      record(level: .warn, source: "ble.alarm", title: "alarm.write.blocked", body: commandCharacteristic.uuid.uuidString)
      return
    }
    guard let writeType = writeType(for: commandCharacteristic) else {
      alarmCommandStatus = "Alarm write blocked: command characteristic is not writable"
      record(level: .warn, source: "ble.alarm", title: "alarm.write.blocked", body: commandCharacteristic.uuid.uuidString)
      return
    }

    let sequence = nextAlarmSequence()
    let frame = Self.buildV5CommandFrame(
      sequence: sequence,
      command: kind.commandNumber,
      data: kind.payload
    )
    pendingAlarmCommand = PendingAlarmCommand(kind: kind, sequence: sequence)
    scheduleAlarmCommandTimeout(kind: kind, sequence: sequence)
    lastAlarmCommandFrameHex = frame.hexString
    lastAlarmResponseSummary = "Waiting for \(kind.name) response seq \(sequence)"
    lastAlarmResponsePayloadHex = ""
    lastAlarmEventSummary = "No alarm event for this command yet"
    lastAlarmEventPayloadHex = ""
    alarmCommandStatus = "\(kind.name) sent; waiting for strap response"
    activePeripheral.writeValue(frame, for: commandCharacteristic, type: writeType)
    emitCommandWrite(
      source: "ble.alarm",
      commandName: kind.name,
      commandNumber: kind.commandNumber,
      sequence: sequence,
      payload: Data(kind.payload),
      frame: frame,
      peripheral: activePeripheral,
      characteristic: commandCharacteristic,
      writeType: writeType
    )
    record(
      source: "ble.alarm",
      title: "alarm.command.sent",
      body: "\(kind.name) seq=\(sequence) command=\(kind.commandNumber) payload=\(Data(kind.payload).hexString) writeType=\(writeTypeName(writeType)) frame=\(frame.hexString)"
    )
  }

  func nextAlarmSequence() -> UInt8 {
    let sequence = nextAlarmCommandSequence
    nextAlarmCommandSequence = nextAlarmCommandSequence == UInt8.max ? 64 : nextAlarmCommandSequence + 1
    return sequence
  }

  func scheduleAlarmCommandTimeout(kind: AlarmCommandKind, sequence: UInt8) {
    alarmCommandTimeoutWorkItem?.cancel()
    let workItem = DispatchWorkItem { [weak self] in
      guard let self,
            let pending = self.pendingAlarmCommand,
            pending.kind.commandNumber == kind.commandNumber,
            pending.sequence == sequence else {
        return
      }
      self.failAlarmCommand("\(kind.name) timed out waiting for command response sequence \(sequence).")
    }
    alarmCommandTimeoutWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + 8, execute: workItem)
  }

  func writeSensorStreamCommands(
    _ commands: [SensorStreamCommandKind],
    requestedStatus: String,
    updatePhysiologyStatus: Bool = true
  ) {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.writeSensorStreamCommands(commands, requestedStatus: requestedStatus, updatePhysiologyStatus: updatePhysiologyStatus) }
      return
    }
    guard !isHistoricalSyncing else {
      if updatePhysiologyStatus {
        physiologyCaptureStatus = "Blocked during historical sync"
      }
      record(level: .warn, source: "ble.sensor", title: "sensor.write.blocked", body: "Blocked during historical sync")
      return
    }
    guard let activePeripheral, let commandCharacteristic else {
      if updatePhysiologyStatus {
        physiologyCaptureStatus = "Needs an active WHOOP command characteristic"
      }
      record(level: .warn, source: "ble.sensor", title: "sensor.write.blocked", body: "Needs an active WHOOP command characteristic")
      return
    }
    guard connectionState == "ready" else {
      if updatePhysiologyStatus {
        physiologyCaptureStatus = "Needs ready connection; current state \(connectionState)"
      }
      record(level: .warn, source: "ble.sensor", title: "sensor.write.blocked", body: "Needs ready connection; current state \(connectionState)")
      return
    }
    guard supportsSensorCommands else {
      if updatePhysiologyStatus {
        physiologyCaptureStatus = "Needs command characteristic"
      }
      record(level: .warn, source: "ble.sensor", title: "sensor.write.blocked", body: commandCharacteristic.uuid.uuidString)
      return
    }
    guard let writeType = writeType(for: commandCharacteristic) else {
      if updatePhysiologyStatus {
        physiologyCaptureStatus = "Command characteristic is not writable"
      }
      record(level: .warn, source: "ble.sensor", title: "sensor.write.blocked", body: commandCharacteristic.uuid.uuidString)
      return
    }

    if updatePhysiologyStatus {
      physiologyCaptureStatus = requestedStatus
    }
    for (index, command) in commands.enumerated() {
      let delay = Double(index) * 0.25
      DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self, weak activePeripheral, weak commandCharacteristic] in
        guard
          let self,
          let activePeripheral,
          let commandCharacteristic
        else {
          return
        }
        self.writeSensorStreamCommand(
          command,
          peripheral: activePeripheral,
          characteristic: commandCharacteristic,
          writeType: writeType,
          updatePhysiologyStatus: updatePhysiologyStatus
        )
      }
    }
  }

  func writeSensorStreamCommand(
    _ command: SensorStreamCommandKind,
    peripheral: CBPeripheral,
    characteristic: CBCharacteristic,
    writeType: CBCharacteristicWriteType,
    updatePhysiologyStatus: Bool = true
  ) {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.writeSensorStreamCommand(command, peripheral: peripheral, characteristic: characteristic, writeType: writeType, updatePhysiologyStatus: updatePhysiologyStatus) }
      return
    }
    let sequence = nextSensorCommandSequence
    nextSensorCommandSequence = nextSensorCommandSequence == UInt8.max ? 180 : nextSensorCommandSequence + 1
    let frame = Self.buildV5CommandFrame(
      sequence: sequence,
      command: command.commandNumber,
      data: command.payload
    )
    if updatePhysiologyStatus {
      lastPhysiologyCommandSummary = "\(command.name) seq \(sequence) sent"
    } else if command.commandNumber == 96 || command.commandNumber == 97 {
      lastHighFrequencyHistorySyncResponse = "\(command.name) seq \(sequence) sent"
    }
    peripheral.writeValue(frame, for: characteristic, type: writeType)
    emitCommandWrite(
      source: "ble.sensor",
      commandName: command.name,
      commandNumber: command.commandNumber,
      sequence: sequence,
      payload: Data(command.payload),
      frame: frame,
      peripheral: peripheral,
      characteristic: characteristic,
      writeType: writeType
    )
    record(
      source: "ble.sensor",
      title: "sensor.command.sent",
      body: "\(command.name) seq=\(sequence) command=\(command.commandNumber) payload=\(Data(command.payload).hexString) writeType=\(writeTypeName(writeType)) frame=\(frame.hexString)"
    )
  }

  func failClockCommand(_ message: String) {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.failClockCommand(message) }
      return
    }
    clockCommandTimeoutWorkItem?.cancel()
    pendingClockCommand = nil
    strapClockStatus = message
    record(level: .error, source: "ble.clock", title: "clock.command.failed", body: message)
  }

  func failAlarmCommand(_ message: String) {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.failAlarmCommand(message) }
      return
    }
    alarmCommandTimeoutWorkItem?.cancel()
    pendingAlarmCommand = nil
    alarmCommandStatus = message
    lastAlarmResponseSummary = message
    record(level: .error, source: "ble.alarm", title: "alarm.command.failed", body: message)
  }

  func loadRememberedDevice() {
    rememberedDeviceName = defaults.string(forKey: DefaultsKey.rememberedDeviceName)
    if let idString = defaults.string(forKey: DefaultsKey.rememberedDeviceID) {
      rememberedDeviceID = UUID(uuidString: idString)
    }
    rememberedDeviceValidated = defaults.bool(forKey: DefaultsKey.rememberedDeviceValidated)
    updateRememberedDeviceDescription()
  }

  func loadPersistedBatterySample() {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.loadPersistedBatterySample() }
      return
    }
    guard defaults.object(forKey: DefaultsKey.lastBatteryPercent) != nil else {
      return
    }
    let percent = defaults.integer(forKey: DefaultsKey.lastBatteryPercent)
    let capturedAt = defaults.object(forKey: DefaultsKey.lastBatteryCapturedAt) as? Date
    let normalizedPercent = min(max(percent, 0), 100)
    batteryLevelPercent = normalizedPercent
    batteryUpdatedAt = capturedAt
    if let capturedAt {
      lastBatteryLevelSample = (normalizedPercent, capturedAt)
    }
    if let chargingUntil = defaults.object(forKey: DefaultsKey.inferredBatteryChargingUntil) as? Date,
       chargingUntil > Date() {
      inferredBatteryChargingUntil = chargingUntil
      batteryIsCharging = true
      batteryPowerStatus = "Charging (inferred)"
    }
  }

  func loadPersistedHRVSample() {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.loadPersistedHRVSample() }
      return
    }
    guard defaults.object(forKey: DefaultsKey.liveHRVRMSSD) != nil else {
      return
    }
    let rmssd = defaults.double(forKey: DefaultsKey.liveHRVRMSSD)
    let count = defaults.integer(forKey: DefaultsKey.liveHRVRRIntervalCount)
    let sampleCount = defaults.integer(forKey: DefaultsKey.liveHRVRMSSDSampleCount)
    let source = defaults.string(forKey: DefaultsKey.liveHRVSource) ?? "ble.hr.standard.average"
    guard rmssd.isFinite, rmssd >= 0, count >= 2, sampleCount > 0 else {
      return
    }
    liveHRVRMSSD = rmssd
    liveHRVRRIntervalCount = count
    liveHRVRMSSDSampleCount = sampleCount
    liveHRVUpdatedAt = defaults.object(forKey: DefaultsKey.liveHRVUpdatedAt) as? Date
    liveHRVSource = source
    lastPublishedHRVRMSSD = rmssd
    lastHRVPublishedAt = liveHRVUpdatedAt ?? Date.distantPast
  }

  func loadPersistedRestingHeartRateEstimate() {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.loadPersistedRestingHeartRateEstimate() }
      return
    }
    guard defaults.object(forKey: DefaultsKey.restingHeartRateEstimateBPM) != nil else {
      return
    }
    let bpm = defaults.double(forKey: DefaultsKey.restingHeartRateEstimateBPM)
    let count = defaults.integer(forKey: DefaultsKey.restingHeartRateEstimateSampleCount)
    guard bpm.isFinite, bpm > 0, count >= Self.restingHeartRateMinimumSampleCount else {
      return
    }
    restingHeartRateEstimateBPM = bpm
    restingHeartRateEstimateSampleCount = count
    restingHeartRateEstimateUpdatedAt = defaults.object(forKey: DefaultsKey.restingHeartRateEstimateUpdatedAt) as? Date
    restingHeartRateEstimateSource = defaults.string(forKey: DefaultsKey.restingHeartRateEstimateSource) ?? "ble.hr.standard.low_quartile"
    lastRestingHeartRateEstimateBPM = bpm
    lastRestingHeartRateEstimatePublishedAt = restingHeartRateEstimateUpdatedAt ?? Date.distantPast
  }

  func persistRestingHeartRateEstimate(bpm: Double, sampleCount: Int, source: String, capturedAt: Date) {
    defaults.set(bpm, forKey: DefaultsKey.restingHeartRateEstimateBPM)
    defaults.set(sampleCount, forKey: DefaultsKey.restingHeartRateEstimateSampleCount)
    defaults.set(capturedAt, forKey: DefaultsKey.restingHeartRateEstimateUpdatedAt)
    defaults.set(source, forKey: DefaultsKey.restingHeartRateEstimateSource)
  }

  func persistHRVSample(rmssd: Double, rrIntervalCount: Int, sampleCount: Int, source: String, capturedAt: Date) {
    defaults.set(rmssd, forKey: DefaultsKey.liveHRVRMSSD)
    defaults.set(rrIntervalCount, forKey: DefaultsKey.liveHRVRRIntervalCount)
    defaults.set(sampleCount, forKey: DefaultsKey.liveHRVRMSSDSampleCount)
    defaults.set(capturedAt, forKey: DefaultsKey.liveHRVUpdatedAt)
    defaults.set(source, forKey: DefaultsKey.liveHRVSource)
  }

  func persistBatterySample(percent: Int, capturedAt: Date) {
    defaults.set(percent, forKey: DefaultsKey.lastBatteryPercent)
    defaults.set(capturedAt, forKey: DefaultsKey.lastBatteryCapturedAt)
  }

  func persistInferredBatteryChargingUntil(_ date: Date?) {
    if let date {
      defaults.set(date, forKey: DefaultsKey.inferredBatteryChargingUntil)
    } else {
      defaults.removeObject(forKey: DefaultsKey.inferredBatteryChargingUntil)
    }
  }

  func clearRememberedDevice(reason: String, source: String = "ble") {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.clearRememberedDevice(reason: reason, source: source) }
      return
    }
    let previous = rememberedDeviceDescription
    defaults.removeObject(forKey: DefaultsKey.rememberedDeviceID)
    defaults.removeObject(forKey: DefaultsKey.rememberedDeviceName)
    defaults.removeObject(forKey: DefaultsKey.rememberedDeviceValidated)
    if let rememberedDeviceID {
      whoopCandidateIDs.remove(rememberedDeviceID)
    }
    rememberedDeviceID = nil
    rememberedDeviceName = nil
    rememberedDeviceValidated = false
    autoReconnectTargetID = nil
    cancelReconnectCycle()
    if activePeripheral == nil {
      activeDeviceIdentifier = nil
      updateActiveDeviceName("WHOOP")
    }
    updateRememberedDeviceDescription()
    updateReconnectState(reason == "manual" ? "forgotten" : "remembered rejected")
    record(source: source, title: "remembered_device.forgotten", body: "reason=\(reason) previous=\(previous)")
  }

  func updateRememberedDeviceDescription() {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.updateRememberedDeviceDescription() }
      return
    }
    guard let rememberedDeviceID else {
      rememberedDeviceDescription = "none"
      return
    }
    if let rememberedDeviceName, !rememberedDeviceName.isEmpty {
      rememberedDeviceDescription = "\(Self.sanitizedWhoopDisplayName(rememberedDeviceName)) \(rememberedDeviceID.uuidString)"
    } else {
      rememberedDeviceDescription = rememberedDeviceID.uuidString
    }
  }

  func rememberPeripheral(_ peripheral: CBPeripheral, fallbackName: String? = nil, evidence: String? = nil) {
    guard let evidence = evidence ?? whoopIdentityEvidence(for: peripheral, fallbackName: fallbackName) else {
      record(
        level: .warn,
        source: "ble",
        title: "remembered_device.rejected",
        body: "\(peripheral.name ?? fallbackName ?? "unknown") \(peripheral.identifier.uuidString)"
      )
      return
    }
    let name = Self.sanitizedWhoopDisplayName(peripheral.name ?? fallbackName ?? rememberedDeviceName ?? "WHOOP")
    whoopCandidateIDs.insert(peripheral.identifier)
    rememberedDeviceID = peripheral.identifier
    rememberedDeviceName = name
    rememberedDeviceValidated = true
    updateActiveDevice(peripheral, fallbackName: name)
    defaults.set(peripheral.identifier.uuidString, forKey: DefaultsKey.rememberedDeviceID)
    defaults.set(name, forKey: DefaultsKey.rememberedDeviceName)
    defaults.set(true, forKey: DefaultsKey.rememberedDeviceValidated)
    updateRememberedDeviceDescription()
    record(source: "ble", title: "remembered_device.saved", body: "\(rememberedDeviceDescription) evidence=\(evidence)")
  }

  func connect(_ peripheral: CBPeripheral, reason: String) {
    guard let central, central.state == .poweredOn else {
      updateConnectionState("bluetooth unavailable")
      updateReconnectState("blocked")
      record(level: .warn, source: "ble", title: "connect.blocked", body: "reason=\(reason) bluetooth unavailable")
      return
    }
    let fallbackName = discoveredName(for: peripheral.identifier)
    guard let evidence = whoopIdentityEvidence(for: peripheral, fallbackName: fallbackName) else {
      updateConnectionState("not a WHOOP device")
      updateReconnectState("blocked")
      rejectNonWhoopPeripheral(peripheral, reason: "connect_without_whoop_evidence", fallbackName: fallbackName)
      return
    }
    if activePeripheral?.identifier == peripheral.identifier,
       connectionState == "connecting" || connectionState == "discovering" || connectionState == "ready" {
      record(level: .debug, source: "ble", title: "connect.skipped", body: "already \(connectionState)")
      return
    }
    whoopCandidateIDs.insert(peripheral.identifier)
    resetLiveDeviceFieldsIfNeeded(for: peripheral)
    clientHelloSentForCurrentConnection = false
    updateActiveDevice(peripheral, fallbackName: fallbackName)
    activePeripheral = peripheral
    peripheral.delegate = self
    updateConnectionState("connecting")
    updateReconnectState(reason.hasPrefix("auto") || reason == "restore" ? "connecting" : reconnectState)
    record(source: "ble", title: "connect.started", body: "reason=\(reason) evidence=\(evidence) \(peripheral.name ?? fallbackName ?? rememberedDeviceName ?? "WHOOP") \(peripheral.identifier.uuidString)")
    pendingConnectionReason = reason
    central.connect(
      peripheral,
      options: [
        CBConnectPeripheralOptionNotifyOnConnectionKey: true,
        CBConnectPeripheralOptionNotifyOnDisconnectionKey: true,
      ]
    )
  }

  // Cancels any pending scheduled retry and bumps the generation token so a stale
  // closure already in-flight (via asyncAfter) is a no-op when it runs.
  // Must be called on coreBluetoothQueue (or main thread via delegate dispatch).
  func cancelReconnectCycle() {
    reconnectWorkItem?.cancel()
    reconnectWorkItem = nil
    reconnectGeneration += 1
  }

  // Schedules the next reconnect attempt using the backoff state.
  // On circuit breaker exhaustion (10 attempts) transitions to failed state.
  // Must be called on coreBluetoothQueue (or main thread via delegate dispatch).
  func scheduleNextReconnect(reason: String) {
    guard let delay = reconnectBackoff.nextDelay() else {
      // Circuit breaker: all attempts exhausted.
      updateReconnectState("failed after 10 attempts")
      reconnectBackoff.reset()
      return
    }
    // Update status AFTER nextDelay() incremented attemptCount.
    updateReconnectState(reconnectBackoff.statusString)
    let generation = reconnectGeneration
    let item = DispatchWorkItem { [weak self] in
      guard let self, self.reconnectGeneration == generation else { return }
      self.attemptAutomaticReconnect(reason: reason)
    }
    reconnectWorkItem = item
    // first attempt fires after baseDelay (1s), not immediately
    coreBluetoothQueue.asyncAfter(deadline: .now() + delay, execute: item)
  }

  func stopReconnect() {
    coreBluetoothQueue.async { [weak self] in
      guard let self else { return }
      self.cancelReconnectCycle()
      self.reconnectBackoff.reset()
      self.updateReconnectState("idle")
      // remembered device is NOT cleared (D-05)
    }
  }

  func retryReconnect() {
    coreBluetoothQueue.async { [weak self] in
      guard let self else { return }
      self.cancelReconnectCycle()
      self.reconnectBackoff.reset()
      self.scheduleNextReconnect(reason: "manual_retry")
    }
  }

  func attemptAutomaticReconnect(reason: String) {
    guard let central, central.state == .poweredOn else {
      updateReconnectState("waiting for bluetooth")
      return
    }
    guard activePeripheral == nil else {
      updateReconnectState("already connected")
      return
    }
    guard !isReconnecting || reason == "backoff_retry" || reason == "manual_retry" || reason == "startup" else {
      record(level: .debug, source: "ble", title: "reconnect.skipped", body: "already in flight")
      return
    }

    if let rememberedDeviceID {
      if !rememberedDeviceValidated,
         let rememberedDeviceName,
         !isWhoopName(rememberedDeviceName) {
        clearRememberedDevice(reason: "legacy_name_mismatch")
        updateReconnectState("no remembered device")
        return
      }
      updateReconnectState("retrieving remembered")
      let retrieved = central.retrievePeripherals(withIdentifiers: [rememberedDeviceID])
      if let peripheral = retrieved.first {
        peripherals[peripheral.identifier] = peripheral
        if whoopIdentityEvidence(for: peripheral) != nil {
          selectedDeviceID = peripheral.identifier
          let connectReason = prioritizeLiveCaptureOnReady
            ? "auto_live_capture_remembered"
            : "auto.\(reason).remembered"
          connect(peripheral, reason: connectReason)
        } else if let name = peripheral.name, !isWhoopName(name) {
          updateReconnectState("remembered was not WHOOP")
          rejectNonWhoopPeripheral(peripheral, reason: "remembered_name_mismatch")
        } else {
          autoReconnectTargetID = rememberedDeviceID
          updateReconnectState("scanning for remembered WHOOP")
          record(source: "ble", title: "reconnect.remembered_unverified", body: rememberedDeviceID.uuidString)
          startScan(reason: "auto_reconnect_unverified", clearDiscovered: false)
        }
      } else {
        autoReconnectTargetID = rememberedDeviceID
        updateReconnectState("scanning for remembered")
        record(source: "ble", title: "reconnect.scan_fallback", body: rememberedDeviceID.uuidString)
        startScan(reason: "auto_reconnect", clearDiscovered: false)
      }
      return
    }

    if prioritizeLiveCaptureOnReady {
      beginAutoPhysiologyDiscovery(reason: reason)
      return
    }
    updateReconnectState("no remembered device")
  }

  func beginAutoPhysiologyDiscovery(reason: String) {
    guard central?.state == .poweredOn else {
      updateReconnectState("waiting for bluetooth")
      return
    }
    guard activePeripheral == nil else {
      updateReconnectState("already connected")
      return
    }
    guard !autoConnectForPhysiologyCapture else {
      record(level: .debug, source: "ble.sensor", title: "physiology_capture.scan.skipped", body: "already scanning")
      return
    }
    autoConnectForPhysiologyCapture = true
    updateReconnectState("scanning for WHOOP physiology")
    record(source: "ble.sensor", title: "physiology_capture.scan.started", body: "reason=\(reason)")
    startScan(reason: "auto_physiology_capture", clearDiscovered: false)
  }

  func notificationCandidate(_ characteristic: CBCharacteristic) -> Bool {
    notificationCharacteristicIDs.contains(characteristic.uuid)
      || characteristic.uuid == standardHeartRateMeasurementID
      || characteristic.uuid == batteryLevelCharacteristicID
      || characteristic.uuid == batteryLevelStatusCharacteristicID
  }

  func debugMenuCandidate(_ characteristic: CBCharacteristic) -> Bool {
    let uuid = characteristic.uuid.uuidString.lowercased()
    return uuid.hasPrefix("fd4b0007") || uuid.hasPrefix("61080007")
  }

  func standardReadableCharacteristic(_ characteristic: CBCharacteristic) -> Bool {
    characteristic.uuid == batteryLevelCharacteristicID
      || characteristic.uuid == batteryLevelStatusCharacteristicID
      || characteristic.uuid == modelNumberCharacteristicID
      || characteristic.uuid == firmwareRevisionCharacteristicID
      || characteristic.uuid == hardwareRevisionCharacteristicID
      || characteristic.uuid == softwareRevisionCharacteristicID
      || characteristic.uuid == manufacturerNameCharacteristicID
  }

  func readStandardValueIfPossible(
    _ peripheral: CBPeripheral,
    _ characteristic: CBCharacteristic,
    reason: String = "discovery"
  ) {
    guard standardReadableCharacteristic(characteristic) else {
      return
    }
    guard characteristic.properties.contains(.read) else {
      record(
        level: .debug,
        source: "ble",
        title: "metadata.read.skipped",
        body: "\(characteristic.uuid.uuidString) properties=\(propertyNames(characteristic.properties))"
      )
      return
    }
    guard characteristic.value == nil else {
      record(source: "ble", title: "metadata.read.skipped", body: "\(characteristic.uuid.uuidString) value_already_cached reason=\(reason)")
      return
    }
    peripheral.readValue(for: characteristic)
    record(source: "ble", title: "metadata.read.requested", body: "\(characteristic.uuid.uuidString) reason=\(reason)")
  }

  func subscribeIfPossible(_ peripheral: CBPeripheral, _ characteristic: CBCharacteristic) {
    guard notificationCandidate(characteristic) else {
      return
    }
    guard !characteristic.isNotifying else {
      record(source: "ble", title: "notify.skipped", body: "\(characteristic.uuid.uuidString) already_subscribed")
      return
    }
    guard characteristic.properties.contains(.notify) || characteristic.properties.contains(.indicate) else {
      record(
        level: .warn,
        source: "ble",
        title: "notify.blocked",
        body: "\(characteristic.uuid.uuidString) properties=\(propertyNames(characteristic.properties))"
      )
      return
    }
    peripheral.setNotifyValue(true, for: characteristic)
    record(source: "ble", title: "notify.requested", body: "\(characteristic.uuid.uuidString)")
    if debugMenuCandidate(characteristic) {
      debugMenuCharacteristic = characteristic
      record(
        source: "ble.debug_menu",
        title: "debug_menu.characteristic.subscribed",
        body: "\(characteristic.uuid.uuidString) properties=\(propertyNames(characteristic.properties))"
      )
      scheduleDebugSkinTemperatureCommandIfNeeded(reason: "notify_subscribe")
    }
  }

  func processCachedServicesIfAvailable(_ peripheral: CBPeripheral, reason: String) {
    guard let services = peripheral.services, !services.isEmpty else {
      return
    }
    record(source: "ble", title: "gatt.services.cached", body: "\(reason) \(uuidList(services.map(\.uuid)))")
    if services.contains(where: { isWhoopService($0.uuid) }) {
      whoopCandidateIDs.insert(peripheral.identifier)
    }
    for service in services {
      if let characteristics = service.characteristics, !characteristics.isEmpty {
        processDiscoveredCharacteristics(characteristics, for: service, peripheral: peripheral, cached: true)
      } else {
        peripheral.discoverCharacteristics(nil, for: service)
      }
    }
  }

  func processDiscoveredCharacteristics(
    _ characteristics: [CBCharacteristic],
    for service: CBService,
    peripheral: CBPeripheral,
    cached: Bool
  ) {
    for characteristic in characteristics {
      if shouldUseCommandCharacteristic(characteristic) {
        commandCharacteristic = characteristic
        activeDescriptor = characteristic.uuid.uuidString.lowercased().hasPrefix("61080002")
          ? .whoopGen4 : .whoopGen5
        record(
          source: "ble",
          title: cached ? "command_characteristic.cached" : "command_characteristic.discovered",
          body: "\(service.uuid.uuidString) \(characteristic.uuid.uuidString) properties=\(propertyNames(characteristic.properties))"
        )
        record(
          source: "ble",
          title: "wearable_descriptor.set",
          body: characteristic.uuid.uuidString.lowercased().hasPrefix("61080002") ? "gen4" : "gen5"
        )
      } else if commandCharacteristicIDs.contains(characteristic.uuid) {
        record(
          level: .debug,
          source: "ble",
          title: "command_characteristic.ignored",
          body: "\(service.uuid.uuidString) \(characteristic.uuid.uuidString) keeping=\(commandCharacteristic?.uuid.uuidString ?? "none")"
        )
      }
      if debugMenuCandidate(characteristic) {
        debugMenuCharacteristic = characteristic
        record(
          source: "ble.debug_menu",
          title: cached ? "debug_menu.characteristic.cached" : "debug_menu.characteristic.discovered",
          body: "\(service.uuid.uuidString) \(characteristic.uuid.uuidString) properties=\(propertyNames(characteristic.properties))"
        )
        scheduleDebugSkinTemperatureCommandIfNeeded(reason: cached ? "cached_gatt" : "gatt_discovery")
      }
      if characteristic.uuid == batteryLevelCharacteristicID {
        batteryLevelCharacteristic = characteristic
        record(
          source: "ble.metadata",
          title: cached ? "battery_characteristic.cached" : "battery_characteristic.discovered",
          body: "\(service.uuid.uuidString) properties=\(propertyNames(characteristic.properties))"
        )
      }
      if characteristic.uuid == batteryLevelStatusCharacteristicID {
        batteryLevelStatusCharacteristic = characteristic
        record(
          source: "ble.metadata",
          title: cached ? "battery_status_characteristic.cached" : "battery_status_characteristic.discovered",
          body: "\(service.uuid.uuidString) properties=\(propertyNames(characteristic.properties))"
        )
      }
      subscribeIfPossible(peripheral, characteristic)
      readStandardValueIfPossible(peripheral, characteristic)
    }

    if commandCharacteristic != nil {
      updateConnectionState("ready")
      sendClientHelloIfNeeded(reason: cached ? "cached_gatt" : "gatt_discovery")
      scheduleDebugSkinTemperatureCommandIfNeeded(reason: cached ? "cached_ready" : "ready")
      scheduleAutomaticHistoricalSyncIfNeeded()
      scheduleAutomaticPhysiologyCaptureIfNeeded()
    } else if connectionState == "discovering" {
      updateConnectionState("connected")
    }
  }

  func scheduleAutomaticPhysiologyCaptureIfNeeded() {
    guard autoStartPhysiologyCaptureOnReady,
          !autoStartedPhysiologyCapture,
          connectionState == "ready",
          activePeripheral != nil,
          commandCharacteristic != nil,
          supportsSensorCommands else {
      return
    }

    autoStartedPhysiologyCapture = true
    record(source: "ble.sensor", title: "physiology_capture.auto_scheduled")
    DispatchQueue.main.asyncAfter(deadline: .now() + 5) { [weak self] in
      guard let self else {
        return
      }
      self.record(source: "ble.sensor", title: "physiology_capture.auto_start")
      self.startPhysiologySignalCapture()
    }
  }

  func scheduleAutomaticHistoricalSyncIfNeeded() {
    guard let reason = pendingAutomaticHistoricalSyncReason,
          autoHistoricalSyncOnReady,
          connectionState == "ready",
          activePeripheral != nil,
          commandCharacteristic != nil,
          supportsHistoricalSync,
          !isHistoricalSyncing else {
      return
    }

    readySyncWorkItem?.cancel()
    let workItem = DispatchWorkItem { [weak self] in
      guard let self else {
        return
      }
      guard self.pendingAutomaticHistoricalSyncReason == reason else {
        return
      }
      self.pendingAutomaticHistoricalSyncReason = nil
      self.beginHistoricalSync(trigger: reason, automatic: true)
    }
    readySyncWorkItem = workItem
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.8, execute: workItem)
    record(source: "ble.sync", title: "historical_sync.scheduled", body: reason)
  }

}
