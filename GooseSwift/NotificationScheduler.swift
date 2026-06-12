import Foundation
@preconcurrency import UserNotifications


actor NotificationScheduler {
  static let shared = NotificationScheduler()
  private init() {}

  // Generic internal dispatch — guards authorization before adding request
  private func schedule(title: String, body: String, identifier: String) {
    let center = UNUserNotificationCenter.current()
    center.getNotificationSettings { settings in
      guard settings.authorizationStatus == .authorized else { return }
      let content = UNMutableNotificationContent()
      content.title = title
      content.body = body
      content.sound = .default
      let trigger = UNTimeIntervalNotificationTrigger(timeInterval: 1, repeats: false)
      let request = UNNotificationRequest(identifier: identifier, content: content, trigger: trigger)
      center.add(request)
    }
  }

  // FEAT-03 site 1: sleep sync completion
  func scheduleSleepProcessed(durationMinutes: Int, hrvMS: Double?, recoveryPercent: Double?) {
    let hours = durationMinutes / 60
    let minutes = durationMinutes % 60
    let dur = hours > 0 ? "\(hours)h \(minutes)m" : "\(minutes)m"
    var parts: [String] = [dur]
    if let hrv = hrvMS { parts.append(String(format: "HRV %dms", Int(hrv))) }
    if let rec = recoveryPercent { parts.append(String(format: "Recovery %d%%", Int(rec))) }
    schedule(
      title: "Sleep synced",
      body: parts.joined(separator: " · "),
      identifier: "goose.sleep.processed.\(Int(Date().timeIntervalSince1970))"
    )
  }

  // FEAT-03 site 2: passive workout detection
  func scheduleWorkoutDetected(activity: String, durationSeconds: Double, strain: Double?) {
    let minutes = Int(durationSeconds / 60)
    var body = "\(activity) · \(minutes)m"
    if let s = strain { body += String(format: " · Strain %.1f", s) }
    schedule(
      title: "Workout detected",
      body: body,
      identifier: "goose.workout.detected.\(Int(Date().timeIntervalSince1970))"
    )
  }

  // FEAT-03 site 3: battery low (static identifier — Bool gate prevents duplicates per session)
  func scheduleBatteryLow(percent: Int) {
    schedule(
      title: "WHOOP battery low",
      body: String(format: "Battery at %d%%. Charge your device.", percent),
      identifier: "goose.battery.low"
    )
  }
}
