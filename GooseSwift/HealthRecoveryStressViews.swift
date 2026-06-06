import Darwin
import Foundation
import SwiftUI
import UIKit

struct RecoveryV2OverviewPage: View {
  @EnvironmentObject private var router: AppRouter
  @Environment(GooseAppModel.self) private var model
  var store: HealthDataStore
  @Binding var selectedDate: Date
  @Environment(\.colorScheme) private var colorScheme
  @State private var showingDatePicker = false
  @State private var selectedTrend: HealthMetricSnapshot?
  @State private var scrollOffsetY: CGFloat = 0

  private let heroHeight: CGFloat = 320
  private let heroBackgroundHeight: CGFloat = 560
  private let statColumns = [
    GridItem(.flexible(), spacing: 12),
    GridItem(.flexible(), spacing: 12),
  ]

  var body: some View {
    let palette = SleepV2Palette(colorScheme: colorScheme, theme: .recovery)
    ScrollViewReader { _ in
      ZStack(alignment: .top) {
        palette.background
          .ignoresSafeArea()

        RecoveryV2ScenicBackground(palette: palette)
          .frame(height: heroBackgroundHeight)
          .offset(y: min(scrollOffsetY, 0))
          .ignoresSafeArea(edges: .top)
          .allowsHitTesting(false)

        ScrollView {
          LazyVStack(alignment: .leading, spacing: 0) {
            SleepV2ScrollOffsetProbe()

            if store.packetScoreStatus.hasPrefix("Extracting") {
              ProgressView()
                .tint(palette.accent)
                .frame(height: heroHeight)
            } else {
              SleepV2Hero(
                palette: palette,
                title: "Recovery",
                dateLabel: dateLabel,
                score: recoveryScore,
                gaugeLabel: "Recovery",
                onDateTap: { showingDatePicker = true }
              )
              .frame(height: heroHeight)
            }

            VStack(alignment: .leading, spacing: 14) {
              LazyVGrid(columns: statColumns, spacing: 12) {
                SleepV2StatCard(
                  palette: palette,
                  systemImage: "waveform.path.ecg",
                  label: "Resting HRV",
                  value: store.recoveryHRVDisplayText(for: selectedDate)
                )
                .frame(height: 96)

                SleepV2StatCard(
                  palette: palette,
                  systemImage: "heart.fill",
                  label: "Resting HR",
                  value: store.recoveryRestingHRDisplayText(for: selectedDate)
                )
                .frame(height: 96)

                SleepV2StatCard(
                  palette: palette,
                  systemImage: "lungs.fill",
                  label: "Respiratory Rate",
                  value: store.recoveryRespiratoryRateDisplayText(for: selectedDate)
                )
                .frame(height: 96)

                SleepV2StatCard(
                  palette: palette,
                  systemImage: "drop.fill",
                  label: "Oxygen Saturation",
                  value: store.recoveryOxygenSaturationDisplayText(for: selectedDate)
                )
                .frame(height: 96)
              }

              SleepV2StatCard(
                palette: palette,
                systemImage: "thermometer.medium",
                label: "Wrist Temperature",
                value: store.recoveryWristTemperatureDisplayText(for: selectedDate)
              )
              .frame(height: 96)

              SleepV2StatCard(
                palette: palette,
                systemImage: "target",
                label: "Target Strain Today",
                value: store.strainTargetDisplayText()
              )
              .frame(height: 96)

              SleepV2CoachingCard(palette: palette, tip: coachTip) {
                openCoachTip()
              }

              SleepV2SectionHeader(title: "Timeline", palette: palette)

              RecoveryV2EmptyStateCard(
                palette: palette,
                systemImage: "timeline.selection",
                title: "No recovery timeline",
                value: "0 events"
              )

              SleepV2SectionHeader(title: "Insights", palette: palette)

              RecoveryV2EmptyStateCard(
                palette: palette,
                systemImage: "sparkles",
                title: "No recovery insights",
                value: "0 signals"
              )

              SleepV2SectionHeader(title: "Trends", palette: palette)

              VStack(spacing: 14) {
                ForEach(recoveryTrendRows) { snapshot in
                  RecoveryV2TrendCard(palette: palette, snapshot: snapshot) {
                    selectedTrend = snapshot
                  }
                }
              }
            }
            .padding(.horizontal, 18)
            .padding(.bottom, 34)
          }
        }
        .coordinateSpace(name: SleepV2ScrollOffsetProbe.coordinateSpaceName)
        .onPreferenceChange(SleepV2ScrollOffsetPreferenceKey.self) { value in
          scrollOffsetY = value
        }
      }
    }
    .navigationTitle("Recovery")
    .navigationBarTitleDisplayMode(.inline)
    .toolbarBackground(.hidden, for: .navigationBar)
    .toolbar {
      ToolbarItem(placement: .principal) {
        Text("Recovery")
          .font(.headline.weight(.semibold))
          .foregroundStyle(palette.text)
      }
    }
    .sheet(isPresented: $showingDatePicker) {
      ScoreDatePickerSheet(
        title: "Recovery",
        routes: [.recovery],
        snapshots: [store.snapshot(for: .recovery)],
        selectedDate: $selectedDate
      )
    }
    .sheet(item: $selectedTrend) { snapshot in
      SleepV2BevelTrendSheet(snapshot: snapshot)
    }
    .onAppear {
      store.loadBridgeCatalogsIfNeeded()
      store.runPacketScores()
    }
    .onChange(of: model.packetImportRevision) { _, _ in
      store.runPacketScores()
    }
  }

  private var selectedSnapshot: HealthMetricSnapshot {
    ScoreDateTimeline.datedSnapshot(
      from: store.snapshot(for: .recovery),
      date: selectedDate
    )
  }

  private var recoveryScore: Int {
    if let selectedScore = SleepV2Numbers.firstInt(in: selectedSnapshot.value) {
      return selectedScore
    }
    return Calendar.current.isDate(selectedDate, inSameDayAs: Date())
      ? store.recoveryScoreDisplayValue()
      : 0
  }

  private var isSelectedDateToday: Bool {
    Calendar.current.isDate(selectedDate, inSameDayAs: Date())
  }

  private var recoveryTrendRows: [HealthMetricSnapshot] {
    store.recoveryTrendOverviewRows()
  }

  private var dateLabel: String {
    let suffix = selectedDate.formatted(.dateTime.day().month(.abbreviated))
    let prefix = ScoreDateTimeline.dateLabel(for: selectedDate)
    return "\(prefix), \(suffix)"
  }

  private var coachTip: CoachInlineTip {
    CoachTipFactory.metricTip(route: .recovery, healthStore: store, appModel: model)
  }

  private func openCoachTip() {
    router.openCoach(prompt: coachTip.prompt)
    model.recordUIAction("coach.opened", detail: "recovery v2 inline tip")
  }
}

struct StressV2OverviewPage: View {
  @EnvironmentObject private var router: AppRouter
  @Environment(GooseAppModel.self) private var model
  var store: HealthDataStore
  @Binding var selectedDate: Date
  @Environment(\.colorScheme) private var colorScheme
  @State private var showingDatePicker = false
  @State private var selectedTrend: HealthMetricSnapshot?
  @State private var scrollOffsetY: CGFloat = 0

  private let heroHeight: CGFloat = 334
  private let heroBackgroundHeight: CGFloat = 560

  var body: some View {
    let palette = SleepV2Palette(colorScheme: colorScheme, theme: .stress)
    ZStack(alignment: .top) {
      palette.background
        .ignoresSafeArea()

      StressV2ScenicBackground(palette: palette)
        .frame(height: heroBackgroundHeight)
        .offset(y: min(scrollOffsetY, 0))
        .ignoresSafeArea(edges: .top)
        .allowsHitTesting(false)

      ScrollView {
        LazyVStack(alignment: .leading, spacing: 0) {
          SleepV2ScrollOffsetProbe()

          StressV2Hero(
            palette: palette,
            title: "Stress",
            dateLabel: dateLabel,
            score: stressScore,
            status: summary.status,
            onDateTap: { showingDatePicker = true }
          )
          .frame(height: heroHeight)

          VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 12) {
              SleepV2StatCard(
                palette: palette,
                systemImage: "checkmark.seal.fill",
                label: "Confidence",
                value: stressConfidenceText
              )
              SleepV2StatCard(
                palette: palette,
                systemImage: "heart.fill",
                label: "Average HR",
                value: averageHeartRateText
              )
            }
            .frame(height: 96)

            SleepV2CoachingCard(palette: palette, tip: coachTip) {
              openCoachTip()
            }

            SleepV2SectionHeader(title: "Timeline", palette: palette)

            StressV2TimelineSection(palette: palette, summary: summary, dateLabel: dateLabel)

            SleepV2SectionHeader(title: "Breakdown", palette: palette)

            StressV2BreakdownSection(palette: palette, summary: summary)

            SleepV2SectionHeader(title: "Trends", palette: palette)

            if trendRows.isEmpty {
              StrainV2EmptyStateCard(
                palette: palette,
                systemImage: "chart.line.uptrend.xyaxis",
                title: "No stress trends",
                message: "Stress trends will appear after local heart-rate samples are captured for this day."
              )
            } else {
              VStack(spacing: 14) {
                ForEach(trendRows) { snapshot in
                  RecoveryV2TrendCard(palette: palette, snapshot: snapshot) {
                    selectedTrend = snapshot
                  }
                }
              }
            }
          }
          .padding(.horizontal, 18)
          .padding(.bottom, 34)
        }
      }
      .coordinateSpace(name: SleepV2ScrollOffsetProbe.coordinateSpaceName)
      .onPreferenceChange(SleepV2ScrollOffsetPreferenceKey.self) { value in
        scrollOffsetY = value
      }
    }
    .navigationTitle("Stress")
    .navigationBarTitleDisplayMode(.inline)
    .toolbarBackground(.hidden, for: .navigationBar)
    .toolbar {
      ToolbarItem(placement: .principal) {
        Text("Stress")
          .font(.headline.weight(.semibold))
          .foregroundStyle(palette.text)
      }
      ToolbarItem(placement: .topBarTrailing) {
        Button {
          showingDatePicker = true
        } label: {
          Image(systemName: "calendar")
        }
        .accessibilityLabel("Choose Stress date")
      }
    }
    .sheet(isPresented: $showingDatePicker) {
      StressV2DatePickerSheet(selectedDate: $selectedDate)
    }
    .sheet(item: $selectedTrend) { snapshot in
      SleepV2BevelTrendSheet(snapshot: snapshot)
    }
  }

  private var summary: StressAlgorithmSummary {
    store.stressAlgorithmSummary(for: selectedDate)
  }

  private var stressScore: Int {
    Int((summary.score ?? 0).rounded())
  }

  private var trendRows: [HealthMetricSnapshot] {
    Calendar.current.isDate(selectedDate, inSameDayAs: Date()) ? store.trendRows(for: .stress) : []
  }

  private var dateLabel: String {
    selectedDate.formatted(.dateTime.day().month(.wide).year())
  }

  private var averageHeartRateText: String {
    guard let value = summary.averageHeartRate,
          let text = HealthDataStore.numberText(value, fractionDigits: 0) else {
      return "No data"
    }
    return "\(text) bpm"
  }

  private var stressConfidenceText: String {
    guard let confidence = summary.confidence,
          let text = HealthDataStore.numberText(confidence, fractionDigits: 2) else {
      return "No data"
    }
    return text
  }

  private var coachTip: CoachInlineTip {
    CoachTipFactory.metricTip(route: .stress, healthStore: store, appModel: model)
  }

  private func openCoachTip() {
    router.openCoach(prompt: coachTip.prompt)
    model.recordUIAction("coach.opened", detail: "stress v2 inline tip")
  }
}

struct StressV2DatePickerSheet: View {
  @Binding var selectedDate: Date
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    NavigationStack {
      DatePicker("Stress Date", selection: $selectedDate, displayedComponents: .date)
        .datePickerStyle(.graphical)
        .padding()
        .navigationTitle("Stress")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
          ToolbarItem(placement: .topBarTrailing) {
            Button("Done") {
              dismiss()
            }
            .fontWeight(.semibold)
          }
        }
    }
    .presentationDetents([.medium])
  }
}

struct StressV2Hero: View {
  let palette: SleepV2Palette
  let title: String
  let dateLabel: String
  let score: Int
  let status: String
  let onDateTap: () -> Void

  var body: some View {
    VStack(spacing: 0) {
      Spacer().frame(height: 38)

      Text(title)
        .font(.system(size: 38, weight: .semibold, design: .rounded))
        .foregroundStyle(palette.text)
        .lineLimit(1)

      Button(action: onDateTap) {
        HStack(spacing: 7) {
          Text(dateLabel)
          Image(systemName: "chevron.down")
            .font(.caption.weight(.semibold))
        }
        .font(.title3.weight(.semibold))
        .foregroundStyle(palette.secondaryText)
        .padding(.top, 5)
      }
      .buttonStyle(.plain)

      StressV2ScoreGauge(palette: palette, score: score, status: status)
        .frame(width: 206, height: 206)
        .padding(.top, 18)
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
  }
}

struct StressV2ScoreGauge: View {
  let palette: SleepV2Palette
  let score: Int
  let status: String

  private let tickCount = 92

  private var progress: Double {
    min(max(Double(score) / 100.0, 0), 1)
  }

  var body: some View {
    GeometryReader { proxy in
      let side = min(proxy.size.width, proxy.size.height)
      let tickHeight = max(9, side * 0.055)
      let tickWidth = max(2, side * 0.012)
      let innerInset = side * 0.21

      ZStack {
        Circle()
          .fill(palette.surface.opacity(palette.light ? 0.86 : 0.72))
          .shadow(color: palette.shadow.opacity(0.48), radius: 18, x: 0, y: 8)

        Circle()
          .stroke(palette.separator.opacity(0.70), lineWidth: 2)
          .padding(2)

        ForEach(0..<tickCount, id: \.self) { index in
          let pct = Double(index) / Double(max(tickCount - 1, 1))
          Capsule()
            .fill(tickColor(percent: pct, active: pct <= progress))
            .frame(width: tickWidth, height: tickHeight)
            .offset(y: -(side / 2 - tickHeight * 1.25))
            .rotationEffect(.degrees(pct * 285 - 142.5))
        }
        .rotationEffect(.degrees(90))

        Circle()
          .stroke(palette.accent.opacity(0.30), lineWidth: 2)
          .padding(innerInset)

        VStack(spacing: 4) {
          Text("\(score)")
            .font(.system(size: 58, weight: .semibold, design: .rounded))
            .foregroundStyle(palette.text)
            .lineLimit(1)
          Text(status)
            .font(.title3.weight(.semibold))
            .foregroundStyle(statusColor)
            .lineLimit(1)
            .minimumScaleFactor(0.76)
        }

        VStack {
          Spacer()
          HStack {
            Text("0")
            Spacer()
            Text("100")
          }
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(palette.mutedText)
          .padding(.horizontal, side * 0.24)
          .padding(.bottom, side * 0.12)
        }
      }
      .frame(width: side, height: side)
      .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
  }

  private var statusColor: Color {
    if score >= 66 {
      return .red
    }
    if score >= 33 {
      return palette.accentAlt
    }
    return palette.accent
  }

  private func tickColor(percent: Double, active: Bool) -> Color {
    let base: Color
    if percent >= 0.66 {
      base = .red
    } else if percent >= 0.33 {
      base = palette.accentAlt
    } else {
      base = palette.accent
    }
    return active ? base : base.opacity(palette.light ? 0.20 : 0.16)
  }
}

struct StressV2ScenicBackground: View {
  let palette: SleepV2Palette

  var body: some View {
    ZStack {
      LinearGradient(
        colors: palette.light
          ? [Color(red: 0.91, green: 0.94, blue: 0.96), Color(red: 0.88, green: 0.91, blue: 0.92), palette.background]
          : [Color(red: 0.09, green: 0.10, blue: 0.14), Color(red: 0.13, green: 0.13, blue: 0.17), palette.background],
        startPoint: .top,
        endPoint: .bottom
      )

      Canvas { context, size in
        for index in 0..<26 {
          let x = CGFloat((index * 71 + 19) % max(1, Int(size.width)))
          let y = CGFloat(38 + ((index * 47) % max(1, Int(size.height * 0.38))))
          let radius = index % 8 == 0 ? CGFloat(1.1) : CGFloat(0.65)
          context.fill(
            Path(ellipseIn: CGRect(x: x, y: y, width: radius * 2, height: radius * 2)),
            with: .color(.white.opacity(palette.light ? 0.16 : 0.20))
          )
        }

        let waveY = size.height * 0.34
        var wave = Path()
        wave.move(to: CGPoint(x: -20, y: waveY))
        wave.addCurve(
          to: CGPoint(x: size.width + 20, y: waveY + 12),
          control1: CGPoint(x: size.width * 0.24, y: waveY - 28),
          control2: CGPoint(x: size.width * 0.74, y: waveY + 36)
        )
        context.stroke(
          wave,
          with: .linearGradient(
            Gradient(colors: [
              palette.accent.opacity(palette.light ? 0.18 : 0.28),
              palette.accentAlt.opacity(palette.light ? 0.10 : 0.18),
            ]),
            startPoint: CGPoint(x: 0, y: waveY),
            endPoint: CGPoint(x: size.width, y: waveY)
          ),
          style: StrokeStyle(lineWidth: 2, lineCap: .round)
        )
      }

      VStack {
        Spacer()
        Rectangle()
          .fill(
            LinearGradient(
              colors: [.clear, palette.background.opacity(0.72), palette.background],
              startPoint: .top,
              endPoint: .bottom
            )
          )
          .frame(height: 180)
      }
    }
  }
}

enum StressV2Format {
  static func durationClockText(_ minutes: Double) -> String {
    let totalSeconds = max(Int((minutes * 60).rounded()), 0)
    let hours = totalSeconds / 3600
    let remainingSeconds = totalSeconds % 3600
    let mins = remainingSeconds / 60
    let seconds = remainingSeconds % 60
    return String(format: "%d:%02d:%02d", hours, mins, seconds)
  }
}

struct StressV2TimelineSection: View {
  let palette: SleepV2Palette
  let summary: StressAlgorithmSummary
  let dateLabel: String

  var body: some View {
    SleepV2Panel(palette: palette, padding: 16, radius: 16) {
      VStack(alignment: .leading, spacing: 14) {
        HStack(alignment: .firstTextBaseline) {
          VStack(alignment: .leading, spacing: 4) {
            Text(dateLabel)
              .font(.headline.weight(.semibold))
              .foregroundStyle(palette.text)
            Text(summary.freshness)
              .font(.caption.weight(.semibold))
              .foregroundStyle(palette.mutedText)
          }

          Spacer(minLength: 12)

          Text("Duration \(StressV2Format.durationClockText(totalDurationMinutes))")
            .font(.caption.weight(.semibold))
            .foregroundStyle(palette.secondaryText)
            .lineLimit(1)
            .minimumScaleFactor(0.78)
        }

        StressV2TimelineChart(palette: palette, windows: summary.windows)
          .frame(height: 190)
        if summary.hasData {
          Text(summary.inputSummary)
            .font(.caption.weight(.medium))
            .foregroundStyle(palette.secondaryText)
            .lineLimit(2)
            .fixedSize(horizontal: false, vertical: true)
        }
      }
    }
  }

  private var totalDurationMinutes: Double {
    summary.high.durationMinutes + summary.medium.durationMinutes + summary.low.durationMinutes
  }
}

struct StressV2TimelineChart: View {
  let palette: SleepV2Palette
  let windows: [StressWindowPoint]

  var body: some View {
    GeometryReader { proxy in
      if windows.isEmpty {
        RoundedRectangle(cornerRadius: 18, style: .continuous)
          .fill(palette.surfaceElevated.opacity(0.66))
          .overlay {
            Text("No stress timeline")
              .font(.subheadline.weight(.semibold))
              .foregroundStyle(palette.secondaryText)
          }
      } else {
        ZStack(alignment: .topLeading) {
          RoundedRectangle(cornerRadius: 12, style: .continuous)
            .fill(palette.surfaceElevated.opacity(palette.light ? 0.54 : 0.42))

          ForEach(Array(windows.enumerated()), id: \.element.id) { index, window in
            if window.isSleepWindow {
              Rectangle()
                .fill(Color.indigo.opacity(palette.light ? 0.10 : 0.20))
                .frame(width: max(proxy.size.width / CGFloat(max(windows.count, 1)), 12), height: proxy.size.height - 34)
                .position(x: chartPoint(index: index, size: proxy.size).x, y: (proxy.size.height - 34) / 2)
            }
          }

          ForEach([25, 50, 75, 100], id: \.self) { value in
            let y = yPosition(value: Double(value), height: proxy.size.height)
            Path { path in
              path.move(to: CGPoint(x: 0, y: y))
              path.addLine(to: CGPoint(x: proxy.size.width - 34, y: y))
            }
            .stroke(palette.separator.opacity(0.64), style: StrokeStyle(lineWidth: 1, dash: [4, 5]))
          }

          ForEach(Array(0..<max(windows.count - 1, 0)), id: \.self) { index in
            let stress = (windows[index].stress + windows[index + 1].stress) / 2
            Path { path in
              path.move(to: chartPoint(index: index, size: proxy.size))
              path.addLine(to: chartPoint(index: index + 1, size: proxy.size))
            }
            .stroke(
              color(for: stress),
              style: StrokeStyle(lineWidth: 3.4, lineCap: .round, lineJoin: .round)
            )
          }

          if let last = windows.last, let lastIndex = windows.indices.last {
            Circle()
              .fill(color(for: last.stress))
              .frame(width: 12, height: 12)
              .position(chartPoint(index: lastIndex, size: proxy.size))
          }

          if windows.contains(where: \.isSleepWindow) {
            Image(systemName: "moon.fill")
              .font(.caption.weight(.bold))
              .foregroundStyle(Color.indigo.opacity(0.88))
              .position(x: proxy.size.width * 0.18, y: 17)
          }

          if let peakIndex = windows.indices.max(by: { windows[$0].stress < windows[$1].stress }) {
            Image(systemName: "figure.run")
              .font(.caption.weight(.bold))
              .foregroundStyle(Color.orange.opacity(0.92))
              .position(x: chartPoint(index: peakIndex, size: proxy.size).x, y: 17)
          }

          VStack(alignment: .trailing) {
            Text("100")
            Spacer()
            Text("75")
            Spacer()
            Text("50")
            Spacer()
            Text("25")
            Spacer()
            Text("0")
          }
          .font(.caption.weight(.semibold))
          .foregroundStyle(palette.mutedText)
          .frame(width: proxy.size.width - 8, height: proxy.size.height - 18, alignment: .trailing)
          .padding(.top, 6)

          HStack {
            Text(windows.first?.timeLabel ?? "")
            Spacer()
            Text(windows.indices.contains(windows.count / 2) ? windows[windows.count / 2].timeLabel : "")
            Spacer()
            Text(windows.last?.timeLabel ?? "")
          }
          .font(.caption.weight(.semibold))
          .foregroundStyle(palette.mutedText)
          .padding(.horizontal, 10)
          .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
          .padding(.trailing, 28)
        }
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
      }
    }
  }

  private func chartPoint(index: Int, size: CGSize) -> CGPoint {
    CGPoint(
      x: xPosition(index: index, width: size.width),
      y: yPosition(value: windows[index].stress, height: size.height)
    )
  }

  private func xPosition(index: Int, width: CGFloat) -> CGFloat {
    let left: CGFloat = 12
    let right: CGFloat = 40
    let usableWidth = max(width - left - right, 1)
    return left + usableWidth * CGFloat(index) / CGFloat(max(windows.count - 1, 1))
  }

  private func yPosition(value: Double, height: CGFloat) -> CGFloat {
    let top: CGFloat = 14
    let bottom: CGFloat = 30
    let usableHeight = max(height - top - bottom, 1)
    return top + usableHeight * CGFloat(1 - min(max(value / 100, 0), 1))
  }

  private func color(for stress: Double) -> Color {
    if stress >= 66 {
      return .red
    }
    if stress >= 33 {
      return palette.accentAlt
    }
    return palette.accent
  }
}

struct StressV2BreakdownSection: View {
  let palette: SleepV2Palette
  let summary: StressAlgorithmSummary

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack(alignment: .firstTextBaseline) {
        Text("Duration:")
          .foregroundStyle(palette.mutedText)
        Text(StressV2Format.durationClockText(totalDurationMinutes))
          .foregroundStyle(palette.text)
        Spacer()
      }
      .font(.subheadline.weight(.semibold))

      StressV2BreakdownRow(palette: palette, label: "High", zone: summary.high, color: .red)
      StressV2BreakdownRow(palette: palette, label: "Med", zone: summary.medium, color: palette.accentAlt)
      StressV2BreakdownRow(palette: palette, label: "Low", zone: summary.low, color: palette.accent)
    }
  }

  private var totalDurationMinutes: Double {
    summary.high.durationMinutes + summary.medium.durationMinutes + summary.low.durationMinutes
  }
}

struct StressV2BreakdownRow: View {
  let palette: SleepV2Palette
  let label: String
  let zone: StressZoneSummary
  let color: Color

  var body: some View {
    HStack(spacing: 14) {
      Text(label)
        .font(.headline.weight(.semibold))
        .foregroundStyle(palette.text)
        .frame(width: 46, alignment: .leading)

      GeometryReader { proxy in
        ZStack(alignment: .leading) {
          Capsule()
            .fill(color.opacity(palette.light ? 0.14 : 0.16))
          Capsule()
            .fill(color)
            .frame(width: proxy.size.width * CGFloat(min(max(zone.percent, 0), 1)))
        }
      }
      .frame(height: 10)

      Text("\(Int((zone.percent * 100).rounded()))%")
        .font(.headline.weight(.semibold))
        .fontDesign(.rounded)
        .foregroundStyle(palette.text)
        .frame(width: 46, alignment: .trailing)

      Text(StressV2Format.durationClockText(zone.durationMinutes))
        .font(.headline.weight(.semibold))
        .fontDesign(.rounded)
        .foregroundStyle(palette.mutedText)
        .frame(width: 74, alignment: .trailing)
        .minimumScaleFactor(0.78)
    }
    .padding(.horizontal, 16)
    .frame(height: 64)
    .background(
      RoundedRectangle(cornerRadius: 16, style: .continuous)
        .fill(palette.surface)
        .shadow(color: palette.shadow.opacity(0.30), radius: 8, x: 0, y: 3)
    )
    .overlay(
      RoundedRectangle(cornerRadius: 16, style: .continuous)
        .stroke(palette.separator.opacity(0.70), lineWidth: 1)
    )
  }
}
