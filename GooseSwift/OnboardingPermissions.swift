import CoreBluetooth
import CoreLocation
import HealthKit
import SwiftUI
import UIKit
import UserNotifications

enum HealthKitPermissionRequester {
  static func requestAccess() async -> HealthKitProfileImportResult {
    guard HKHealthStore.isHealthDataAvailable() else {
      return HealthKitProfileImportResult(status: "Unavailable on this device", autofill: .empty)
    }

    return await HealthKitProfileImporter.requestProfileAccess()
  }
}

struct LocationPermissionResult {
  let status: String
  let isResolved: Bool
}

@MainActor
final class OnboardingLocationPermissionRequester: NSObject, ObservableObject, CLLocationManagerDelegate {
  private let manager = CLLocationManager()
  private var continuation: CheckedContinuation<LocationPermissionResult, Never>?
  private var fallbackTask: Task<Void, Never>?

  override init() {
    super.init()
    manager.delegate = self
  }

  func requestAccess() async -> LocationPermissionResult {
    guard continuation == nil else {
      return LocationPermissionResult(status: "Request already in progress", isResolved: false)
    }

    let status = manager.authorizationStatus
    switch status {
    case .notDetermined, .authorizedWhenInUse:
      return await withCheckedContinuation { continuation in
        self.continuation = continuation
        if status == .authorizedWhenInUse {
          scheduleFallbackResume()
        }
        manager.requestAlwaysAuthorization()
      }
    case .authorizedAlways, .denied, .restricted:
      return Self.result(for: status)
    @unknown default:
      return LocationPermissionResult(status: "Location unavailable", isResolved: true)
    }
  }

  nonisolated func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
    Task { @MainActor in
      guard continuation != nil else {
        return
      }
      let status = manager.authorizationStatus
      guard status != .notDetermined else {
        return
      }
      finish(with: status)
    }
  }

  private func scheduleFallbackResume() {
    fallbackTask?.cancel()
    fallbackTask = Task { @MainActor [weak self] in
      try? await Task.sleep(nanoseconds: 2_000_000_000)
      guard let self, self.continuation != nil else {
        return
      }
      self.finish(with: self.manager.authorizationStatus)
    }
  }

  private func finish(with status: CLAuthorizationStatus) {
    fallbackTask?.cancel()
    fallbackTask = nil
    let result = Self.result(for: status)
    let continuation = continuation
    self.continuation = nil
    continuation?.resume(returning: result)
  }

  private static func result(for status: CLAuthorizationStatus) -> LocationPermissionResult {
    switch status {
    case .authorizedAlways:
      return LocationPermissionResult(status: "Allowed Always", isResolved: true)
    case .authorizedWhenInUse:
      return LocationPermissionResult(status: "Allowed While Using", isResolved: true)
    case .denied:
      return LocationPermissionResult(status: "Not allowed", isResolved: true)
    case .restricted:
      return LocationPermissionResult(status: "Restricted", isResolved: true)
    case .notDetermined:
      return LocationPermissionResult(status: "Not requested", isResolved: false)
    @unknown default:
      return LocationPermissionResult(status: "Location unavailable", isResolved: true)
    }
  }
}
