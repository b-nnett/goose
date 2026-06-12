import CoreLocation
import MapKit
import SwiftUI
import UIKit

struct FitnessLiveWorkoutView: View {
  @Binding var selectedPage: FitnessWorkoutPage
  let activity: ActivityKind
  @ObservedObject var session: ActivitySessionModel
  var ble: GooseBLEClient
  @ObservedObject var locationTracker: ActivityLocationTracker
  let segmentNumber: Int
  @Binding var dockExpanded: Bool
  @Binding var controlsLocked: Bool
  let onPrimaryAction: () -> Void
  let onEndWorkout: () -> Void
  let onStopViewing: () -> Void
  let onLockControls: () -> Void
  let onUnlockControls: () -> Void
  let onActivityTap: () -> Void
  let onSegmentTap: () -> Void
  let onHeartPageTap: () -> Void
  @GestureState private var dockDragTranslation: CGFloat = 0

  var body: some View {
    GeometryReader { proxy in
      let compactDockHeight = min(max(proxy.size.height * 0.30, 268), 306)
      let dockIsRaised = dockExpanded
      let expandedBottomOverflow = proxy.safeAreaInsets.bottom + 42
      let expandedDockHeight = proxy.size.height - 56 + expandedBottomOverflow
      let dockHeight = dockIsRaised ? expandedDockHeight : compactDockHeight
      let compactDockWidth = max(proxy.size.width - 24, 0)
      let dockWidth = dockIsRaised ? proxy.size.width : compactDockWidth
      let dockBaseOffsetY = dockIsRaised ? expandedBottomOverflow : proxy.safeAreaInsets.bottom - 20
      let dockDragOffsetY = controlsLocked ? 0 : constrainedDockDragOffset(isExpanded: dockIsRaised)

      ZStack(alignment: .bottom) {
        FitnessPageCarousel(
          selectedPage: $selectedPage,
          activity: activity,
          session: session,
          ble: ble,
          locationTracker: locationTracker,
          segmentNumber: segmentNumber
        )
        .padding(.bottom, compactDockHeight + 26)
        .allowsHitTesting(!controlsLocked)

        if dockIsRaised {
          Color.black.opacity(0.56)
            .ignoresSafeArea()
            .onTapGesture {
              collapseDock()
            }
        } else {
          FitnessPageDots(activity: activity, selectedPage: selectedPage)
            .padding(.bottom, compactDockHeight - 24)
        }

        FitnessControlDock(
          activity: activity,
          elapsed: session.elapsed,
          isActive: session.isActive,
          isPaused: session.isPaused,
          segmentNumber: segmentNumber,
          expanded: $dockExpanded,
          controlsLocked: controlsLocked,
          onPrimaryAction: onPrimaryAction,
          onEndWorkout: onEndWorkout,
          onStopViewing: onStopViewing,
          onLockControls: onLockControls,
          onUnlockControls: onUnlockControls,
          onActivityTap: onActivityTap,
          onSegmentTap: onSegmentTap,
          onHeartPageTap: onHeartPageTap
        )
        .frame(width: dockWidth, height: dockHeight)
        .offset(y: dockBaseOffsetY + dockDragOffsetY)
        .ignoresSafeArea(.container, edges: .bottom)
        .animation(dockAnimation, value: dockExpanded)
        .simultaneousGesture(
          DragGesture(minimumDistance: 12)
            .updating($dockDragTranslation) { value, state, _ in
              guard !controlsLocked else {
                return
              }
              state = value.translation.height
            }
            .onEnded { value in
              guard !controlsLocked else {
                return
              }

              let vertical = value.predictedEndTranslation.height
              if !dockExpanded && (vertical < -34 || value.translation.height < -64) {
                expandDock()
              } else if dockExpanded && (vertical > 34 || value.translation.height > 64) {
                collapseDock()
              }
            }
        )
      }
    }
  }

  private var dockAnimation: Animation {
    .interactiveSpring(response: 0.44, dampingFraction: 0.9, blendDuration: 0.12)
  }

  private func constrainedDockDragOffset(isExpanded: Bool) -> CGFloat {
    if isExpanded {
      return max(0, min(dockDragTranslation, 120))
    }
    return min(0, max(dockDragTranslation, -96))
  }

  private func expandDock() {
    withAnimation(dockAnimation) {
      dockExpanded = true
    }
  }

  private func collapseDock() {
    withAnimation(dockAnimation) {
      dockExpanded = false
    }
  }
}

struct FitnessPageCarousel: View {
  @Binding var selectedPage: FitnessWorkoutPage
  let activity: ActivityKind
  @ObservedObject var session: ActivitySessionModel
  var ble: GooseBLEClient
  @ObservedObject var locationTracker: ActivityLocationTracker
  let segmentNumber: Int

  var body: some View {
    TabView(selection: $selectedPage) {
      ForEach(pages) { page in
        pageContent(page)
          .tag(page)
      }
    }
    .tabViewStyle(.page(indexDisplayMode: .never))
  }

  private var pages: [FitnessWorkoutPage] {
    FitnessWorkoutPage.pages(for: activity)
  }

  @ViewBuilder
  private func pageContent(_ page: FitnessWorkoutPage) -> some View {
    switch page {
    case .overview:
      FitnessOverviewPage(
        activity: activity,
        currentHeartRate: ble.liveHeartRateBPM,
        averageHeartRate: session.averageHeartRate,
        elapsed: session.elapsed,
        distanceMeters: locationTracker.distanceMeters,
        currentPace: locationTracker.currentPaceSecondsPerKilometer,
        averagePace: averagePace
      )
    case .heartRate:
      FitnessHeartRatePage(
        currentHeartRate: ble.liveHeartRateBPM,
        averageHeartRate: session.averageHeartRate,
        zoneDurations: session.zoneDurations,
        elapsed: session.elapsed
      )
    case .segment:
      FitnessSegmentPage(
        title: "SEGMENT",
        number: segmentNumber,
        usesGPS: activity.usesGPS,
        elapsed: session.elapsed,
        distanceMeters: locationTracker.distanceMeters,
        currentHeartRate: ble.liveHeartRateBPM,
        currentPace: locationTracker.currentPaceSecondsPerKilometer
      )
    case .split:
      FitnessSplitPage(
        number: segmentNumber,
        usesGPS: activity.usesGPS,
        elapsed: session.elapsed,
        distanceMeters: locationTracker.distanceMeters,
        currentHeartRate: ble.liveHeartRateBPM,
        currentPace: locationTracker.currentPaceSecondsPerKilometer
      )
    case .elevation:
      FitnessElevationPage(
        elevationMeters: locationTracker.elevationMeters,
        elevationGainMeters: locationTracker.elevationGainMeters,
        currentPace: locationTracker.currentPaceSecondsPerKilometer
      )
    }
  }

  private var averagePace: TimeInterval? {
    guard locationTracker.distanceMeters > 5, session.elapsed > 0 else {
      return nil
    }
    return session.elapsed / (locationTracker.distanceMeters / 1000)
  }
}

struct FitnessOverviewPage: View {
  @Environment(GooseAppModel.self) private var model
  let activity: ActivityKind
  let currentHeartRate: Int?
  let averageHeartRate: Int?
  let elapsed: TimeInterval
  let distanceMeters: CLLocationDistance
  let currentPace: TimeInterval?
  let averagePace: TimeInterval?

  var body: some View {
    FitnessMetricPageLayout {
      VStack(alignment: .leading, spacing: 0) {
        FitnessHeartRateValue(currentHeartRate, size: 76)
          .padding(.top, 62)

        Spacer()

        if activity.usesGPS {
          FitnessPaceBlock(value: formatFitnessPace(currentPace), label: "ROLLING\nKM", color: .white)
            .padding(.bottom, 76)

          FitnessPaceBlock(value: formatFitnessPace(averagePace), label: "AVERAGE\nPACE", color: .white)
            .padding(.bottom, 88)

          let distance = fitnessDistanceParts(distanceMeters)
          FitnessNumberUnit(value: distance.value, unit: distance.unit, color: .white, size: 72, unitSize: 40)
            .padding(.bottom, 18)
        } else {
          FitnessPaceBlock(value: formatDuration(elapsed), label: "WORKOUT\nTIME", color: .white)
            .padding(.bottom, 76)

          FitnessPaceBlock(value: averageHeartRate.map(String.init) ?? "--", label: "AVERAGE\nHR", color: .white)
            .padding(.bottom, 88)

          if model.liveWorkoutStrain > 0 {
            FitnessPaceBlock(value: String(format: "%.1f", model.liveWorkoutStrain), label: "STRAIN", color: .white)
              .padding(.bottom, 76)
          }

          FitnessNumberUnit(value: "\(activeCalories)", unit: "KCAL", color: .white, size: 72, unitSize: 40)
            .padding(.bottom, 18)
        }
      }
    }
  }

  private var activeCalories: Int {
    max(Int(elapsed / 8), 0)
  }
}

struct FitnessHeartRatePage: View {
  let currentHeartRate: Int?
  let averageHeartRate: Int?
  let zoneDurations: [Int: TimeInterval]
  let elapsed: TimeInterval

  var body: some View {
    FitnessMetricPageLayout {
      VStack(spacing: 0) {
        FitnessHeartRateValue(currentHeartRate, size: 82, centered: true)
          .padding(.top, 76)

        Spacer()

        FitnessZoneRibbon(currentHeartRate: currentHeartRate)
          .padding(.bottom, 64)

        HStack(alignment: .top, spacing: 34) {
          VStack(alignment: .leading, spacing: 4) {
            Text(formatDuration(zoneDurations[HeartRateZone.zoneID(for: currentHeartRate ?? 0), default: 0]))
              .font(.system(size: 52, weight: .regular, design: .rounded))
              .foregroundStyle(.white)
              .lineLimit(1)
              .minimumScaleFactor(0.65)
            FitnessMetricLabel("TIME IN ZONE")
          }

          VStack(alignment: .leading, spacing: 4) {
            FitnessNumberUnit(
              value: averageHeartRate.map(String.init) ?? "--",
              unit: "BPM",
              color: .white,
              size: 52,
              unitSize: 28
            )
            FitnessMetricLabel("AVERAGE HR")
          }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.bottom, 24)
      }
    }
  }
}

struct FitnessSegmentPage: View {
  let title: String
  let number: Int
  let usesGPS: Bool
  let elapsed: TimeInterval
  let distanceMeters: CLLocationDistance
  let currentHeartRate: Int?
  let currentPace: TimeInterval?

  var body: some View {
    FitnessMetricPageLayout {
      VStack(alignment: .leading, spacing: 0) {
        HStack(alignment: .center, spacing: 18) {
          Text(formatDuration(elapsed))
            .font(.system(size: 72, weight: .regular, design: .rounded))
            .foregroundStyle(FitnessColor.segmentPink)
            .lineLimit(1)
            .minimumScaleFactor(0.65)
          FitnessSegmentBadge(number: number, size: 72)
        }
        .padding(.top, 74)

        Spacer()

        if usesGPS {
          FitnessPaceBlock(value: formatFitnessPace(currentPace), label: "\(title)\nPACE", color: .white)
            .padding(.bottom, 86)

          HStack(alignment: .lastTextBaseline, spacing: 12) {
            let distance = fitnessDistanceParts(distanceMeters)
            FitnessNumberUnit(value: distance.value, unit: distance.unit, color: .white, size: 72, unitSize: 40)
            FitnessMetricLabel(title)
              .padding(.bottom, 10)
          }
          .padding(.bottom, 70)
        } else {
          FitnessPaceBlock(value: "\(activeCalories)", label: "ACTIVE\nKCAL", color: .white)
            .padding(.bottom, 86)

          HStack(alignment: .lastTextBaseline, spacing: 12) {
            FitnessNumberUnit(value: currentHeartRate.map(String.init) ?? "--", unit: "BPM", color: .white, size: 72, unitSize: 40)
            FitnessMetricLabel("CURRENT HR")
              .padding(.bottom, 10)
          }
          .padding(.bottom, 70)
        }

        FitnessHeartRateValue(currentHeartRate, size: 70)
          .padding(.bottom, 18)
      }
    }
  }

  private var activeCalories: Int {
    max(Int(elapsed / 8), 0)
  }
}

struct FitnessSplitPage: View {
  let number: Int
  let usesGPS: Bool
  let elapsed: TimeInterval
  let distanceMeters: CLLocationDistance
  let currentHeartRate: Int?
  let currentPace: TimeInterval?

  var body: some View {
    FitnessMetricPageLayout {
      VStack(alignment: .leading, spacing: 0) {
        HStack(alignment: .firstTextBaseline, spacing: 16) {
          Text(formatDuration(elapsed))
            .font(.system(size: 72, weight: .regular, design: .rounded))
            .foregroundStyle(FitnessColor.segmentPink)
            .lineLimit(1)
            .minimumScaleFactor(0.65)
          VStack(alignment: .leading, spacing: 0) {
            FitnessMetricLabel("SPLIT")
            Text("\(number)")
              .font(.system(size: 22, weight: .bold, design: .rounded))
              .foregroundStyle(FitnessColor.secondaryText)
          }
        }
        .padding(.top, 74)

        Spacer()

        if usesGPS {
          FitnessPaceBlock(value: formatFitnessPace(currentPace), label: "SPLIT\nPACE", color: .white)
            .padding(.bottom, 86)

          HStack(alignment: .lastTextBaseline, spacing: 12) {
            let distance = fitnessDistanceParts(distanceMeters)
            FitnessNumberUnit(value: distance.value, unit: distance.unit, color: .white, size: 72, unitSize: 40)
            FitnessMetricLabel("SPLIT")
              .padding(.bottom, 10)
          }
          .padding(.bottom, 70)
        } else {
          FitnessPaceBlock(value: "\(activeCalories)", label: "ACTIVE\nKCAL", color: .white)
            .padding(.bottom, 86)

          HStack(alignment: .lastTextBaseline, spacing: 12) {
            FitnessNumberUnit(value: currentHeartRate.map(String.init) ?? "--", unit: "BPM", color: .white, size: 72, unitSize: 40)
            FitnessMetricLabel("CURRENT HR")
              .padding(.bottom, 10)
          }
          .padding(.bottom, 70)
        }

        FitnessHeartRateValue(currentHeartRate, size: 70)
          .padding(.bottom, 18)
      }
    }
  }

  private var activeCalories: Int {
    max(Int(elapsed / 8), 0)
  }
}

struct FitnessElevationPage: View {
  let elevationMeters: CLLocationDistance
  let elevationGainMeters: CLLocationDistance
  let currentPace: TimeInterval?

  var body: some View {
    FitnessMetricPageLayout {
      VStack(spacing: 0) {
        FitnessNumberUnit(
          value: "\(Int(max(elevationGainMeters, 0).rounded()))",
          unit: "M",
          color: FitnessColor.exerciseGreen,
          size: 64,
          unitSize: 34
        )
        .padding(.top, 58)
        FitnessMetricLabel("ELEVATION GAINED")
          .foregroundStyle(FitnessColor.exerciseGreen)
          .padding(.top, 6)

        FitnessElevationChart()
          .frame(height: 232)
          .padding(.top, 26)

        Spacer()

        HStack(alignment: .bottom, spacing: 36) {
          VStack(alignment: .leading, spacing: 4) {
            FitnessNumberUnit(
              value: "\(Int(max(elevationMeters, 0).rounded()))",
              unit: "M",
              color: .white,
              size: 52,
              unitSize: 30
            )
            FitnessMetricLabel("ELEVATION")
          }
          .frame(maxWidth: .infinity, alignment: .leading)

          VStack(alignment: .leading, spacing: 4) {
            Text(formatFitnessPace(currentPace))
              .font(.system(size: 38, weight: .regular, design: .rounded))
              .foregroundStyle(.white)
              .lineLimit(1)
              .minimumScaleFactor(0.72)
            FitnessMetricLabel("CURRENT PACE")
          }
          .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.bottom, 8)
      }
    }
  }
}

struct FitnessRingsPage: View {
  let elapsed: TimeInterval

  var body: some View {
    FitnessMetricPageLayout {
      VStack(alignment: .leading, spacing: 0) {
        ActivityRingsView(
          moveProgress: 0.49,
          exerciseProgress: min(max(elapsed / 1800, 0.18), 1.0),
          standProgress: 0.75,
          lineWidth: 28
        )
        .frame(width: 292, height: 292)
        .frame(maxWidth: .infinity)
        .padding(.top, 50)
        .padding(.bottom, 48)

        VStack(alignment: .leading, spacing: 40) {
          VStack(alignment: .leading, spacing: 0) {
            Text("\(activeCalories)/940")
              .font(.system(size: 58, weight: .regular, design: .rounded))
              .foregroundStyle(FitnessColor.movePink)
              .lineLimit(1)
              .minimumScaleFactor(0.7)
            FitnessMetricLabel("MOVE")
          }

          HStack(alignment: .top, spacing: 42) {
            VStack(alignment: .leading, spacing: 0) {
              Text("\(exerciseMinutes)/30")
                .font(.system(size: 52, weight: .regular, design: .rounded))
                .foregroundStyle(FitnessColor.exerciseGreen)
                .lineLimit(1)
                .minimumScaleFactor(0.75)
              FitnessMetricLabel("EXERCISE")
            }

            VStack(alignment: .leading, spacing: 0) {
              Text("\(standHours)/12")
                .font(.system(size: 52, weight: .regular, design: .rounded))
                .foregroundStyle(FitnessColor.standCyan)
                .lineLimit(1)
                .minimumScaleFactor(0.75)
              FitnessMetricLabel("STAND")
            }
          }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
      }
    }
  }

  private var exerciseMinutes: Int {
    max(Int(elapsed / 60), 0)
  }

  private var activeCalories: Int {
    max(Int(elapsed / 8), 0)
  }

  private var standHours: Int {
    min(12, max(1, Int(elapsed / 3600) + 9))
  }
}

