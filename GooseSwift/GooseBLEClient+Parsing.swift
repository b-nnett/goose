import CoreBluetooth
import Foundation
import OSLog


extension GooseBLEClient {
  func handleStandardHeartRate(
    _ value: Data,
    characteristic: CBCharacteristic,
    capturedAt: Date = Date()
  ) {
    guard let measurement = Self.parseStandardHeartRateMeasurement(value) else {
      record(level: .warn, source: "ble.hr.standard", title: "heart_rate.parse_failed", body: value.hexString)
      return
    }
    recordLiveHeartRate(measurement.bpm, source: "ble.hr.standard", at: capturedAt)
    recordRRIntervals(measurement.rrIntervalsMS, source: "ble.hr.standard", at: capturedAt)
  }

  struct BatteryLevelStatus {
    let batteryLevelPercent: Int?
    let isCharging: Bool?
    let summary: String
  }

  func applyBatteryLevel(_ rawLevel: Int, capturedAt: Date, sourceTitle: String) {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.applyBatteryLevel(rawLevel, capturedAt: capturedAt, sourceTitle: sourceTitle) }
      return
    }
    let normalizedLevel = min(max(rawLevel, 0), 100)
    let previousSample = lastBatteryLevelSample
    batteryLevelPercent = normalizedLevel
    batteryUpdatedAt = capturedAt
    lastSyncAt = capturedAt
    updateBatteryChargingInference(
      currentPercent: normalizedLevel,
      previousSample: previousSample,
      capturedAt: capturedAt
    )
    lastBatteryLevelSample = (normalizedLevel, capturedAt)
    persistBatterySample(percent: normalizedLevel, capturedAt: capturedAt)
    record(
      source: "ble.metadata",
      title: sourceTitle,
      body: "\(normalizedLevel)% charging=\(batteryIsCharging.map { $0 ? "true" : "false" } ?? "unknown") status=\(batteryPowerStatus)"
    )
  }

  func updateBatteryChargingInference(
    currentPercent: Int,
    previousSample: (percent: Int, capturedAt: Date)?,
    capturedAt: Date
  ) {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.updateBatteryChargingInference(currentPercent: currentPercent, previousSample: previousSample, capturedAt: capturedAt) }
      return
    }
    guard let previousSample else {
      return
    }
    let sampleAge = capturedAt.timeIntervalSince(previousSample.capturedAt)
    guard sampleAge > 0, sampleAge <= 6 * 60 * 60 else {
      return
    }
    guard currentPercent > previousSample.percent else {
      if inferredBatteryChargingUntil.map({ $0 > capturedAt }) == true, batteryIsCharging == nil {
        batteryIsCharging = true
        batteryPowerStatus = "Charging (inferred)"
      }
      return
    }

    inferredBatteryChargingUntil = capturedAt.addingTimeInterval(30 * 60)
    persistInferredBatteryChargingUntil(inferredBatteryChargingUntil)
    if batteryIsCharging != true || batteryPowerStatus == "Unknown" {
      batteryIsCharging = true
      batteryPowerStatus = "Charging (inferred)"
      record(
        source: "ble.metadata",
        title: "battery.charging.inferred",
        body: "\(previousSample.percent)% -> \(currentPercent)% over \(Int(sampleAge.rounded()))s"
      )
    }
  }

  func hasRecentInferredBatteryCharging(at date: Date) -> Bool {
    inferredBatteryChargingUntil.map { $0 > date } == true
  }

  func applyBatteryStatus(_ status: BatteryLevelStatus, rawValue: Data, capturedAt: Date) {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.applyBatteryStatus(status, rawValue: rawValue, capturedAt: capturedAt) }
      return
    }
    if let batteryLevel = status.batteryLevelPercent {
      applyBatteryLevel(batteryLevel, capturedAt: capturedAt, sourceTitle: "battery.status.level")
    } else {
      batteryUpdatedAt = capturedAt
      lastSyncAt = capturedAt
    }

    if status.isCharging == true {
      batteryIsCharging = true
      inferredBatteryChargingUntil = nil
      persistInferredBatteryChargingUntil(nil)
      batteryPowerStatus = status.summary
    } else if status.isCharging == false {
      if hasRecentInferredBatteryCharging(at: capturedAt) {
        batteryIsCharging = true
        batteryPowerStatus = "Charging (inferred)"
        record(
          source: "ble.metadata",
          title: "battery.status.inference_kept",
          body: "\(status.summary) raw=\(rawValue.hexString)"
        )
      } else {
        batteryIsCharging = false
        inferredBatteryChargingUntil = nil
        persistInferredBatteryChargingUntil(nil)
        batteryPowerStatus = status.summary
      }
    } else if hasRecentInferredBatteryCharging(at: capturedAt) {
      batteryIsCharging = true
      batteryPowerStatus = "Charging (inferred)"
    } else {
      batteryIsCharging = nil
      inferredBatteryChargingUntil = nil
      persistInferredBatteryChargingUntil(nil)
      batteryPowerStatus = status.summary
    }
  }

  @discardableResult
  func handleStandardReadValue(
    _ value: Data,
    characteristic: CBCharacteristic,
    capturedAt: Date
  ) -> Bool {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.handleStandardReadValue(value, characteristic: characteristic, capturedAt: capturedAt) }
      return true
    }
    switch characteristic.uuid {
    case batteryLevelCharacteristicID:
      guard let raw = value.first else {
        record(level: .warn, source: "ble.metadata", title: "battery.read.empty")
        return true
      }
      applyBatteryLevel(Int(raw), capturedAt: capturedAt, sourceTitle: "battery.read")
      return true
    case batteryLevelStatusCharacteristicID:
      guard let status = Self.parseBatteryLevelStatus(value) else {
        record(level: .warn, source: "ble.metadata", title: "battery.status.parse_failed", body: value.hexString)
        return true
      }
      applyBatteryStatus(status, rawValue: value, capturedAt: capturedAt)
      record(source: "ble.metadata", title: "battery.status.read", body: "\(status.summary) raw=\(value.hexString)")
      return true
    case modelNumberCharacteristicID:
      modelNumber = decodedMetadataString(value)
    case firmwareRevisionCharacteristicID:
      firmwareVersion = decodedMetadataString(value)
    case hardwareRevisionCharacteristicID:
      hardwareRevision = decodedMetadataString(value)
    case softwareRevisionCharacteristicID:
      softwareRevision = decodedMetadataString(value)
    case manufacturerNameCharacteristicID:
      manufacturerName = decodedMetadataString(value)
    default:
      return false
    }

    lastSyncAt = capturedAt
    let stringValue = decodedMetadataString(value) ?? value.hexString
    record(source: "ble.metadata", title: "device_info.read", body: "\(characteristic.uuid.uuidString)=\(stringValue)")
    return true
  }

  func decodedMetadataString(_ data: Data) -> String? {
    var trimSet = CharacterSet.whitespacesAndNewlines
    trimSet.formUnion(.controlCharacters)
    guard let string = String(data: data, encoding: .utf8)?
      .trimmingCharacters(in: trimSet),
      !string.isEmpty
    else {
      return nil
    }
    return string
  }

  static func parseBatteryLevelStatus(_ data: Data) -> BatteryLevelStatus? {
    let bytes = Array(data)
    guard bytes.count >= 3 else {
      return nil
    }

    let flags = bytes[0]
    let powerState = UInt16(bytes[1]) | (UInt16(bytes[2]) << 8)
    let batteryPresent = powerState & 0x01 != 0
    let wiredPower = Int((powerState >> 1) & 0x03)
    let wirelessPower = Int((powerState >> 3) & 0x03)
    let chargeState = Int((powerState >> 5) & 0x03)
    let chargeLevel = Int((powerState >> 7) & 0x03)
    let chargingType = Int((powerState >> 9) & 0x07)
    let chargingFault = Int((powerState >> 12) & 0x07)
    let externalPowerConnected = wiredPower == 1 || wirelessPower == 1

    var index = 3
    if flags & 0x01 != 0 {
      index += 2
    }

    let statusBatteryLevel: Int?
    if flags & 0x02 != 0, bytes.count > index {
      statusBatteryLevel = min(max(Int(bytes[index]), 0), 100)
      index += 1
    } else {
      statusBatteryLevel = nil
    }

    let additionalStatus: UInt8?
    if flags & 0x04 != 0, bytes.count > index {
      additionalStatus = bytes[index]
    } else {
      additionalStatus = nil
    }

    let isCharging: Bool?
    let stateText: String
    switch chargeState {
    case 1:
      isCharging = true
      stateText = "Charging"
    case 2:
      isCharging = externalPowerConnected ? true : false
      stateText = externalPowerConnected ? "On charger" : "Discharging"
    case 3:
      isCharging = externalPowerConnected ? true : false
      stateText = externalPowerConnected ? "On charger" : "Idle"
    default:
      isCharging = externalPowerConnected ? true : nil
      stateText = externalPowerConnected ? "On charger" : "Unknown"
    }

    let externalPowerText: String
    switch (wiredPower, wirelessPower) {
    case (1, _):
      externalPowerText = "wired power"
    case (_, 1):
      externalPowerText = "wireless power"
    case (2, _), (_, 2):
      externalPowerText = "power unknown"
    default:
      externalPowerText = ""
    }

    let chargeLevelText: String
    switch chargeLevel {
    case 1:
      chargeLevelText = "good"
    case 2:
      chargeLevelText = "low"
    case 3:
      chargeLevelText = "critical"
    default:
      chargeLevelText = ""
    }

    let chargingTypeText: String
    switch chargingType {
    case 1:
      chargingTypeText = "constant current"
    case 2:
      chargingTypeText = "constant voltage"
    case 3:
      chargingTypeText = "trickle"
    case 4:
      chargingTypeText = "float"
    default:
      chargingTypeText = ""
    }

    var parts = [stateText]
    if !batteryPresent {
      parts.append("battery absent")
    }
    if !externalPowerText.isEmpty {
      parts.append(externalPowerText)
    }
    if !chargingTypeText.isEmpty {
      parts.append(chargingTypeText)
    }
    if !chargeLevelText.isEmpty {
      parts.append(chargeLevelText)
    }
    if chargingFault != 0 {
      parts.append("fault")
    }
    if let additionalStatus {
      let serviceRequired = Int(additionalStatus & 0x03)
      let batteryFault = additionalStatus & 0x04 != 0
      if serviceRequired == 1 {
        parts.append("service required")
      }
      if batteryFault {
        parts.append("battery fault")
      }
    }

    return BatteryLevelStatus(
      batteryLevelPercent: statusBatteryLevel,
      isCharging: isCharging,
      summary: parts.joined(separator: " | ")
    )
  }

  func whoopIdentityEvidence(
    for peripheral: CBPeripheral,
    fallbackName: String? = nil,
    advertisedServices: [CBUUID] = [],
    allowRememberedValidation: Bool = true
  ) -> String? {
    if advertisedServices.contains(where: isWhoopService) {
      return "advertised WHOOP service"
    }
    if whoopCandidateIDs.contains(peripheral.identifier) {
      return "cached WHOOP service match"
    }
    if isWhoopName(peripheral.name) {
      return "peripheral name \(peripheral.name ?? "")"
    }
    if isWhoopName(fallbackName) {
      return "advertised name \(fallbackName ?? "")"
    }
    if allowRememberedValidation,
       rememberedDeviceID == peripheral.identifier,
       rememberedDeviceLooksLikeWhoop {
      return "validated remembered WHOOP"
    }
    return nil
  }

  var rememberedDeviceLooksLikeWhoop: Bool {
    rememberedDeviceValidated || isWhoopName(rememberedDeviceName)
  }

  func isWhoopService(_ uuid: CBUUID) -> Bool {
    whoopServices.contains(uuid)
  }

  // Gen4 service UUID prefix 61080001-, Gen5 prefix fd4b0001-
  // WearableDescriptor stores prefix constants in lowercase; lowercased() is applied defensively
  // here even though CBUUID.uuidString always returns uppercase on iOS (CoreBluetooth contract),
  // keeping the comparison convention consistent with isCommandCharacteristic and debugMenuCandidate.
  static func generation(from serviceUUIDs: [CBUUID]) -> String {
    for uuid in serviceUUIDs {
      let lower = uuid.uuidString.lowercased()
      if lower.hasPrefix("61080001") { return "4.0" }
      if lower.hasPrefix("fd4b0001") { return "5.0" }
    }
    return "unknown"
  }

  func isWhoopName(_ name: String?) -> Bool {
    guard let name = name?.trimmingCharacters(in: .whitespacesAndNewlines), !name.isEmpty else {
      return false
    }
    return name.range(of: "whoop", options: [.caseInsensitive, .diacriticInsensitive]) != nil
  }

  static func sanitizedWhoopDisplayName(_ name: String?) -> String {
    let fallback = "WHOOP"
    guard let trimmed = name?.trimmingCharacters(in: .whitespacesAndNewlines), !trimmed.isEmpty else {
      return fallback
    }
    guard let whoopRange = trimmed.range(of: "whoop", options: [.caseInsensitive, .diacriticInsensitive]) else {
      return trimmed
    }
    let publicName = String(trimmed[..<whoopRange.upperBound])
      .trimmingCharacters(in: .whitespacesAndNewlines)
    return publicName.isEmpty ? fallback : String(publicName)
  }

  func advertisedServiceUUIDs(from advertisementData: [String: Any]) -> [CBUUID] {
    var uuids = advertisementData[CBAdvertisementDataServiceUUIDsKey] as? [CBUUID] ?? []
    uuids.append(contentsOf: advertisementData[CBAdvertisementDataOverflowServiceUUIDsKey] as? [CBUUID] ?? [])
    uuids.append(contentsOf: advertisementData[CBAdvertisementDataSolicitedServiceUUIDsKey] as? [CBUUID] ?? [])
    if let serviceData = advertisementData[CBAdvertisementDataServiceDataKey] as? [CBUUID: Data] {
      uuids.append(contentsOf: serviceData.keys)
    }
    return uuids
  }

  func shouldAutoConnectDiscoveredWhoop(_ peripheral: CBPeripheral) -> Bool {
    autoReconnectTargetID != nil
      && rememberedDeviceLooksLikeWhoop
      && activePeripheral == nil
      && rememberedDeviceID != nil
      && peripheral.identifier != rememberedDeviceID
  }

  func rejectNonWhoopPeripheral(
    _ peripheral: CBPeripheral,
    reason: String,
    fallbackName: String? = nil,
    disconnect: Bool = false
  ) {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.rejectNonWhoopPeripheral(peripheral, reason: reason, fallbackName: fallbackName, disconnect: disconnect) }
      return
    }
    let name = peripheral.name ?? fallbackName ?? "unknown"
    record(
      level: .warn,
      source: "ble",
      title: "whoop_filter.rejected",
      body: "reason=\(reason) name=\(name) id=\(peripheral.identifier.uuidString)"
    )
    peripherals.removeValue(forKey: peripheral.identifier)
    whoopCandidateIDs.remove(peripheral.identifier)
    discoveredDevices.removeAll { $0.id == peripheral.identifier }
    if selectedDeviceID == peripheral.identifier {
      selectedDeviceID = discoveredDevices.first?.id
    }
    if rememberedDeviceID == peripheral.identifier {
      clearRememberedDevice(reason: "non_whoop_\(reason)")
    }
    if activePeripheral?.identifier == peripheral.identifier {
      activePeripheral = nil
      activeDeviceIdentifier = nil
      updateActiveDeviceName("WHOOP")
      commandCharacteristic = nil
      batteryLevelCharacteristic = nil
      batteryLevelStatusCharacteristic = nil
      connectedAt = nil
      failAllDebugCommands("Connection rejected as non-WHOOP device.")
      updateConnectionState("not a WHOOP device")
    }
    if disconnect {
      central?.cancelPeripheralConnection(peripheral)
    }
  }

  func discoveredName(for id: UUID) -> String? {
    discoveredDevices.first { $0.id == id }?.name
  }

  func updateActiveDevice(_ peripheral: CBPeripheral, fallbackName: String? = nil) {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.updateActiveDevice(peripheral, fallbackName: fallbackName) }
      return
    }
    activeDeviceIdentifier = peripheral.identifier
    updateActiveDeviceName(Self.sanitizedWhoopDisplayName(peripheral.name ?? fallbackName ?? rememberedDeviceName ?? "WHOOP strap"))
  }

  func resetLiveDeviceFieldsIfNeeded(for peripheral: CBPeripheral) {
    if !Thread.isMainThread {
      DispatchQueue.main.async { [weak self] in self?.resetLiveDeviceFieldsIfNeeded(for: peripheral) }
      return
    }
    guard activeDeviceIdentifier != peripheral.identifier else {
      return
    }
    batteryLevelCharacteristic = nil
    batteryLevelStatusCharacteristic = nil
    batteryLevelPercent = nil
    batteryUpdatedAt = nil
    batteryIsCharging = nil
    batteryPowerStatus = "Unknown"
    firmwareVersion = nil
    modelNumber = nil
    hardwareRevision = nil
    softwareRevision = nil
    manufacturerName = nil
    historicalSyncStatus = "idle"
    historicalPacketCount = 0
    liveHeartRateBPM = nil
    liveHeartRateSource = "waiting"
    liveHeartRateUpdatedAt = nil
    self.resetRealtimeHeartRatePublishState()
    connectedAt = nil
    lastSyncAt = nil
    alarmCommandTimeoutWorkItem?.cancel()
    pendingAlarmCommand = nil
    clockCommandTimeoutWorkItem?.cancel()
    pendingClockCommand = nil
    failAllDebugCommands("Device changed before debug command response.")
    alarmCommandStatus = "No alarm command sent"
    lastAlarmCommandFrameHex = ""
    lastAlarmResponseSummary = "No alarm response yet"
    lastAlarmResponsePayloadHex = ""
    lastAlarmEventSummary = "No alarm event yet"
    lastAlarmEventPayloadHex = ""
    lastAlarmScheduledAt = nil
    lastAlarmID = nil
    highFrequencyHistorySyncStatus = "Off"
    highFrequencyHistorySyncActive = false
    highFrequencyHistorySyncRequestedExpiry = nil
    highFrequencyHistorySyncExpiresAt = nil
    lastHighFrequencyHistorySyncResponse = "No high-frequency sync response yet"
    lastHighFrequencyHistorySyncEvent = "No high-frequency sync event yet"
    strapClockDate = nil
    strapClockOffsetSeconds = nil
    strapClockUpdatedAt = nil
    strapClockStatus = "Not read"
    lastClockCommandFrameHex = ""
    lastClockResponsePayloadHex = ""
  }

  func resetRealtimeHeartRatePublishState() {
    realtimeVitalsQueue.async { [weak self] in
      self?.lastHeartRateLogAt = nil
      self?.lastHeartRateLogBPM = nil
      self?.lastHeartRateLogSource = ""
      self?.lastHeartRatePublishedAt = Date.distantPast
      self?.lastHeartRatePublishedBPM = nil
      self?.lastHeartRatePublishedSource = "waiting"
      self?.lastHeartRateCallbackAt = Date.distantPast
      self?.lastHeartRateCallbackSource = ""
    }
  }

  struct StandardHeartRateMeasurement {
    let bpm: Int
    let rrIntervalsMS: [Double]
  }

  static func parseStandardHeartRateMeasurement(_ value: Data) -> StandardHeartRateMeasurement? {
    guard value.count >= 2 else {
      return nil
    }
    let flags = value[0]
    var offset = 1
    let bpm: Int
    if flags & 0x01 == 0 {
      bpm = Int(value[offset])
      offset += 1
    } else {
      guard value.count >= offset + 2 else {
        return nil
      }
      bpm = Int(UInt16(value[offset]) | UInt16(value[offset + 1]) << 8)
      offset += 2
    }

    if flags & 0x08 != 0 {
      guard value.count >= offset + 2 else {
        return StandardHeartRateMeasurement(bpm: bpm, rrIntervalsMS: [])
      }
      offset += 2
    }

    var rrIntervalsMS: [Double] = []
    if flags & 0x10 != 0 {
      while value.count >= offset + 2 {
        let raw = UInt16(value[offset]) | UInt16(value[offset + 1]) << 8
        rrIntervalsMS.append(Double(raw) * 1000.0 / 1024.0)
        offset += 2
      }
    }
    return StandardHeartRateMeasurement(bpm: bpm, rrIntervalsMS: rrIntervalsMS)
  }

  static func rmssdMS(from intervalsMS: [Double]) -> Double? {
    guard intervalsMS.count >= 2 else {
      return nil
    }
    var squaredDifferenceTotal = 0.0
    var differenceCount = 0
    for index in 1..<intervalsMS.count {
      let difference = intervalsMS[index] - intervalsMS[index - 1]
      squaredDifferenceTotal += difference * difference
      differenceCount += 1
    }
    guard differenceCount > 0 else {
      return nil
    }
    return sqrt(squaredDifferenceTotal / Double(differenceCount))
  }

  static func lowQuartileMeanBPM(from samples: [Int]) -> Double {
    let sorted = samples.sorted()
    let count = max(1, sorted.count / 4)
    let lowQuartile = sorted.prefix(count)
    let total = lowQuartile.reduce(0, +)
    return Double(total) / Double(count)
  }

  func commandResultName(_ value: UInt8) -> String {
    switch value {
    case 0:
      return "FAILURE"
    case 1:
      return "SUCCESS"
    case 2:
      return "PENDING"
    case 3:
      return "UNSUPPORTED"
    default:
      return "RESULT_\(value)"
    }
  }

  func updateHistoricalRangeDebugStatus(_ status: String) {
    lastHistoricalRangeCommandStatus = status
    defaults.set(status, forKey: DefaultsKey.debugHistoricalRangeStatus)
  }

  func isValidHistoricalRangeResponse(_ payload: [UInt8]) -> Bool {
    let body = Array(payload.dropFirst(5))
    return body.count >= 25
  }

  func historicalResponseDetail(command: HistoricalCommandKind, payload: [UInt8]) -> String {
    guard command == .getDataRange else {
      return ""
    }
    let body = Array(payload.dropFirst(5))
    guard !body.isEmpty else {
      return " body=empty"
    }

    var words: [UInt32] = []
    var offset = 1
    while offset + 3 < body.count, offset < 25 {
      if let word = Self.readUInt32LE(body, at: offset) {
        words.append(word)
      }
      offset += 4
    }

    var parts = [
      "body=\(Data(body).hexString)",
      "revision_or_status=\(body[0])",
      "u32_words_from_offset_1=[\(words.map(String.init).joined(separator: ","))]",
    ]
    if words.count >= 6 {
      let pageCurrent = words[2]
      let pageOldest = words[3]
      let pageEnd = words[5]
      let pagesBehind: Int64 = pageCurrent < pageOldest
        ? Int64(pageCurrent) + Int64(pageEnd) - Int64(pageOldest)
        : Int64(pageCurrent) - Int64(pageOldest)
      parts.append("page_current=\(pageCurrent)")
      parts.append("page_oldest=\(pageOldest)")
      parts.append("page_end=\(pageEnd)")
      parts.append("pages_behind=\(pagesBehind)")
    }
    return " | " + parts.joined(separator: " ")
  }

  func emitHistoricalRangeTelemetry(
    status: String,
    pending: PendingHistoricalCommand,
    resultCode: UInt8,
    resultName: String,
    payload: [UInt8],
    notes: String
  ) {
    let body = Array(payload.dropFirst(5))
    var words: [UInt32] = []
    var offset = 1
    while offset + 3 < body.count, offset < 25 {
      if let word = Self.readUInt32LE(body, at: offset) {
        words.append(word)
      }
      offset += 4
    }

    let pageCurrent = words.count >= 3 ? words[2] : nil
    let pageOldest = words.count >= 4 ? words[3] : nil
    let pageEnd = words.count >= 6 ? words[5] : nil
    let pagesBehind: Int64?
    if let pageCurrent, let pageOldest, let pageEnd {
      pagesBehind = pageCurrent < pageOldest
        ? Int64(pageCurrent) + Int64(pageEnd) - Int64(pageOldest)
        : Int64(pageCurrent) - Int64(pageOldest)
    } else {
      pagesBehind = nil
    }

    onHistoricalRangeTelemetry?(
      GooseHistoricalRangeTelemetry(
        capturedAt: Date(),
        status: status,
        commandSequence: pending.sequence,
        resultCode: resultCode,
        resultName: resultName,
        payloadHex: Data(payload).hexString,
        bodyHex: Data(body).hexString,
        revisionOrStatus: body.first,
        wordsFromOffset1: words,
        pageCurrent: pageCurrent,
        pageOldest: pageOldest,
        pageEnd: pageEnd,
        pagesBehind: pagesBehind,
        pendingResponseCount: historicalRangePendingResponses,
        retryCount: historicalRangeRetryCount,
        notes: notes
      )
    )
  }

  func alarmResponseDetail(command: AlarmCommandKind, body: [UInt8]) -> String {
    switch command {
    case .set, .run:
      guard body.count >= 2 else {
        return ""
      }
      return " | \(hapticsAlarmStatusName(body[1]))"
    case .get:
      guard !body.isEmpty else {
        return ""
      }
      return " | body=\(Data(body).hexString)"
    case .disableAll:
      return ""
    }
  }

  func hapticsAlarmStatusName(_ value: UInt8) -> String {
    switch value {
    case 0:
      return "UNKNOWN"
    case 1:
      return "VALID_PATTERN"
    case 2:
      return "INVALID_EFFECT"
    case 3:
      return "INVALID_LOOPS"
    case 4:
      return "INVALID_DURATION"
    case 5:
      return "SUCCESSFUL_PLAY"
    case 6:
      return "HAPTICS_FAILURE"
    case 7:
      return "HAPTICS_TIMEOUT"
    case 8:
      return "HAPTICS_BUSY"
    case 9:
      return "HAPTICS_STOPPED"
    case 10:
      return "INVALID_ALARM_TIME"
    case 11:
      return "INVALID_ALARM_ID"
    default:
      return "HAPTICS_STATUS_\(value)"
    }
  }

  func hapticsTerminationName(_ value: UInt8) -> String {
    switch value {
    case 0:
      return "expired"
    case 1:
      return "error"
    case 2:
      return "user terminated"
    case 255:
      return "undefined"
    default:
      return "code \(value)"
    }
  }

  func uuidList(_ uuids: [CBUUID]) -> String {
    uuids.map(\.uuidString).joined(separator: ",")
  }

  func propertyNames(_ properties: CBCharacteristicProperties) -> String {
    var names: [String] = []
    if properties.contains(.read) { names.append("read") }
    if properties.contains(.write) { names.append("write") }
    if properties.contains(.writeWithoutResponse) { names.append("writeWithoutResponse") }
    if properties.contains(.notify) { names.append("notify") }
    if properties.contains(.indicate) { names.append("indicate") }
    if properties.contains(.broadcast) { names.append("broadcast") }
    if properties.contains(.authenticatedSignedWrites) { names.append("authenticatedSignedWrites") }
    if properties.contains(.extendedProperties) { names.append("extendedProperties") }
    return names.isEmpty ? "none" : names.joined(separator: ",")
  }

  func writeTypeName(_ writeType: CBCharacteristicWriteType) -> String {
    switch writeType {
    case .withResponse:
      return "withResponse"
    case .withoutResponse:
      return "withoutResponse"
    @unknown default:
      return "unknown"
    }
  }

  func emitCommandWrite(
    source: String,
    commandName: String,
    commandNumber: UInt8?,
    sequence: UInt8?,
    payload: Data,
    frame: Data,
    peripheral: CBPeripheral,
    characteristic: CBCharacteristic,
    writeType: CBCharacteristicWriteType
  ) {
    onCommandWrite?(
      GooseCommandWriteEvent(
        deviceID: peripheral.identifier,
        serviceUUID: characteristic.service?.uuid.uuidString ?? "unknown",
        characteristicUUID: characteristic.uuid.uuidString,
        commandName: commandName,
        commandNumber: commandNumber,
        sequence: sequence,
        payload: payload,
        frame: frame,
        writeType: writeTypeName(writeType),
        source: source,
        capturedAt: Date()
      )
    )
  }

  static func nextFutureAlarmDate(from localWakeTime: Date, now: Date = Date(), calendar: Calendar = .current) -> Date {
    let time = calendar.dateComponents([.hour, .minute, .second, .nanosecond], from: localWakeTime)
    var target = calendar.dateComponents([.year, .month, .day], from: now)
    target.hour = time.hour
    target.minute = time.minute
    target.second = time.second
    target.nanosecond = time.nanosecond

    let candidate = calendar.date(from: target) ?? localWakeTime
    if candidate > now {
      return candidate
    }
    return calendar.date(byAdding: .day, value: 1, to: candidate) ?? candidate.addingTimeInterval(24 * 60 * 60)
  }

  static func alarmTimestampParts(for date: Date) -> (seconds: UInt32, subseconds: UInt16) {
    let milliseconds = max(0, Int64((date.timeIntervalSince1970 * 1000).rounded()))
    let seconds = UInt32(min(Int64(UInt32.max), milliseconds / 1000))
    let millisecondRemainder = UInt32(milliseconds % 1000)
    let subseconds = UInt16((millisecondRemainder * 32768) / 1000)
    return (seconds, subseconds)
  }

  static func clockTimestampParts(for date: Date) -> (seconds: UInt32, subseconds: UInt32) {
    let milliseconds = max(0, Int64((date.timeIntervalSince1970 * 1000).rounded()))
    let seconds = UInt32(min(Int64(UInt32.max), milliseconds / 1000))
    let millisecondRemainder = UInt32(milliseconds % 1000)
    let subseconds = (millisecondRemainder * 32768) / 1000
    return (seconds, subseconds)
  }

  static func parseClockTimestamp(_ body: [UInt8]) -> Date? {
    guard let seconds = readUInt32LE(body, at: 0),
          let subseconds = readUInt32LE(body, at: 4) else {
      return nil
    }
    return Date(timeIntervalSince1970: TimeInterval(seconds) + TimeInterval(subseconds) / 32768.0)
  }

  static func readUInt32LE(_ bytes: [UInt8], at offset: Int) -> UInt32? {
    guard offset >= 0, bytes.count >= offset + 4 else {
      return nil
    }
    return UInt32(bytes[offset])
      | UInt32(bytes[offset + 1]) << 8
      | UInt32(bytes[offset + 2]) << 16
      | UInt32(bytes[offset + 3]) << 24
  }

  static func historicalDataResultPayload(fromHistoryEndMetadataPayload payload: [UInt8]) -> [UInt8]? {
    guard payload.count > 21 else {
      return nil
    }

    // Mirrors the Android ACK builder: success byte, then HistoryEnd body bytes 4...11.
    var result: [UInt8] = [1]
    result.append(contentsOf: payload[13..<21])
    return result
  }

  static func clockOffsetText(_ offset: TimeInterval) -> String {
    let rounded = Int(offset.rounded())
    if rounded == 0 {
      return "0s"
    }
    let sign = rounded > 0 ? "+" : "-"
    return "\(sign)\(abs(rounded))s"
  }

  static func appendUInt16LE(_ value: UInt16, to bytes: inout [UInt8]) {
    bytes.append(UInt8(value & 0xff))
    bytes.append(UInt8((value >> 8) & 0xff))
  }

  static func appendUInt32LE(_ value: UInt32, to bytes: inout [UInt8]) {
    bytes.append(UInt8(value & 0xff))
    bytes.append(UInt8((value >> 8) & 0xff))
    bytes.append(UInt8((value >> 16) & 0xff))
    bytes.append(UInt8((value >> 24) & 0xff))
  }

  static func v5Frames(in data: Data) -> [Data] {
    var bytes = Array(data)
    var frames: [Data] = []
    while let startIndex = bytes.firstIndex(of: 0xaa) {
      if startIndex > 0 {
        bytes.removeFirst(startIndex)
      }
      guard bytes.count >= 8 else {
        break
      }
      let declaredLength = Int(UInt16(bytes[2]) | UInt16(bytes[3]) << 8)
      guard declaredLength >= 4 else {
        bytes.removeFirst()
        continue
      }
      let expectedLength = declaredLength + 8
      guard bytes.count >= expectedLength else {
        break
      }
      frames.append(Data(bytes[0..<expectedLength]))
      bytes.removeFirst(expectedLength)
    }
    return frames
  }

  static func v5Payload(in frame: Data) -> [UInt8]? {
    let bytes = Array(frame)
    guard bytes.count >= 12 else {
      return nil
    }
    let declaredLength = Int(UInt16(bytes[2]) | UInt16(bytes[3]) << 8)
    let expectedLength = declaredLength + 8
    guard bytes.count == expectedLength, declaredLength >= 4 else {
      return nil
    }
    return Array(bytes[8..<(bytes.count - 4)])
  }

  static func buildV5CommandFrame(sequence: UInt8, command: UInt8, data: [UInt8]) -> Data {
    var payload = [V5PacketType.command, sequence, command]
    payload.append(contentsOf: data)
    let padding = payload.count % 4 == 0 ? 0 : 4 - payload.count % 4
    if padding > 0 {
      payload.append(contentsOf: repeatElement(UInt8(0), count: padding))
    }

    let payloadCRC = crc32(payload)
    let declaredLength = UInt16(payload.count + 4)
    var frame: [UInt8] = [
      0xaa,
      0x01,
      UInt8(declaredLength & 0xff),
      UInt8((declaredLength >> 8) & 0xff),
      0x00,
      0x01,
    ]
    let headerCRC = crc16Modbus(frame)
    frame.append(UInt8(headerCRC & 0xff))
    frame.append(UInt8((headerCRC >> 8) & 0xff))
    frame.append(contentsOf: payload)
    frame.append(UInt8(payloadCRC & 0xff))
    frame.append(UInt8((payloadCRC >> 8) & 0xff))
    frame.append(UInt8((payloadCRC >> 16) & 0xff))
    frame.append(UInt8((payloadCRC >> 24) & 0xff))
    return Data(frame)
  }

  static func crc16Modbus(_ bytes: [UInt8]) -> UInt16 {
    var crc = UInt16(0xffff)
    for byte in bytes {
      crc ^= UInt16(byte)
      for _ in 0..<8 {
        if crc & 1 == 1 {
          crc = (crc >> 1) ^ 0xa001
        } else {
          crc >>= 1
        }
      }
    }
    return crc
  }

  static func crc32(_ bytes: [UInt8]) -> UInt32 {
    var crc = UInt32(0xffffffff)
    for byte in bytes {
      crc ^= UInt32(byte)
      for _ in 0..<8 {
        if crc & 1 == 1 {
          crc = (crc >> 1) ^ 0xedb88320
        } else {
          crc >>= 1
        }
      }
    }
    return ~crc
  }
}
