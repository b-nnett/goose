import Darwin
import Foundation
import SwiftUI
import UIKit

struct HealthMetricFamilyView: View {
  @Environment(GooseAppModel.self) private var model
  @EnvironmentObject private var router: AppRouter
  let route: HealthRoute
  var store: HealthDataStore
  var externalSelectedDate: Binding<Date>? = nil
  @State private var selectedTrend: HealthMetricSnapshot?
  @State private var selectedPrimarySleep: PrimarySleepDetail?
  @State private var showAddSleepUnavailable = false
  @State private var localSelectedDate = Date()
  @State private var showingScoreDatePicker = false

  var body: some View {
    if route == .sleep {
      SleepV2OverviewPage(store: store, ble: model.ble, selectedDate: selectedDateBinding)
    } else if route == .recovery {
      RecoveryV2OverviewPage(store: store, selectedDate: selectedDateBinding)
    } else if route == .strain {
      StrainV2OverviewPage(store: store, selectedDate: selectedDateBinding)
    } else if route == .stress {
      StressV2OverviewPage(store: store, selectedDate: selectedDateBinding)
    } else {
      metricFamilyBody
    }
  }

  private var metricFamilyBody: some View {
    ScrollView {
      LazyVStack(alignment: .leading, spacing: 18) {
        HealthHero(snapshot: selectedSnapshot, subtitle: subtitle)
        ForEach(heroRows) { row in
          HealthInfoRow(row: row)
            .padding(.horizontal, 0)
        }

        CoachTipCard(tip: coachTip) {
          openCoachTip()
        }

        if route == .sleep {
          SleepDataBridgeSection(store: store, ble: model.ble)
          SleepAlarmBridgeSection(ble: model.ble)
        }

        if route == .stress {
          StressDailyChart(summary: store.stressAlgorithmSummary())
          StressBreakdownRows(summary: store.stressAlgorithmSummary())
        }

        if route == .strain {
          HeartRateZonesSection()
        }

        if route == .sleep {
          SleepTimelineSection(
            session: store.primarySleep(),
            onAddSleep: { showAddSleepUnavailable = true },
            onSelectPrimarySleep: { selectedPrimarySleep = $0 }
          )
        } else {
          HealthSectionTitle("Timeline")
          ForEach(timelineRows) { row in
            HealthInfoRow(row: row)
          }
        }

        HealthSectionTitle("Insights")
        ForEach(insightRows) { row in
          HealthInfoRow(row: row)
        }

        HealthSectionTitle("Trends")
        ForEach(store.trendRows(for: route)) { snapshot in
          Button {
            selectedTrend = snapshot
          } label: {
            HealthTrendRow(snapshot: snapshot)
          }
          .buttonStyle(.plain)
        }
      }
      .padding(16)
    }
    .gooseScreenBackground()
    .navigationTitle(route.title)
    .navigationBarTitleDisplayMode(.inline)
    .toolbarBackground(.hidden, for: .navigationBar)
    .toolbar {
      if route.supportsScoreDatePicker {
        ToolbarItem(placement: .principal) {
          ScoreDateTitleButton(
            title: route.title,
            subtitle: ScoreDateTimeline.dateLabel(for: selectedDateBinding.wrappedValue),
            action: { showingScoreDatePicker = true }
          )
        }
      }
    }
    .sheet(item: $selectedTrend) { snapshot in
      HealthTrendSheet(snapshot: snapshot)
    }
    .sheet(item: $selectedPrimarySleep) { sleep in
      PrimarySleepDetailSheet(sleep: sleep)
    }
    .sheet(isPresented: $showingScoreDatePicker) {
      ScoreDatePickerSheet(
        title: route.title,
        routes: [route],
        snapshots: [store.snapshot(for: route)],
        selectedDate: selectedDateBinding
      )
    }
    .alert("Add Sleep Unavailable", isPresented: $showAddSleepUnavailable) {
      Button("OK", role: .cancel) {}
    } message: {
      Text(store.sleepTimelineEmptyActionSummary())
    }
  }

  private var selectedSnapshot: HealthMetricSnapshot {
    ScoreDateTimeline.datedSnapshot(from: store.snapshot(for: route), date: selectedDateBinding.wrappedValue)
  }

  private var selectedDateBinding: Binding<Date> {
    Binding {
      externalSelectedDate?.wrappedValue ?? localSelectedDate
    } set: { newValue in
      if let externalSelectedDate {
        externalSelectedDate.wrappedValue = newValue
      } else {
        localSelectedDate = newValue
      }
    }
  }

  private var coachTip: CoachInlineTip {
    CoachTipFactory.metricTip(route: route, healthStore: store, appModel: model)
  }

  private func openCoachTip() {
    router.openCoach(prompt: coachTip.prompt)
    model.recordUIAction("coach.opened", detail: "\(route.rawValue) inline tip")
  }

  private var subtitle: String {
    switch route {
    case .sleep: "Score, stages, sleep needed, alarm, and trend surfaces"
    case .recovery: "Recovery score, HRV, resting HR, vitals, and unavailable states"
    case .strain: "Daily strain, activity, energy, and trend readiness"
    case .stress: "Stress score, HRV/HR inputs, daily chart, and breakdown"
    default: ""
    }
  }

  private var heroRows: [HealthSummaryRow] {
    switch route {
    case .sleep:
      return [
        HealthSummaryRow("Quality", value: primarySleepQualitySummary, source: store.packetScoreSource("sleep score output"), systemImage: "bed.double"),
        HealthSummaryRow("Time in bed", value: store.primarySleep()?.timeInBedText ?? "No data", source: store.packetScoreSource("sleep window"), systemImage: "clock"),
        HealthSummaryRow("Time asleep", value: store.primarySleep()?.durationText ?? "No data", source: store.packetScoreSource("sleep window"), systemImage: "moon.zzz"),
        HealthSummaryRow("Sleep Needed", value: "No target sleep input", source: .unavailable("sleep need requires target sleep amount and band sleep history"), systemImage: "alarm"),
        HealthSummaryRow("Alarm", value: model.ble.alarmDisplaySummary, source: alarmRowSource, systemImage: "bell"),
      ]
    case .recovery:
      let selectedDate = selectedDateBinding.wrappedValue
      let isToday = Calendar.current.isDate(selectedDate, inSameDayAs: Date())
      return [
        HealthSummaryRow(
          "Recovery Score",
          value: isToday ? store.recoveryScoreDisplayText() : "--",
          source: isToday ? store.snapshot(for: .recovery).source : .unavailable("selected date has no stored recovery score"),
          systemImage: "battery.100percent"
        ),
        HealthSummaryRow("Resting HRV", value: store.recoveryHRVDisplayText(for: selectedDate), source: store.recoveryHRVSource(for: selectedDate), systemImage: "waveform.path.ecg"),
        HealthSummaryRow("Resting HR", value: store.recoveryRestingHRDisplayText(for: selectedDate), source: store.recoveryRestingHRSource(for: selectedDate), systemImage: "heart"),
        HealthSummaryRow("Provided vitals", value: store.recoveryRespiratoryRateDisplayText(for: selectedDate), source: store.recoveryRespiratoryRateSource(for: selectedDate), systemImage: "lungs"),
      ]
    case .strain:
      let selectedDate = selectedDateBinding.wrappedValue
      let isToday = Calendar.current.isDate(selectedDate, inSameDayAs: Date())
      return [
        HealthSummaryRow(
          "Strain Score",
          value: store.strainScoreDisplayText(for: selectedDate),
          source: isToday ? store.snapshot(for: .strain).source : .unavailable("selected date has no stored strain score"),
          systemImage: "figure.run"
        ),
        HealthSummaryRow("Target strain", value: store.strainTargetDisplayText(), source: .unavailable("strain target unavailable"), systemImage: "target"),
        HealthSummaryRow("Duration", value: store.strainDurationDisplayText(), source: .unavailable("activity sessions unavailable"), systemImage: "timer"),
        HealthSummaryRow("Total Energy", value: store.strainEnergyDisplayText(for: selectedDate), source: store.whoopTotalCaloriesSource(for: selectedDate), systemImage: "flame"),
        HealthSummaryRow("Steps", value: store.strainActivityCountText(for: selectedDate), source: store.whoopStepsSource(for: selectedDate), systemImage: "shoeprints.fill"),
      ]
    case .stress:
      let selectedDate = selectedDateBinding.wrappedValue
      let isToday = Calendar.current.isDate(selectedDate, inSameDayAs: Date())
      let summary = store.stressAlgorithmSummary(for: selectedDate)
      let scoreText = summary.score.flatMap { HealthDataStore.numberText($0, fractionDigits: 0) } ?? "--"
      let averageHRText = summary.averageHeartRate.flatMap { HealthDataStore.numberText($0, fractionDigits: 0) }
        .map { "\($0) bpm avg" } ?? "No HR data"
      let confidenceText = summary.confidence.flatMap { HealthDataStore.numberText($0, fractionDigits: 2) } ?? "--"
      return [
        HealthSummaryRow("Stress score", value: summary.hasData ? "\(scoreText)% | \(summary.status)" : summary.status, source: summary.source, systemImage: "waveform.path.ecg"),
        HealthSummaryRow("Confidence", value: summary.hasData ? confidenceText : "--", source: summary.source, systemImage: "checkmark.seal"),
        HealthSummaryRow("Inputs", value: summary.hasData ? summary.inputSummary : "No local stress inputs", source: summary.source, systemImage: "checklist"),
        HealthSummaryRow("HRV Input", value: isToday ? store.hrvFeatureSummary() : "--", source: isToday ? store.packetInputSource("HRV feature") : store.recoveryHRVSource(for: selectedDate), systemImage: "waveform.path.ecg"),
        HealthSummaryRow("Average HR", value: averageHRText, source: summary.source, systemImage: "heart"),
      ]
    default:
      return []
    }
  }

  private var primarySleepQualitySummary: String {
    guard let sleep = store.primarySleep() else {
      return "-- | No data"
    }
    return "\(sleep.scoreDisplayText) | \(sleep.qualityText)"
  }

  private var alarmRowSource: HealthDataSource {
    if model.ble.lastAlarmScheduledAt != nil {
      return .live("WHOOP alarm command response")
    }
    if model.ble.canWriteAlarm {
      return .live("WHOOP alarm write ready")
    }
    return .unavailable(model.ble.alarmWriteSupportSummary)
  }

  private var timelineRows: [HealthSummaryRow] {
    switch route {
    case .sleep:
      if let sleep = store.primarySleep() {
        return [
          HealthSummaryRow("Primary sleep", value: "\(sleep.startLabel) - \(sleep.endLabel) | \(sleep.durationText) | \(sleep.scoreDisplayText)", source: sleep.source, systemImage: "bed.double"),
          HealthSummaryRow("Timeline", value: sleep.stages.isEmpty ? "No stage timeline" : "\(sleep.stages.count) stage rows", source: sleep.source, systemImage: "timeline.selection"),
        ]
      }
      return [
        HealthSummaryRow("Primary sleep", value: "No band sleep data", source: .unavailable("band sleep import not available"), systemImage: "bed.double"),
        HealthSummaryRow("Timeline", value: "No sleep timeline", source: .unavailable("sleep stage import not available"), systemImage: "timeline.selection"),
      ]
    case .recovery:
      return [
        HealthSummaryRow("Recovery timeline", value: "0 events", source: .unavailable("recovery timeline not available"), systemImage: "timeline.selection"),
      ]
    case .strain:
      return [
        HealthSummaryRow("Activities", value: "No activities", source: .unavailable("activity sessions unavailable"), systemImage: "plus.circle"),
      ]
    case .stress:
      let summary = store.stressAlgorithmSummary(for: selectedDateBinding.wrappedValue)
      return [
        HealthSummaryRow("Daily timeline", value: summary.hasData ? "\(summary.windows.count) stress windows" : summary.status, source: summary.source, systemImage: "timeline.selection"),
      ]
    default:
      return []
    }
  }

  private var insightRows: [HealthSummaryRow] {
    switch route {
    case .sleep:
      return [
        HealthSummaryRow("Score impacts", value: store.sleepV1ComponentBreakdownRows().isEmpty ? "No score component data" : "\(store.sleepV1ComponentBreakdownRows().count) components", source: store.packetScoreSource("sleep score components"), systemImage: "sparkles"),
        HealthSummaryRow("Confidence", value: store.sleepV1ArchitectureCalibrationSummary().isEmpty ? "No confidence data" : store.sleepV1ArchitectureCalibrationSummary(), source: store.packetScoreSource("sleep score output"), systemImage: "lock"),
      ]
    case .recovery:
      return [
        HealthSummaryRow("Recovery insights", value: "0 signals", source: .unavailable("recovery insights not available"), systemImage: "sparkles"),
        HealthSummaryRow("Vitals unavailable", value: "0 trusted vitals", source: .unavailable("vital packet proof pending"), systemImage: "exclamationmark.triangle"),
      ]
    case .strain:
      return [
        HealthSummaryRow("Coaching", value: store.strainEmptyStateSummary(), source: .unavailable("strain insights unavailable"), systemImage: "sparkles"),
      ]
    case .stress:
      let summary = store.stressAlgorithmSummary(for: selectedDateBinding.wrappedValue)
      return [
        HealthSummaryRow("Breakdown", value: summary.hasData ? "High \(Int((summary.high.percent * 100).rounded()))% | Medium \(Int((summary.medium.percent * 100).rounded()))% | Low \(Int((summary.low.percent * 100).rounded()))%" : summary.status, source: summary.source, systemImage: "chart.bar"),
      ]
    default:
      return []
    }
  }
}

struct StrainV2ActivityBackground: View {
  let palette: SleepV2Palette
  var showsDecorations = true

  var body: some View {
    ZStack {
      LinearGradient(
        colors: palette.light
          ? [
            Color(red: 0.98, green: 0.95, blue: 0.89),
            Color(red: 0.94, green: 0.96, blue: 0.92),
            palette.background,
          ]
          : [
            Color(red: 0.14, green: 0.12, blue: 0.10),
            Color(red: 0.10, green: 0.13, blue: 0.11),
            palette.background,
          ],
        startPoint: .top,
        endPoint: .bottom
      )

      if showsDecorations {
        Canvas { context, size in
          drawZoneGrid(context: &context, size: size)
          drawEffortBars(context: &context, size: size)
        }
      }

      VStack {
        Spacer()
        Rectangle()
          .fill(
            LinearGradient(
              colors: [.clear, palette.background.opacity(0.76), palette.background],
              startPoint: .top,
              endPoint: .bottom
            )
          )
          .frame(height: 160)
      }
    }
  }

  private func drawZoneGrid(context: inout GraphicsContext, size: CGSize) {
    let lineColor = palette.light
      ? Color.black.opacity(0.055)
      : Color.white.opacity(0.055)
    let labelColor = palette.light
      ? Color(red: 0.70, green: 0.42, blue: 0.20).opacity(0.18)
      : Color(red: 1.0, green: 0.62, blue: 0.30).opacity(0.16)

    for index in 0..<5 {
      let y = size.height * 0.12 + CGFloat(index) * 38
      var path = Path()
      path.move(to: CGPoint(x: 20, y: y))
      path.addLine(to: CGPoint(x: size.width - 20, y: y))
      context.stroke(
        path,
        with: .color(lineColor),
        style: StrokeStyle(lineWidth: 1, lineCap: .round, dash: [3, 12])
      )

      let tickRect = CGRect(x: size.width - 72, y: y - 2, width: 46, height: 4)
      context.fill(
        Path(roundedRect: tickRect, cornerRadius: 2),
        with: .color(labelColor.opacity(index == 4 ? 1 : 0.64))
      )
    }
  }

  private func drawEffortBars(context: inout GraphicsContext, size: CGSize) {
    let colors: [Color] = palette.light
      ? [
        Color(red: 0.32, green: 0.61, blue: 0.40).opacity(0.12),
        Color(red: 0.91, green: 0.58, blue: 0.20).opacity(0.14),
        Color(red: 0.88, green: 0.32, blue: 0.14).opacity(0.13),
      ]
      : [
        Color(red: 0.34, green: 0.72, blue: 0.44).opacity(0.13),
        Color(red: 1.0, green: 0.68, blue: 0.28).opacity(0.15),
        Color(red: 1.0, green: 0.40, blue: 0.20).opacity(0.14),
      ]

    for index in 0..<3 {
      let width = size.width * (0.16 + CGFloat(index) * 0.05)
      let height = size.height * (0.18 + CGFloat(index) * 0.055)
      let rect = CGRect(
        x: size.width * (0.12 + CGFloat(index) * 0.16),
        y: size.height * 0.26 - CGFloat(index) * 18,
        width: width,
        height: height
      )
      context.fill(
        Path(roundedRect: rect, cornerRadius: 10),
        with: .color(colors[index])
      )
    }
  }
}

struct StrainV2OverviewPage: View {
  @EnvironmentObject private var router: AppRouter
  @Environment(GooseAppModel.self) private var model
  var store: HealthDataStore
  @Binding var selectedDate: Date
  @Environment(\.colorScheme) private var colorScheme
  @State private var showingDatePicker = false
  @State private var showingInsightsSheet = false
  @State private var selectedTrend: HealthMetricSnapshot?

  private let heroHeight: CGFloat = 320

  var body: some View {
    let palette = SleepV2Palette(colorScheme: colorScheme, theme: SleepV2PaletteTheme.strain)

    ZStack(alignment: .top) {
      palette.background
        .ignoresSafeArea()

      StrainV2ActivityBackground(palette: palette, showsDecorations: false)
        .ignoresSafeArea(edges: .top)
        .allowsHitTesting(false)

      ScrollView {
        LazyVStack(alignment: .leading, spacing: 0) {
          ZStack(alignment: .top) {
            StrainV2ActivityBackground(palette: palette)
              .frame(height: heroHeight)
              .allowsHitTesting(false)

            StrainV2Hero(
              palette: palette,
              score: store.strainScore0To100(for: selectedDate),
              status: store.strainStatusText(for: selectedDate),
              dateLabel: dateLabel,
              onDateTap: { showingDatePicker = true }
            )
          }
          .frame(height: heroHeight)
          .clipped()

          VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 12) {
              SleepV2StatCard(
                palette: palette,
                systemImage: "target",
                label: "Target Strain",
                value: store.strainTargetDisplayText()
              )
              SleepV2StatCard(
                palette: palette,
                systemImage: "timer",
                label: "Duration",
                value: store.strainDurationDisplayText()
              )
            }
            .frame(height: 96)

            HStack(spacing: 12) {
              SleepV2StatCard(
                palette: palette,
                systemImage: "flame.fill",
                label: "Total Energy",
                value: store.strainEnergyDisplayText(for: selectedDate)
              )
              SleepV2StatCard(
                palette: palette,
                systemImage: "shoeprints.fill",
                label: "Steps",
                value: store.strainActivityCountText(for: selectedDate)
              )
            }
            .frame(height: 96)

            SleepV2CoachingCard(palette: palette, tip: coachTip) {
              openCoachTip()
            }

            SleepV2ActionRow(
              palette: palette,
              systemImage: "exclamationmark.triangle",
              title: "View data gaps",
              action: { showingInsightsSheet = true }
            )

            StrainV2DailyLoadCard(
              palette: palette,
              scoreText: store.strainScoreDisplayText(for: selectedDate),
              targetText: store.strainTargetDisplayText(),
              durationText: store.strainDurationDisplayText(),
              energyText: store.strainEnergyDisplayText(for: selectedDate)
            )

            SleepV2SectionHeader(title: "Activities", palette: palette)
            StrainV2EmptyStateCard(
              palette: palette,
              systemImage: "figure.run.circle",
              title: "No activities",
              message: store.strainEmptyStateSummary()
            )

            SleepV2SectionHeader(title: "Trends", palette: palette)
            if trendRows.isEmpty {
              StrainV2EmptyStateCard(
                palette: palette,
                systemImage: "chart.line.uptrend.xyaxis",
                title: "No strain trends",
                message: "Strain trends will appear after local activity and heart-rate history is available."
              )
            } else {
              VStack(spacing: 14) {
                ForEach(trendRows) { snapshot in
                  SleepV2TrendRow(palette: palette, snapshot: snapshot) {
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
    }
    .navigationTitle("Strain")
    .navigationBarTitleDisplayMode(.inline)
    .toolbarBackground(.hidden, for: .navigationBar)
    .toolbar {
      ToolbarItem(placement: .principal) {
        Text("Strain")
          .font(.headline.weight(.semibold))
          .foregroundStyle(palette.text)
      }
      ToolbarItem(placement: .topBarTrailing) {
        Button {
          showingDatePicker = true
        } label: {
          Image(systemName: "calendar")
        }
        .accessibilityLabel("Choose Strain date")
      }
    }
    .sheet(isPresented: $showingDatePicker) {
      ScoreDatePickerSheet(
        title: "Strain",
        routes: [.strain],
        snapshots: [store.snapshot(for: .strain)],
        selectedDate: $selectedDate
      )
    }
    .sheet(isPresented: $showingInsightsSheet) {
      StrainV2InsightsSheet(palette: palette, store: store)
    }
    .sheet(item: $selectedTrend) { snapshot in
      SleepV2BevelTrendSheet(snapshot: snapshot)
    }
  }

  private var dateLabel: String {
    let suffix = selectedDate.formatted(.dateTime.day().month(.abbreviated))
    let prefix = ScoreDateTimeline.dateLabel(for: selectedDate)
    return "\(prefix), \(suffix)"
  }

  private var coachTip: CoachInlineTip {
    CoachTipFactory.metricTip(route: .strain, healthStore: store, appModel: model)
  }

  private var trendRows: [HealthMetricSnapshot] {
    store.trendRows(for: .strain)
  }

  private func openCoachTip() {
    router.openCoach(prompt: coachTip.prompt)
    model.recordUIAction("coach.opened", detail: "strain inline tip")
  }
}

struct StrainV2Hero: View {
  let palette: SleepV2Palette
  let score: Double
  let status: String
  let dateLabel: String
  let onDateTap: () -> Void

  var body: some View {
    VStack(spacing: 0) {
      Spacer().frame(height: 70)

      StrainV2ScoreGauge(palette: palette, score: score, status: status)
        .frame(width: 188, height: 188)

      Button(action: onDateTap) {
        HStack(spacing: 6) {
          Text(dateLabel)
          Image(systemName: "chevron.down")
            .font(.caption.weight(.semibold))
        }
        .font(.subheadline.weight(.semibold))
        .foregroundStyle(palette.secondaryText)
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(.thinMaterial, in: Capsule())
      }
      .buttonStyle(.plain)
      .padding(.top, 16)
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
  }
}

struct StrainV2ScoreGauge: View {
  let palette: SleepV2Palette
  let score: Double
  let status: String

  private var progress: CGFloat {
    CGFloat(min(max(score / 100.0, 0), 1))
  }

  private var scoreText: String {
    score > 0 ? String(format: "%.0f", score) : "0"
  }

  var body: some View {
    GeometryReader { proxy in
      let side = min(proxy.size.width, proxy.size.height)
      let lineWidth = max(13, side * 0.078)
      let radius = side / 2 - 18
      let end = progressPoint(side: side, radius: radius)
      let tint = Color(red: 1.0, green: 0.52, blue: 0.18)

      ZStack {
        Circle()
          .fill(palette.surface.opacity(palette.light ? 0.94 : 0.84))
          .shadow(color: palette.shadow.opacity(0.48), radius: 18, x: 0, y: 8)
        Circle()
          .stroke(.white.opacity(palette.light ? 0.88 : 0.12), lineWidth: 10)
          .padding(6)
        Circle()
          .inset(by: 18)
          .stroke(palette.separator.opacity(palette.light ? 0.72 : 0.62), lineWidth: lineWidth)
        Circle()
          .inset(by: 18)
          .trim(from: 0, to: progress)
          .stroke(
            LinearGradient(
              colors: [Color(red: 1.0, green: 0.72, blue: 0.36), tint],
              startPoint: .topLeading,
              endPoint: .bottomTrailing
            ),
            style: StrokeStyle(lineWidth: lineWidth, lineCap: .round)
          )
          .rotationEffect(.degrees(-90))

        Circle()
          .fill(tint)
          .frame(width: lineWidth * 0.95, height: lineWidth * 0.95)
          .shadow(color: tint.opacity(0.32), radius: 6, x: 0, y: 2)
          .position(end)

        VStack(spacing: 4) {
          Text(scoreText)
            .font(.system(size: 52, weight: .semibold, design: .rounded))
            .foregroundStyle(palette.text)
          Text(status)
            .font(.footnote.weight(.semibold))
            .foregroundStyle(palette.secondaryText)
            .lineLimit(1)
            .minimumScaleFactor(0.7)
        }
      }
      .frame(width: side, height: side)
      .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
  }

  private func progressPoint(side: CGFloat, radius: CGFloat) -> CGPoint {
    let angle = Double(progress) * 2 * Double.pi - Double.pi / 2
    let center = side / 2
    return CGPoint(
      x: center + CGFloat(cos(angle)) * radius,
      y: center + CGFloat(sin(angle)) * radius
    )
  }
}

struct StrainV2DailyLoadCard: View {
  let palette: SleepV2Palette
  let scoreText: String
  let targetText: String
  let durationText: String
  let energyText: String

  var body: some View {
    VStack(alignment: .leading, spacing: 18) {
      HStack(alignment: .top) {
        VStack(alignment: .leading, spacing: 4) {
          Text("Daily load")
            .font(.title3.weight(.semibold))
            .foregroundStyle(palette.text)
          Text("Today")
            .font(.subheadline.weight(.medium))
            .foregroundStyle(palette.secondaryText)
        }
        Spacer()
        Image(systemName: "figure.run")
          .font(.headline.weight(.semibold))
          .foregroundStyle(Color(red: 1.0, green: 0.52, blue: 0.18))
          .frame(width: 34, height: 34)
          .background(Color(red: 1.0, green: 0.52, blue: 0.18).opacity(0.12), in: Circle())
      }

      HStack(spacing: 10) {
        StrainV2LoadTile(palette: palette, systemImage: "gauge.with.dots.needle.50percent", title: "Score", value: scoreText)
        StrainV2LoadTile(palette: palette, systemImage: "target", title: "Target", value: targetText)
      }

      HStack(spacing: 10) {
        StrainV2LoadTile(palette: palette, systemImage: "timer", title: "Duration", value: durationText)
        StrainV2LoadTile(palette: palette, systemImage: "flame.fill", title: "Energy", value: energyText)
      }

      StrainV2ZoneMeter(palette: palette)
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

struct StrainV2LoadTile: View {
  let palette: SleepV2Palette
  let systemImage: String
  let title: String
  let value: String

  var body: some View {
    HStack(alignment: .top, spacing: 10) {
      Image(systemName: systemImage)
        .font(.caption.weight(.semibold))
        .foregroundStyle(Color(red: 1.0, green: 0.52, blue: 0.18))
        .frame(width: 28, height: 28)
        .background(Color(red: 1.0, green: 0.52, blue: 0.18).opacity(0.12), in: Circle())
      VStack(alignment: .leading, spacing: 4) {
        Text(title)
          .font(.caption.weight(.semibold))
          .foregroundStyle(palette.secondaryText)
        Text(value)
          .font(.title3.weight(.semibold))
          .fontDesign(.rounded)
          .foregroundStyle(palette.text)
          .lineLimit(1)
          .minimumScaleFactor(0.7)
      }
      Spacer(minLength: 0)
    }
    .padding(12)
    .frame(maxWidth: .infinity, alignment: .leading)
    .background(palette.surfaceElevated.opacity(0.48), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
  }
}

struct StrainV2ZoneMeter: View {
  let palette: SleepV2Palette

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack {
        Text("Heart rate zones")
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(palette.text)
        Spacer()
        Text("0 min")
          .font(.subheadline.weight(.semibold))
          .fontDesign(.rounded)
          .foregroundStyle(palette.secondaryText)
      }

      HStack(spacing: 5) {
        ForEach(0..<5, id: \.self) { _ in
          Capsule()
            .fill(palette.separator.opacity(0.75))
            .frame(height: 9)
        }
      }

      HStack {
        ForEach(["Z1", "Z2", "Z3", "Z4", "Z5"], id: \.self) { zone in
          Text(zone)
            .font(.caption2.weight(.semibold))
            .foregroundStyle(palette.mutedText)
            .frame(maxWidth: .infinity)
        }
      }
    }
    .padding(14)
    .background(palette.surfaceElevated.opacity(0.48), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
  }
}

struct StrainV2EmptyStateCard: View {
  let palette: SleepV2Palette
  let systemImage: String
  let title: String
  let message: String

  var body: some View {
    SleepV2Panel(palette: palette, padding: 16, radius: 18) {
      HStack(alignment: .top, spacing: 12) {
        Image(systemName: systemImage)
          .font(.title3.weight(.semibold))
          .foregroundStyle(palette.mutedText)
          .frame(width: 40, height: 40)
          .background(palette.surfaceElevated.opacity(0.64), in: Circle())

        VStack(alignment: .leading, spacing: 5) {
          Text(title)
            .font(.headline.weight(.semibold))
            .foregroundStyle(palette.text)
          Text(message)
            .font(.subheadline)
            .foregroundStyle(palette.secondaryText)
            .fixedSize(horizontal: false, vertical: true)
        }
      }
      .frame(maxWidth: .infinity, alignment: .leading)
    }
  }
}

struct StrainV2InsightsSheet: View {
  let palette: SleepV2Palette
  var store: HealthDataStore
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    NavigationStack {
      ScrollView {
        VStack(alignment: .leading, spacing: 16) {
          StrainV2EmptyStateCard(
            palette: palette,
            systemImage: "exclamationmark.triangle",
            title: "No strain insights",
            message: store.strainEmptyStateSummary()
          )

          SleepV2Panel(palette: palette, padding: 16, radius: 18) {
            VStack(spacing: 0) {
              StrainV2FactRow(label: "Score", value: store.strainScoreDisplayText(), palette: palette)
              Divider().overlay(palette.separator)
              StrainV2FactRow(label: "Target", value: store.strainTargetDisplayText(), palette: palette)
              Divider().overlay(palette.separator)
              StrainV2FactRow(label: "Duration", value: store.strainDurationDisplayText(), palette: palette)
              Divider().overlay(palette.separator)
              StrainV2FactRow(label: "Total Energy", value: store.strainEnergyDisplayText(), palette: palette)
            }
          }
        }
        .padding(18)
      }
      .background(palette.background.ignoresSafeArea())
      .navigationTitle("Strain Data")
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
  }
}

struct StrainV2FactRow: View {
  let label: String
  let value: String
  let palette: SleepV2Palette

  var body: some View {
    HStack {
      Text(label)
        .font(.subheadline.weight(.semibold))
        .foregroundStyle(palette.secondaryText)
      Spacer(minLength: 12)
      Text(value)
        .font(.subheadline.weight(.semibold))
        .fontDesign(.rounded)
        .foregroundStyle(palette.text)
        .lineLimit(1)
        .minimumScaleFactor(0.7)
    }
    .padding(.vertical, 12)
  }
}

struct RecoveryV2EmptyStateCard: View {
  let palette: SleepV2Palette
  let systemImage: String
  let title: String
  let value: String

  var body: some View {
    SleepV2Panel(palette: palette, padding: 16, radius: 16) {
      HStack(spacing: 12) {
        Image(systemName: systemImage)
          .font(.headline.weight(.semibold))
          .foregroundStyle(palette.accent)
          .frame(width: 34, height: 34)
          .background(palette.accent.opacity(0.10), in: Circle())

        VStack(alignment: .leading, spacing: 4) {
          Text(title)
            .font(.headline.weight(.semibold))
            .foregroundStyle(palette.text)
          Text(value)
            .font(.subheadline.weight(.medium))
            .fontDesign(.rounded)
            .foregroundStyle(palette.secondaryText)
        }

        Spacer(minLength: 8)
      }
      .frame(maxWidth: .infinity, alignment: .leading)
    }
  }
}
