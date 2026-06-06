import Foundation

enum CoachStreamState: Equatable {
  case idle
  case streaming
  case failed(String)

  var isStreaming: Bool {
    if case .streaming = self {
      return true
    }
    return false
  }
}

struct CoachToolEvent: Identifiable, Equatable, Codable {
  let id: String
  var name: String
  var status: String
  var arguments: String
  var resultSummary: String?
}

struct CoachChatMessage: Identifiable, Equatable, Codable {
  enum Role: Equatable, Codable {
    case user
    case assistant
  }

  let id: UUID
  let role: Role
  var text: String
  var toolEvents: [CoachToolEvent]
  var isStreaming: Bool
  var isCancelled: Bool
  let createdAt: Date

  init(
    id: UUID = UUID(),
    role: Role,
    text: String,
    toolEvents: [CoachToolEvent] = [],
    isStreaming: Bool = false,
    isCancelled: Bool = false,
    createdAt: Date = Date()
  ) {
    self.id = id
    self.role = role
    self.text = text
    self.toolEvents = toolEvents
    self.isStreaming = isStreaming
    self.isCancelled = isCancelled
    self.createdAt = createdAt
  }
}

enum CoachModelPreset: String, CaseIterable, Identifiable {
  case gpt55Low
  case gpt55Medium
  case gpt55High
  // Wave 2: Claude presets
  case claudeOpus48
  case claudeSonnet46
  case claudeHaiku45
  // Wave 4: Gemini presets
  case gemini25Pro
  case gemini25Flash

  var id: String { rawValue }

  static let defaultValue: CoachModelPreset = .gpt55Medium

  var title: String {
    switch self {
    case .gpt55Low:
      return String(localized: "GPT-5.5 Low")
    case .gpt55Medium:
      return String(localized: "GPT-5.5 Medium")
    case .gpt55High:
      return String(localized: "GPT-5.5 High")
    case .claudeOpus48:
      return String(localized: "Claude Opus 4.8")
    case .claudeSonnet46:
      return String(localized: "Claude Sonnet 4.6")
    case .claudeHaiku45:
      return String(localized: "Claude Haiku 4.5")
    case .gemini25Pro:
      return String(localized: "Gemini 2.5 Pro")
    case .gemini25Flash:
      return String(localized: "Gemini 2.5 Flash")
    }
  }

  var modelID: String {
    "gpt-5.5"
  }

  var effort: String {
    switch self {
    case .gpt55Low:
      return "low"
    case .gpt55Medium:
      return "medium"
    case .gpt55High:
      return "high"
    default:
      return "medium"
    }
  }

  var claudeModelID: String? {
    switch self {
    case .claudeOpus48: return "claude-opus-4-8"
    case .claudeSonnet46: return "claude-sonnet-4-6"
    case .claudeHaiku45: return "claude-haiku-4-5-20251001"
    default: return nil
    }
  }

  var geminiModelID: String? {
    switch self {
    case .gemini25Pro: return "gemini-2.5-pro"
    case .gemini25Flash: return "gemini-2.5-flash"
    default: return nil
    }
  }
}

enum CoachConversationStore {
  private static let defaultsKey = "goose.coach.conversation.v1"
  private static let maxPersistedMessages = 80

  static func load() -> [CoachChatMessage] {
    guard let data = UserDefaults.standard.data(forKey: defaultsKey) else {
      return []
    }
    let decoder = JSONDecoder()
    decoder.dateDecodingStrategy = .iso8601
    return (try? decoder.decode([CoachChatMessage].self, from: data)) ?? []
  }

  static func save(_ messages: [CoachChatMessage]) {
    let encoder = JSONEncoder()
    encoder.dateEncodingStrategy = .iso8601
    let persisted = Array(messages.suffix(maxPersistedMessages))
    guard let data = try? encoder.encode(persisted) else {
      return
    }
    UserDefaults.standard.set(data, forKey: defaultsKey)
  }

  static func clear() {
    UserDefaults.standard.removeObject(forKey: defaultsKey)
  }
}
