import SwiftUI

struct RootView: View {
  @Environment(GooseAppModel.self) private var model
  @AppStorage(OnboardingStorage.onboardingComplete) private var onboardingComplete = false
  @AppStorage(OnboardingStorage.onboardingRedoRequested) private var onboardingRedoRequested = false

  var body: some View {
    ZStack(alignment: .top) {
      Group {
        if onboardingComplete {
          AppShellView()
        } else {
          OnboardingView {
            onboardingRedoRequested = false
            onboardingComplete = true
            model.completeOnboarding()
          }
        }
      }
      SyncToastHost(ble: model.ble)
    }
    .gooseScreenBackground()
    .onAppear {
      mirrorCurrentOnboardingStateIfNeeded()
      restorePersistedOnboardingStateIfNeeded()
      syncModelOnboardingState()
    }
    .onChange(of: onboardingComplete) { _, _ in
      mirrorCurrentOnboardingStateIfNeeded()
      syncModelOnboardingState()
    }
  }

  private func mirrorCurrentOnboardingStateIfNeeded() {
    guard onboardingComplete else {
      return
    }
    OnboardingProfilePersistence.saveProfileFromDefaults(onboardingComplete: true)
  }

  private func restorePersistedOnboardingStateIfNeeded() {
    guard !onboardingComplete, !onboardingRedoRequested else {
      return
    }
    // Restore profile data (name, height, weight, etc.) from Keychain so fields are
    // pre-filled — but do NOT restore onboardingComplete. Keychain survives app deletion,
    // so a reinstall should show onboarding again with pre-filled data, not skip it.
    _ = OnboardingProfilePersistence.restoreIntoDefaultsIfAvailable(restoreCompletion: false)
  }

  private func syncModelOnboardingState() {
    guard model.onboardingComplete != onboardingComplete else {
      return
    }
    model.onboardingComplete = onboardingComplete
  }
}

private struct SyncToastHost: View {
  var ble: GooseBLEClient

  var body: some View {
    @Bindable var ble = ble
    VStack {
      if let toast = ble.syncToast {
        Button {
          if toast.phase == .failed, let failure = ble.lastSyncFailure {
            ble.syncFailureSheet = failure
          }
        } label: {
          SyncStatusToastView(toast: toast)
        }
        .buttonStyle(.plain)
        .allowsHitTesting(toast.phase == .failed)
        .padding(.horizontal, 16)
        .padding(.top, 12)
        .transition(.asymmetric(
          insertion: .move(edge: .top).combined(with: .opacity),
          removal: .move(edge: .top).combined(with: .opacity)
        ))
      }
      Spacer(minLength: 0)
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
    .allowsHitTesting(ble.syncToast?.phase == .failed)
    .animation(.spring(response: 0.34, dampingFraction: 0.86), value: ble.syncToast?.id)
    .sheet(item: $ble.syncFailureSheet) { failure in
      SyncFailureSheet(failure: failure)
    }
  }
}

private struct SyncStatusToastView: View {
  let toast: GooseSyncToast
  @Environment(\.colorScheme) private var colorScheme

  var body: some View {
    HStack(spacing: 8) {
      SyncToastIcon(systemImage: systemImage, tint: tint, isSyncing: toast.phase == .syncing)

      Text(toast.title)
        .font(.system(size: 14, weight: .bold))
        .foregroundStyle(.primary)
        .lineLimit(1)

      if toast.phase == .failed {
        Image(systemName: "chevron.up")
          .font(.system(size: 12, weight: .black))
          .foregroundStyle(tint)
      }
    }
    .padding(.horizontal, 13)
    .padding(.vertical, 8)
    .fixedSize(horizontal: true, vertical: false)
    .background {
      Capsule(style: .continuous)
        .fill(toastFill)
    }
    .overlay {
      Capsule(style: .continuous)
        .strokeBorder(tint, lineWidth: 1.5)
    }
    .shadow(color: .black.opacity(0.22), radius: 14, x: 0, y: 7)
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(accessibilityText)
  }

  private var systemImage: String {
    switch toast.phase {
    case .syncing: "arrow.triangle.2.circlepath"
    case .synced: "checkmark.circle.fill"
    case .failed: "exclamationmark.triangle.fill"
    }
  }

  private var tint: Color {
    switch toast.phase {
    case .syncing: Color(red: 0.18, green: 0.48, blue: 0.95)
    case .synced: Color(red: 0.20, green: 0.68, blue: 0.27)
    case .failed: Color(red: 0.95, green: 0.23, blue: 0.18)
    }
  }

  private var toastFill: Color {
    if colorScheme == .dark {
      switch toast.phase {
      case .syncing:
        Color(red: 0.07, green: 0.16, blue: 0.25)
      case .synced:
        Color(red: 0.07, green: 0.20, blue: 0.12)
      case .failed:
        Color(red: 0.26, green: 0.10, blue: 0.09)
      }
    } else {
      switch toast.phase {
      case .syncing:
        Color(red: 0.84, green: 0.91, blue: 1.0)
      case .synced:
        Color(red: 0.86, green: 0.96, blue: 0.88)
      case .failed:
        Color(red: 1.0, green: 0.88, blue: 0.86)
      }
    }
  }

  private var accessibilityText: String {
    guard !toast.detail.isEmpty else {
      return toast.title
    }
    return "\(toast.title), \(toast.detail)"
  }
}

private struct SyncToastIcon: View {
  let systemImage: String
  let tint: Color
  let isSyncing: Bool
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    if isSyncing && !reduceMotion {
      TimelineView(.animation(minimumInterval: 1.0 / 60.0)) { context in
        symbol(rotationDegrees: rotationDegrees(for: context.date))
      }
    } else {
      symbol(rotationDegrees: 0)
    }
  }

  private func symbol(rotationDegrees: Double) -> some View {
    Image(systemName: systemImage)
      .font(.system(size: 14, weight: .black))
      .frame(width: 18, height: 18)
      .foregroundStyle(tint)
      .rotationEffect(.degrees(isSyncing ? rotationDegrees : 0))
      .transaction { transaction in
        transaction.disablesAnimations = true
        transaction.animation = nil
      }
  }

  private func rotationDegrees(for date: Date) -> Double {
    let duration = 0.95
    let progress = date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: duration) / duration
    return progress * 360
  }
}

private struct SyncFailureSheet: View {
  let failure: GooseSyncFailure
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    NavigationStack {
      ScrollView {
        VStack(alignment: .leading, spacing: 16) {
          VStack(alignment: .leading, spacing: 6) {
            Text(failure.title)
              .font(.title2.bold())
            Text(failure.occurredAt, style: .date)
              .font(.subheadline.weight(.semibold))
              .foregroundStyle(.secondary)
          }

          Text(failure.message)
            .font(.system(size: 14, weight: .semibold, design: .monospaced))
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(14)
            .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        }
        .padding(20)
      }
      .gooseScreenBackground()
      .navigationTitle("Sync Error")
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .topBarTrailing) {
          Button("Done") {
            dismiss()
          }
        }
      }
    }
  }
}
