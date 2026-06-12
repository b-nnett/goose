import SwiftUI

struct CoachView: View {
  @Environment(GooseAppModel.self) private var model
  @EnvironmentObject private var router: AppRouter
  var healthStore: HealthDataStore
  @State private var registry = CoachProviderRegistry()
  @State private var chat: CoachChatModel
  @State private var promptDraft = ""
  @State private var appliedCoachPromptRequestID = 0
  @State private var showingChat = false
  @State private var showingSettings = false

  init(healthStore: HealthDataStore) {
    self.healthStore = healthStore
    let registry = CoachProviderRegistry()
    self._registry = State(initialValue: registry)
    self._chat = State(initialValue: CoachChatModel(registry: registry))
  }

  var body: some View {
    CoachOverviewScreen(
      snapshot: coachSnapshot,
      chatIsSignedIn: chat.isSignedIn,
      chatStatus: chatStatus,
      openChat: {
          if chat.isSignedIn {
            openChat(prompt: nil)
          } else {
            showingSettings = true
          }
        },
      openHealth: router.openHealth,
      openMore: router.openMore,
      openChatPrompt: openChat(prompt:),
      healthStore: healthStore
    )
    .gooseScreenBackground()
    .navigationTitle("Coach")
    .navigationBarTitleDisplayMode(.inline)
    .toolbarBackground(.hidden, for: .navigationBar)
    .toolbar {
      if let providerName = registry.activeProvider?.displayName {
        ToolbarItem(placement: .principal) {
          VStack(spacing: 1) {
            Text("Coach")
              .font(.headline.weight(.semibold))
            Text(providerName)
              .font(.caption2)
              .foregroundStyle(.secondary)
          }
        }
      }
      ToolbarItem(placement: .topBarTrailing) {
        Button {
          showingSettings = true
        } label: {
          Image(systemName: "gearshape")
        }
        .accessibilityLabel(String(localized: "Coach settings"))
      }
      if chat.isSignedIn {
        ToolbarItem(placement: .topBarTrailing) {
          CoachProfileMenu(chat: chat)
        }
      }
    }
    .sheet(isPresented: $showingSettings) {
      NavigationStack {
        CoachSettingsSheet(registry: registry, chat: chat)
      }
      .presentationDetents([.large])
      .presentationDragIndicator(.visible)
    }
    .sheet(isPresented: $showingChat) {
      NavigationStack {
        chatSheetContent
          .gooseScreenBackground()
          .navigationTitle(chat.isSignedIn ? "Coach Chat" : "Coach Sign In")
          .navigationBarTitleDisplayMode(.inline)
          .toolbarBackground(.hidden, for: .navigationBar)
          .toolbar {
            ToolbarItem(placement: .topBarLeading) {
              Button("Done") {
                showingChat = false
              }
            }
            if chat.isSignedIn {
              ToolbarItem(placement: .topBarTrailing) {
                CoachProfileMenu(chat: chat)
              }
            }
          }
      }
    }
    .onAppear {
      model.recordUIAction("page.opened", detail: "Coach")
      Task { await healthStore.loadBridgeCatalogsIfNeeded() }
      healthStore.refreshPacketInputsIfNeeded()
      chat.refreshAuth()
      applyRequestedCoachPromptIfNeeded()
      refreshCoachSnapshot()
    }
    .onChange(of: healthStore.packetScoreStatus) { _, _ in
      refreshCoachSnapshot()
    }
    .onChange(of: router.codexEmbeddedLoginRequestID) { _, requestID in
      guard requestID > 0, !chat.isSignedIn else {
        return
      }
      showingChat = true
      chat.startOAuthSignIn()
    }
    .onChange(of: router.coachPromptRequestID) { _, _ in
      applyRequestedCoachPromptIfNeeded()
    }
  }

  @ViewBuilder
  private var chatSheetContent: some View {
    if chat.isSignedIn {
      CoachChatScreen(
        chat: chat,
        healthStore: healthStore,
        appModel: model,
        draft: $promptDraft,
        scrollToBottomRequestID: router.coachScrollToBottomRequestID
      )
    } else {
      CoachSignInScreen(
        loginStatus: chat.loginStatus,
        deviceCode: chat.deviceCode,
        errorMessage: chat.errorMessage,
        signIn: chat.startOAuthSignIn
      )
    }
  }

  private var chatStatus: String {
    if chat.isSignedIn {
      return chat.streamState.isStreaming ? "Streaming" : "Signed in"
    }
    return chat.loginStatus
  }

  @State private var cachedCoachSnapshot: CoachOverviewSnapshot?

  private var coachSnapshot: CoachOverviewSnapshot {
    cachedCoachSnapshot ?? CoachOverviewSnapshot.make(healthStore: healthStore, appModel: model)
  }

  private func refreshCoachSnapshot() {
    cachedCoachSnapshot = CoachOverviewSnapshot.make(healthStore: healthStore, appModel: model)
  }

  private func openChat(prompt: String?) {
    if let prompt {
      let trimmedPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
      if !trimmedPrompt.isEmpty {
        promptDraft = trimmedPrompt
      }
    }
    if registry.activeProvider == nil {
      showingSettings = true
    } else {
      showingChat = true
    }
  }

  private func applyRequestedCoachPromptIfNeeded() {
    guard router.coachPromptRequestID != appliedCoachPromptRequestID else {
      return
    }
    appliedCoachPromptRequestID = router.coachPromptRequestID
    let prompt = router.coachPromptDraft.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !prompt.isEmpty else {
      return
    }
    promptDraft = prompt
    showingChat = true
  }
}

private struct CoachOverviewSnapshot {
  let recommendation: CoachRecommendation
  let highlights: [CoachMetricHighlight]
  let gaps: [CoachDataGap]

  @MainActor
  static func make(healthStore: HealthDataStore, appModel: GooseAppModel) -> CoachOverviewSnapshot {
    let homeTip = CoachTipFactory.homeTip(healthStore: healthStore, appModel: appModel)
    let readiness = healthStore.metricInputReadinessSummary()
    let inputNextAction = healthStore.metricInputReadinessNextActionSummary()
    let featureNextAction = healthStore.packetDerivedFeatureNextActionSummary()
    let scoreNextAction = healthStore.packetDerivedScoreNextActionSummary()
    let liveHeartRate = healthStore.latestHeartRateSummary(
      bpm: appModel.ble.liveHeartRateBPM,
      source: appModel.ble.liveHeartRateSource,
      updatedAt: appModel.ble.liveHeartRateUpdatedAt
    )
    let snapshots = [
      healthStore.snapshot(for: .sleep),
      healthStore.snapshot(for: .recovery),
      healthStore.snapshot(for: .strain),
      healthStore.snapshot(for: .stress),
    ]

    let recommendation = CoachRecommendation(
      title: primaryFocusTitle(inputNextAction: inputNextAction, scoreNextAction: scoreNextAction, snapshots: snapshots),
      message: firstUseful(
        inputNextAction,
        scoreNextAction,
        "Review the freshest local metrics before changing training or sleep plans."
      ),
      evidence: [
        "Readiness: \(readiness)",
        "Features: \(featureNextAction)",
        "Scores: \(scoreNextAction)",
        "Latest HR: \(liveHeartRate)",
      ],
      prompt: homeTip.prompt
    )

    var highlights = snapshots.map { snapshot in
      CoachMetricHighlight(
        id: snapshot.route.rawValue,
        title: snapshot.title,
        value: snapshot.displayValue.isEmpty ? "--" : snapshot.displayValue,
        status: snapshot.status,
        freshness: snapshot.freshness,
        provenance: snapshot.source.label,
        systemImage: snapshot.systemImage,
        tint: snapshot.tint,
        route: snapshot.route
      )
    }
    highlights.append(
      CoachMetricHighlight(
        id: "hrv",
        title: "HRV",
        value: healthStore.hrvFeatureSummary(),
        status: "Packet HRV",
        freshness: healthStore.packetInputStatus,
        provenance: healthStore.hrvFeatureProvenanceSummary(),
        systemImage: "waveform.path.ecg",
        tint: .blue,
        route: .healthMonitor
      )
    )
    highlights.append(
      CoachMetricHighlight(
        id: "live-hr",
        title: "Live HR",
        value: liveHeartRate,
        status: appModel.ble.liveHeartRateSource,
        freshness: HealthDataStore.relativeText(for: appModel.ble.liveHeartRateUpdatedAt) ?? "Waiting",
        provenance: healthStore.latestHeartRateProvenanceSummary(source: appModel.ble.liveHeartRateSource),
        systemImage: "heart.fill",
        tint: .red,
        route: .healthMonitor
      )
    )

    return CoachOverviewSnapshot(
      recommendation: recommendation,
      highlights: highlights,
      gaps: dataGaps(
        healthStore: healthStore,
        snapshots: snapshots,
        inputNextAction: inputNextAction,
        featureNextAction: featureNextAction,
        scoreNextAction: scoreNextAction
      )
    )
  }

  private static func primaryFocusTitle(
    inputNextAction: String,
    scoreNextAction: String,
    snapshots: [HealthMetricSnapshot]
  ) -> String {
    if snapshots.contains(where: { $0.source.kind == .unavailable }) {
      return "Close the data gaps first"
    }
    if !inputNextAction.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
      return "Refresh trusted inputs"
    }
    if !scoreNextAction.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
      return "Refresh score outputs"
    }
    return "Review today"
  }

  @MainActor
  private static func dataGaps(
    healthStore: HealthDataStore,
    snapshots: [HealthMetricSnapshot],
    inputNextAction: String,
    featureNextAction: String,
    scoreNextAction: String
  ) -> [CoachDataGap] {
    var gaps: [CoachDataGap] = []

    appendGap(
      &gaps,
      id: "readiness",
      title: "Input readiness",
      detail: inputNextAction,
      systemImage: "square.stack.3d.up",
      tint: .blue,
      actionTitle: "Review Inputs",
      action: .health(.packetInputs)
    )

    appendGap(
      &gaps,
      id: "features",
      title: "Packet features",
      detail: featureNextAction,
      systemImage: "dot.radiowaves.left.and.right",
      tint: .cyan,
      actionTitle: "Review Inputs",
      action: .health(.packetInputs)
    )

    appendGap(
      &gaps,
      id: "scores",
      title: "Score outputs",
      detail: scoreNextAction,
      systemImage: "function",
      tint: .purple,
      actionTitle: "Review Algorithms",
      action: .health(.algorithms)
    )

    for snapshot in snapshots where snapshot.source.kind == .unavailable {
      let action: CoachOverviewAction = snapshot.route == .sleep ? .more(.healthSync) : .more(.capture)
      appendGap(
        &gaps,
        id: "missing-\(snapshot.route.rawValue)",
        title: "\(snapshot.title) missing",
        detail: snapshot.source.detail,
        systemImage: snapshot.systemImage,
        tint: snapshot.tint,
        actionTitle: snapshot.route == .sleep ? "Open Health Sync" : "Open Capture",
        action: action
      )
    }

    appendGap(
      &gaps,
      id: "calibration",
      title: "Calibration",
      detail: healthStore.calibrationNextActionSummary(),
      systemImage: "slider.horizontal.3",
      tint: .mint,
      actionTitle: "Open Calibration",
      action: .health(.calibration)
    )

    return Array(gaps.prefix(5))
  }

  private static func appendGap(
    _ gaps: inout [CoachDataGap],
    id: String,
    title: String,
    detail: String,
    systemImage: String,
    tint: Color,
    actionTitle: String,
    action: CoachOverviewAction
  ) {
    let trimmed = detail.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty, trimmed.localizedCaseInsensitiveContains("review calibrated") == false else {
      return
    }
    guard gaps.contains(where: { $0.id == id }) == false else {
      return
    }
    gaps.append(
      CoachDataGap(
        id: id,
        title: title,
        detail: trimmed,
        systemImage: systemImage,
        tint: tint,
        actionTitle: actionTitle,
        action: action
      )
    )
  }

  private static func firstUseful(_ values: String...) -> String {
    values
      .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
      .first { !$0.isEmpty } ?? "Review the freshest local metrics before changing training or sleep plans."
  }
}

private struct CoachRecommendation {
  let title: String
  let message: String
  let evidence: [String]
  let prompt: String
}

private struct CoachMetricHighlight: Identifiable {
  let id: String
  let title: String
  let value: String
  let status: String
  let freshness: String
  let provenance: String
  let systemImage: String
  let tint: Color
  let route: HealthRoute
}

private struct CoachDataGap: Identifiable {
  let id: String
  let title: String
  let detail: String
  let systemImage: String
  let tint: Color
  let actionTitle: String
  let action: CoachOverviewAction
}

private enum CoachOverviewAction: Hashable {
  case health(HealthRoute)
  case more(MoreRoute)
  case chat(String)
}

private struct CoachOverviewScreen: View {
  let snapshot: CoachOverviewSnapshot
  let chatIsSignedIn: Bool
  let chatStatus: String
  let openChat: () -> Void
  let openHealth: (HealthRoute?) -> Void
  let openMore: (MoreRoute?) -> Void
  let openChatPrompt: (String) -> Void
  var healthStore: HealthDataStore
  @State private var showingJournal = false
  @State private var todayEntry: DailyJournalEntry? = DailyJournalStore.today()
  @State private var vowDismissed = false

  var body: some View {
    ScrollView {
      LazyVStack(alignment: .leading, spacing: 16) {
        CoachRecommendationCard(recommendation: snapshot.recommendation) {
          openChatPrompt(snapshot.recommendation.prompt)
        }

        CoachOverviewChatCard(
          signedIn: chatIsSignedIn,
          status: chatStatus,
          action: openChat
        )

        CoachJournalCard(entry: todayEntry) {
          showingJournal = true
        }

        if !vowDismissed, let nudge = CoachVOWNudge.resolve(healthStore: healthStore) {
          CoachVOWCard(nudge: nudge) { vowDismissed = true }
            .opacity(vowDismissed ? 0 : 1)
            .animation(.easeOut(duration: 0.2), value: vowDismissed)
        }

        CoachRoutesSection(healthStore: healthStore)

        CoachOverviewSectionTitle("Metric Highlights")
        LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 10) {
          ForEach(snapshot.highlights) { highlight in
            Button {
              openHealth(highlight.route)
            } label: {
              CoachMetricHighlightCard(highlight: highlight)
            }
            .buttonStyle(.plain)
          }
        }

        if !snapshot.gaps.isEmpty {
          CoachOverviewSectionTitle("Data Gaps")
          VStack(spacing: 10) {
            ForEach(snapshot.gaps) { gap in
              CoachDataGapCard(gap: gap) {
                handle(gap.action)
              }
            }
          }
        }
      }
      .padding(.horizontal, 16)
      .padding(.vertical, 18)
    }
    .scrollClipDisabled()
    .sheet(isPresented: $showingJournal, onDismiss: {
      todayEntry = DailyJournalStore.today()
    }) {
      DailyJournalSheet(existing: todayEntry)
    }
  }

  private func handle(_ action: CoachOverviewAction) {
    switch action {
    case .health(let route):
      openHealth(route)
    case .more(let route):
      openMore(route)
    case .chat(let prompt):
      openChatPrompt(prompt)
    }
  }
}

private struct CoachRecommendationCard: View {
  let recommendation: CoachRecommendation
  let ask: () -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 13) {
      HStack(alignment: .top, spacing: 12) {
        Image(systemName: "sparkles")
          .font(.system(size: 18, weight: .semibold))
          .foregroundStyle(.purple)
          .frame(width: 38, height: 38)
          .background(.purple.opacity(0.12), in: RoundedRectangle(cornerRadius: 8, style: .continuous))

        VStack(alignment: .leading, spacing: 5) {
          Text(recommendation.title)
            .font(.title3.weight(.semibold))
          Text(recommendation.message)
            .font(.subheadline)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        }
      }

      VStack(alignment: .leading, spacing: 7) {
        ForEach(recommendation.evidence, id: \.self) { evidence in
          Label(evidence, systemImage: "checkmark.seal")
            .font(.caption)
            .foregroundStyle(.secondary)
            .lineLimit(2)
            .fixedSize(horizontal: false, vertical: true)
        }
      }

      Button(action: ask) {
        Label("Ask About This", systemImage: "bubble.left.and.bubble.right")
          .font(.subheadline.weight(.semibold))
          .frame(maxWidth: .infinity)
      }
      .buttonStyle(.borderedProminent)
    }
    .padding(16)
    .coachCardSurface(tint: .purple, prominent: true)
  }
}

private struct CoachOverviewChatCard: View {
  let signedIn: Bool
  let status: String
  let action: () -> Void

  var body: some View {
    HStack(spacing: 12) {
      Image(systemName: signedIn ? "bubble.left.and.bubble.right.fill" : "person.crop.circle.badge.checkmark")
        .font(.system(size: 17, weight: .semibold))
        .foregroundStyle(signedIn ? .blue : .secondary)
        .frame(width: 36, height: 36)
        .background((signedIn ? Color.blue : Color.secondary).opacity(0.12), in: RoundedRectangle(cornerRadius: 8, style: .continuous))

      VStack(alignment: .leading, spacing: 3) {
        Text(signedIn ? "Chat ready" : "Chat signed out")
          .font(.headline)
        Text(status.isEmpty ? "Local Coach works without chat" : status)
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(1)
      }

      Spacer(minLength: 8)

      Button(action: action) {
        if signedIn {
          Text("Open")
        } else {
          Label("Sign In", systemImage: "gearshape")
        }
      }
      .font(.caption.weight(.semibold))
      .buttonStyle(.bordered)
      .controlSize(.small)
    }
    .padding(14)
    .coachCardSurface(tint: .blue)
  }
}

private struct CoachMetricHighlightCard: View {
  let highlight: CoachMetricHighlight

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      HStack(spacing: 8) {
        Image(systemName: highlight.systemImage)
          .font(.caption.weight(.bold))
          .foregroundStyle(highlight.tint)
        Text(highlight.title)
          .font(.caption.weight(.bold))
          .foregroundStyle(.secondary)
          .lineLimit(1)
        Spacer(minLength: 0)
      }

      Text(highlight.value)
        .font(.title3.weight(.semibold))
        .fontDesign(.rounded)
        .lineLimit(2)
        .minimumScaleFactor(0.70)

      VStack(alignment: .leading, spacing: 3) {
        Text(highlight.status)
          .font(.caption.weight(.semibold))
          .foregroundStyle(.primary)
          .lineLimit(1)
        Text(highlight.freshness)
          .font(.caption2)
          .foregroundStyle(.secondary)
          .lineLimit(1)
        Text(highlight.provenance)
          .font(.caption2)
          .foregroundStyle(.tertiary)
          .lineLimit(2)
          .fixedSize(horizontal: false, vertical: true)
      }

      Spacer(minLength: 0)
    }
    .frame(maxWidth: .infinity, minHeight: 154, alignment: .topLeading)
    .padding(13)
    .coachCardSurface(tint: highlight.tint)
  }
}

private struct CoachDataGapCard: View {
  let gap: CoachDataGap
  let action: () -> Void

  var body: some View {
    HStack(alignment: .top, spacing: 12) {
      Image(systemName: gap.systemImage)
        .font(.system(size: 16, weight: .semibold))
        .foregroundStyle(gap.tint)
        .frame(width: 34, height: 34)
        .background(gap.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 8, style: .continuous))

      VStack(alignment: .leading, spacing: 5) {
        Text(gap.title)
          .font(.subheadline.weight(.semibold))
        Text(gap.detail)
          .font(.caption)
          .foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }

      Spacer(minLength: 8)

      Button(gap.actionTitle, action: action)
        .font(.caption.weight(.semibold))
        .buttonStyle(.bordered)
        .controlSize(.small)
    }
    .padding(13)
    .coachCardSurface(tint: gap.tint)
  }
}

private struct CoachOverviewSectionTitle: View {
  let title: String

  init(_ title: String) {
    self.title = title
  }

  var body: some View {
    Text(title)
      .font(.headline.weight(.semibold))
      .frame(maxWidth: .infinity, alignment: .leading)
      .padding(.top, 2)
  }
}

private extension View {
  func coachCardSurface(tint: Color, prominent: Bool = false) -> some View {
    background(
      RoundedRectangle(cornerRadius: 8, style: .continuous)
        .fill(Color(.secondarySystemGroupedBackground))
        .shadow(color: tint.opacity(prominent ? 0.16 : 0.08), radius: prominent ? 14 : 8, x: 0, y: prominent ? 7 : 3)
    )
    .overlay {
      RoundedRectangle(cornerRadius: 8, style: .continuous)
        .stroke(tint.opacity(prominent ? 0.18 : 0.10), lineWidth: 1)
    }
  }
}

private struct CoachProfileMenu: View {
  var chat: CoachChatModel

  var body: some View {
    Menu {
      Section("Model") {
        ForEach(CoachModelPreset.allCases) { preset in
          Button {
            chat.selectModelPreset(preset)
          } label: {
            if chat.activePreset == preset {
              Label(preset.title, systemImage: "checkmark")
            } else {
              Text(preset.title)
            }
          }
        }
      }

      Button(role: .destructive) {
        chat.startNewConversation()
      } label: {
        Label("New Conversation", systemImage: "plus.message")
      }
      .disabled(chat.streamState.isStreaming)

      Button(role: .destructive) {
        chat.signOut()
      } label: {
        Label("Sign Out", systemImage: "rectangle.portrait.and.arrow.right")
      }
    } label: {
      Image(systemName: "person.crop.circle")
    }
    .accessibilityLabel("Coach account")
  }
}

// MARK: - COACH-08: Daily Journal

struct DailyJournalEntry: Codable, Identifiable {
  let id: String
  let dateKey: String
  var text: String
  var tags: [String]
  var savedAt: Double

  init(dateKey: String, text: String, tags: [String]) {
    self.id = dateKey
    self.dateKey = dateKey
    self.text = text
    self.tags = tags
    self.savedAt = Date().timeIntervalSince1970
  }

  static let allTags = ["sleep", "recovery", "strain", "stress", "mood", "nutrition", "notes"]
}

enum DailyJournalStore {
  private static let key = "goose.coach.journal.entries"

  static func todayKey() -> String {
    let fmt = DateFormatter()
    fmt.dateFormat = "yyyy-MM-dd"
    return fmt.string(from: Date())
  }

  static func load() -> [String: DailyJournalEntry] {
    guard let data = UserDefaults.standard.data(forKey: key),
          let entries = try? JSONDecoder().decode([String: DailyJournalEntry].self, from: data) else {
      return [:]
    }
    return entries
  }

  static func save(_ entry: DailyJournalEntry) {
    var all = load()
    all[entry.dateKey] = entry
    if let data = try? JSONEncoder().encode(all) {
      UserDefaults.standard.set(data, forKey: key)
    }
  }

  static func today() -> DailyJournalEntry? {
    load()[todayKey()]
  }
}

struct DailyJournalSheet: View {
  @Environment(\.dismiss) private var dismiss
  @State private var text: String
  @State private var selectedTags: Set<String>
  private let dateKey: String

  init(existing: DailyJournalEntry?) {
    let key = DailyJournalStore.todayKey()
    self.dateKey = key
    _text = State(initialValue: existing?.text ?? "")
    _selectedTags = State(initialValue: Set(existing?.tags ?? []))
  }

  var body: some View {
    NavigationStack {
      VStack(alignment: .leading, spacing: 16) {
        Text(formattedDate)
          .font(.caption)
          .foregroundStyle(.secondary)
          .padding(.horizontal, 16)

        TextEditor(text: $text)
          .font(.body)
          .frame(minHeight: 140)
          .padding(12)
          .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 10))
          .padding(.horizontal, 16)

        VStack(alignment: .leading, spacing: 8) {
          Text("TAGS")
            .font(.system(size: 11, weight: .black))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 16)

          ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
              ForEach(DailyJournalEntry.allTags, id: \.self) { tag in
                let selected = selectedTags.contains(tag)
                Button {
                  if selected { selectedTags.remove(tag) } else { selectedTags.insert(tag) }
                } label: {
                  Text(tag)
                    .font(.caption.weight(.semibold))
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                    .background(selected ? Color.accentColor : Color.secondary.opacity(0.15), in: Capsule())
                    .foregroundStyle(selected ? .white : .primary)
                }
                .buttonStyle(.plain)
              }
            }
            .padding(.horizontal, 16)
          }
        }

        Spacer()
      }
      .padding(.top, 8)
      .navigationTitle("Diário")
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .topBarLeading) {
          Button("Cancelar") { dismiss() }
        }
        ToolbarItem(placement: .topBarTrailing) {
          Button("Guardar") { save() }
            .font(.subheadline.weight(.semibold))
            .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
      }
    }
    .presentationDetents([.medium, .large])
    .presentationDragIndicator(.visible)
  }

  private var formattedDate: String {
    let fmt = DateFormatter()
    fmt.dateStyle = .full
    return fmt.string(from: Date())
  }

  private func save() {
    let entry = DailyJournalEntry(
      dateKey: dateKey,
      text: text.trimmingCharacters(in: .whitespacesAndNewlines),
      tags: Array(selectedTags).sorted()
    )
    DailyJournalStore.save(entry)
    dismiss()
  }
}

private enum CoachVOWNudge {
  case criticalRecovery(Double)
  case lowRecovery(Double)
  case highStrain(Double)
  case lowHRV(Double)

  @MainActor
  static func resolve(healthStore: HealthDataStore) -> CoachVOWNudge? {
    let recoveryValue = healthStore.snapshot(for: .recovery).value
    let strainValue = healthStore.snapshot(for: .strain).value
    let hrv = HRVSeriesStore.shared.dailyEstimate()?.rmssdMS

    if let r = Double(recoveryValue), r < 33 { return .criticalRecovery(r) }
    if let r = Double(recoveryValue), r < 66 { return .lowRecovery(r) }
    if let s = Double(strainValue), s > 18 { return .highStrain(s) }
    if let h = hrv, h < 30 { return .lowHRV(h) }
    return nil
  }

  var title: String {
    switch self {
    case .criticalRecovery: "Critical Recovery"
    case .lowRecovery: "Low Recovery"
    case .highStrain: "High Strain"
    case .lowHRV: "Low HRV"
    }
  }

  var body: String {
    switch self {
    case .criticalRecovery: "Recovery is critically low. Prioritise rest and avoid high-strain activity today."
    case .lowRecovery: "Recovery is below 66%. Consider light training only."
    case .highStrain: "Strain is high. Allow adequate recovery before the next session."
    case .lowHRV: "HRV is low this week. Monitor stress and sleep quality."
    }
  }

  var systemImage: String {
    switch self {
    case .criticalRecovery: "heart.slash"
    case .lowRecovery: "heart"
    case .highStrain: "figure.run"
    case .lowHRV: "waveform.path.ecg"
    }
  }

  var tint: Color {
    switch self {
    case .criticalRecovery: .red
    case .lowRecovery: .orange
    case .highStrain: .orange
    case .lowHRV: .blue
    }
  }
}

private struct CoachVOWCard: View {
  let nudge: CoachVOWNudge
  let onDismiss: () -> Void

  var body: some View {
    HStack(spacing: 12) {
      Image(systemName: nudge.systemImage)
        .font(.system(size: 17, weight: .semibold))
        .foregroundStyle(nudge.tint)
        .frame(width: 36, height: 36)
        .background(nudge.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 8, style: .continuous))

      VStack(alignment: .leading, spacing: 4) {
        Text(nudge.title)
          .font(.subheadline.weight(.semibold))
        Text(nudge.body)
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(2)
      }

      Spacer(minLength: 8)

      Button(action: onDismiss) {
        Image(systemName: "xmark")
          .font(.caption2.weight(.semibold))
          .foregroundStyle(.tertiary)
      }
      .accessibilityLabel("Dismiss nudge")
    }
    .padding(12)
    .coachCardSurface(tint: nudge.tint)
    .accessibilityElement(children: .combine)
    .accessibilityLabel("\(nudge.title). \(nudge.body). Double-tap to dismiss.")
    .gesture(DragGesture().onEnded { if $0.translation.height > 30 { onDismiss() } })
  }
}

private struct CoachJournalCard: View {
  let entry: DailyJournalEntry?
  let onOpen: () -> Void

  var body: some View {
    Button(action: onOpen) {
      HStack(spacing: 12) {
        let iconTint: Color = entry == nil ? .secondary : .orange
        Image(systemName: entry == nil ? "book.pages" : "book.pages.fill")
          .font(.system(size: 17, weight: .semibold))
          .foregroundStyle(iconTint)
          .frame(width: 36, height: 36)
          .background(iconTint.opacity(0.12), in: RoundedRectangle(cornerRadius: 8, style: .continuous))

        VStack(alignment: .leading, spacing: 3) {
          Text(entry == nil ? "Diário de Hoje" : "Diário de Hoje")
            .font(.headline)
          if let entry {
            Text(entry.text.prefix(60))
              .font(.caption)
              .foregroundStyle(.secondary)
              .lineLimit(1)
          } else {
            Text("Escrever nota do dia")
              .font(.caption)
              .foregroundStyle(.secondary)
          }
        }

        Spacer(minLength: 8)
        Image(systemName: "chevron.right")
          .font(.caption.weight(.semibold))
          .foregroundStyle(.tertiary)
      }
      .padding(14)
      .coachCardSurface(tint: .orange)
    }
    .buttonStyle(.plain)
  }
}

#Preview("Signed out") {
  NavigationStack {
    CoachView(healthStore: HealthDataStore())
      .environment(GooseAppModel(startBLE: false))
      .environmentObject(AppRouter())
  }
}
