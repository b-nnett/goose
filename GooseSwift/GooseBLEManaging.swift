import Foundation

/// Protocol abstracting GooseBLEClient for dependency injection in tests.
/// Minimal surface — extend as test coverage grows.
protocol GooseBLEManaging: AnyObject {
  var connectionState: String { get }
  var isScanning: Bool { get }
  func startScanning()
  func stopScanning()
}

// TODO(future): extension GooseBLEClient: GooseBLEManaging {}
