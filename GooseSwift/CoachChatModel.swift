import Foundation

@MainActor @Observable
final class CoachChatModel {
  private static let modelPresetDefaultsKey = "goose.coach.modelPreset"
  private static let seedPromptText = "What should we look at today?"

  private(set) var messages: [CoachChatMessage] = []
  private(set) var streamState: CoachStreamState = .idle
  private(set) var errorMessage: String?
  private(set) var activePreset: CoachModelPreset

  private let registry: CoachProviderRegistry
  private var sendTask: Task<Void, Never>?

  // Passthrough auth state — delegates to the ChatGPT provider when it is active
  var isSignedIn: Bool { registry.activeProvider?.isAuthenticated ?? false }

  var loginStatus: String {
    (registry.activeProvider as? ChatGPTCoachProvider)?.loginStatus ?? "Not signed in"
  }

  var deviceCode: CodexLoginDeviceCode? {
    (registry.activeProvider as? ChatGPTCoachProvider)?.deviceCode
  }

  init(registry: CoachProviderRegistry) {
    self.registry = registry
    let storedRawValue = UserDefaults.standard.string(forKey: Self.modelPresetDefaultsKey)
    activePreset = storedRawValue.flatMap(CoachModelPreset.init(rawValue:)) ?? .defaultValue
    messages = Self.normalizedPersistedMessages(CoachConversationStore.load())
    if !messages.isEmpty {
      persistConversation()
    }
  }

  nonisolated func cancel() {
    // Called from external cleanup contexts
  }

  func refreshAuth() {
    guard let chatGPT = registry.activeProvider as? ChatGPTCoachProvider else { return }
    Task { [chatGPT] in
      await chatGPT.refreshAuth()
      if chatGPT.isAuthenticated {
        seedAssistantPromptIfNeeded()
      }
    }
  }

  func selectModelPreset(_ preset: CoachModelPreset) {
    activePreset = preset
    UserDefaults.standard.set(preset.rawValue, forKey: Self.modelPresetDefaultsKey)
  }

  func startNewConversation() {
    sendTask?.cancel()
    sendTask = nil
    streamState = .idle
    errorMessage = nil
    messages.removeAll()
    CoachConversationStore.clear()
    seedAssistantPromptIfNeeded()
  }

  func startOAuthSignIn() {
    guard let chatGPT = registry.activeProvider as? ChatGPTCoachProvider else { return }
    Task { [chatGPT, weak self] in
      do {
        try await chatGPT.startOAuthSignIn()
        self?.seedAssistantPromptIfNeeded()
      } catch is CancellationError {
        // cancelled — no-op
      } catch {
        self?.errorMessage = error.localizedDescription
      }
    }
  }

  func signOut() {
    sendTask?.cancel()
    sendTask = nil
    registry.activeProvider?.signOut()
    streamState = .idle
    messages.removeAll()
    CoachConversationStore.clear()
  }

  func cancelStreaming() {
    sendTask?.cancel()
    sendTask = nil
    streamState = .idle
    cancelStreamingMessages()
  }

  func send(
    _ prompt: String,
    healthStore: HealthDataStore,
    appModel: GooseAppModel
  ) {
    let trimmedPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmedPrompt.isEmpty, !streamState.isStreaming else { return }
    guard let provider = registry.activeProvider, provider.isAuthenticated else {
      errorMessage = "Sign in first."
      return
    }

    // Bind tool context for ChatGPT provider (Pitfall 4 — tool calls stay internal)
    if let chatGPT = provider as? ChatGPTCoachProvider {
      chatGPT.toolContextProvider = { [weak healthStore, weak appModel] in
        guard let healthStore, let appModel else { return [:] }
        return CoachLocalToolContext.build(healthStore: healthStore, appModel: appModel)
      }
    }

    let assistantID = UUID()
    messages.append(CoachChatMessage(role: .user, text: trimmedPrompt))
    messages.append(CoachChatMessage(id: assistantID, role: .assistant, text: "", isStreaming: true))
    streamState = .streaming
    errorMessage = nil
    persistConversation()

    let systemPrompt = buildSystemPrompt(healthStore: healthStore, appModel: appModel)
    let currentMessages = messages.filter { !($0.id == assistantID) }
    let preset = activePreset

    sendTask?.cancel()
    sendTask = Task { [weak self] in
      guard let self else { return }
      do {
        let stream = try await provider.send(
          messages: currentMessages,
          systemPrompt: systemPrompt,
          preset: preset
        )
        for await delta in stream {
          try Task.checkCancellation()
          appendAssistantText(delta, to: assistantID)
        }
        finishAssistantMessage(assistantID)
        streamState = .idle
      } catch is CancellationError {
        markAssistantMessageCancelled(assistantID)
        streamState = .idle
      } catch where isCancelledError(error) {
        markAssistantMessageCancelled(assistantID)
        streamState = .idle
      } catch {
        let message = describe(error)
        appendAssistantText("\n\(message)", to: assistantID)
        finishAssistantMessage(assistantID)
        errorMessage = message
        streamState = .failed(message)
      }
    }
  }

  private func buildSystemPrompt(healthStore: HealthDataStore, appModel: GooseAppModel) -> String {
    let context = CoachLocalToolContext.build(healthStore: healthStore, appModel: appModel)
    guard JSONSerialization.isValidJSONObject(context),
          let data = try? JSONSerialization.data(withJSONObject: context, options: [.sortedKeys]),
          let json = String(data: data, encoding: .utf8) else {
      return ""
    }
    return """
    You are Goose Coach inside a user-owned WHOOP companion app. Use the local health context below before making claims. Cite data sources inline. Keep coaching practical and say when data is missing or stale. Do not diagnose, prescribe, or infer medical conditions.

    Local health context (JSON):
    \(json)
    """
  }

  private func appendAssistantText(_ delta: String, to id: UUID) {
    guard let index = messages.firstIndex(where: { $0.id == id }) else { return }
    messages[index].text += delta
  }

  private func isAssistantTextEmpty(_ id: UUID) -> Bool {
    guard let message = messages.first(where: { $0.id == id }) else { return true }
    return message.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
  }

  private func finishAssistantMessage(_ id: UUID) {
    guard let index = messages.firstIndex(where: { $0.id == id }) else { return }
    messages[index].isStreaming = false
    if messages[index].text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
       messages[index].toolEvents.isEmpty,
       !messages[index].isCancelled {
      messages.remove(at: index)
    }
    persistConversation()
  }

  private func markAssistantMessageCancelled(_ id: UUID) {
    guard let index = messages.firstIndex(where: { $0.id == id }) else { return }
    messages[index].isStreaming = false
    messages[index].isCancelled = true
    markUnfinishedToolEventsStopped(in: index)
    persistConversation()
  }

  private func cancelStreamingMessages() {
    for index in messages.indices {
      guard messages[index].isStreaming else { continue }
      messages[index].isStreaming = false
      if messages[index].role == .assistant {
        messages[index].isCancelled = true
        markUnfinishedToolEventsStopped(in: index)
      }
    }
    persistConversation()
  }

  private func markUnfinishedToolEventsStopped(in messageIndex: Int) {
    for eventIndex in messages[messageIndex].toolEvents.indices {
      if messages[messageIndex].toolEvents[eventIndex].status != "Returned" {
        messages[messageIndex].toolEvents[eventIndex].status = "Stopped"
      }
    }
  }

  private func seedAssistantPromptIfNeeded() {
    guard messages.isEmpty else { return }
    messages.append(
      CoachChatMessage(
        role: .assistant,
        text: Self.seedPromptText
      )
    )
    persistConversation()
  }

  private func persistConversation() {
    CoachConversationStore.save(messages)
  }

  private func describe(_ error: Error) -> String {
    if isCancelledError(error) {
      return "Generation stopped."
    }
    if let localizedError = error as? LocalizedError, let description = localizedError.errorDescription {
      return description
    }
    return String(describing: error)
  }

  private func isCancelledError(_ error: Error) -> Bool {
    if let urlError = error as? URLError {
      return urlError.code == .cancelled
    }
    let nsError = error as NSError
    return nsError.domain == NSURLErrorDomain && nsError.code == NSURLErrorCancelled
  }

  private static func normalizedPersistedMessages(_ storedMessages: [CoachChatMessage]) -> [CoachChatMessage] {
    storedMessages.map { message in
      var normalized = message
      if normalized.isStreaming {
        normalized.isStreaming = false
        normalized.isCancelled = true
      }
      if normalized.isCancelled {
        for index in normalized.toolEvents.indices where normalized.toolEvents[index].status != "Returned" {
          normalized.toolEvents[index].status = "Stopped"
        }
      }
      return normalized
    }
  }
}
