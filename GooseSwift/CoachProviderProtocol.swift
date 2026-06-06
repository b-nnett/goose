import Foundation

protocol CoachProvider: AnyObject {
  var id: String { get }
  var displayName: String { get }
  var isAuthenticated: Bool { get }
  var availablePresets: [CoachModelPreset] { get }
  func send(
    messages: [CoachChatMessage],
    systemPrompt: String,
    preset: CoachModelPreset
  ) async throws -> AsyncStream<String>
  func signOut()
}

@MainActor @Observable
final class CoachProviderRegistry {
  private static let activeProviderDefaultsKey = "goose.coach.activeProviderId"

  private(set) var allProviders: [any CoachProvider]
  private(set) var activeProvider: (any CoachProvider)?

  init() {
    let chatGPT = ChatGPTCoachProvider()
    let claude = ClaudeCoachProvider()
    let custom = CustomEndpointCoachProvider()
    let gemini = GeminiCoachProvider()
    allProviders = [chatGPT, claude, custom, gemini]

    let storedID = UserDefaults.standard.string(forKey: Self.activeProviderDefaultsKey)
    if let storedID, let match = allProviders.first(where: { $0.id == storedID }) {
      activeProvider = match
    } else {
      activeProvider = allProviders.first(where: { $0.isAuthenticated }) ?? allProviders.first
    }
  }

  func selectProvider(id: String) {
    guard let match = allProviders.first(where: { $0.id == id }) else {
      return
    }
    activeProvider = match
    UserDefaults.standard.set(match.id, forKey: Self.activeProviderDefaultsKey)
  }
}
