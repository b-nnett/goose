import Foundation

enum GooseRustBridgeError: Error {
  case encodingFailed
  case nullResponse
  case malformedResponse
  case methodFailed(String)
}

struct GooseRustBridgeTiming {
  let method: String
  let methodElapsedMicroseconds: Int
  let requestEncodeMicroseconds: Int
  let ffiRoundTripMicroseconds: Int
  let responseDecodeMicroseconds: Int

  var boundaryMicroseconds: Int {
    requestEncodeMicroseconds + ffiRoundTripMicroseconds + responseDecodeMicroseconds
  }
}

final class GooseRustBridge: @unchecked Sendable {
  private var counter = 0
  private(set) var lastTiming: GooseRustBridgeTiming?

  func request(method: String, args: [String: Any] = [:]) throws -> [String: Any] {
    try requestValue(method: method, args: args) as? [String: Any] ?? [:]
  }

  func requestValue(method: String, args: [String: Any] = [:]) throws -> Any {
    lastTiming = nil
    counter += 1
    let payload: [String: Any] = [
      "schema": "goose.bridge.request.v1",
      "request_id": "goose-swift-\(Date().timeIntervalSince1970)-\(counter)",
      "method": method,
      "args": args,
    ]
    let encodeStarted = DispatchTime.now()
    let data = try JSONSerialization.data(withJSONObject: payload)
    guard let request = String(data: data, encoding: .utf8) else {
      throw GooseRustBridgeError.encodingFailed
    }
    let requestEncodeMicroseconds = Self.elapsedMicroseconds(since: encodeStarted)

    var responsePointer: UnsafeMutablePointer<CChar>?
    let ffiStarted = DispatchTime.now()
    request.withCString { pointer in
      responsePointer = goose_bridge_handle_json(pointer)
    }
    let ffiRoundTripMicroseconds = Self.elapsedMicroseconds(since: ffiStarted)
    guard let responsePointer else {
      throw GooseRustBridgeError.nullResponse
    }
    defer {
      goose_bridge_free_string(responsePointer)
    }

    let responseDecodeStarted = DispatchTime.now()
    let responseText = String(cString: responsePointer)
    let responseData = Data(responseText.utf8)
    guard
      let response = try JSONSerialization.jsonObject(with: responseData) as? [String: Any],
      let ok = response["ok"] as? Bool
    else {
      throw GooseRustBridgeError.malformedResponse
    }
    let responseDecodeMicroseconds = Self.elapsedMicroseconds(since: responseDecodeStarted)
    lastTiming = Self.timing(
      from: response,
      requestEncodeMicroseconds: requestEncodeMicroseconds,
      ffiRoundTripMicroseconds: ffiRoundTripMicroseconds,
      responseDecodeMicroseconds: responseDecodeMicroseconds
    )
    if !ok {
      let error = response["error"] as? [String: Any]
      let message = error?["message"] as? String ?? "Rust bridge method failed"
      throw GooseRustBridgeError.methodFailed(message)
    }
    return response["result"] ?? [:]
  }

  private static func timing(
    from response: [String: Any],
    requestEncodeMicroseconds: Int,
    ffiRoundTripMicroseconds: Int,
    responseDecodeMicroseconds: Int
  ) -> GooseRustBridgeTiming? {
    guard let timing = response["timing"] as? [String: Any],
          let method = timing["method"] as? String else {
      return nil
    }
    if let elapsed = timing["method_elapsed_us"] as? Int {
      return GooseRustBridgeTiming(
        method: method,
        methodElapsedMicroseconds: elapsed,
        requestEncodeMicroseconds: requestEncodeMicroseconds,
        ffiRoundTripMicroseconds: ffiRoundTripMicroseconds,
        responseDecodeMicroseconds: responseDecodeMicroseconds
      )
    }
    if let elapsed = timing["method_elapsed_us"] as? Double {
      return GooseRustBridgeTiming(
        method: method,
        methodElapsedMicroseconds: Int(elapsed),
        requestEncodeMicroseconds: requestEncodeMicroseconds,
        ffiRoundTripMicroseconds: ffiRoundTripMicroseconds,
        responseDecodeMicroseconds: responseDecodeMicroseconds
      )
    }
    return nil
  }

  private static func elapsedMicroseconds(since started: DispatchTime) -> Int {
    let elapsedNanoseconds = DispatchTime.now().uptimeNanoseconds - started.uptimeNanoseconds
    let elapsedMicroseconds = elapsedNanoseconds / 1_000
    return Int(min(elapsedMicroseconds, UInt64(Int.max)))
  }
}
