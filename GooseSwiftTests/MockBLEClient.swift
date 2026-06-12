import Foundation
@testable import GooseSwift

/// Minimal test double for GooseBLEManaging.
final class MockBLEClient: GooseBLEManaging {
  var connectionState: String = "disconnected"
  var isScanning: Bool = false
  var didCallStartScanning = false
  var didCallStopScanning = false

  func startScanning() { didCallStartScanning = true }
  func stopScanning() { didCallStopScanning = true }
}
