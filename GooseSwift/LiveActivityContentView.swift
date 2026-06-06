import CoreLocation
import MapKit
import SwiftUI
import UIKit

enum FitnessWorkoutPage: Int, CaseIterable, Identifiable {
  case overview
  case heartRate
  case segment
  case split
  case elevation

  var id: Int { rawValue }

  static func pages(for activity: ActivityKind) -> [FitnessWorkoutPage] {
    activity.usesGPS ? allCases : [.overview, .heartRate, .segment, .split]
  }
}

struct LiveActivityContentView: View {
  @Environment(\.dismiss) private var dismiss
  @Environment(GooseAppModel.self) private var model
  @AppStorage("goose.swift.activity.lockHintSeen") private var lockHintSeen = false
  @AppStorage("goose.swift.activity.recentWorkouts") private var recentWorkoutRawValues = ""
  var ble: GooseBLEClient
  @ObservedObject var session: ActivitySessionModel
  @ObservedObject var locationTracker: ActivityLocationTracker

  @State private var selectedPage: FitnessWorkoutPage = .overview
  @State private var dockExpanded = false
  @State private var controlsLocked = false
  @State private var showingActivityPicker = false
  @State private var showingLockHint = false
  @State private var countdownValue: Int?
  @State private var countdownTimer: Timer?
  @State private var segmentReturnTask: Task<Void, Never>?
  @State private var segmentNumber = 1

  var body: some View {
    ZStack {
      FitnessColor.background
        .ignoresSafeArea()

      if let countdownValue {
        FitnessCountdownView(value: countdownValue, activity: session.selectedActivity, onSkip: skipCountdown)
      } else if showingSummary {
        FitnessSummaryView(activity: session.selectedActivity, session: session, ble: ble, locationTracker: locationTracker) {
          dismiss()
        }
      } else if !session.isActive {
        FitnessActivityPickerStartView(
          selectedActivity: session.selectedActivity,
          recentActivities: recentActivities,
          onStart: { activity in
            startFromPicker(activity)
          }
        )
      } else {
        FitnessLiveWorkoutView(
          selectedPage: $selectedPage,
          activity: session.selectedActivity,
          session: session,
          ble: ble,
          locationTracker: locationTracker,
          segmentNumber: segmentNumber,
          dockExpanded: $dockExpanded,
          controlsLocked: $controlsLocked,
          onPrimaryAction: primaryAction,
          onEndWorkout: endActivity,
          onStopViewing: { dismiss() },
          onLockControls: lockControls,
          onUnlockControls: unlockControls,
          onActivityTap: {
            if !session.isActive {
              showingActivityPicker = true
              model.recordUIAction("activity.picker.opened", detail: session.selectedActivity.title)
            }
          },
          onSegmentTap: markSegment,
          onHeartPageTap: {
            selectedPage = .heartRate
            model.recordUIAction("activity.page.shortcut", detail: "Heart Rate")
          }
        )
      }
    }
    .preferredColorScheme(.dark)
    .navigationTitle(navigationTitle)
    .navigationBarTitleDisplayMode(showingSummary ? .inline : .large)
    .toolbar(shouldShowNavigationBar ? .visible : .hidden, for: .navigationBar)
    .toolbarColorScheme(.dark, for: .navigationBar)
    .toolbarBackground(FitnessColor.background, for: .navigationBar)
    .toolbarBackground(shouldShowNavigationBar ? .visible : .hidden, for: .navigationBar)
    .toolbar(.hidden, for: .tabBar)
    .sheet(isPresented: $showingActivityPicker) {
      FitnessActivityPickerSheet(selectedActivity: session.selectedActivity, recentActivities: recentActivities, onSelect: select)
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        .preferredColorScheme(.dark)
    }
    .alert("Controls Locked", isPresented: $showingLockHint) {
      Button("OK", role: .cancel) {}
    } message: {
      Text("Hold the pause/resume button for 5 seconds to unlock it.")
    }
    .onAppear {
      model.recordUIAction("page.opened", detail: "Live Activity")
    }
    .onDisappear {
      countdownTimer?.invalidate()
      countdownTimer = nil
      segmentReturnTask?.cancel()
      segmentReturnTask = nil
    }
    .onChange(of: locationTracker.authorizationStatus) { _, status in
      model.recordUIAction("activity.location.authorization", detail: authorizationText(status))
    }
    .onReceive(session.$elapsed) { _ in
      updateWorkoutLiveActivity()
    }
    .onReceive(locationTracker.$distanceMeters) { _ in
      updateWorkoutLiveActivity()
    }
  }

  private var showingSummary: Bool {
    session.endedAt != nil && !session.isActive
  }

  private var shouldShowNavigationBar: Bool {
    showingSummary || (countdownValue == nil && !session.isActive)
  }

  private var navigationTitle: String {
    showingSummary ? summaryDate : "Workout"
  }

  private var summaryDate: String {
    let formatter = DateFormatter()
    formatter.dateFormat = "E d MMM"
    return formatter.string(from: session.startedAt ?? Date())
  }

  private var recentActivities: [ActivityKind] {
    var seen = Set<String>()
    return recentWorkoutRawValues
      .split(separator: ",")
      .compactMap { ActivityKind(rawValue: String($0)) }
      .filter { activity in
        seen.insert(activity.rawValue).inserted
      }
      .prefix(5)
      .map { $0 }
  }

  private func select(_ activity: ActivityKind) {
    session.select(activity)
    selectedPage = .overview
    model.recordUIAction("activity.selected", detail: activity.title)
  }

  private func startFromPicker(_ activity: ActivityKind) {
    select(activity)
    beginCountdown()
  }

  private func skipCountdown() {
    guard countdownValue != nil else {
      return
    }
    countdownTimer?.invalidate()
    countdownTimer = nil
    countdownValue = nil
    model.recordUIAction("activity.countdown.skip", detail: session.selectedActivity.title)
    startWorkoutNow()
  }

  private func primaryAction() {
    guard countdownValue == nil else {
      return
    }

    if session.isActive && session.isPaused {
      session.resume {
        ble.liveHeartRateBPM
      }
      if session.selectedActivity.usesGPS {
        locationTracker.start(reset: false)
      }
      updateWorkoutLiveActivity(force: true)
      model.recordUIAction("activity.resume", detail: session.selectedActivity.title)
      return
    }

    if session.isActive {
      session.pause(heartRate: ble.liveHeartRateBPM)
      if session.selectedActivity.usesGPS {
        locationTracker.stop()
      }
      updateWorkoutLiveActivity(force: true)
      model.recordUIAction("activity.pause", detail: session.selectedActivity.title)
      return
    }

    beginCountdown()
  }

  private func beginCountdown() {
    countdownTimer?.invalidate()
    countdownValue = 3
    dockExpanded = false
    controlsLocked = false
    selectedPage = .overview
    model.recordUIAction("activity.countdown.start", detail: session.selectedActivity.title)

    countdownTimer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { timer in
      guard let countdownValue else {
        timer.invalidate()
        return
      }

      if countdownValue > 1 {
        self.countdownValue = countdownValue - 1
      } else {
        timer.invalidate()
        self.countdownTimer = nil
        self.countdownValue = nil
        self.startWorkoutNow()
      }
    }
    countdownTimer?.tolerance = 0.05
  }

  private func startWorkoutNow() {
    segmentNumber = 1
    let startedAt = Date()
    session.start(now: startedAt) {
      ble.liveHeartRateBPM
    }
    rememberRecentActivity(session.selectedActivity)
    model.beginActivityRecording(activity: session.selectedActivity, startedAt: startedAt)
    if session.selectedActivity.usesGPS {
      locationTracker.start(reset: true)
    } else {
      locationTracker.stop()
      locationTracker.resetRoute()
    }
    WorkoutLiveActivityController.shared.start(
      activity: session.selectedActivity,
      session: session,
      heartRate: ble.liveHeartRateBPM,
      distanceMeters: locationTracker.distanceMeters
    )
    model.recordUIAction("activity.start", detail: session.selectedActivity.title)
  }

  private func rememberRecentActivity(_ activity: ActivityKind) {
    let existing = recentActivities.filter { $0 != activity }
    let updated = ([activity] + existing).prefix(5).map(\.rawValue)
    recentWorkoutRawValues = updated.joined(separator: ",")
  }

  private func endActivity() {
    countdownTimer?.invalidate()
    countdownTimer = nil
    segmentReturnTask?.cancel()
    segmentReturnTask = nil
    countdownValue = nil
    controlsLocked = false
    let endedAt = Date()
    session.end(now: endedAt, heartRate: ble.liveHeartRateBPM)
    locationTracker.stop()
    WorkoutLiveActivityController.shared.end(
      session: session,
      heartRate: ble.liveHeartRateBPM,
      distanceMeters: locationTracker.distanceMeters
    )
    model.finishActivityRecording(
      activity: session.selectedActivity,
      startedAt: session.startedAt,
      endedAt: endedAt,
      elapsed: session.elapsed,
      averageHeartRate: session.averageHeartRate,
      maxHeartRate: session.maxHeartRate,
      zoneDurations: session.zoneDurations,
      distanceMeters: locationTracker.distanceMeters,
      elevationGainMeters: locationTracker.elevationGainMeters,
      routePointCount: locationTracker.routePointCount
    )
    dockExpanded = false
    model.recordUIAction("activity.end", detail: "\(session.selectedActivity.title) \(formatDuration(session.elapsed))")
  }

  private func markSegment() {
    guard session.isActive else {
      return
    }
    let returnPage = selectedPage == .segment ? .overview : selectedPage
    segmentNumber += 1
    selectedPage = .segment
    withAnimation(.interactiveSpring(response: 0.44, dampingFraction: 0.9, blendDuration: 0.12)) {
      dockExpanded = false
    }
    UIImpactFeedbackGenerator(style: .heavy).impactOccurred()
    model.recordUIAction("activity.segment.marked", detail: "\(segmentNumber)")
    scheduleSegmentReturn(to: returnPage)
  }

  private func scheduleSegmentReturn(to page: FitnessWorkoutPage) {
    segmentReturnTask?.cancel()
    segmentReturnTask = Task {
      try? await Task.sleep(nanoseconds: 10_000_000_000)
      guard !Task.isCancelled else {
        return
      }
      await MainActor.run {
        guard selectedPage == .segment, session.isActive else {
          return
        }
        withAnimation(.easeInOut(duration: 0.28)) {
          selectedPage = page
        }
      }
    }
  }

  private func updateWorkoutLiveActivity(force: Bool = false) {
    WorkoutLiveActivityController.shared.update(
      session: session,
      heartRate: ble.liveHeartRateBPM,
      distanceMeters: locationTracker.distanceMeters,
      force: force
    )
  }

  private func lockControls() {
    controlsLocked = true
    withAnimation(.interactiveSpring(response: 0.44, dampingFraction: 0.9, blendDuration: 0.12)) {
      dockExpanded = false
    }
    UIImpactFeedbackGenerator(style: .medium).impactOccurred()
    if !lockHintSeen {
      lockHintSeen = true
      showingLockHint = true
    }
    model.recordUIAction("activity.controls.locked", detail: session.selectedActivity.title)
  }

  private func unlockControls() {
    controlsLocked = false
    UINotificationFeedbackGenerator().notificationOccurred(.success)
    model.recordUIAction("activity.controls.unlocked", detail: session.selectedActivity.title)
  }
}
