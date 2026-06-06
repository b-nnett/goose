import SwiftUI

// MARK: - CoachSettingsSheet

struct CoachSettingsSheet: View {
  @Bindable var registry: CoachProviderRegistry
  var chat: CoachChatModel
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    List {
      Section(String(localized: "Provider")) {
        ForEach(registry.allProviders, id: \.id) { provider in
          CoachProviderPickerRow(
            provider: provider,
            isActive: provider.id == registry.activeProvider?.id
          ) {
            registry.selectProvider(id: provider.id)
          }
        }
      }

      Section(String(localized: "Configuration")) {
        CoachProviderConfigView(registry: registry)
      }

      if let active = registry.activeProvider, !active.availablePresets.isEmpty {
        Section(String(localized: "Model")) {
          CoachModelPresetPickerView(chat: chat, presets: active.availablePresets)
        }
      }
    }
    .navigationTitle(String(localized: "Coach Settings"))
    .navigationBarTitleDisplayMode(.inline)
    .toolbar {
      ToolbarItem(placement: .topBarLeading) {
        Button(String(localized: "Done")) {
          dismiss()
        }
      }
    }
  }
}

// MARK: - CoachProviderPickerRow

struct CoachProviderPickerRow: View {
  let provider: any CoachProvider
  let isActive: Bool
  let onSelect: () -> Void

  private var iconName: String {
    switch provider.id {
    case "chatgpt": return "bubble.left.and.bubble.right.fill"
    case "claude": return "sparkles"
    case "gemini": return "globe"
    case "custom": return "server.rack"
    default: return "questionmark.circle"
    }
  }

  private var tintColor: Color {
    switch provider.id {
    case "chatgpt": return .green
    case "claude": return .orange
    case "gemini": return .blue
    case "custom": return .purple
    default: return .secondary
    }
  }

  var body: some View {
    Button {
      onSelect()
    } label: {
      HStack(spacing: 12) {
        Image(systemName: iconName)
          .font(.system(size: 17, weight: .semibold))
          .foregroundStyle(isActive ? tintColor : .secondary)
          .frame(width: 32, height: 32)

        Text(provider.displayName)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(.primary)

        Spacer(minLength: 8)

        HStack(spacing: 6) {
          Image(systemName: provider.isAuthenticated ? "checkmark.circle.fill" : "circle")
            .font(.caption2.weight(.semibold))
            .foregroundStyle(provider.isAuthenticated ? tintColor : .secondary)

          Text(provider.isAuthenticated ? String(localized: "Signed in") : String(localized: "Not signed in"))
            .font(.caption2.weight(.semibold))
            .foregroundStyle(.secondary)

          if isActive {
            Image(systemName: "checkmark")
              .font(.caption.weight(.semibold))
              .foregroundStyle(.primary)
          }
        }
      }
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .accessibilityAddTraits(isActive ? .isSelected : [])
    .accessibilityLabel("\(provider.displayName), \(provider.isAuthenticated ? String(localized: "Signed in") : String(localized: "Not signed in"))\(isActive ? String(localized: ", active") : "")")
  }
}

// MARK: - CoachProviderConfigView

struct CoachProviderConfigView: View {
  @Bindable var registry: CoachProviderRegistry

  var body: some View {
    if let active = registry.activeProvider {
      switch active.id {
      case "chatgpt":
        ChatGPTConfigView(provider: active)
      case "claude":
        if let claude = active as? ClaudeCoachProvider {
          ClaudeConfigView(provider: claude)
        }
      case "gemini":
        if let gemini = active as? GeminiCoachProvider {
          GeminiConfigView(provider: gemini)
        }
      case "custom":
        if let custom = active as? CustomEndpointCoachProvider {
          CustomEndpointConfigView(provider: custom)
        }
      default:
        Text(String(localized: "Select a provider above to get started."))
          .foregroundStyle(.secondary)
          .font(.subheadline)
      }
    } else {
      Text(String(localized: "Select a provider above to get started."))
        .foregroundStyle(.secondary)
        .font(.subheadline)
    }
  }
}

// MARK: - ChatGPTConfigView

private struct ChatGPTConfigView: View {
  let provider: any CoachProvider
  @State private var showingSignOutConfirm = false

  var body: some View {
    if provider.isAuthenticated {
      HStack {
        Text(String(localized: "Signed in"))
          .foregroundStyle(.secondary)
        Spacer()
        Button(role: .destructive) {
          showingSignOutConfirm = true
        } label: {
          Label(String(localized: "Sign Out"), systemImage: "rectangle.portrait.and.arrow.right")
        }
        .buttonStyle(.bordered)
        .controlSize(.small)
        .confirmationDialog(
          String(localized: "Sign Out?"),
          isPresented: $showingSignOutConfirm,
          titleVisibility: .visible
        ) {
          Button(String(localized: "Sign Out"), role: .destructive) {
            provider.signOut()
          }
          Button(String(localized: "Cancel"), role: .cancel) {}
        } message: {
          Text(String(localized: "You will need to sign in again to use this provider."))
        }
      }
    } else {
      VStack(alignment: .leading, spacing: 12) {
        Text(String(localized: "Not signed in"))
          .foregroundStyle(.secondary)
          .font(.subheadline)

        Button {
          // ChatGPT OAuth sign-in is handled by CoachChatModel.startOAuthSignIn()
          // which is accessible via the existing CoachSignInScreen flow
        } label: {
          Label(String(localized: "Sign in with ChatGPT"), systemImage: "person.crop.circle.badge.checkmark")
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.borderedProminent)
      }
    }
  }
}

// MARK: - ClaudeConfigView

private struct ClaudeConfigView: View {
  let provider: ClaudeCoachProvider
  @State private var apiKey = ""
  @State private var showingRemoveConfirm = false
  @State private var keyStatus: String = ""

  private var hasKey: Bool { provider.isAuthenticated }

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack {
        SecureField(String(localized: "Anthropic API key"), text: $apiKey)
          .textContentType(.password)
          .autocorrectionDisabled()

        Image(systemName: "lock")
          .foregroundStyle(.secondary)
          .font(.caption)
      }

      HStack(spacing: 12) {
        Button {
          saveClaudeKey()
        } label: {
          Text(String(localized: "Save API Key"))
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.borderedProminent)
        .disabled(apiKey.isEmpty)
        .accessibilityHint(apiKey.isEmpty ? String(localized: "Enter an API key first") : "")

        if hasKey {
          Button(role: .destructive) {
            showingRemoveConfirm = true
          } label: {
            Text(String(localized: "Remove Key"))
          }
          .buttonStyle(.bordered)
          .confirmationDialog(
            String(localized: "Remove API Key"),
            isPresented: $showingRemoveConfirm,
            titleVisibility: .visible
          ) {
            Button(String(localized: "Remove"), role: .destructive) {
              provider.signOut()
              keyStatus = ""
            }
            Button(String(localized: "Cancel"), role: .cancel) {}
          } message: {
            Text(String(localized: "The key will be deleted from Keychain. You can add a new key at any time."))
          }
        }
      }

      Text(hasKey ? String(localized: "Key saved") : String(localized: "No key saved"))
        .font(.caption)
        .foregroundStyle(.secondary)
    }
  }

  private func saveClaudeKey() {
    guard !apiKey.isEmpty else { return }
    try? provider.saveAPIKey(apiKey)
    apiKey = ""
    keyStatus = "saved"
  }
}

// MARK: - GeminiConfigView

private struct GeminiConfigView: View {
  @Bindable var provider: GeminiCoachProvider
  @State private var clientId = ""
  @State private var showingOAuthSheet = false
  @State private var showingSignOutConfirm = false
  @State private var codeVerifier = ""
  @State private var oauthError: String?

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      TextField(String(localized: "Google Client ID"), text: $clientId)
        .keyboardType(.default)
        .autocorrectionDisabled()
        .textInputAutocapitalization(.never)
        .onAppear {
          clientId = provider.oauthClientId
        }
        .onChange(of: clientId) { _, newValue in
          UserDefaults.standard.set(newValue, forKey: GeminiCoachProvider.oauthClientIdKey)
        }

      if provider.isAuthenticated {
        HStack {
          Text(String(localized: "Signed in"))
            .foregroundStyle(.secondary)
          Spacer()
          Button(role: .destructive) {
            showingSignOutConfirm = true
          } label: {
            Label(String(localized: "Sign Out"), systemImage: "rectangle.portrait.and.arrow.right")
          }
          .buttonStyle(.bordered)
          .controlSize(.small)
          .confirmationDialog(
            String(localized: "Sign Out?"),
            isPresented: $showingSignOutConfirm,
            titleVisibility: .visible
          ) {
            Button(String(localized: "Sign Out"), role: .destructive) {
              provider.signOut()
            }
            Button(String(localized: "Cancel"), role: .cancel) {}
          } message: {
            Text(String(localized: "You will need to sign in again to use this provider."))
          }
        }
      } else {
        if provider.isExchangingToken {
          HStack(spacing: 8) {
            ProgressView()
              .controlSize(.small)
            Text(String(localized: "Signing in..."))
              .font(.subheadline)
              .foregroundStyle(.secondary)
          }
        } else {
          Button {
            startGeminiSignIn()
          } label: {
            Label(String(localized: "Sign in with Google"), systemImage: "person.crop.circle.badge.checkmark")
              .frame(maxWidth: .infinity)
          }
          .buttonStyle(.borderedProminent)
          .disabled(clientId.isEmpty)
        }

        if let oauthError {
          Text(oauthError)
            .font(.caption)
            .foregroundStyle(.red)
        }
      }
    }
    .sheet(isPresented: $showingOAuthSheet) {
      GeminiOAuthWebView(
        clientId: clientId,
        codeVerifier: codeVerifier,
        onCode: { code in
          showingOAuthSheet = false
          Task {
            do {
              try await provider.handleRedirect(code: code, codeVerifier: codeVerifier)
              oauthError = nil
            } catch {
              oauthError = String(localized: "Sign-in failed. Try again.")
            }
          }
        }
      )
    }
  }

  private func startGeminiSignIn() {
    codeVerifier = GeminiCoachProvider.generateCodeVerifier()
    oauthError = nil
    showingOAuthSheet = true
  }
}

// MARK: - CustomEndpointConfigView

private struct CustomEndpointConfigView: View {
  let provider: CustomEndpointCoachProvider
  @State private var baseURL = ""
  @State private var apiKey = ""
  @State private var modelID = ""
  @State private var showingValidationError = false
  @State private var savedLabel = false

  private var urlIsInvalid: Bool {
    !baseURL.isEmpty && !CustomEndpointCoachProvider.validateBaseURL(baseURL)
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      VStack(alignment: .leading, spacing: 4) {
        TextField(String(localized: "Base URL"), text: $baseURL)
          .keyboardType(.URL)
          .autocorrectionDisabled()
          .textInputAutocapitalization(.never)

        if urlIsInvalid {
          Text(String(localized: "Must start with https://"))
            .font(.caption)
            .foregroundStyle(.red)
        }
      }

      SecureField(String(localized: "API Key"), text: $apiKey)
        .textContentType(.password)
        .autocorrectionDisabled()

      TextField(String(localized: "Model ID"), text: $modelID)
        .autocorrectionDisabled()
        .textInputAutocapitalization(.never)

      Button {
        saveCustomEndpoint()
      } label: {
        Text(savedLabel ? String(localized: "Saved") : String(localized: "Save Endpoint"))
          .frame(maxWidth: .infinity)
      }
      .buttonStyle(.borderedProminent)
    }
    .onAppear {
      baseURL = provider.baseURL
      modelID = provider.modelID
    }
  }

  private func saveCustomEndpoint() {
    guard CustomEndpointCoachProvider.validateBaseURL(baseURL) else {
      return
    }
    provider.baseURL = baseURL
    provider.modelID = modelID
    if !apiKey.isEmpty {
      try? provider.saveEndpoint(apiKey: apiKey)
      apiKey = ""
    } else {
      // Re-evaluate isAuthenticated with updated URL even if no new key provided
      try? provider.saveEndpoint(apiKey: CustomEndpointCredentialStore.currentKey())
    }
    savedLabel = true
    Task {
      try? await Task.sleep(for: .seconds(2))
      await MainActor.run { savedLabel = false }
    }
  }
}

// MARK: - CoachModelPresetPickerView

struct CoachModelPresetPickerView: View {
  var chat: CoachChatModel
  let presets: [CoachModelPreset]

  var body: some View {
    Picker(String(localized: "Model"), selection: Binding(
      get: { chat.activePreset },
      set: { chat.selectModelPreset($0) }
    )) {
      ForEach(presets) { preset in
        Text(preset.title).tag(preset)
      }
    }
    .pickerStyle(.inline)
    .labelsHidden()
  }
}
