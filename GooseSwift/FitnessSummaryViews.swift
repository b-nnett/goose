import CoreLocation
import MapKit
import SwiftUI
import UIKit

struct FitnessSummaryView: View {
  let activity: ActivityKind
  @ObservedObject var session: ActivitySessionModel
  var ble: GooseBLEClient
  @ObservedObject var locationTracker: ActivityLocationTracker
  let onDone: () -> Void

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 28) {
        HStack(spacing: 24) {
          FitnessWorkoutIcon(activity: activity, size: 88, backgroundOpacity: 0.34)

          VStack(alignment: .leading, spacing: 6) {
            Text(activity.fitnessTitle)
              .font(.system(size: 24, weight: .regular, design: .rounded))
              .foregroundStyle(.white)
            Text(summaryTimeRange)
              .font(.system(size: 22, weight: .regular, design: .rounded))
              .foregroundStyle(FitnessColor.secondaryText)
            Label(summaryLocationText, systemImage: summaryLocationIcon)
              .font(.system(size: 22, weight: .regular, design: .rounded))
              .foregroundStyle(FitnessColor.secondaryText)
          }
        }

        VStack(alignment: .leading, spacing: 14) {
          HStack(spacing: 8) {
            Text("Workout Details")
              .font(.system(size: 30, weight: .bold, design: .rounded))
              .foregroundStyle(.white)
            Image(systemName: "chevron.right")
              .font(.system(size: 24, weight: .bold))
              .foregroundStyle(FitnessColor.secondaryText)
          }

          FitnessWorkoutDetailsCard(
            workoutTime: formatDuration(session.elapsed),
            elapsedTime: formatDuration(session.elapsed + 12),
            activeCalories: "\(activeCalories)KCAL",
            totalCalories: "\(activeCalories + 2)KCAL",
            detailMetricTitle: detailMetricTitle,
            detailMetricValue: detailMetricValue,
            averageHeartRate: averageHeartRateText
          )
        }

        if activity.usesGPS {
          FitnessRouteSummaryCard(activity: activity, locationTracker: locationTracker)
            .padding(.top, 4)
        }
      }
      .padding(.horizontal, 18)
      .padding(.top, 22)
      .padding(.bottom, 48)
    }
    .background(FitnessColor.background)
    .toolbar {
      ToolbarItem(placement: .topBarTrailing) {
        Button(action: onDone) {
          Image(systemName: "checkmark")
            .font(.system(size: 18, weight: .bold))
            .foregroundStyle(FitnessColor.lime)
        }
      }
    }
  }

  private var activeCalories: Int {
    max(Int(session.elapsed / 8), 0)
  }

  private var averagePaceText: String {
    guard locationTracker.distanceMeters > 5, session.elapsed > 0 else {
      return "--'--\"/KM"
    }
    return "\(formatFitnessPace(session.elapsed / (locationTracker.distanceMeters / 1000)))/KM"
  }

  private var detailMetricTitle: String {
    activity.usesGPS ? "Avg Pace" : "Peak Heart Rate"
  }

  private var detailMetricValue: String {
    activity.usesGPS ? averagePaceText : peakHeartRateText
  }

  private var averageHeartRateText: String {
    guard let heartRate = session.averageHeartRate ?? ble.liveHeartRateBPM else {
      return "--BPM"
    }
    return "\(heartRate)BPM"
  }

  private var peakHeartRateText: String {
    guard let heartRate = session.maxHeartRate ?? ble.liveHeartRateBPM else {
      return "--BPM"
    }
    return "\(heartRate)BPM"
  }

  private var summaryTimeRange: String {
    let formatter = DateFormatter()
    formatter.dateFormat = "HH:mm"
    let start = session.startedAt ?? Date()
    let end = session.endedAt ?? Date()
    return "\(formatter.string(from: start))-\(formatter.string(from: end))"
  }

  private var summaryLocationText: String {
    switch activity.environment {
    case .outdoor: "Outdoor"
    case .indoor: "Indoor"
    case .pool: "Pool"
    }
  }

  private var summaryLocationIcon: String {
    switch activity.environment {
    case .outdoor: "location.fill"
    case .indoor: "house.fill"
    case .pool: "drop.fill"
    }
  }
}

struct FitnessWorkoutDetailsCard: View {
  let workoutTime: String
  let elapsedTime: String
  let activeCalories: String
  let totalCalories: String
  let detailMetricTitle: String
  let detailMetricValue: String
  let averageHeartRate: String

  var body: some View {
    VStack(spacing: 0) {
      HStack(spacing: 24) {
        FitnessSummaryMetric(title: "Workout Time", value: workoutTime, color: FitnessColor.workoutYellow)
        FitnessSummaryMetric(title: "Elapsed Time", value: elapsedTime, color: FitnessColor.workoutYellow)
      }
      .padding(.bottom, 22)

      Divider().background(FitnessColor.separator)

      HStack(spacing: 24) {
        FitnessSummaryMetric(title: "Active Kilocalories", value: activeCalories, color: FitnessColor.movePink)
        FitnessSummaryMetric(title: "Total Kilocalories", value: totalCalories, color: FitnessColor.movePink)
      }
      .padding(.vertical, 22)

      Divider().background(FitnessColor.separator)

      HStack(spacing: 24) {
        FitnessSummaryMetric(title: detailMetricTitle, value: detailMetricValue, color: FitnessColor.standCyan)
        FitnessSummaryMetric(title: "Avg Heart Rate", value: averageHeartRate, color: FitnessColor.heartRed)
      }
      .padding(.top, 22)
    }
    .padding(18)
    .background(FitnessColor.panel, in: RoundedRectangle(cornerRadius: 30, style: .continuous))
  }
}

struct FitnessSummaryMetric: View {
  let title: String
  let value: String
  let color: Color

  var body: some View {
    VStack(alignment: .leading, spacing: 4) {
      Text(title)
        .font(.system(size: 20, weight: .regular, design: .rounded))
        .foregroundStyle(.white)
        .lineLimit(1)
        .minimumScaleFactor(0.7)
      Text(value)
        .font(.system(size: 28, weight: .bold, design: .rounded))
        .foregroundStyle(color)
        .lineLimit(1)
        .minimumScaleFactor(0.62)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }
}

struct FitnessRouteSummaryCard: View {
  let activity: ActivityKind
  @ObservedObject var locationTracker: ActivityLocationTracker
  @State private var cameraPosition: MapCameraPosition = .userLocation(fallback: .automatic)

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      Text("Route")
        .font(.system(size: 24, weight: .bold, design: .rounded))
        .foregroundStyle(.white)

      Map(position: $cameraPosition) {
        UserAnnotation()
        ForEach(locationTracker.routeSegments(for: activity)) { segment in
          MapPolyline(coordinates: segment.coordinates)
            .stroke(segment.zone.color, lineWidth: 5)
        }
      }
      .mapStyle(.standard(elevation: .realistic))
      .frame(height: 220)
      .clipShape(RoundedRectangle(cornerRadius: 30, style: .continuous))
    }
  }
}

struct FitnessPaceBlock: View {
  let value: String
  let label: String
  let color: Color

  var body: some View {
    HStack(alignment: .center, spacing: 30) {
      Text(value)
        .font(.system(size: 48, weight: .regular, design: .rounded))
        .foregroundStyle(color)
        .lineLimit(1)
        .minimumScaleFactor(0.72)
      FitnessMetricLabel(label)
        .frame(width: 120, alignment: .leading)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }
}

struct FitnessZoneRibbon: View {
  let currentHeartRate: Int?

  var body: some View {
    GeometryReader { proxy in
      let spacing: CGFloat = 4
      let selectedWidth = min(max(proxy.size.width * 0.40, 134), 162)
      let inactiveWidth = max((proxy.size.width - selectedWidth - spacing * 4) / 4, 42)
      let inactiveHeight: CGFloat = 70
      let selectedHeight = 92 + nextZoneProgress * 96

      ZStack(alignment: .bottomLeading) {
        HStack(alignment: .bottom, spacing: spacing) {
          ForEach(HeartRateZone.zones) { zone in
            let selected = zone.id == selectedZone
            RoundedRectangle(cornerRadius: 9, style: .continuous)
              .fill(zoneColor(zone.id).opacity(selected ? 1 : 0.42))
              .frame(width: selected ? selectedWidth : inactiveWidth, height: selected ? selectedHeight : inactiveHeight)
              .overlay(alignment: .bottomLeading) {
                if selected {
                  HStack(spacing: 6) {
                    Image(systemName: "heart.fill")
                      .font(.system(size: 17, weight: .bold))
                    Text("ZONE \(zone.id)")
                      .font(.system(size: 20, weight: .heavy, design: .rounded))
                      .lineLimit(1)
                      .minimumScaleFactor(0.62)
                  }
                  .foregroundStyle(.black)
                  .padding(.horizontal, 12)
                  .padding(.bottom, 18)
                }
              }
          }
        }

        Triangle()
          .fill(.white)
          .frame(width: 22, height: 16)
          .offset(x: CGFloat(selectedZone - 1) * (inactiveWidth + spacing) + 16, y: 14)
      }
      .frame(width: proxy.size.width, height: proxy.size.height, alignment: .bottomLeading)
    }
    .frame(height: 190)
    .animation(.spring(response: 0.28, dampingFraction: 0.82), value: selectedZone)
    .animation(.spring(response: 0.28, dampingFraction: 0.82), value: nextZoneProgress)
  }

  private var selectedZone: Int {
    guard let currentHeartRate else {
      return 1
    }
    return HeartRateZone.zoneID(for: currentHeartRate)
  }

  private var nextZoneProgress: CGFloat {
    guard let currentHeartRate else {
      return 0.2
    }

    let bpm = CGFloat(currentHeartRate)
    let maxHeartRate = CGFloat(HeartRateZone.maxHeartRate)
    let lower: CGFloat
    let upper: CGFloat

    switch selectedZone {
    case 1:
      lower = 0
      upper = maxHeartRate * 0.60
    case 2:
      lower = maxHeartRate * 0.60
      upper = maxHeartRate * 0.70
    case 3:
      lower = maxHeartRate * 0.70
      upper = maxHeartRate * 0.80
    case 4:
      lower = maxHeartRate * 0.80
      upper = maxHeartRate * 0.90
    default:
      lower = maxHeartRate * 0.90
      upper = maxHeartRate
    }

    return min(max((bpm - lower) / max(upper - lower, 1), 0), 1)
  }

  private func zoneColor(_ id: Int) -> Color {
    switch id {
    case 1: FitnessColor.zoneBlue
    case 2: FitnessColor.zoneTeal
    case 3: FitnessColor.zoneGreen
    case 4: FitnessColor.zoneOrange
    default: FitnessColor.zoneRed
    }
  }
}

struct Triangle: Shape {
  func path(in rect: CGRect) -> Path {
    var path = Path()
    path.move(to: CGPoint(x: rect.midX, y: rect.minY))
    path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY))
    path.addLine(to: CGPoint(x: rect.minX, y: rect.maxY))
    path.closeSubpath()
    return path
  }
}

struct FitnessElevationChart: View {
  var body: some View {
    ZStack {
      VStack {
        Text("10")
          .frame(maxWidth: .infinity, alignment: .trailing)
        Spacer()
        Text("0")
          .frame(maxWidth: .infinity, alignment: .trailing)
      }
      .font(.system(size: 16, weight: .bold, design: .rounded))
      .foregroundStyle(FitnessColor.secondaryText)

      HStack {
        Text("30 MIN AGO")
        Spacer()
        Text("NOW")
      }
      .font(.system(size: 16, weight: .bold, design: .rounded))
      .foregroundStyle(FitnessColor.secondaryText)
      .frame(maxHeight: .infinity, alignment: .bottom)
    }
  }
}

struct ActivityRingsView: View {
  let moveProgress: Double
  let exerciseProgress: Double
  let standProgress: Double
  let lineWidth: CGFloat

  var body: some View {
    GeometryReader { proxy in
      let size = min(proxy.size.width, proxy.size.height)
      ZStack {
        FitnessRing(progress: moveProgress, color: FitnessColor.movePink, lineWidth: lineWidth, inset: 0)
        FitnessRing(progress: exerciseProgress, color: FitnessColor.exerciseGreen, lineWidth: lineWidth, inset: lineWidth * 1.42)
        FitnessRing(progress: standProgress, color: FitnessColor.standCyan, lineWidth: lineWidth, inset: lineWidth * 2.84)
      }
      .frame(width: size, height: size)
      .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
  }
}

struct FitnessRing: View {
  let progress: Double
  let color: Color
  let lineWidth: CGFloat
  let inset: CGFloat

  var body: some View {
    ZStack {
      Circle()
        .inset(by: inset)
        .stroke(color.opacity(0.22), style: StrokeStyle(lineWidth: lineWidth, lineCap: .round))
      Circle()
        .inset(by: inset)
        .trim(from: 0, to: min(max(progress, 0), 1))
        .stroke(color, style: StrokeStyle(lineWidth: lineWidth, lineCap: .round))
        .rotationEffect(.degrees(-90))
    }
  }
}

struct FitnessWorkoutIcon: View {
  let activity: ActivityKind
  let size: CGFloat
  let backgroundOpacity: Double

  var body: some View {
    Image(systemName: activity.systemImage)
      .font(.system(size: size * 0.48, weight: .semibold))
      .foregroundStyle(FitnessColor.exerciseGreen)
      .frame(width: size, height: size)
      .background(FitnessColor.exerciseGreen.opacity(backgroundOpacity), in: Circle())
  }
}

struct FitnessSegmentBadge: View {
  let number: Int
  let size: CGFloat

  var body: some View {
    Text("\(number)")
      .font(.system(size: size * 0.48, weight: .regular, design: .rounded))
      .foregroundStyle(.white)
      .frame(width: size, height: size)
      .overlay {
        Circle()
          .stroke(.white, lineWidth: max(3, size * 0.06))
      }
  }
}

struct FitnessHeartRateValue: View {
  let value: Int?
  let size: CGFloat
  let centered: Bool

  init(_ value: Int?, size: CGFloat, centered: Bool = false) {
    self.value = value
    self.size = size
    self.centered = centered
  }

  var body: some View {
    HStack(alignment: .lastTextBaseline, spacing: 8) {
      Text(value.map(String.init) ?? "--")
        .font(.system(size: size, weight: .regular, design: .rounded))
        .foregroundStyle(.white)
        .lineLimit(1)
        .minimumScaleFactor(0.6)
      Image(systemName: "heart.fill")
        .font(.system(size: size * 0.34, weight: .bold))
        .foregroundStyle(FitnessColor.heartRed)
        .baselineOffset(size * 0.06)
    }
    .frame(maxWidth: .infinity, alignment: centered ? .center : .leading)
  }
}

struct FitnessNumberUnit: View {
  let value: String
  let unit: String
  let color: Color
  let size: CGFloat
  let unitSize: CGFloat

  var body: some View {
    HStack(alignment: .lastTextBaseline, spacing: 4) {
      Text(value)
        .font(.system(size: size, weight: .regular, design: .rounded))
      Text(unit)
        .font(.system(size: unitSize, weight: .semibold, design: .rounded))
        .baselineOffset(size * 0.03)
    }
    .foregroundStyle(color)
    .lineLimit(1)
    .minimumScaleFactor(0.64)
  }
}

struct FitnessMetricLabel: View {
  let text: String

  init(_ text: String) {
    self.text = text
  }

  var body: some View {
    Text(text)
      .font(.system(size: 18, weight: .heavy, design: .rounded))
      .foregroundStyle(FitnessColor.secondaryText)
      .lineLimit(2)
      .multilineTextAlignment(.leading)
      .minimumScaleFactor(0.75)
  }
}

