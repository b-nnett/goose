import CoreBluetooth
import CoreLocation
import Foundation
import UserNotifications

enum OnboardingStep: Int, CaseIterable {
  case healthKit
  case location
  case bluetooth
  case notifications
  case connect
  case profile

  var title: String {
    switch self {
    case .healthKit:
      return "Import Weight"
    case .location:
      return "Enable Location"
    case .bluetooth:
      return "Enable Bluetooth"
    case .notifications:
      return "Enable Notifications"
    case .connect:
      return "Connect your WHOOP (4.0 or 5.0)"
    case .profile:
      return "Personal details"
    }
  }

  var progress: Double {
    Double(rawValue + 1) / Double(Self.allCases.count)
  }

  var stepLabel: String {
    "Step \(rawValue + 1) of \(Self.allCases.count)"
  }

  var next: OnboardingStep? {
    Self(rawValue: rawValue + 1)
  }

  var previous: OnboardingStep? {
    Self(rawValue: rawValue - 1)
  }
}

enum OnboardingInputField: Hashable {
  case firstName
  case heightCentimeters
  case heightFeet
  case heightInches
  case weight
}

enum OnboardingUnitSystem: String, CaseIterable, Identifiable {
  case imperial
  case metric

  var id: String { rawValue }

  var title: String {
    switch self {
    case .imperial:
      return "Imperial"
    case .metric:
      return "Metric"
    }
  }
}

enum OnboardingGender: String, CaseIterable, Identifiable {
  case female
  case male
  case nonBinary = "non_binary"
  case preferNotToSay = "prefer_not_to_say"

  var id: String { rawValue }

  var title: String {
    switch self {
    case .female:
      return "Female"
    case .male:
      return "Male"
    case .nonBinary:
      return "Non-binary"
    case .preferNotToSay:
      return "Prefer not to say"
    }
  }
}

enum OnboardingPermissionState {
  static func locationResolved() -> Bool {
    let status = CLLocationManager().authorizationStatus
    switch status {
    case .notDetermined:
      return false
    case .authorizedAlways, .authorizedWhenInUse, .denied, .restricted:
      return true
    @unknown default:
      return true
    }
  }

  static func bluetoothResolved() -> Bool {
    switch CBManager.authorization {
    case .notDetermined:
      return false
    case .allowedAlways, .denied, .restricted:
      return true
    @unknown default:
      return false
    }
  }

  static func notificationResolved() async -> Bool {
    await withCheckedContinuation { continuation in
      UNUserNotificationCenter.current().getNotificationSettings { settings in
        continuation.resume(returning: settings.authorizationStatus != .notDetermined)
      }
    }
  }
}

enum OnboardingDate {
  static func parse(_ value: String) -> Date? {
    let formatter = dateFormatter
    guard let date = formatter.date(from: value) else {
      return nil
    }
    return Calendar.current.startOfDay(for: date)
  }

  static func dateOnlyString(_ date: Date) -> String {
    dateFormatter.string(from: date)
  }

  static func defaultDateOfBirth() -> Date {
    clamp(Calendar.current.date(byAdding: .year, value: -30, to: Date()) ?? Date())
  }

  static func minimumDateOfBirth() -> Date {
    Calendar.current.date(byAdding: .year, value: -120, to: Date()) ?? Date.distantPast
  }

  static func maximumDateOfBirth() -> Date {
    Calendar.current.date(byAdding: .year, value: -13, to: Date()) ?? Date()
  }

  static func clamp(_ date: Date) -> Date {
    let normalized = Calendar.current.startOfDay(for: date)
    let minimum = Calendar.current.startOfDay(for: minimumDateOfBirth())
    let maximum = Calendar.current.startOfDay(for: maximumDateOfBirth())
    if normalized < minimum {
      return minimum
    }
    if normalized > maximum {
      return maximum
    }
    return normalized
  }

  private static var dateFormatter: DateFormatter {
    let formatter = DateFormatter()
    formatter.calendar = Calendar(identifier: .gregorian)
    formatter.locale = Locale(identifier: "en_US_POSIX")
    formatter.dateFormat = "yyyy-MM-dd"
    return formatter
  }
}
