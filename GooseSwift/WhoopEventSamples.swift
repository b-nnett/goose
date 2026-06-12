import Foundation
import UIKit

struct WhoopEventSample {
  let capturedAt: Date
  let eventID: Int?
  let eventName: String
  let dataHex: String
  let rawI16LE: Int?
  let rawU16LE: Int?
  let temperatureCandidates: [TemperatureEventCandidate]
  let deviceTimestampSeconds: Int?
  let deviceTimestampSubseconds: Int?

  var isTemperatureLevelEvent: Bool {
    eventID == 17 || eventName == "TEMPERATURE_LEVEL"
  }

  var dataByteCount: Int {
    dataHex.count / 2
  }

  var primaryTemperatureCandidate: TemperatureEventCandidate? {
    temperatureCandidates.first
  }

  var statusSummary: String {
    let idText = eventID.map { "\($0)" } ?? "?"
    return "\(eventName)(\(idText)) body=\(dataByteCount) bytes"
  }

  var temperatureCandidateSummary: String {
    guard isTemperatureLevelEvent else {
      return "Latest event is not temperature"
    }
    if let primaryTemperatureCandidate {
      return "\(primaryTemperatureCandidate.summary) | candidates=\(temperatureCandidates.count) | body=\(dataByteCount) bytes"
    }
    guard rawI16LE != nil || rawU16LE != nil else {
      return "TEMPERATURE_LEVEL body too short"
    }
    return "TEMPERATURE_LEVEL no plausible Celsius candidate | raw_i16=\(rawI16LE.map(String.init) ?? "?") raw_u16=\(rawU16LE.map(String.init) ?? "?") | body=\(dataByteCount) bytes"
  }

  var logSummary: String {
    let idText = eventID.map { "\($0)" } ?? "?"
    let deviceTime = deviceTimestampSeconds.map { "device_ts=\($0).\(deviceTimestampSubseconds ?? 0)" } ?? "device_ts=?"
    let rawText = rawI16LE.map { "raw_i16=\($0)" } ?? "raw_i16=?"
    let rawUnsignedText = rawU16LE.map { "raw_u16=\($0)" } ?? "raw_u16=?"
    let candidateText = primaryTemperatureCandidate.map { "temp_candidate={\($0.logSummary)}" } ?? "temp_candidate=?"
    return "event=\(eventName)(\(idText)) bytes=\(dataByteCount) \(rawText) \(rawUnsignedText) candidates=\(temperatureCandidates.count) \(candidateText) \(deviceTime) captured_at=\(capturedAt.formatted(date: .omitted, time: .standard)) data_hex=\(truncatedDataHex)"
  }

  private var truncatedDataHex: String {
    guard dataHex.count > 96 else {
      return dataHex
    }
    return "\(dataHex.prefix(96))...(+\(dataByteCount - 48) bytes)"
  }

  static func fromParsedFrame(_ parsed: [String: Any], capturedAt: Date) -> WhoopEventSample? {
    guard
      let payload = parsed["parsed_payload"] as? [String: Any],
      payload["kind"] as? String == "event"
    else {
      return nil
    }

    let dataHex = payload["data_hex"] as? String ?? ""
    let data = Data(hexString: dataHex)
    let rawI16 = Self.int16LE(data)
    let rawU16 = Self.uint16LE(data)
    let eventID = intValue(payload["event_id"])
    let eventName = (payload["event_name"] as? String) ?? eventID.map { "event_\($0)" } ?? "unknown"
    let isTemperature = eventID == 17 || eventName == "TEMPERATURE_LEVEL"
    let temperatureCandidates = isTemperature ? TemperatureEventCandidate.candidates(in: data ?? Data()) : []

    return WhoopEventSample(
      capturedAt: capturedAt,
      eventID: eventID,
      eventName: eventName,
      dataHex: dataHex,
      rawI16LE: rawI16,
      rawU16LE: rawU16,
      temperatureCandidates: temperatureCandidates,
      deviceTimestampSeconds: intValue(payload["timestamp_seconds"]),
      deviceTimestampSubseconds: intValue(payload["timestamp_subseconds"])
    )
  }

  static func fromCompactSummary(_ compact: NotificationFrameCompactSummary, capturedAt: Date) -> WhoopEventSample? {
    guard compact.payloadKind == "event" else {
      return nil
    }

    let dataHex = compact.dataHex ?? ""
    let data = Data(hexString: dataHex)
    let eventName = compact.eventName ?? compact.eventID.map { "event_\($0)" } ?? "unknown"
    let isTemperature = compact.eventID == 17 || eventName == "TEMPERATURE_LEVEL"
    let temperatureCandidates = isTemperature ? TemperatureEventCandidate.candidates(in: data ?? Data()) : []

    return WhoopEventSample(
      capturedAt: capturedAt,
      eventID: compact.eventID,
      eventName: eventName,
      dataHex: dataHex,
      rawI16LE: Self.int16LE(data),
      rawU16LE: Self.uint16LE(data),
      temperatureCandidates: temperatureCandidates,
      deviceTimestampSeconds: compact.timestampSeconds,
      deviceTimestampSubseconds: compact.timestampSubseconds
    )
  }

  private static func int16LE(_ data: Data?) -> Int? {
    guard let data, data.count >= 2 else {
      return nil
    }
    let raw = UInt16(data[0]) | (UInt16(data[1]) << 8)
    return Int(Int16(bitPattern: raw))
  }

  private static func uint16LE(_ data: Data?) -> Int? {
    guard let data, data.count >= 2 else {
      return nil
    }
    return Int(UInt16(data[0]) | (UInt16(data[1]) << 8))
  }

  private static func intValue(_ value: Any?) -> Int? {
    if let int = value as? Int {
      return int
    }
    if let number = value as? NSNumber {
      return number.intValue
    }
    if let string = value as? String {
      return Int(string)
    }
    return nil
  }
}

struct TemperatureEventCandidate {
  private struct Spec {
    let encoding: String
    let width: Int
    let signed: Bool
    let scale: Double
    let rank: Int
  }

  let bodyOffset: Int
  let absoluteOffset: Int
  let encoding: String
  let rawValue: Int
  let celsius: Double
  let rank: Int

  var summary: String {
    "\(String(format: "%.2f C", celsius)) candidate | \(encoding) body+\(bodyOffset)"
  }

  var logSummary: String {
    "offset=\(bodyOffset) absolute_offset=\(absoluteOffset) encoding=\(encoding) raw=\(rawValue) celsius=\(String(format: "%.4f", celsius)) confidence=range_only_capture_gated"
  }

  static func candidates(in data: Data) -> [TemperatureEventCandidate] {
    let specs = [
      Spec(encoding: "i16_le_x100", width: 2, signed: true, scale: 100.0, rank: 0),
      Spec(encoding: "u16_le_x100", width: 2, signed: false, scale: 100.0, rank: 1),
      Spec(encoding: "i16_le_x1000", width: 2, signed: true, scale: 1000.0, rank: 2),
      Spec(encoding: "u16_le_x1000", width: 2, signed: false, scale: 1000.0, rank: 3),
      Spec(encoding: "i16_le_x10", width: 2, signed: true, scale: 10.0, rank: 4),
      Spec(encoding: "u16_le_x10", width: 2, signed: false, scale: 10.0, rank: 5),
      Spec(encoding: "u32_le_x100", width: 4, signed: false, scale: 100.0, rank: 6),
      Spec(encoding: "u32_le_x1000", width: 4, signed: false, scale: 1000.0, rank: 7),
    ]

    let maxOffset = min(data.count, 12)
    var candidates: [TemperatureEventCandidate] = []
    for offset in 0..<maxOffset {
      for spec in specs where data.count >= offset + spec.width {
        guard let rawValue = rawInteger(data, offset: offset, width: spec.width, signed: spec.signed) else {
          continue
        }
        let celsius = Double(rawValue) / spec.scale
        guard (20.0...45.0).contains(celsius) else {
          continue
        }
        candidates.append(
          TemperatureEventCandidate(
            bodyOffset: offset,
            absoluteOffset: 12 + offset,
            encoding: spec.encoding,
            rawValue: rawValue,
            celsius: celsius,
            rank: spec.rank
          )
        )
      }
    }

    return candidates.sorted { lhs, rhs in
      if lhs.bodyOffset != rhs.bodyOffset {
        return lhs.bodyOffset < rhs.bodyOffset
      }
      if lhs.rank != rhs.rank {
        return lhs.rank < rhs.rank
      }
      return abs(lhs.celsius - 35.0) < abs(rhs.celsius - 35.0)
    }
  }

  private static func rawInteger(_ data: Data, offset: Int, width: Int, signed: Bool) -> Int? {
    guard width == 2 || width == 4, data.count >= offset + width else {
      return nil
    }

    var value: UInt32 = 0
    for byteIndex in 0..<width {
      value |= UInt32(data[offset + byteIndex]) << UInt32(byteIndex * 8)
    }

    if signed {
      if width == 2 {
        return Int(Int16(bitPattern: UInt16(value & 0xffff)))
      }
      return Int(Int32(bitPattern: value))
    }
    return Int(value)
  }
}

struct WhoopDataSignalSample {
  let capturedAt: Date
  let packetType: Int?
  let packetK: Int
  let domain: String
  let bodyKind: String
  let bodyHex: String
  let bodyByteCount: Int
  let counterOrPage: Int?
  let deviceTimestampSeconds: Int?
  let deviceTimestampSubseconds: Int?
  let historyTemperature: HistoryTemperatureCandidate?
  let historyRespiratoryRate: HistoryRespiratoryRateCandidate?
  let r21Motion: R21MotionCandidate?
  let r17Flags: Int?
  let r17SampleCount: Int?
  let r17ParsedSampleCount: Int?
  let r17Min: Int?
  let r17Max: Int?
  let r17ChannelsOrGain: [Int]
  let rawDiagnostic: RawBodyDiagnostic?

  var isPulseInformationPacket: Bool {
    packetK == 25 || packetK == 26
  }

  var isRealtimeStatusPacket: Bool {
    packetK == 2
  }

  var isRawResearchPacket: Bool {
    packetK == 20
  }

  var isRawStreamCountedPacket: Bool {
    packetK == 11
  }

  var statusSummary: String {
    if let respiratoryRate = historyRespiratoryRate?.respiratoryRateRPM {
      return "K\(packetK) \(domain) body=\(bodyByteCount) bytes rr=\(String(format: "%.1f", respiratoryRate)) rpm"
    }
    return "K\(packetK) \(domain) body=\(bodyByteCount) bytes"
  }

  var pulseInformationSummary: String {
    "K\(packetK) pulse-information body=\(bodyByteCount) bytes captured"
  }

  var r17OpticalSummary: String? {
    guard packetK == 16 || packetK == 17 else {
      return nil
    }
    let label = packetK == 16 ? "K16 ECG" : "R17 optical"
    let flagsText = r17Flags.map { String(format: "0x%04x", $0) } ?? "?"
    let samplesText = r17SampleCount.map { "\($0)" } ?? "?"
    let parsedText = r17ParsedSampleCount.map { "\($0)" } ?? "?"
    let rangeText: String
    if let r17Min, let r17Max {
      rangeText = "\(r17Min)...\(r17Max)"
    } else {
      rangeText = "?"
    }
    return "\(label) samples=\(parsedText)/\(samplesText) flags=\(flagsText) range=\(rangeText)"
  }

  var opticalSummary: String? {
    r17OpticalSummary
  }

  var r21MotionSummary: String? {
    r21Motion?.summary
  }

  var rawDiagnosticSummary: String {
    rawDiagnostic?.summary(packetK: packetK) ?? "K\(packetK) \(domain) body=\(bodyByteCount) bytes"
  }

  var rawDiagnosticDetail: String {
    rawDiagnostic?.logSummary ?? "body_kind=\(bodyKind) body_hex=\(truncatedBodyHex)"
  }

  var logSummary: String {
    let packetTypeText = packetType.map { "packet_type=\($0)" } ?? "packet_type=?"
    let counterText = counterOrPage.map { "counter=\($0)" } ?? "counter=?"
    let deviceTime = deviceTimestampSeconds.map { "device_ts=\($0).\(deviceTimestampSubseconds ?? 0)" } ?? "device_ts=?"
    let tempText = historyTemperature.map { " temp={\($0.logSummary)}" } ?? ""
    let respiratoryText = historyRespiratoryRate.map { " resp={\($0.logSummary)}" } ?? ""
    let r21Text = r21Motion.map { " r21_motion={\($0.logSummary)}" } ?? ""
    let channelText = r17ChannelsOrGain.isEmpty ? "" : " channels=\(r17ChannelsOrGain.map(String.init).joined(separator: ","))"
    let opticalText = r17OpticalSummary.map { " optical={\($0)\(channelText)}" } ?? ""
    let rawText = rawDiagnostic.map { " raw={\($0.logSummary)}" } ?? ""
    return "\(packetTypeText) K\(packetK) domain=\(domain) body_kind=\(bodyKind) bytes=\(bodyByteCount) \(counterText) \(deviceTime)\(tempText)\(respiratoryText)\(r21Text)\(opticalText)\(rawText) body_hex=\(truncatedBodyHex)"
  }

  private var truncatedBodyHex: String {
    guard bodyHex.count > 96 else {
      return bodyHex
    }
    return "\(bodyHex.prefix(96))...(+\(bodyByteCount - 48) bytes)"
  }

  static func fromParsedFrame(_ parsed: [String: Any], capturedAt: Date) -> WhoopDataSignalSample? {
    guard
      let payload = parsed["parsed_payload"] as? [String: Any],
      payload["kind"] as? String == "data_packet",
      let packetK = intValue(payload["packet_k"]),
      [2, 9, 10, 11, 12, 16, 17, 18, 20, 21, 24, 25, 26, 47].contains(packetK)
    else {
      return nil
    }

    let bodyHex = payload["body_hex"] as? String ?? ""
    let bodyData = Data(hexString: bodyHex)
    let bodySummary = payload["body_summary"] as? [String: Any]
    let samples = bodySummary?["samples"] as? [String: Any]
    let channels = (bodySummary?["channels_or_gain"] as? [Any])?.compactMap { intValue($0) } ?? []

    return WhoopDataSignalSample(
      capturedAt: capturedAt,
      packetType: intValue(parsed["packet_type"]),
      packetK: packetK,
      domain: payload["domain"] as? String ?? "unknown",
      bodyKind: bodySummary?["kind"] as? String ?? "raw",
      bodyHex: bodyHex,
      bodyByteCount: bodyHex.count / 2,
      counterOrPage: intValue(payload["counter_or_page"]),
      deviceTimestampSeconds: intValue(payload["timestamp_seconds"]),
      deviceTimestampSubseconds: intValue(payload["timestamp_subseconds"]),
      historyTemperature: HistoryTemperatureCandidate.from(packetK: packetK, body: bodyData),
      historyRespiratoryRate: HistoryRespiratoryRateCandidate.from(packetK: packetK, body: bodyData),
      r21Motion: R21MotionCandidate.from(packetK: packetK, bodySummary: bodySummary)
        ?? R21MotionCandidate.from(packetK: packetK, body: bodyData),
      r17Flags: intValue(bodySummary?["flags"]),
      r17SampleCount: intValue(bodySummary?["sample_count"]),
      r17ParsedSampleCount: intValue(samples?["parsed_count"]),
      r17Min: intValue(samples?["min"]),
      r17Max: intValue(samples?["max"]),
      r17ChannelsOrGain: channels,
      rawDiagnostic: RawBodyDiagnostic.from(packetK: packetK, body: bodyData)
    )
  }

  static func fromCompactSummary(_ compact: NotificationFrameCompactSummary, capturedAt: Date) -> WhoopDataSignalSample? {
    guard
      compact.payloadKind == "data_packet",
      let packetK = compact.packetK,
      [2, 9, 10, 11, 12, 16, 17, 18, 20, 21, 24, 25, 26, 47].contains(packetK)
    else {
      return nil
    }

    let bodyHex = compact.bodyHex ?? ""
    let bodyData = Data(hexString: bodyHex)
    return WhoopDataSignalSample(
      capturedAt: capturedAt,
      packetType: compact.packetType,
      packetK: packetK,
      domain: compact.domain ?? "unknown",
      bodyKind: compact.bodyKind ?? "raw",
      bodyHex: bodyHex,
      bodyByteCount: compact.bodyByteCount ?? bodyHex.count / 2,
      counterOrPage: compact.counterOrPage,
      deviceTimestampSeconds: compact.timestampSeconds,
      deviceTimestampSubseconds: compact.timestampSubseconds,
      historyTemperature: HistoryTemperatureCandidate.from(packetK: packetK, body: bodyData),
      historyRespiratoryRate: HistoryRespiratoryRateCandidate.from(packetK: packetK, body: bodyData),
      r21Motion: R21MotionCandidate.from(packetK: packetK, body: bodyData),
      r17Flags: compact.r17Flags,
      r17SampleCount: compact.r17SampleCount,
      r17ParsedSampleCount: compact.r17ParsedSampleCount,
      r17Min: compact.r17Min,
      r17Max: compact.r17Max,
      r17ChannelsOrGain: compact.r17ChannelsOrGain,
      rawDiagnostic: RawBodyDiagnostic.from(packetK: packetK, body: bodyData)
    )
  }

  private static func intValue(_ value: Any?) -> Int? {
    if let int = value as? Int {
      return int
    }
    if let number = value as? NSNumber {
      return number.intValue
    }
    if let string = value as? String {
      return Int(string)
    }
    return nil
  }
}
