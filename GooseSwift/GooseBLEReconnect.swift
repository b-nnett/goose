import CoreBluetooth
import Foundation


struct ReconnectBackoff {
  var attemptCount: Int = 0
  let baseDelay: TimeInterval = 1.0
  let maxDelay: TimeInterval = 60.0
  let maxAttempts: Int = 10

  // Returns the delay before the next attempt, or nil if maxAttempts exhausted.
  // The first call returns 1s (baseDelay * 2^0) and sets attemptCount to 1.
  mutating func nextDelay() -> TimeInterval? {
    guard attemptCount < maxAttempts else { return nil }
    let delay = min(baseDelay * pow(2.0, Double(attemptCount)), maxDelay)
    attemptCount += 1
    return delay
  }

  mutating func reset() {
    attemptCount = 0
  }

  var statusString: String {
    "reconnecting (attempt \(attemptCount)/\(maxAttempts))"
  }
}
