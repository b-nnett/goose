import Foundation

// Pure Swift replacement for NotificationFrameParser (Rust bridge).
// Eliminates JSON+FFI overhead on every BLE notification — the main source of UI lag.
//
// Frame format (8-byte header, Maverick/Puffin/Goose/MG):
//   [0]    0xaa
//   [1]    0x01
//   [2,3]  payload_len LE
//   [4,7]  constants / header CRC
//   [8..]  payload
//
// Payload layout by packet type:
//   Data (40/43/47/51/52):  [type][k][status][counter u32][ts_s u32][ts_sub u16][body...]
//   Event (48/53/54):       [type][seq][event_id u16][ts_s u32][ts_sub u16][pad 2][data...]
//   Command (35/37):        [type][seq][command][data...]
//   Command resp (36/38):   [type][seq][cmd][origin_seq][result_code][data...]

final class WhoopFrameParser {

    func parseBatch(
        frameHexes: [String],
        deviceType: String
    ) -> ([NotificationFrameParseResult], GooseRustBridgeTiming?, NotificationFrameBatchTiming?) {
        guard !frameHexes.isEmpty else { return ([], nil, nil) }
        let isGen4 = deviceType == "GEN4"
        let results = frameHexes.map { parseFrame(hex: $0, isGen4: isGen4) }
        return (results, nil, nil)
    }

    private func parseFrame(hex: String, isGen4: Bool) -> NotificationFrameParseResult {
        guard let frameData = Data(hexString: hex) else {
            return .err("invalid hex")
        }
        let headerLen = isGen4 ? 4 : 8
        guard frameData.count >= headerLen + 1 else {
            return .err("frame too short (\(frameData.count) bytes)")
        }
        let payload = Array(frameData[headerLen...])
        let compact = buildCompact(payload: payload)
        return NotificationFrameParseResult(parsed: nil, compact: compact, errorDescription: nil)
    }

    private func buildCompact(payload: [UInt8]) -> NotificationFrameCompactSummary? {
        guard let typeByte = payload.first else { return nil }
        let packetType = Int(typeByte)
        let typeName = Self.packetTypeName(typeByte)
        switch typeByte {
        case 40, 43, 47, 51, 52: return buildDataCompact(payload: payload, packetType: packetType, typeName: typeName)
        case 48, 53, 54:         return buildEventCompact(payload: payload, packetType: packetType, typeName: typeName)
        case 35, 37:             return buildCommandCompact(payload: payload, packetType: packetType, typeName: typeName)
        case 36, 38:             return buildCmdRespCompact(payload: payload, packetType: packetType, typeName: typeName)
        default:                 return buildRawCompact(payload: payload, packetType: packetType, typeName: typeName)
        }
    }

    // MARK: - Data packets

    private func buildDataCompact(
        payload: [UInt8], packetType: Int, typeName: String?
    ) -> NotificationFrameCompactSummary? {
        let packetK: Int? = payload.count > 1 ? Int(payload[1]) : nil
        let domain = packetK.flatMap { Self.dataPacketDomain(k: $0) }
        let counterOrPage: Int? = readU32LE(payload, offset: 3).map(Int.init)
        let tsSeconds: Int? = readU32LE(payload, offset: 7).map(Int.init)
        let tsSubseconds: Int? = readU16LE(payload, offset: 11).map(Int.init)
        let bodyBytes: [UInt8] = payload.count > 13 ? Array(payload[13...]) : []
        let bodyHex: String? = bodyBytes.isEmpty ? nil : Data(bodyBytes).hexString
        let bodyKind: String? = packetK.map(Self.dataBodyKind(k:))
        let bodyByteCount = bodyBytes.count

        var heartRateBPM: Int?
        var movement: NotificationFrameCompactSummary.Movement?
        var r17Flags: Int?
        var r17ChannelsOrGain: [Int] = []
        var r17SampleCount: Int?

        if let k = packetK {
            switch k {
            case 10:
                if payload.count > 17 {
                    let rawHR = Int(payload[17])
                    if (20...240).contains(rawHR) { heartRateBPM = rawHR }
                }
                movement = computeK10Movement(payload: payload)
            case 16, 17:
                r17Flags = readU16LE(payload, offset: 13).map(Int.init)
                r17ChannelsOrGain = (15...20).compactMap { i in
                    payload.count > i ? Int(payload[i]) : nil
                }
                r17SampleCount = readU16LE(payload, offset: 24).map(Int.init)
            default:
                break
            }
        }

        let summary = "packet=\(typeName ?? "DATA")(\(packetType)) data.k=\(packetK.map(String.init) ?? "?") domain=\(domain ?? "unknown") body=\(bodyKind ?? "none")"
        let movDict: [String: Any]? = movement.map { movementDict($0) }

        return NotificationFrameCompactSummary(raw: [
            "summary": summary,
            "packet_type": packetType,
            "packet_type_name": typeName as Any,
            "sequence": packetK as Any,
            "warnings_count": 0,
            "payload_kind": "data_packet",
            "packet_k": packetK as Any,
            "domain": domain as Any,
            "counter_or_page": counterOrPage as Any,
            "timestamp_seconds": tsSeconds as Any,
            "timestamp_subseconds": tsSubseconds as Any,
            "body_hex": bodyHex as Any,
            "body_kind": bodyKind as Any,
            "body_byte_count": bodyByteCount,
            "heart_rate": heartRateBPM as Any,
            "r17_flags": r17Flags as Any,
            "r17_channels_or_gain": r17ChannelsOrGain,
            "r17_sample_count": r17SampleCount as Any,
            "movement": movDict as Any,
        ])
    }

    // MARK: - Event packets

    private func buildEventCompact(
        payload: [UInt8], packetType: Int, typeName: String?
    ) -> NotificationFrameCompactSummary? {
        let sequence: Int? = payload.count > 1 ? Int(payload[1]) : nil
        let eventID: Int? = readU16LE(payload, offset: 2).map(Int.init)
        let eventName: String? = eventID.flatMap { Self.strapEventName(id: $0) }
        let tsSeconds: Int? = readU32LE(payload, offset: 4).map(Int.init)
        let tsSubseconds: Int? = readU16LE(payload, offset: 8).map(Int.init)
        let dataBytes: [UInt8] = payload.count > 12 ? Array(payload[12...]) : []
        let dataHex: String? = dataBytes.isEmpty ? nil : Data(dataBytes).hexString
        let eventByteCount = dataBytes.count
        let eventLabel = eventName ?? eventID.map { "event_\($0)" } ?? "unknown"
        let summary = "packet=\(typeName ?? "EVENT")(\(packetType)) event=\(eventLabel) bytes=\(eventByteCount)"

        return NotificationFrameCompactSummary(raw: [
            "summary": summary,
            "packet_type": packetType,
            "packet_type_name": typeName as Any,
            "sequence": sequence as Any,
            "warnings_count": 0,
            "payload_kind": "event",
            "event_id": eventID as Any,
            "event_name": eventName as Any,
            "event_byte_count": eventByteCount,
            "data_hex": dataHex as Any,
            "timestamp_seconds": tsSeconds as Any,
            "timestamp_subseconds": tsSubseconds as Any,
        ])
    }

    // MARK: - Command / response packets

    private func buildCommandCompact(
        payload: [UInt8], packetType: Int, typeName: String?
    ) -> NotificationFrameCompactSummary? {
        let sequence: Int? = payload.count > 1 ? Int(payload[1]) : nil
        let summary = "packet=\(typeName ?? "COMMAND")(\(packetType)) seq=\(sequence.map(String.init) ?? "?")"
        return NotificationFrameCompactSummary(raw: [
            "summary": summary,
            "packet_type": packetType,
            "packet_type_name": typeName as Any,
            "sequence": sequence as Any,
            "warnings_count": 0,
            "payload_kind": "command",
        ])
    }

    private func buildCmdRespCompact(
        payload: [UInt8], packetType: Int, typeName: String?
    ) -> NotificationFrameCompactSummary? {
        let sequence: Int? = payload.count > 1 ? Int(payload[1]) : nil
        let summary = "packet=\(typeName ?? "COMMAND_RESPONSE")(\(packetType)) seq=\(sequence.map(String.init) ?? "?")"
        return NotificationFrameCompactSummary(raw: [
            "summary": summary,
            "packet_type": packetType,
            "packet_type_name": typeName as Any,
            "sequence": sequence as Any,
            "warnings_count": 0,
            "payload_kind": "command_response",
        ])
    }

    private func buildRawCompact(
        payload: [UInt8], packetType: Int, typeName: String?
    ) -> NotificationFrameCompactSummary? {
        let sequence: Int? = payload.count > 1 ? Int(payload[1]) : nil
        let summary = "packet=\(typeName ?? "unknown")(\(packetType)) payload=raw"
        return NotificationFrameCompactSummary(raw: [
            "summary": summary,
            "packet_type": packetType,
            "packet_type_name": typeName as Any,
            "sequence": sequence as Any,
            "warnings_count": 0,
            "payload_kind": "raw",
        ])
    }

    // MARK: - K10 motion intensity
    // Offsets into the full payload (same reference frame as Rust protocol.rs parse_k10_raw_motion_summary).
    // acc_x@85, acc_y@285, acc_z@485, gyro_x@688, gyro_y@888, gyro_z@1088 — 100 i16 samples each.

    private func computeK10Movement(payload: [UInt8]) -> NotificationFrameCompactSummary.Movement? {
        let axes: [(isAccelerometer: Bool, offset: Int, count: Int)] = [
            (true,  85,   100),
            (true,  285,  100),
            (true,  485,  100),
            (false, 688,  100),
            (false, 888,  100),
            (false, 1088, 100),
        ]
        var axisCount = 0
        var parsedSampleCount = 0
        var rawPeakRange = 0.0
        var rawPeakAbs = 0.0
        var accelerometerPeakRange = 0.0
        var gyroscopePeakRange = 0.0
        var accelerometerRangeSquaredTotal = 0.0

        for axis in axes {
            let available = payload.count > axis.offset
                ? (payload.count - axis.offset) / 2
                : 0
            let parsed = min(axis.count, available)
            guard parsed > 0 else { continue }

            var minVal = Int16.max
            var maxVal = Int16.min
            for i in 0..<parsed {
                let off = axis.offset + i * 2
                let sample = Int16(bitPattern: UInt16(payload[off]) | (UInt16(payload[off + 1]) << 8))
                if sample < minVal { minVal = sample }
                if sample > maxVal { maxVal = sample }
            }
            let range = Double(maxVal) - Double(minVal)
            let peakAbs = max(abs(Double(minVal)), abs(Double(maxVal)))
            axisCount += 1
            parsedSampleCount += parsed
            rawPeakRange = max(rawPeakRange, range)
            rawPeakAbs = max(rawPeakAbs, peakAbs)
            if axis.isAccelerometer {
                accelerometerPeakRange = max(accelerometerPeakRange, range)
                accelerometerRangeSquaredTotal += range * range
            } else {
                gyroscopePeakRange = max(gyroscopePeakRange, range)
            }
        }

        guard parsedSampleCount > 0 else { return nil }

        let accelerometerVectorRange = accelerometerRangeSquaredTotal.squareRoot()
        let motionIntensity = min(
            1.0,
            max(rawPeakRange / 32767.0, accelerometerVectorRange / 8192.0)
        )
        return NotificationFrameCompactSummary.Movement(raw: [
            "axis_count": axisCount,
            "parsed_sample_count": parsedSampleCount,
            "raw_peak_range": rawPeakRange,
            "raw_peak_abs": rawPeakAbs,
            "accelerometer_peak_range": accelerometerPeakRange,
            "gyroscope_peak_range": gyroscopePeakRange,
            "accelerometer_vector_range": accelerometerVectorRange,
            "motion_intensity": motionIntensity,
        ])
    }

    private func movementDict(_ m: NotificationFrameCompactSummary.Movement) -> [String: Any] {
        [
            "axis_count": m.axisCount,
            "parsed_sample_count": m.parsedSampleCount,
            "raw_peak_range": m.rawPeakRange,
            "raw_peak_abs": m.rawPeakAbs,
            "accelerometer_peak_range": m.accelerometerPeakRange,
            "gyroscope_peak_range": m.gyroscopePeakRange,
            "accelerometer_vector_range": m.accelerometerVectorRange,
            "motion_intensity": m.motionIntensity,
        ]
    }

    // MARK: - Byte readers

    private func readU16LE(_ payload: [UInt8], offset: Int) -> UInt16? {
        guard payload.count > offset + 1 else { return nil }
        return UInt16(payload[offset]) | (UInt16(payload[offset + 1]) << 8)
    }

    private func readU32LE(_ payload: [UInt8], offset: Int) -> UInt32? {
        guard payload.count > offset + 3 else { return nil }
        return UInt32(payload[offset])
            | (UInt32(payload[offset + 1]) << 8)
            | (UInt32(payload[offset + 2]) << 16)
            | (UInt32(payload[offset + 3]) << 24)
    }

    // MARK: - Lookup tables (ported from Rust protocol.rs)

    static func packetTypeName(_ type: UInt8) -> String? {
        switch type {
        case 35: return "COMMAND"
        case 36: return "COMMAND_RESPONSE"
        case 37: return "PUFFIN_COMMAND"
        case 38: return "PUFFIN_COMMAND_RESPONSE"
        case 40: return "REALTIME_DATA"
        case 43: return "REALTIME_RAW_DATA"
        case 47: return "HISTORICAL_DATA"
        case 48: return "EVENT"
        case 49: return "METADATA"
        case 50: return "CONSOLE_LOGS"
        case 51: return "REALTIME_IMU_DATA_STREAM"
        case 52: return "HISTORICAL_IMU_DATA_STREAM"
        case 53: return "RELATIVE_PUFFIN_EVENTS"
        case 54: return "PUFFIN_EVENTS_FROM_STRAP"
        case 55: return "RELATIVE_BATTERY_PACK_CONSOLE_LOGS"
        case 56: return "PUFFIN_METADATA"
        default: return nil
        }
    }

    static func dataPacketDomain(k: Int) -> String? {
        switch k {
        case 7:              return "legacy_raw_or_research_counted"
        case 9, 12, 18, 24: return "normal_history_with_hr_marker"
        case 10, 21:         return "raw_motion_stream_result"
        case 11:             return "raw_stream_counted"
        case 16:             return "raw_ecg_labrador"
        case 17:             return "r17_optical_or_labrador_filtered"
        case 19, 22:         return "research_packet"
        case 20:             return "raw_or_research_counted"
        case 25, 26:         return "pulse_information_packet"
        default:             return nil
        }
    }

    static func dataBodyKind(k: Int) -> String {
        switch k {
        case 7, 9, 12, 18, 24: return "normal_history"
        case 16:               return "raw_ecg_labrador"
        case 17:               return "r17_optical_or_labrador_filtered"
        case 10:               return "raw_motion_k10"
        case 21:               return "raw_motion_k21"
        default:               return "none"
        }
    }

    static func strapEventName(id: Int) -> String? {
        switch id {
        case 0:   return "UNDEFINED"
        case 1:   return "ERROR"
        case 2:   return "CONSOLE_OUTPUT"
        case 3:   return "BATTERY_LEVEL"
        case 4:   return "SYSTEM_CONTROL"
        case 7:   return "CHARGING_ON"
        case 8:   return "CHARGING_OFF"
        case 9:   return "WRIST_ON"
        case 10:  return "WRIST_OFF"
        case 11:  return "BLE_CONNECTION_UP"
        case 12:  return "BLE_CONNECTION_DOWN"
        case 13:  return "RTC_LOST"
        case 14:  return "DOUBLE_TAP"
        case 15:  return "BOOT"
        case 16:  return "SET_RTC"
        case 17:  return "TEMPERATURE_LEVEL"
        case 18:  return "PAIRING_MODE"
        case 28:  return "FLASH_INIT_COMPLETE"
        case 29:  return "STRAP_CONDITION_REPORT"
        case 33:  return "BLE_REALTIME_HR_ON"
        case 34:  return "BLE_REALTIME_HR_OFF"
        case 56:  return "STRAP_DRIVEN_ALARM_SET"
        case 57:  return "STRAP_DRIVEN_ALARM_EXECUTED"
        case 58:  return "APP_DRIVEN_ALARM_EXECUTED"
        case 59:  return "STRAP_DRIVEN_ALARM_DISABLED"
        case 60:  return "HAPTICS_FIRED"
        case 63:  return "EXTENDED_BATTERY_INFORMATION"
        case 96:  return "HIGH_FREQ_SYNC_PROMPT"
        case 97:  return "HIGH_FREQ_SYNC_ENABLED"
        case 98:  return "HIGH_FREQ_SYNC_DISABLED"
        case 100: return "HAPTICS_TERMINATED"
        case 109: return "BATTERY_PACK_INFO"
        case 123: return "GENERIC_FIRMWARE_EVENT"
        default:  return nil
        }
    }
}

// MARK: - Convenience

private extension NotificationFrameParseResult {
    static func err(_ description: String) -> NotificationFrameParseResult {
        NotificationFrameParseResult(parsed: nil, compact: nil, errorDescription: description)
    }
}
