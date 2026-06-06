import SwiftUI

struct CoachChatScreen: View {
  var chat: CoachChatModel
  var healthStore: HealthDataStore
  var appModel: GooseAppModel
  @Binding var draft: String
  let scrollToBottomRequestID: Int
  @FocusState private var composerFocused: Bool

  private let suggestions = [
    CoachPromptSuggestion(
      id: "blockers",
      title: "Find blockers",
      detail: "Score readiness, stale inputs, and the next fix.",
      prompt: "What is blocking today's scores?",
      systemImage: "chart.bar.xaxis"
    ),
    CoachPromptSuggestion(
      id: "recovery",
      title: "Read recovery",
      detail: "A concise recovery take with missing data called out.",
      prompt: "Summarize my recovery signals and what is missing.",
      systemImage: "waveform.path.ecg"
    ),
    CoachPromptSuggestion(
      id: "capture",
      title: "Next capture",
      detail: "What to collect next to improve confidence.",
      prompt: "What should I capture next to improve Coach confidence?",
      systemImage: "dot.radiowaves.left.and.right"
    ),
  ]

  var body: some View {
    ScrollViewReader { proxy in
      ScrollView {
        LazyVStack(alignment: .leading, spacing: 12) {
          if chat.streamState != .idle {
            CoachConnectionStrip(streamState: chat.streamState)
          }

          ForEach(chat.messages) { message in
            CoachMessageBubble(message: message)
              .id(message.id)
          }

          if chat.messages.count <= 1 {
            CoachSuggestionStack(suggestions: suggestions) { suggestion in
              composerFocused = false
              draft = ""
              send(prompt: suggestion.prompt)
            }
          }

          if let errorMessage = chat.errorMessage, !errorMessage.isEmpty {
            Label(errorMessage, systemImage: "exclamationmark.triangle")
              .font(.footnote)
              .foregroundStyle(.red)
              .padding(.horizontal, 2)
          }
        }
        .padding(.horizontal, 16)
        .padding(.top, 14)
        .padding(.bottom, 92)
      }
      .contentShape(Rectangle())
      .simultaneousGesture(
        TapGesture().onEnded {
          composerFocused = false
        }
      )
      .scrollDismissesKeyboard(.interactively)
      .onChange(of: chat.messages) { _, messages in
        scrollToBottom(proxy: proxy, messages: messages, animated: true)
      }
      .onChange(of: scrollToBottomRequestID) { _, _ in
        composerFocused = false
        scrollToBottom(proxy: proxy, messages: chat.messages, animated: true)
      }
    }
    .safeAreaInset(edge: .bottom, spacing: 0) {
      CoachComposer(
        draft: $draft,
        focused: $composerFocused,
        isStreaming: chat.streamState.isStreaming,
        send: sendDraft,
        cancel: {
          composerFocused = false
          chat.cancelStreaming()
        }
      )
    }
  }

  private func sendDraft() {
    let prompt = draft.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !prompt.isEmpty else {
      return
    }
    draft = ""
    send(prompt: prompt)
  }

  private func send(prompt: String) {
    let trimmedPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmedPrompt.isEmpty else {
      return
    }
    chat.send(trimmedPrompt, healthStore: healthStore, appModel: appModel)
  }

  private func scrollToBottom(
    proxy: ScrollViewProxy,
    messages: [CoachChatMessage],
    animated: Bool
  ) {
    guard let lastID = messages.last?.id else {
      return
    }
    let action = {
      proxy.scrollTo(lastID, anchor: .bottom)
    }
    if animated {
      withAnimation(.easeOut(duration: 0.18)) {
        action()
      }
    } else {
      action()
    }
  }
}

private struct CoachConnectionStrip: View {
  let streamState: CoachStreamState

  var body: some View {
    HStack(spacing: 8) {
      Image(systemName: streamState.isStreaming ? "dot.radiowaves.left.and.right" : "checkmark.seal.fill")
        .font(.caption.weight(.bold))
        .foregroundStyle(streamState.isStreaming ? .blue : .green)

      Text("Coach")
        .font(.caption.weight(.semibold))
        .foregroundStyle(.secondary)
        .lineLimit(1)

      Spacer()

      Text(statusText)
        .font(.caption2.weight(.bold))
        .foregroundStyle(.secondary)
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(.ultraThinMaterial, in: Capsule(style: .continuous))
    }
    .padding(.horizontal, 12)
    .padding(.vertical, 10)
    .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
  }

  private var statusText: String {
    switch streamState {
    case .idle:
      return "Ready"
    case .streaming:
      return "Streaming"
    case .failed:
      return "Needs attention"
    }
  }
}

private struct CoachPromptSuggestion: Identifiable, Equatable {
  let id: String
  let title: String
  let detail: String
  let prompt: String
  let systemImage: String
}

private struct CoachSuggestionStack: View {
  let suggestions: [CoachPromptSuggestion]
  let send: (CoachPromptSuggestion) -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      Text("Start Here")
        .font(.caption.weight(.bold))
        .foregroundStyle(.secondary)
        .textCase(.uppercase)

      ForEach(suggestions) { suggestion in
        CoachSuggestionButton(suggestion: suggestion) {
          send(suggestion)
        }
      }
    }
    .padding(.top, 2)
  }
}

private struct CoachSuggestionButton: View {
  let suggestion: CoachPromptSuggestion
  let send: () -> Void

  var body: some View {
    Button(action: send) {
      HStack(spacing: 12) {
        Image(systemName: suggestion.systemImage)
          .font(.system(size: 16, weight: .semibold))
          .foregroundStyle(.blue)
          .frame(width: 34, height: 34)
          .background(.blue.opacity(0.12), in: RoundedRectangle(cornerRadius: 8, style: .continuous))

        VStack(alignment: .leading, spacing: 3) {
          Text(suggestion.title)
            .font(.subheadline.weight(.semibold))
            .foregroundStyle(.primary)
          Text(suggestion.detail)
            .font(.caption)
            .foregroundStyle(.secondary)
            .lineLimit(2)
            .fixedSize(horizontal: false, vertical: true)
        }

        Spacer(minLength: 8)

        Image(systemName: "arrow.up.forward")
          .font(.caption.weight(.bold))
          .foregroundStyle(.tertiary)
      }
      .padding(12)
      .frame(maxWidth: .infinity, minHeight: 68, alignment: .leading)
      .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
      .overlay {
        RoundedRectangle(cornerRadius: 8, style: .continuous)
          .stroke(Color(.separator).opacity(0.22), lineWidth: 1)
      }
    }
    .buttonStyle(.plain)
  }
}
