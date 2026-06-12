import SwiftUI

enum MoreRoute: String, CaseIterable, Identifiable, Hashable {
  case profile
  case device
  case hrMonitor
  case connectionLab
  case capture
  case localStore
  case healthSync
  case rawExport
  case algorithms
  case debug
  case privacy
  case remoteServer
  case support
  case about
  case developer
  case breathe
  case intervalTimer
  case metricExplorer

  var id: String { rawValue }

  var title: String {
    switch self {
    case .profile: String(localized: "Profile")
    case .device: String(localized: "Device")
    case .hrMonitor: String(localized: "HR Monitor")
    case .connectionLab: String(localized: "Connection Lab")
    case .capture: String(localized: "Capture")
    case .localStore: String(localized: "Local Store")
    case .healthSync: String(localized: "Apple Health Profile")
    case .rawExport: String(localized: "Raw Export")
    case .algorithms: String(localized: "Algorithms")
    case .debug: String(localized: "Debug")
    case .privacy: String(localized: "Privacy")
    case .remoteServer: String(localized: "Remote Server")
    case .support: String(localized: "Support")
    case .about: String(localized: "About")
    case .developer: String(localized: "Developer")
    case .breathe: String(localized: "Breathe")
    case .intervalTimer: String(localized: "Interval Timer")
    case .metricExplorer: String(localized: "Metric Explorer")
    }
  }

  var subtitle: String {
    switch self {
    case .profile: String(localized: "Name, birthday, height, weight, and profile basics")
    case .device: String(localized: "WHOOP band, connection, battery, and pairing")
    case .hrMonitor: String(localized: "Connect and view live heart rate from a Bluetooth HR monitor")
    case .connectionLab: String(localized: "Low-level Bluetooth, hello, and event diagnostics")
    case .capture: String(localized: "Notification capture, imports, and command evidence")
    case .localStore: String(localized: "SQLite path, schema, and storage health")
    case .healthSync: String(localized: "Profile weight autofill only")
    case .rawExport: String(localized: "Bundle windows, data scopes, validation, and lint")
    case .algorithms: String(localized: "Operational algorithm preferences")
    case .debug: String(localized: "Rust, parser, command groups, and gated controls")
    case .privacy: String(localized: "Local data, export, lint, and deletion state")
    case .remoteServer: String(localized: "Server URL, API key, and upload toggle")
    case .support: String(localized: "Logs, support bundles, and troubleshooting")
    case .about: String(localized: "App, Rust core, and licenses")
    case .developer: String(localized: "Capture, exports, bridge diagnostics, and debug tools")
    case .breathe: String(localized: "Paced breathing with haptics")
    case .intervalTimer: String(localized: "Work/rest intervals with haptic transitions")
    case .metricExplorer: String(localized: "Current metric values from your WHOOP data")
    }
  }

  var systemImage: String {
    switch self {
    case .profile: "person.crop.circle"
    case .device: "sensor.tag.radiowaves.forward"
    case .hrMonitor: "heart.circle"
    case .connectionLab: "antenna.radiowaves.left.and.right"
    case .capture: "record.circle"
    case .localStore: "externaldrive"
    case .healthSync: "heart.text.square"
    case .rawExport: "square.and.arrow.up"
    case .algorithms: "function"
    case .debug: "terminal"
    case .privacy: "hand.raised"
    case .remoteServer: "network"
    case .support: "lifepreserver"
    case .about: "info.circle"
    case .developer: "hammer"
    case .breathe: "wind"
    case .intervalTimer: "timer"
    case .metricExplorer: "chart.bar.doc.horizontal"
    }
  }

  var statusKeyPath: KeyPath<MoreRouteStatus, MoreStatusKind> {
    switch self {
    case .profile: \.profile
    case .device: \.device
    case .hrMonitor: \.hrMonitor
    case .connectionLab: \.connectionLab
    case .capture: \.capture
    case .localStore: \.localStore
    case .healthSync: \.healthSync
    case .rawExport: \.rawExport
    case .algorithms: \.algorithms
    case .debug: \.debug
    case .privacy: \.privacy
    case .remoteServer: \.remoteServer
    case .support: \.support
    case .about: \.about
    case .developer: \.developer
    case .breathe: \.breathe
    case .intervalTimer: \.intervalTimer
    case .metricExplorer: \.metricExplorer
    }
  }

  static let deviceRoutes: [MoreRoute] = [.device, .hrMonitor]
  static let appRoutes: [MoreRoute] = [.healthSync]
  static let settingsRoutes: [MoreRoute] = [.privacy, .remoteServer]
  static let supportRoutes: [MoreRoute] = [.support, .about]
  static let developerRoutes: [MoreRoute] = [.developer]
  static let developerToolRoutes: [MoreRoute] = [.connectionLab, .capture, .localStore, .rawExport, .algorithms, .debug]
  static let wellnessRoutes: [MoreRoute] = [.breathe, .intervalTimer]
  static let dataRoutes: [MoreRoute] = [.metricExplorer]
}

struct MoreRouteStatus: Equatable {
  var profile: MoreStatusKind
  var device: MoreStatusKind
  var hrMonitor: MoreStatusKind
  var connectionLab: MoreStatusKind
  var capture: MoreStatusKind
  var localStore: MoreStatusKind
  var healthSync: MoreStatusKind
  var rawExport: MoreStatusKind
  var algorithms: MoreStatusKind
  var debug: MoreStatusKind
  var privacy: MoreStatusKind
  var remoteServer: MoreStatusKind
  var support: MoreStatusKind
  var about: MoreStatusKind
  var developer: MoreStatusKind
  var breathe: MoreStatusKind
  var intervalTimer: MoreStatusKind
  var metricExplorer: MoreStatusKind
}

enum MoreStatusKind: String, CaseIterable {
  case ready
  case pending
  case blocked
  case unavailable
  case stale

  var title: String {
    switch self {
    case .ready:       String(localized: "Pronto")
    case .pending:     String(localized: "Pendente")
    case .blocked:     String(localized: "Bloqueado")
    case .unavailable: String(localized: "Indisponível")
    case .stale:       String(localized: "Desatualizado")
    }
  }

  var tint: Color {
    switch self {
    case .ready: .green
    case .pending: .blue
    case .blocked: .orange
    case .unavailable: .gray
    case .stale: .yellow
    }
  }

  var systemImage: String {
    switch self {
    case .ready: "checkmark.circle.fill"
    case .pending: "clock.fill"
    case .blocked: "exclamationmark.triangle.fill"
    case .unavailable: "minus.circle.fill"
    case .stale: "arrow.clockwise.circle.fill"
    }
  }
}

