import Foundation

final class ChatGPTCoachProvider: CoachProvider {
  let id = "chatgpt"
  let displayName = "ChatGPT"
  let availablePresets: [CoachModelPreset] = [.gpt55Low, .gpt55Medium, .gpt55High]

  private(set) var isAuthenticated = false
  private(set) var deviceCode: CodexLoginDeviceCode?
  private(set) var loginStatus = "Not signed in"

  private let authClient = CodexSelfContainedAuthClient()
  private let client = OpenAIResponsesClient()
  private var auth: CodexStoredChatGPTAuth?

  // Tool context closure — bound by CoachChatModel before send() is called (Task 3b hook)
  var toolContextProvider: (() -> [String: Any])?

  init() {}

  func refreshAuth() async {
    do {
      if let storedAuth = try await authClient.storedAuth(refreshIfNeeded: true) {
        auth = storedAuth
        isAuthenticated = true
        deviceCode = nil
        loginStatus = "Signed in"
      } else {
        auth = nil
        isAuthenticated = false
        deviceCode = nil
        loginStatus = "Not signed in"
      }
    } catch {
      auth = nil
      isAuthenticated = false
      deviceCode = nil
      loginStatus = "Auth check failed"
    }
  }

  func startOAuthSignIn() async throws {
    loginStatus = "Requesting OAuth code"
    deviceCode = nil

    let code = try await authClient.requestDeviceCodeWithRetry()
    deviceCode = CodexLoginDeviceCode(
      verificationURL: code.verificationURL,
      userCode: code.userCode
    )
    loginStatus = "Waiting for approval"

    let storedAuth = try await authClient.completeDeviceCodeLogin(code)
    auth = storedAuth
    isAuthenticated = true
    deviceCode = nil
    loginStatus = "Signed in"
  }

  func signOut() {
    Task { [authClient] in
      try? await authClient.clearStoredAuth()
    }
    auth = nil
    deviceCode = nil
    isAuthenticated = false
    loginStatus = "Not signed in"
  }

  func send(
    messages: [CoachChatMessage],
    systemPrompt: String,
    preset: CoachModelPreset
  ) async throws -> AsyncStream<String> {
    guard let auth else {
      throw OpenAIResponsesError.missingOAuthSession
    }

    return AsyncStream { continuation in
      Task { [weak self] in
        guard let self else {
          continuation.finish()
          return
        }
        do {
          try await self.streamResponseLoop(
            messages: messages,
            systemPrompt: systemPrompt,
            auth: auth,
            preset: preset,
            continuation: continuation
          )
          continuation.finish()
        } catch is CancellationError {
          continuation.finish()
        } catch {
          continuation.finish()
        }
      }
    }
  }

  private func streamResponseLoop(
    messages: [CoachChatMessage],
    systemPrompt: String,
    auth: CodexStoredChatGPTAuth,
    preset: CoachModelPreset,
    continuation: AsyncStream<String>.Continuation
  ) async throws {
    let activeAuth = try await authClient.storedAuth(refreshIfNeeded: true) ?? auth
    self.auth = activeAuth

    let contextualPrompt = buildContextualPrompt(from: messages)
    var conversationInput = OpenAICoachRequestFactory.userInput(contextualPrompt)
    var input: Any = conversationInput
    var toolMode: OpenAICoachRequestFactory.ToolMode = .required
    let originalPrompt = messages.last(where: { $0.role == .user })?.text ?? ""

    for _ in 0..<2 {
      var completedToolCalls: [OpenAICoachToolCall] = []
      var inFlightToolCalls: [String: OpenAICoachToolCall] = [:]

      let requestBody = OpenAICoachRequestFactory.makeRequest(
        input: input,
        toolMode: toolMode,
        modelPreset: preset
      )

      try await client.stream(auth: activeAuth, body: requestBody) { [weak self] event in
        guard let self else { return }
        try self.handleEvent(
          event,
          continuation: continuation,
          inFlightToolCalls: &inFlightToolCalls,
          completedToolCalls: &completedToolCalls
        )
      }

      guard !completedToolCalls.isEmpty else {
        return
      }

      let toolItems = completedToolCalls.flatMap { call -> [[String: Any]] in
        let output = executeToolCall(call)
        return [
          [
            "type": "function_call",
            "id": call.id,
            "call_id": call.callID,
            "name": call.name,
            "arguments": call.arguments.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "{}" : call.arguments,
          ],
          [
            "type": "function_call_output",
            "call_id": call.callID,
            "output": output,
          ],
        ]
      }
      conversationInput.append(contentsOf: toolItems)
      conversationInput.append(OpenAICoachRequestFactory.finalAnswerInput(originalPrompt: originalPrompt))
      input = conversationInput
      toolMode = .none
    }
  }

  private func handleEvent(
    _ event: OpenAIResponseStreamEvent,
    continuation: AsyncStream<String>.Continuation,
    inFlightToolCalls: inout [String: OpenAICoachToolCall],
    completedToolCalls: inout [OpenAICoachToolCall]
  ) throws {
    switch event.type {
    case "response.output_text.delta":
      if let delta = event.payload["delta"] as? String {
        continuation.yield(delta)
      }
    case "response.output_item.added":
      guard let item = event.payload["item"] as? [String: Any],
            let call = toolCall(from: item, fallbackID: fallbackToolID(from: event.payload)) else {
        return
      }
      inFlightToolCalls[call.id] = call
    case "response.function_call_arguments.delta":
      let id = fallbackToolID(from: event.payload)
      guard let id, let delta = event.payload["delta"] as? String else { return }
      var call = inFlightToolCalls[id] ?? OpenAICoachToolCall(id: id, callID: id, name: "function", arguments: "")
      call.arguments += delta
      inFlightToolCalls[id] = call
    case "response.function_call_arguments.done":
      completeToolCall(from: event.payload, inFlightToolCalls: &inFlightToolCalls, completedToolCalls: &completedToolCalls)
    case "response.output_item.done":
      completeToolCall(from: event.payload, inFlightToolCalls: &inFlightToolCalls, completedToolCalls: &completedToolCalls)
    case "response.failed", "error":
      throw OpenAIResponsesError.api(errorMessageFrom(event.payload))
    default:
      break
    }
  }

  private func completeToolCall(
    from payload: [String: Any],
    inFlightToolCalls: inout [String: OpenAICoachToolCall],
    completedToolCalls: inout [OpenAICoachToolCall]
  ) {
    let fallbackID = fallbackToolID(from: payload)
    let finishedCall: OpenAICoachToolCall?
    if let item = payload["item"] as? [String: Any],
       let itemCall = toolCall(from: item, fallbackID: fallbackID) {
      finishedCall = itemCall
    } else if let fallbackID, var call = inFlightToolCalls[fallbackID] {
      if let arguments = payload["arguments"] as? String {
        call.arguments = arguments
      }
      finishedCall = call
    } else {
      finishedCall = nil
    }

    guard let finishedCall else { return }
    guard !completedToolCalls.contains(where: { $0.id == finishedCall.id || $0.callID == finishedCall.callID }) else {
      return
    }
    completedToolCalls.append(finishedCall)
    inFlightToolCalls[finishedCall.id] = finishedCall
  }

  private func executeToolCall(_ call: OpenAICoachToolCall) -> String {
    let context = toolContextProvider?() ?? [:]
    let tools = context["tools"] as? [String: Any] ?? [:]
    let output: Any

    switch call.name {
    case "load_stats", "get_activities", "get_capture_sessions":
      output = tools[call.name] ?? ["error": "tool_not_available", "tool": call.name]
    case "get_data_gaps":
      output = tools["get_data_gaps"] ?? ["error": "tool_not_available", "tool": call.name]
    default:
      output = ["error": "unknown_tool", "tool": call.name]
    }

    return jsonString(output)
  }

  private func buildContextualPrompt(from messages: [CoachChatMessage]) -> String {
    guard let lastUser = messages.last(where: { $0.role == .user }) else {
      return ""
    }
    let transcript = messages.compactMap { message -> String? in
      guard !message.isStreaming, !message.isCancelled else { return nil }
      let text = message.text.trimmingCharacters(in: .whitespacesAndNewlines)
      guard !text.isEmpty else { return nil }
      if message.role == .user, text == lastUser.text { return nil }
      return message.role == .user ? "User: \(text)" : "Coach: \(text)"
    }.suffix(12).joined(separator: "\n\n")

    guard !transcript.isEmpty else {
      return lastUser.text
    }
    return """
    Recent Coach conversation context:
    \(transcript)

    Current user message:
    \(lastUser.text)
    """
  }

  private func toolCall(from item: [String: Any], fallbackID: String?) -> OpenAICoachToolCall? {
    let itemID = item["id"] as? String ?? fallbackID
    let callID = item["call_id"] as? String ?? itemID
    let name = item["name"] as? String ?? (item["function"] as? [String: Any])?["name"] as? String
    let arguments = item["arguments"] as? String ?? (item["function"] as? [String: Any])?["arguments"] as? String ?? ""
    guard let itemID, let callID, let name else { return nil }
    return OpenAICoachToolCall(id: itemID, callID: callID, name: name, arguments: arguments)
  }

  private func fallbackToolID(from payload: [String: Any]) -> String? {
    payload["item_id"] as? String ??
      payload["call_id"] as? String ??
      (payload["output_index"] as? Int).map { "tool-\($0)" }
  }

  private func errorMessageFrom(_ payload: [String: Any]) -> String {
    if let error = payload["error"] as? [String: Any] {
      return error["message"] as? String ?? "\(error)"
    }
    return payload["message"] as? String ?? "Coach stream failed."
  }

  private func jsonString(_ value: Any) -> String {
    guard JSONSerialization.isValidJSONObject(value),
          let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]),
          let string = String(data: data, encoding: .utf8) else {
      return "{\"error\":\"json_encoding_failed\"}"
    }
    return string
  }
}
