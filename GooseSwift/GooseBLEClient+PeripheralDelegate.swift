import CoreBluetooth
import Foundation
import OSLog

extension GooseBLEClient: CBPeripheralDelegate {
  func peripheral(_ peripheral: CBPeripheral, didDiscoverServices error: Error?) {
    if dispatchCoreBluetoothDelegateToMainIfNeeded({ [weak self] in
      self?.peripheral(peripheral, didDiscoverServices: error)
    }) {
      return
    }

    if let error {
      updateConnectionState(error.localizedDescription)
      record(level: .error, source: "ble", title: "gatt.services.failed", body: error.localizedDescription)
      return
    }
    let services = peripheral.services ?? []
    record(source: "ble", title: "gatt.services", body: uuidList(services.map(\.uuid)))
    let hasWhoopService = services.contains(where: { isWhoopService($0.uuid) })
    if hasWhoopService {
      whoopCandidateIDs.insert(peripheral.identifier)
    } else if whoopCandidateIDs.contains(peripheral.identifier)
        || activePeripheral?.identifier == peripheral.identifier
        || rememberedDeviceID == peripheral.identifier {
      record(level: .debug, source: "ble", title: "gatt.services.partial", body: uuidList(services.map(\.uuid)))
    } else {
      rejectNonWhoopPeripheral(peripheral, reason: "gatt_missing_whoop_service", disconnect: true)
      return
    }
    for service in services {
      peripheral.discoverCharacteristics(nil, for: service)
    }
  }

  func peripheral(
    _ peripheral: CBPeripheral,
    didDiscoverCharacteristicsFor service: CBService,
    error: Error?
  ) {
    if dispatchCoreBluetoothDelegateToMainIfNeeded({ [weak self] in
      self?.peripheral(peripheral, didDiscoverCharacteristicsFor: service, error: error)
    }) {
      return
    }

    if let error {
      updateConnectionState(error.localizedDescription)
      record(level: .error, source: "ble", title: "gatt.characteristics.failed", body: "\(service.uuid.uuidString) \(error.localizedDescription)")
      return
    }

    if service.uuid == batteryServiceID {
      batteryCharacteristicDiscoveryPending = false
    }

    let characteristics = service.characteristics ?? []
    let characteristicSummary = characteristics
      .map { "\($0.uuid.uuidString)[\(propertyNames($0.properties))]" }
      .joined(separator: ",")
    record(source: "ble", title: "gatt.characteristics", body: "\(service.uuid.uuidString) \(characteristicSummary)")

    processDiscoveredCharacteristics(characteristics, for: service, peripheral: peripheral, cached: false)
  }

  func peripheral(
    _ peripheral: CBPeripheral,
    didUpdateValueFor characteristic: CBCharacteristic,
    error: Error?
  ) {
    let capturedAt = Date()
    let value = characteristic.value
    if !Thread.isMainThread,
       error == nil,
       let value,
       characteristic.uuid == standardHeartRateMeasurementID {
      let event = notificationEvent(
        peripheral,
        characteristic: characteristic,
        value: value,
        capturedAt: capturedAt
      )
      fanOutRawNotification(event)
      handleStandardHeartRate(value, characteristic: characteristic, capturedAt: capturedAt)
      return
    }
    if !Thread.isMainThread,
       error == nil,
       let value,
       shouldFanOutNotificationBeforeMain(characteristic) {
      let event = notificationEvent(
        peripheral,
        characteristic: characteristic,
        value: value,
        capturedAt: capturedAt
      )
      fanOutNotification(event)
      guard shouldDispatchNotificationSideEffectsToMain(value, characteristic: characteristic) else {
        recordSkippedNotificationSideEffect(value, characteristic: characteristic, capturedAt: capturedAt)
        publishNotificationSyncTimestampIfNeeded(capturedAt)
        return
      }
      DispatchQueue.main.async { [weak self] in
        self?.handlePeripheralValueUpdate(
          peripheral,
          characteristic: characteristic,
          value: value,
          capturedAt: capturedAt,
          error: nil,
          fanOutNotifications: false
        )
      }
      return
    }
    if dispatchCoreBluetoothDelegateToMainIfNeeded({ [weak self] in
      self?.handlePeripheralValueUpdate(
        peripheral,
        characteristic: characteristic,
        value: value,
        capturedAt: capturedAt,
        error: error,
        fanOutNotifications: true
      )
    }) {
      return
    }
    handlePeripheralValueUpdate(
      peripheral,
      characteristic: characteristic,
      value: value,
      capturedAt: capturedAt,
      error: error,
      fanOutNotifications: true
    )
  }

  func shouldFanOutNotificationBeforeMain(_ characteristic: CBCharacteristic) -> Bool {
    guard !standardReadableCharacteristic(characteristic),
          characteristic.uuid != standardHeartRateMeasurementID else {
      return false
    }
    return characteristic.properties.contains(.notify)
      || characteristic.properties.contains(.indicate)
  }

  func shouldDispatchNotificationSideEffectsToMain(_ value: Data, characteristic: CBCharacteristic) -> Bool {
    guard notificationCharacteristicIDs.contains(characteristic.uuid) else {
      return false
    }

    for frame in Self.v5Frames(in: value) {
      guard let payload = Self.v5Payload(in: frame),
            let packetType = payload.first else {
        continue
      }
      switch packetType {
      case V5PacketType.commandResponse,
           V5PacketType.puffinCommandResponse,
           V5PacketType.event,
           V5PacketType.metadata,
           V5PacketType.puffinMetadata:
        return true
      case V5PacketType.historicalData,
           V5PacketType.historicalIMUDataStream:
        // Route historical body packets to the main handler only while a sync
        // is active. Outside a sync these are high-rate live-stream frames that
        // should stay off-main for performance. Without this guard,
        // historicalPacketsReceivedThisSync is never incremented and every
        // sync fails with "no packet47 bodies" even when the band is streaming.
        if isHistoricalSyncing {
          return true
        }
        continue
      default:
        continue
      }
    }
    return false
  }

  func recordSkippedNotificationSideEffect(
    _ value: Data,
    characteristic: CBCharacteristic,
    capturedAt: Date
  ) {
    notificationSideEffectSkipCount += 1
    notificationSideEffectSkipBytes += value.count
    let shouldLog = notificationSideEffectSkipCount == 1
      || notificationSideEffectSkipCount.isMultiple(of: Self.notificationSideEffectSkipLogStride)
      || capturedAt.timeIntervalSince(lastNotificationSideEffectSkipLoggedAt) >= Self.notificationSideEffectSkipLogInterval
    guard shouldLog else {
      return
    }
    lastNotificationSideEffectSkipLoggedAt = capturedAt
    record(
      source: "ble.perf",
      title: "notification.side_effect.skipped",
      body: "count=\(notificationSideEffectSkipCount) bytes=\(notificationSideEffectSkipBytes) char=\(characteristic.uuid.uuidString) reason=data_stream_no_control_frame main_handler=false"
    )
  }

  func publishNotificationSyncTimestampIfNeeded(_ capturedAt: Date) {
    guard capturedAt.timeIntervalSince(lastNotificationSyncPublishedAt) >= Self.notificationSyncPublishInterval else {
      return
    }
    lastNotificationSyncPublishedAt = capturedAt
    bleUIStateAggregator.publishLastSyncAt(capturedAt)
  }

  func notificationEvent(
    _ peripheral: CBPeripheral,
    characteristic: CBCharacteristic,
    value: Data,
    capturedAt: Date
  ) -> GooseNotificationEvent {
    GooseNotificationEvent(
      deviceID: peripheral.identifier,
      serviceUUID: characteristic.service?.uuid.uuidString ?? "",
      characteristicUUID: characteristic.uuid.uuidString,
      value: value,
      capturedAt: capturedAt
    )
  }

  func fanOutNotification(_ event: GooseNotificationEvent) {
    fanOutRawNotification(event)
    onNotification?(event)
  }

  func fanOutRawNotification(_ event: GooseNotificationEvent) {
    if let onRawNotificationWithContext {
      onRawNotificationWithContext(event, notificationContextSnapshot())
    } else {
      onRawNotification?(event)
    }
  }

  func handlePeripheralValueUpdate(
    _ peripheral: CBPeripheral,
    characteristic: CBCharacteristic,
    value: Data?,
    capturedAt: Date,
    error: Error?,
    fanOutNotifications: Bool = true
  ) {
    let readValue = standardReadableCharacteristic(characteristic)
    if let error {
      record(
        level: .error,
        source: "ble",
        title: readValue ? "metadata.read.failed" : "notification.error",
        body: error.localizedDescription
      )
      return
    }
    guard let value else {
      record(
        level: .warn,
        source: "ble",
        title: readValue ? "metadata.read.empty" : "notification.empty",
        body: characteristic.uuid.uuidString
      )
      return
    }

    let event = notificationEvent(
      peripheral,
      characteristic: characteristic,
      value: value,
      capturedAt: capturedAt
    )
    if fanOutNotifications {
      fanOutRawNotification(event)
    }

    if handleStandardReadValue(value, characteristic: characteristic, capturedAt: capturedAt) {
      return
    }
    if characteristic.uuid == standardHeartRateMeasurementID {
      handleStandardHeartRate(value, characteristic: characteristic, capturedAt: capturedAt)
      return
    }

    handleDebugCommandValue(value, characteristic: characteristic)
    handleHistoricalSyncValue(value, characteristic: characteristic)
    handleAlarmValue(value, characteristic: characteristic)
    handleSensorStreamValue(value, characteristic: characteristic)
    handleClockValue(value, characteristic: characteristic)

    bleUIStateAggregator.publishLastSyncAt(event.capturedAt)
    record(
      level: .debug,
      source: "ble",
      title: "notification.received",
      body: "\(event.characteristicUUID) bytes=\(value.count)"
    )
    if fanOutNotifications {
      onNotification?(event)
    }
  }

  func peripheral(
    _ peripheral: CBPeripheral,
    didWriteValueFor characteristic: CBCharacteristic,
    error: Error?
  ) {
    if dispatchCoreBluetoothDelegateToMainIfNeeded({ [weak self] in
      self?.peripheral(peripheral, didWriteValueFor: characteristic, error: error)
    }) {
      return
    }

    if let error {
      record(level: .error, source: "ble", title: "write.failed", body: "\(characteristic.uuid.uuidString) \(error.localizedDescription)")
      if isHistoricalSyncing && characteristic.uuid == commandCharacteristic?.uuid {
        failHistoricalSync("Write to \(characteristic.uuid.uuidString) failed during historical sync: \(error.localizedDescription)")
      }
      if pendingAlarmCommand != nil && characteristic.uuid == commandCharacteristic?.uuid {
        failAlarmCommand("Write to \(characteristic.uuid.uuidString) failed during alarm command: \(error.localizedDescription)")
      }
      if pendingClockCommand != nil && characteristic.uuid == commandCharacteristic?.uuid {
        failClockCommand("Write to \(characteristic.uuid.uuidString) failed during clock command: \(error.localizedDescription)")
      }
      if !pendingDebugCommands.isEmpty && characteristic.uuid == commandCharacteristic?.uuid {
        failAllDebugCommands("Write to \(characteristic.uuid.uuidString) failed during debug command: \(error.localizedDescription)")
      }
    } else {
      record(source: "ble", title: "write.accepted", body: characteristic.uuid.uuidString)
    }
  }

  func peripheral(
    _ peripheral: CBPeripheral,
    didUpdateNotificationStateFor characteristic: CBCharacteristic,
    error: Error?
  ) {
    if dispatchCoreBluetoothDelegateToMainIfNeeded({ [weak self] in
      self?.peripheral(peripheral, didUpdateNotificationStateFor: characteristic, error: error)
    }) {
      return
    }

    if let error {
      record(level: .error, source: "ble", title: "notify.failed", body: "\(characteristic.uuid.uuidString) \(error.localizedDescription)")
    } else {
      let state = characteristic.isNotifying ? "subscribed" : "unsubscribed"
      record(source: "ble", title: "notify.state", body: "\(characteristic.uuid.uuidString) \(state)")
    }
  }
}
