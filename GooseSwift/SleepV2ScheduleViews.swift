import Darwin
import Foundation
import SwiftUI
import UIKit

struct SleepV2SleepWindowCard: View {
  let palette: SleepV2Palette
  let onWakeTap: () -> Void
  let onSleepNeeded: () -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 18) {
      HStack(alignment: .top) {
        VStack(alignment: .leading, spacing: 4) {
          Text("Sleep schedule")
            .font(.title3.weight(.semibold))
            .foregroundStyle(palette.text)
          Text("Tonight")
            .font(.subheadline.weight(.medium))
            .foregroundStyle(palette.secondaryText)
        }
        Spacer()
        Image(systemName: "moon.zzz.fill")
          .font(.headline.weight(.semibold))
          .foregroundStyle(palette.accent)
          .frame(width: 34, height: 34)
          .background(palette.accent.opacity(0.12), in: Circle())
      }

      HStack(spacing: 10) {
        SleepV2ScheduleTimeTile(
          palette: palette,
          systemImage: "wind",
          title: "Wind down",
          value: "21:20"
        )
        SleepV2ScheduleTimeTile(
          palette: palette,
          systemImage: "bed.double.fill",
          title: "Target bedtime",
          value: "21:50"
        )
      }

      SleepV2ScheduleTimeline(palette: palette)

      VStack(spacing: 0) {
        SleepV2ScheduleActionRow(
          palette: palette,
          systemImage: "moon.stars.fill",
          title: "Tonight's sleep needed",
          value: "7h 39m",
          action: onSleepNeeded
        )

        Divider()
          .overlay(palette.separator)
          .padding(.leading, 46)

        SleepV2ScheduleActionRow(
          palette: palette,
          systemImage: "alarm.fill",
          title: "Wake up at",
          value: "05:30",
          action: onWakeTap
        )
      }
      .background(palette.surfaceElevated.opacity(0.48), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
    }
    .padding(20)
    .background(
      RoundedRectangle(cornerRadius: 28, style: .continuous)
        .fill(palette.surface)
        .shadow(color: palette.shadow.opacity(0.42), radius: 12, x: 0, y: 5)
    )
    .overlay(
      RoundedRectangle(cornerRadius: 28, style: .continuous)
        .stroke(palette.separator.opacity(0.70), lineWidth: 1)
    )
    .clipShape(RoundedRectangle(cornerRadius: 28, style: .continuous))
  }
}

struct SleepV2BandSyncCard: View {
  var store: HealthDataStore
  var ble: GooseBLEClient
  let palette: SleepV2Palette
  let onSync: () -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      HStack(alignment: .top, spacing: 12) {
        Image(systemName: "antenna.radiowaves.left.and.right")
          .font(.headline.weight(.semibold))
          .foregroundStyle(palette.accent)
          .frame(width: 36, height: 36)
          .background(palette.accent.opacity(0.12), in: Circle())

        VStack(alignment: .leading, spacing: 4) {
          Text("Band sync")
            .font(.title3.weight(.semibold))
            .foregroundStyle(palette.text)
          Text(syncSubtitle)
            .font(.subheadline.weight(.medium))
            .foregroundStyle(palette.secondaryText)
            .fixedSize(horizontal: false, vertical: true)
        }

        Spacer(minLength: 8)
      }

      VStack(spacing: 0) {
        SleepV2BandSyncRow(
          palette: palette,
          title: "History",
          value: ble.historicalSyncStatus.localizedHistoricalSyncStatus,
          systemImage: "arrow.triangle.2.circlepath"
        )
        Divider().overlay(palette.separator).padding(.leading, 42)
        SleepV2BandSyncRow(
          palette: palette,
          title: "Packets",
          value: packetText,
          systemImage: "square.stack.3d.up"
        )
        Divider().overlay(palette.separator).padding(.leading, 42)
        SleepV2BandSyncRow(
          palette: palette,
          title: "Sleep score",
          value: store.bandSleepImportStatus,
          systemImage: "bed.double.fill"
        )
      }

      HStack(spacing: 10) {
        Button(action: onSync) {
          Label("Sync from band", systemImage: "arrow.down.circle.fill")
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.borderedProminent)
        .disabled(!ble.canSyncHistorical)

        Button {
          store.refreshSleepAfterBandSync(packetCount: ble.historicalPacketCount)
        } label: {
          Label("Refresh score", systemImage: "chart.xyaxis.line")
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.bordered)
      }
      .font(.subheadline.weight(.semibold))
    }
    .padding(20)
    .background(
      RoundedRectangle(cornerRadius: 28, style: .continuous)
        .fill(palette.surface)
        .shadow(color: palette.shadow.opacity(0.36), radius: 10, x: 0, y: 4)
    )
    .overlay(
      RoundedRectangle(cornerRadius: 28, style: .continuous)
        .stroke(palette.separator.opacity(0.70), lineWidth: 1)
    )
  }

  private var syncSubtitle: String {
    if ble.canSyncHistorical {
      return "Pulls overnight packets from the connected band, then recomputes sleep locally."
    }
    return "Connect the band and wait for ready state to pull overnight packets."
  }

  private var packetText: String {
    ble.historicalPacketCount == 1 ? "1 packet" : "\(ble.historicalPacketCount) packets"
  }
}

struct SleepV2BandSyncRow: View {
  let palette: SleepV2Palette
  let title: String
  let value: String
  let systemImage: String

  var body: some View {
    HStack(alignment: .top, spacing: 10) {
      Image(systemName: systemImage)
        .font(.caption.weight(.semibold))
        .foregroundStyle(palette.accent)
        .frame(width: 32, height: 32)
        .background(palette.accent.opacity(0.10), in: Circle())
      VStack(alignment: .leading, spacing: 3) {
        Text(title)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(palette.text)
        Text(value)
          .font(.caption.weight(.medium))
          .foregroundStyle(palette.secondaryText)
          .lineLimit(2)
          .minimumScaleFactor(0.74)
      }
      Spacer(minLength: 0)
    }
    .padding(.vertical, 11)
  }
}

struct SleepV2ScheduleTimeTile: View {
  let palette: SleepV2Palette
  let systemImage: String
  let title: String
  let value: String

  var body: some View {
    HStack(alignment: .top, spacing: 10) {
      Image(systemName: systemImage)
        .font(.caption.weight(.semibold))
        .foregroundStyle(palette.accent)
        .frame(width: 28, height: 28)
        .background(palette.accent.opacity(0.12), in: Circle())
      VStack(alignment: .leading, spacing: 4) {
        Text(title)
          .font(.caption.weight(.semibold))
          .foregroundStyle(palette.secondaryText)
        Text(value)
          .font(.title3.weight(.semibold))
          .fontDesign(.rounded)
          .foregroundStyle(palette.text)
      }
      Spacer(minLength: 0)
    }
    .padding(12)
    .frame(maxWidth: .infinity, alignment: .leading)
    .background(palette.surfaceElevated.opacity(0.48), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
  }
}

struct SleepV2ScheduleTimeline: View {
  let palette: SleepV2Palette

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      HStack {
        Text("21:20")
        Spacer()
        Text("21:50")
        Spacer()
        Text("05:30")
      }
      .font(.caption.weight(.semibold))
      .fontDesign(.rounded)
      .foregroundStyle(palette.text)

      GeometryReader { proxy in
        let width = proxy.size.width
        ZStack(alignment: .leading) {
          Capsule()
            .fill(palette.separator.opacity(0.8))
            .frame(height: 5)
          Capsule()
            .fill(palette.accent.opacity(0.30))
            .frame(width: width * 0.18, height: 5)
            .offset(x: width * 0.10)
          Capsule()
            .fill(palette.accent)
            .frame(width: width * 0.58, height: 5)
            .offset(x: width * 0.28)
          scheduleDot(x: width * 0.10, filled: false)
          scheduleDot(x: width * 0.28, filled: true)
          scheduleDot(x: width * 0.86, filled: true)
        }
      }
      .frame(height: 18)

      HStack {
        Text("Wind down")
        Spacer()
        Text("Sleep")
        Spacer()
        Text("Wake")
      }
      .font(.caption2.weight(.semibold))
      .foregroundStyle(palette.secondaryText)
    }
    .padding(14)
    .background(palette.surfaceElevated.opacity(0.48), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
  }

  private func scheduleDot(x: CGFloat, filled: Bool) -> some View {
    Circle()
      .fill(filled ? palette.accent : palette.surface)
      .frame(width: 13, height: 13)
      .overlay(Circle().stroke(palette.accent, lineWidth: 2))
      .position(x: x, y: 9)
  }
}

struct SleepV2ScheduleActionRow: View {
  let palette: SleepV2Palette
  let systemImage: String
  let title: String
  let value: String
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      HStack(spacing: 12) {
        Image(systemName: systemImage)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(palette.accent)
          .frame(width: 24, height: 24)
        Text(title)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(palette.text)
          .lineLimit(1)
          .minimumScaleFactor(0.72)
        Spacer(minLength: 8)
        Text(value)
          .font(.subheadline.weight(.semibold))
          .fontDesign(.rounded)
          .foregroundStyle(palette.text)
        Image(systemName: "chevron.right")
          .font(.caption.weight(.bold))
          .foregroundStyle(palette.mutedText)
      }
      .padding(.horizontal, 14)
      .padding(.vertical, 13)
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
  }
}

struct SleepV2ClockDial: View {
  let palette: SleepV2Palette

  var body: some View {
    GeometryReader { proxy in
      let side = min(proxy.size.width, proxy.size.height)
      let ringWidth = max(14, side * 0.075)
      ZStack {
        Circle()
          .fill(palette.surfaceElevated.opacity(palette.light ? 0.55 : 0.36))
        Circle()
          .stroke(palette.separator.opacity(0.75), lineWidth: ringWidth)
          .padding(ringWidth)
        Circle()
          .trim(from: 0.58, to: 0.64)
          .stroke(
            palette.accent.opacity(0.38),
            style: StrokeStyle(lineWidth: ringWidth, lineCap: .round)
          )
          .rotationEffect(.degrees(-90))
          .padding(ringWidth)
        Circle()
          .trim(from: 0.65, to: 0.92)
          .stroke(
            palette.accent,
            style: StrokeStyle(lineWidth: ringWidth, lineCap: .round)
          )
          .rotationEffect(.degrees(-90))
          .padding(ringWidth)

        VStack(spacing: 6) {
          Image(systemName: "moon.stars.fill")
            .font(.title3.weight(.semibold))
            .foregroundStyle(palette.accent)
          Text("7h 39m")
            .font(.title2.weight(.semibold))
            .fontDesign(.rounded)
            .foregroundStyle(palette.text)
          Text("sleep needed")
            .font(.caption.weight(.semibold))
            .foregroundStyle(palette.secondaryText)
        }

        SleepV2ClockBubble(palette: palette, systemImage: "bed.double.fill", active: true)
          .position(x: side * 0.28, y: side * 0.72)
        SleepV2ClockBubble(palette: palette, systemImage: "alarm.fill", active: true)
          .position(x: side * 0.74, y: side * 0.36)
      }
      .frame(width: side, height: side)
      .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
  }
}

enum SleepV2ClockDrawing {
  static func draw(context: inout GraphicsContext, size: CGSize, palette: SleepV2Palette) {
    let center = CGPoint(x: size.width / 2, y: size.height / 2)
    let radius = min(size.width, size.height) / 2
    let outerRadius = radius - 8
    let arcRadius = outerRadius - 15

    fillCircle(context: &context, center: center, radius: outerRadius, color: palette.shadow.opacity(palette.light ? 0.50 : 1.0))
    fillCircle(context: &context, center: center, radius: outerRadius - 13, color: palette.surfaceElevated)
    fillCircle(context: &context, center: center, radius: outerRadius - 42, color: palette.background.opacity(palette.light ? 0.62 : 0.40))

    context.fill(
      arc(center: center, radius: arcRadius, start: -.pi * 1.06, end: .pi * 0.94, width: 27),
      with: .color(palette.accent.opacity(palette.light ? 0.20 : 0.24))
    )
    context.fill(
      arc(center: center, radius: arcRadius, start: -.pi * 0.98, end: -.pi * 0.76, width: 27),
      with: .color(palette.accentAlt.opacity(palette.light ? 0.26 : 0.30))
    )
    context.fill(
      arc(center: center, radius: arcRadius, start: -.pi * 0.77, end: .pi * 0.01, width: 27),
      with: .linearGradient(
        Gradient(colors: [
          palette.accentAlt.opacity(0.64),
          palette.accentAlt.opacity(0.90),
          palette.accent.opacity(0.74),
        ]),
        startPoint: CGPoint(x: center.x - arcRadius, y: center.y),
        endPoint: CGPoint(x: center.x + arcRadius, y: center.y)
      )
    )

    drawTicks(context: &context, center: center, outerRadius: outerRadius, palette: palette)
    drawLabels(context: &context, center: center, palette: palette)
  }

  private static func fillCircle(
    context: inout GraphicsContext,
    center: CGPoint,
    radius: CGFloat,
    color: Color
  ) {
    let diameter = radius * 2
    let rect = CGRect(
      x: center.x - radius,
      y: center.y - radius,
      width: diameter,
      height: diameter
    )
    context.fill(Path(ellipseIn: rect), with: .color(color))
  }

  private static func arc(
    center: CGPoint,
    radius: CGFloat,
    start: Double,
    end: Double,
    width: CGFloat
  ) -> Path {
    var path = Path()
    path.addArc(
      center: center,
      radius: radius,
      startAngle: .radians(start),
      endAngle: .radians(end),
      clockwise: false
    )
    return path.strokedPath(StrokeStyle(lineWidth: width, lineCap: .round))
  }

  private static func drawTicks(
    context: inout GraphicsContext,
    center: CGPoint,
    outerRadius: CGFloat,
    palette: SleepV2Palette
  ) {
    for index in 0..<96 {
      let angle = -.pi / 2 + (.pi * 2 * Double(index) / 96)
      let major = index % 8 == 0
      let inner = outerRadius - (major ? 67 : 58)
      let outer = outerRadius - 51
      let cosine = CGFloat(Darwin.cos(angle))
      let sine = CGFloat(Darwin.sin(angle))
      let p1 = CGPoint(x: center.x + cosine * inner, y: center.y + sine * inner)
      let p2 = CGPoint(x: center.x + cosine * outer, y: center.y + sine * outer)
      var tick = Path()
      tick.move(to: p1)
      tick.addLine(to: p2)
      context.stroke(
        tick,
        with: .color(palette.mutedText.opacity(0.58)),
        style: StrokeStyle(lineWidth: 1.4, lineCap: .round)
      )
    }
  }

  private static func drawLabels(
    context: inout GraphicsContext,
    center: CGPoint,
    palette: SleepV2Palette
  ) {
    drawLabel("12AM", context: &context, center: center, angle: -.pi / 2, distance: 58, size: 16, color: palette.text)
    drawLabel("6AM", context: &context, center: center, angle: 0, distance: 71, size: 17, color: palette.text)
    drawLabel("12PM", context: &context, center: center, angle: .pi / 2, distance: 79, size: 17, color: palette.text)
    drawLabel("6PM", context: &context, center: center, angle: .pi, distance: 71, size: 17, color: palette.text)

    let secondaryLabels: [(String, Double, CGFloat)] = [
      ("10", -.pi * 0.72, 76),
      ("2", -.pi * 0.28, 76),
      ("4", -.pi * 0.08, 84),
      ("8", .pi * 0.18, 84),
      ("10", .pi * 0.36, 84),
      ("2", .pi * 0.65, 84),
      ("4", .pi * 0.83, 84),
      ("8", -.pi * 0.90, 84),
    ]
    for (text, angle, distance) in secondaryLabels {
      drawLabel(text, context: &context, center: center, angle: angle, distance: distance, size: 13, color: palette.mutedText)
    }
  }

  private static func drawLabel(
    _ text: String,
    context: inout GraphicsContext,
    center: CGPoint,
    angle: Double,
    distance: CGFloat,
    size: CGFloat,
    color: Color
  ) {
    let cosine = CGFloat(Darwin.cos(angle))
    let sine = CGFloat(Darwin.sin(angle))
    let point = CGPoint(
      x: center.x + cosine * distance,
      y: center.y + sine * distance
    )
    context.draw(
      Text(text).font(.system(size: size, weight: .bold)).foregroundStyle(color),
      at: point,
      anchor: .center
    )
  }
}

struct SleepV2ClockBubble: View {
  let palette: SleepV2Palette
  let systemImage: String
  let active: Bool

  var body: some View {
    Image(systemName: systemImage)
      .font(.caption.weight(.semibold))
      .foregroundStyle(active ? .white : palette.secondaryText)
      .frame(width: 32, height: 32)
      .background(Circle().fill(active ? palette.accent : palette.surfaceHeader.opacity(0.74)))
  }
}

