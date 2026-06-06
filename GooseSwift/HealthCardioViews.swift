import Darwin
import Foundation
import SwiftUI
import UIKit

struct CardioLoadSheet: View {
  @Environment(\.dismiss) private var dismiss
  var store: HealthDataStore

  var body: some View {
    NavigationStack {
      CardioLoadDetailSurface(store: store, closeAction: { dismiss() })
    }
    .presentationDetents([.large])
    .presentationDragIndicator(.hidden)
  }
}

struct CardioLoadView: View {
  var store: HealthDataStore

  var body: some View {
    CardioLoadDetailSurface(store: store, closeAction: nil)
  }
}

struct CardioLoadDetailSurface: View {
  var store: HealthDataStore
  let closeAction: (() -> Void)?
  @State private var selectedRange = "30D"
  @State private var selectedDayID: String?
  @State private var showingCalendarPicker = false
  @State private var showingHelp = false

  private let ranges = ["30D", "3M", "6M", "1Y"]

  var body: some View {
    ScrollView {
      LazyVStack(alignment: .leading, spacing: 20) {
        CardioLoadHeroSection(
          day: displayedDay,
          rangeLabel: currentRangeLabel,
          selectedRange: selectedRange,
          hasSelection: selectedDayID != nil
        )

        CardioLoadTrendPanel(
          days: visibleDays,
          selectedDayID: $selectedDayID,
          selectedRange: $selectedRange,
          ranges: ranges,
          openCalendar: { showingCalendarPicker = true },
          selectPrevious: { selectAdjacentDay(-1) },
          selectNext: { selectAdjacentDay(1) }
        )

        CardioLoadBreakdownSection(rows: statusBreakdownRows)

        CardioLoadResourcesSection()

        CardioLoadInfoSection()
      }
      .padding(.horizontal, 18)
      .padding(.top, 18)
      .padding(.bottom, 34)
    }
    .background(cardioLoadBackground.ignoresSafeArea())
    .navigationTitle("Cardio Load")
    .navigationBarTitleDisplayMode(.inline)
    .toolbarBackground(.hidden, for: .navigationBar)
    .toolbarColorScheme(.dark, for: .navigationBar)
    .toolbar {
      if let closeAction {
        ToolbarItem(placement: .topBarLeading) {
          Button(action: closeAction) {
            Image(systemName: "xmark")
              .font(.headline.weight(.semibold))
          }
          .accessibilityLabel("Close")
        }
      }
      ToolbarItem(placement: .topBarTrailing) {
        Button {
          showingHelp = true
        } label: {
          Image(systemName: "questionmark.circle")
            .font(.headline.weight(.semibold))
        }
        .accessibilityLabel("Cardio Load Help")
      }
    }
    .sheet(isPresented: $showingCalendarPicker) {
      CardioLoadCalendarSheet(days: visibleDays, selectedDayID: $selectedDayID)
    }
    .sheet(isPresented: $showingHelp) {
      CardioLoadCalibrationSheet()
    }
    .onChange(of: selectedRange) { _, _ in
      selectedDayID = nil
    }
  }

  private var visibleDays: [CardioLoadDay] {
    store.cardioLoadPoints(range: selectedRange)
  }

  private var displayedDay: CardioLoadDay? {
    if let selectedDayID,
       let selected = visibleDays.first(where: { $0.id == selectedDayID }) {
      return selected
    }
    return visibleDays.last
  }

  private var currentRangeLabel: String {
    guard !visibleDays.isEmpty else {
      return "No range"
    }
    let values = visibleDays.map(\.load)
    let low = Int((values.min() ?? 0).rounded())
    let high = Int((values.max() ?? 0).rounded())
    return "\(low) - \(high)"
  }

  private var statusBreakdownRows: [CardioLoadStatusBreakdown] {
    let statuses = ["Calibrating", "Detraining", "Maintaining", "Peaking", "Productive", "Fatigued", "Overtraining"]
    guard !visibleDays.isEmpty else {
      return statuses.map {
        CardioLoadStatusBreakdown(status: $0, days: 0, percent: 0, color: cardioLoadStatusColor($0))
      }
    }
    let grouped = Dictionary(grouping: visibleDays, by: \.status)
    return statuses.map { status in
      let count = grouped[status]?.count ?? 0
      return CardioLoadStatusBreakdown(
        status: status,
        days: count,
        percent: Double(count) / Double(visibleDays.count),
        color: cardioLoadStatusColor(status)
      )
    }
  }

  private func selectAdjacentDay(_ offset: Int) {
    guard !visibleDays.isEmpty else {
      return
    }
    let currentIndex = selectedDayID.flatMap { id in
      visibleDays.firstIndex { $0.id == id }
    } ?? visibleDays.count - 1
    let nextIndex = min(max(currentIndex + offset, 0), visibleDays.count - 1)
    selectedDayID = visibleDays[nextIndex].id
  }
}

struct CardioLoadHeroSection: View {
  let day: CardioLoadDay?
  let rangeLabel: String
  let selectedRange: String
  let hasSelection: Bool

  var body: some View {
    HStack(alignment: .bottom, spacing: 18) {
      VStack(alignment: .leading, spacing: 8) {
        Text(valueText)
          .font(.system(size: 48, weight: .semibold, design: .rounded))
          .monospacedDigit()
          .foregroundStyle(.white)
          .lineLimit(1)
          .minimumScaleFactor(0.75)

        Text(dateText)
          .font(.callout.weight(.semibold))
          .foregroundStyle(.white.opacity(0.58))
          .lineLimit(1)
      }

      Spacer(minLength: 14)

      VStack(alignment: .trailing, spacing: 8) {
        Text(statusText)
          .font(.headline.weight(.bold))
          .foregroundStyle(.white.opacity(0.52))
          .lineLimit(1)
          .minimumScaleFactor(0.75)

        HStack(spacing: 8) {
          RoundedRectangle(cornerRadius: 1.5, style: .continuous)
            .fill(.white.opacity(0.36))
            .frame(width: 18, height: 3)
          Text(rangeLabel)
            .font(.headline.weight(.bold))
            .monospacedDigit()
            .foregroundStyle(.white)
        }
        .lineLimit(1)
        .minimumScaleFactor(0.75)
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }

  private var valueText: String {
    guard let day else {
      return "--"
    }
    return "\(Int(day.load.rounded()))"
  }

  private var dateText: String {
    guard let day else {
      return "No data"
    }
    return hasSelection ? day.dateLabel : "Latest · \(selectedRange)"
  }

  private var statusText: String {
    day?.status ?? "No data"
  }
}

struct CardioLoadTrendPanel: View {
  let days: [CardioLoadDay]
  @Binding var selectedDayID: String?
  @Binding var selectedRange: String
  let ranges: [String]
  let openCalendar: () -> Void
  let selectPrevious: () -> Void
  let selectNext: () -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      CardioLoadDetailChart(days: days, selectedDayID: $selectedDayID)
        .frame(height: 336)

      HStack(spacing: 12) {
        Button(action: selectPrevious) {
          Image(systemName: "chevron.left")
            .font(.headline.weight(.bold))
            .frame(width: 40, height: 40)
        }
        .buttonStyle(.plain)
        .background(Color.white.opacity(0.07), in: Circle())
        .foregroundStyle(.white)
        .accessibilityLabel("Previous Cardio Load Point")

        Picker("Range", selection: $selectedRange) {
          ForEach(ranges, id: \.self) { range in
            Text(range).tag(range)
          }
        }
        .pickerStyle(.segmented)
        .tint(.white.opacity(0.62))

        Button(action: openCalendar) {
          Image(systemName: "calendar")
            .font(.headline.weight(.bold))
            .frame(width: 40, height: 40)
        }
        .buttonStyle(.plain)
        .background(Color.white.opacity(0.07), in: Circle())
        .foregroundStyle(.white)
        .accessibilityLabel("Pick Cardio Load Date")

        Button(action: selectNext) {
          Image(systemName: "chevron.right")
            .font(.headline.weight(.bold))
            .frame(width: 40, height: 40)
        }
        .buttonStyle(.plain)
        .background(Color.white.opacity(0.07), in: Circle())
        .foregroundStyle(.white)
        .accessibilityLabel("Next Cardio Load Point")
      }
    }
  }
}

struct CardioLoadDetailChart: View {
  let days: [CardioLoadDay]
  @Binding var selectedDayID: String?

  var body: some View {
    GeometryReader { proxy in
      if days.isEmpty {
        RoundedRectangle(cornerRadius: 16, style: .continuous)
          .fill(Color.white.opacity(0.07))
          .overlay {
            ContentUnavailableView("No Cardio Load", systemImage: "heart.circle", description: Text("Cardio Load needs activity sessions and heart-rate data."))
              .foregroundStyle(.white)
          }
      } else {
        ZStack(alignment: .topLeading) {
          RoundedRectangle(cornerRadius: 16, style: .continuous)
            .fill(Color.white.opacity(0.045))

          chartGrid(in: proxy.size)
          cardioLoadBand(in: proxy.size)
            .fill(Color(red: 0.55, green: 0.48, blue: 1.0).opacity(0.24))
          activityFloor(in: proxy.size)
          trendPath(in: proxy.size)
            .stroke(
              Color(red: 0.78, green: 0.72, blue: 1.0),
              style: StrokeStyle(lineWidth: 4, lineCap: .round, lineJoin: .round)
            )

          ForEach(Array(days.enumerated()), id: \.element.id) { index, day in
            let point = chartPoint(index: index, load: day.load, size: proxy.size)
            Circle()
              .fill(cardioLoadStatusColor(day.status))
              .frame(width: day.id == selectedDayID ? 12 : 8, height: day.id == selectedDayID ? 12 : 8)
              .overlay {
                Circle()
                  .stroke(Color(red: 0.18, green: 0.19, blue: 0.23), lineWidth: 4)
              }
              .position(point)
              .opacity(day.id == selectedDayID || shouldShowPoint(index) ? 1 : 0)
          }

          if let selectedIndex,
             days.indices.contains(selectedIndex) {
            let selected = days[selectedIndex]
            let point = chartPoint(index: selectedIndex, load: selected.load, size: proxy.size)
            Rectangle()
              .fill(Color.white.opacity(0.18))
              .frame(width: 1, height: plotHeight(size: proxy.size))
              .position(x: point.x, y: plotTop + plotHeight(size: proxy.size) / 2)
            VStack(spacing: 3) {
              Text("\(Int(selected.load.rounded()))")
                .font(.headline.weight(.bold))
                .monospacedDigit()
              Text(selected.dateLabel)
                .font(.caption2.weight(.bold))
                .foregroundStyle(.white.opacity(0.62))
            }
            .foregroundStyle(.white)
            .padding(.horizontal, 9)
            .padding(.vertical, 6)
            .background(Color(red: 0.12, green: 0.13, blue: 0.16).opacity(0.94), in: Capsule())
            .overlay {
              Capsule().strokeBorder(Color.white.opacity(0.12), lineWidth: 1)
            }
            .position(x: min(max(point.x, 42), proxy.size.width - 42), y: max(point.y - 42, 30))
            .zIndex(10)
          }
        }
        .contentShape(Rectangle())
        .gesture(
          DragGesture(minimumDistance: 0)
            .onChanged { value in
              selectedDayID = dayID(at: value.location.x, width: proxy.size.width)
            }
        )
      }
    }
  }

  @ViewBuilder
  private func chartGrid(in size: CGSize) -> some View {
    ForEach(yMarks, id: \.self) { mark in
      let y = yPosition(load: mark, height: size.height)
      Rectangle()
        .fill(Color.white.opacity(mark == 0 ? 0.14 : 0.07))
        .frame(width: plotWidth(size: size), height: 1)
        .position(x: plotLeft + plotWidth(size: size) / 2, y: y)
      Text("\(Int(mark))")
        .font(.caption.weight(.bold))
        .monospacedDigit()
        .foregroundStyle(.white.opacity(0.24))
        .position(x: size.width - 18, y: y)
    }

    ForEach(axisLabelIndices, id: \.self) { index in
      Text(days[index].dateLabel)
        .font(.caption.weight(.bold))
        .foregroundStyle(.white.opacity(0.34))
        .position(x: chartPoint(index: index, load: 0, size: size).x, y: size.height - 18)
    }
  }

  @ViewBuilder
  private func activityFloor(in size: CGSize) -> some View {
    let baselineY = size.height - 36
    ForEach(Array(days.enumerated()), id: \.element.id) { index, day in
      Capsule()
        .fill(cardioLoadStatusColor(day.status).opacity(0.50))
        .frame(width: max(3, plotWidth(size: size) / CGFloat(max(days.count, 1)) * 0.62), height: 5)
        .position(x: chartPoint(index: index, load: 0, size: size).x, y: baselineY)
    }
  }

  private var selectedIndex: Int? {
    selectedDayID.flatMap { id in
      days.firstIndex { $0.id == id }
    }
  }

  private var axisLabelIndices: [Int] {
    guard !days.isEmpty else {
      return []
    }
    let anchors = [0, days.count / 3, (days.count * 2) / 3, days.count - 1]
    return Array(Set(anchors)).sorted()
  }

  private func trendPath(in size: CGSize) -> Path {
    Path { path in
      for (index, day) in days.enumerated() {
        let point = chartPoint(index: index, load: day.load, size: size)
        if index == 0 {
          path.move(to: point)
        } else {
          path.addLine(to: point)
        }
      }
    }
  }

  private func cardioLoadBand(in size: CGSize) -> Path {
    let values = days.map(\.load)
    var upper: [CGPoint] = []
    var lower: [CGPoint] = []
    for (index, value) in values.enumerated() {
      upper.append(chartPoint(index: index, load: min(value * 1.12 + yAxisMax * 0.08, yAxisMax), size: size))
      lower.append(chartPoint(index: index, load: max(value * 0.72 - 2, 0), size: size))
    }
    return Path { path in
      guard let first = upper.first else {
        return
      }
      path.move(to: first)
      upper.dropFirst().forEach { path.addLine(to: $0) }
      lower.reversed().forEach { path.addLine(to: $0) }
      path.closeSubpath()
    }
  }

  private func shouldShowPoint(_ index: Int) -> Bool {
    days.count <= 14 || index == 0 || index == days.count - 1 || index.isMultiple(of: max(days.count / 8, 1))
  }

  private func dayID(at x: CGFloat, width: CGFloat) -> String? {
    guard !days.isEmpty else {
      return nil
    }
    let relative = min(max((x - plotLeft) / max(width - plotLeft - plotRight, 1), 0), 1)
    let index = Int((relative * CGFloat(days.count - 1)).rounded())
    return days[min(max(index, 0), days.count - 1)].id
  }

  private func chartPoint(index: Int, load: Double, size: CGSize) -> CGPoint {
    let x = plotLeft + plotWidth(size: size) * CGFloat(index) / CGFloat(max(days.count - 1, 1))
    return CGPoint(x: x, y: yPosition(load: load, height: size.height))
  }

  private func yPosition(load: Double, height: CGFloat) -> CGFloat {
    let normalized = min(max(load / yAxisMax, 0), 1)
    return plotTop + plotHeight(size: CGSize(width: 1, height: height)) * CGFloat(1 - normalized)
  }

  private var yMarks: [Double] {
    [yAxisMax, yAxisMax * 2 / 3, yAxisMax / 3, 0]
      .map { (($0 / 5).rounded()) * 5 }
  }

  private var yAxisMax: Double {
    let maximumLoad = days.map(\.load).max() ?? 0
    return max(20, ((maximumLoad * 1.18) / 10).rounded(.up) * 10)
  }

  private func plotWidth(size: CGSize) -> CGFloat {
    max(size.width - plotLeft - plotRight, 1)
  }

  private func plotHeight(size: CGSize) -> CGFloat {
    max(size.height - plotTop - plotBottom, 1)
  }

  private var plotLeft: CGFloat { 12 }
  private var plotRight: CGFloat { 44 }
  private var plotTop: CGFloat { 20 }
  private var plotBottom: CGFloat { 46 }
}

struct CardioLoadBreakdownSection: View {
  let rows: [CardioLoadStatusBreakdown]

  var body: some View {
    VStack(alignment: .leading, spacing: 14) {
      Text("Cardio Status Breakdown")
        .font(.title2.weight(.bold))
        .foregroundStyle(.white)

      VStack(spacing: 0) {
        HStack(spacing: 10) {
          Text("Status")
            .frame(width: 112, alignment: .leading)
          Text("Duration")
            .frame(width: 50, alignment: .leading)
          Text("")
            .frame(maxWidth: .infinity)
          Text("%")
            .frame(width: 42, alignment: .trailing)
        }
        .font(.caption.weight(.bold))
        .foregroundStyle(.white.opacity(0.28))
        .padding(.horizontal, 14)
        .padding(.top, 14)
        .padding(.bottom, 8)

        ForEach(Array(rows.enumerated()), id: \.element.id) { index, row in
          CardioLoadStatusRowView(row: row, showsDivider: index < rows.count - 1)
        }
      }
      .background(Color.white.opacity(0.075), in: RoundedRectangle(cornerRadius: 20, style: .continuous))
      .overlay {
        RoundedRectangle(cornerRadius: 20, style: .continuous)
          .strokeBorder(Color.white.opacity(0.07), lineWidth: 1)
      }
    }
  }
}

struct CardioLoadStatusRowView: View {
  let row: CardioLoadStatusBreakdown
  let showsDivider: Bool

  var body: some View {
    VStack(spacing: 0) {
      HStack(spacing: 10) {
        Text(row.status)
          .font(.system(size: 15, weight: .semibold, design: .rounded))
          .foregroundStyle(.white)
          .lineLimit(1)
          .frame(width: 112, alignment: .leading)

        Text("\(row.days)d")
          .font(.system(size: 15, weight: .semibold, design: .rounded))
          .monospacedDigit()
          .foregroundStyle(.white)
          .frame(width: 50, alignment: .leading)

        GeometryReader { proxy in
          ZStack(alignment: .leading) {
            Capsule()
              .fill(Color.white.opacity(0.07))
            Capsule()
              .fill(row.percent == 0 ? Color.white.opacity(0.08) : row.color)
              .frame(width: max(row.percent == 0 ? 0 : 8, proxy.size.width * row.percent))
          }
        }
        .frame(maxWidth: .infinity)
        .frame(height: 10)

        Text("\(Int((row.percent * 100).rounded()))%")
          .font(.system(size: 15, weight: .semibold, design: .rounded))
          .monospacedDigit()
          .foregroundStyle(.white)
          .frame(width: 42, alignment: .trailing)
      }
      .padding(.horizontal, 14)
      .padding(.vertical, 14)

      if showsDivider {
        Rectangle()
          .fill(Color.white.opacity(0.06))
          .frame(height: 1)
          .padding(.leading, 16)
      }
    }
  }
}

struct CardioLoadResourcesSection: View {
  var body: some View {
    VStack(alignment: .leading, spacing: 14) {
      Text("Resources")
        .font(.title2.weight(.bold))
        .foregroundStyle(.white)

      HStack(spacing: 12) {
        CardioLoadResourceCard(
          title: "The Basics: Cardio Load",
          subtitle: "Train smarter by balancing short-term and long-term load.",
          systemImage: "shoeprints.fill",
          colors: [
            Color(red: 0.42, green: 0.47, blue: 0.52),
            Color(red: 0.18, green: 0.19, blue: 0.22),
          ]
        )
        CardioLoadResourceCard(
          title: "Cardio Status",
          subtitle: "Understand when load is building, stable, or falling.",
          systemImage: "figure.run",
          colors: [
            Color(red: 0.12, green: 0.38, blue: 0.30),
            Color(red: 0.16, green: 0.18, blue: 0.22),
          ]
        )
      }
    }
  }
}

struct CardioLoadResourceCard: View {
  let title: String
  let subtitle: String
  let systemImage: String
  let colors: [Color]

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      ZStack {
        LinearGradient(colors: colors, startPoint: .topLeading, endPoint: .bottomTrailing)
        Image(systemName: systemImage)
          .font(.system(size: 42, weight: .bold))
          .foregroundStyle(.white.opacity(0.74))
      }
      .frame(height: 104)
      .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))

      Text(title)
        .font(.subheadline.weight(.bold))
        .foregroundStyle(.white)
        .lineLimit(2)
        .fixedSize(horizontal: false, vertical: true)

      Text(subtitle)
        .font(.caption.weight(.semibold))
        .foregroundStyle(.white.opacity(0.45))
        .lineLimit(2)
        .fixedSize(horizontal: false, vertical: true)
    }
    .frame(maxWidth: .infinity, alignment: .topLeading)
  }
}

struct CardioLoadInfoSection: View {
  var body: some View {
    VStack(alignment: .leading, spacing: 18) {
      Label("Cardio Load", systemImage: "info.circle")
        .font(.headline.weight(.bold))
        .foregroundStyle(.white.opacity(0.42))

      Text("Cardio Load helps gauge whether training is building, maintaining, or trending toward overtraining or detraining. It compares your short-term load from recent activity against a longer-term baseline.")
        .font(.body.weight(.semibold))
        .lineSpacing(5)
        .foregroundStyle(.white.opacity(0.54))

      Text("The shaded range represents your current adaptive band. Consistent activity and heart-rate data make the band more reliable over time.")
        .font(.body.weight(.semibold))
        .lineSpacing(5)
        .foregroundStyle(.white.opacity(0.54))

      VStack(alignment: .leading, spacing: 12) {
        CardioLoadLegendRow(status: "Calibrating", text: "Building your training profile.", color: cardioLoadStatusColor("Calibrating"))
        CardioLoadLegendRow(status: "Detraining", text: "Load is falling below your recent baseline.", color: cardioLoadStatusColor("Detraining"))
        CardioLoadLegendRow(status: "Maintaining", text: "Training is steady and balanced.", color: cardioLoadStatusColor("Maintaining"))
        CardioLoadLegendRow(status: "Productive", text: "Load is rising at a useful pace.", color: cardioLoadStatusColor("Productive"))
        CardioLoadLegendRow(status: "Fatigued", text: "Load is high relative to recovery.", color: cardioLoadStatusColor("Fatigued"))
        CardioLoadLegendRow(status: "Overtraining", text: "Sustained load may be too high.", color: cardioLoadStatusColor("Overtraining"))
      }
    }
    .padding(18)
    .background(Color.white.opacity(0.055), in: RoundedRectangle(cornerRadius: 20, style: .continuous))
  }
}

struct CardioLoadLegendRow: View {
  let status: String
  let text: String
  let color: Color

  var body: some View {
    HStack(alignment: .firstTextBaseline, spacing: 10) {
      Circle()
        .fill(color)
        .frame(width: 8, height: 8)
      Text(status + ":")
        .font(.body.weight(.bold))
        .foregroundStyle(.white.opacity(0.62))
      Text(text)
        .font(.body.weight(.semibold))
        .foregroundStyle(.white.opacity(0.46))
      Spacer(minLength: 0)
    }
  }
}

struct CardioLoadCalendarSheet: View {
  @Environment(\.dismiss) private var dismiss
  let days: [CardioLoadDay]
  @Binding var selectedDayID: String?

  var body: some View {
    NavigationStack {
      ScrollView {
        LazyVStack(spacing: 0) {
          ForEach(Array(days.reversed().enumerated()), id: \.element.id) { index, day in
            Button {
              selectedDayID = day.id
              dismiss()
            } label: {
              HStack(spacing: 14) {
                Circle()
                  .fill(cardioLoadStatusColor(day.status))
                  .frame(width: 10, height: 10)
                VStack(alignment: .leading, spacing: 2) {
                  Text(day.dateLabel)
                    .font(.headline.weight(.bold))
                  Text(day.status)
                    .font(.caption.weight(.bold))
                    .foregroundStyle(.secondary)
                }
                Spacer()
                Text("\(Int(day.load.rounded()))")
                  .font(.title3.weight(.bold))
                  .monospacedDigit()
                if day.id == selectedDayID {
                  Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(.green)
                }
              }
              .foregroundStyle(.primary)
              .padding(.vertical, 14)
              .padding(.horizontal, 18)
            }
            .buttonStyle(.plain)

            if index < days.count - 1 {
              Divider()
                .padding(.leading, 42)
            }
          }
        }
        .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        .padding(18)
      }
    }
    .navigationTitle("Pick Date")
    .navigationBarTitleDisplayMode(.inline)
    .gooseScreenBackground()
    .presentationDetents([.medium, .large])
  }
}

struct CardioLoadCalibrationSheet: View {
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    VStack(alignment: .leading, spacing: 24) {
      Button {
        dismiss()
      } label: {
        Image(systemName: "xmark")
          .font(.headline.weight(.bold))
          .foregroundStyle(.white)
          .frame(width: 44, height: 44)
          .background(Color.black.opacity(0.22), in: Circle())
      }
      .buttonStyle(.plain)
      .accessibilityLabel("Close")

      HStack(spacing: 8) {
        ForEach(calibrationDays) { day in
          VStack(spacing: 8) {
            ZStack {
              Circle()
                .fill(day.fill)
                .frame(width: 44, height: 44)
                .shadow(color: day.glow, radius: day.isCurrent ? 14 : 0)
              if day.isFuture {
                Circle()
                  .strokeBorder(Color.white.opacity(0.56), style: StrokeStyle(lineWidth: 3, dash: [4, 5]))
                  .frame(width: 44, height: 44)
              } else if let systemImage = day.systemImage {
                Image(systemName: systemImage)
                  .font(.system(size: 19, weight: .bold))
                  .foregroundStyle(.white.opacity(0.86))
              }
            }
            Text(day.label)
              .font(.caption2.weight(.bold))
              .foregroundStyle(.white.opacity(day.isFuture ? 0.30 : 0.62))
          }
          .frame(maxWidth: .infinity)
        }
      }
      .frame(maxWidth: .infinity)

      VStack(alignment: .center, spacing: 12) {
        Text("Cardio Load Calibration")
          .font(.title2.weight(.bold))
          .foregroundStyle(.white)
          .multilineTextAlignment(.center)
        Text("Goose uses historical activity and heart-rate data to calibrate Cardio Load. Consistent workout logging improves the dynamic range, and up to 6 weeks of data gives the most reliable status.")
          .font(.callout.weight(.semibold))
          .lineSpacing(5)
          .foregroundStyle(.white.opacity(0.58))
          .multilineTextAlignment(.center)
      }
      .frame(maxWidth: .infinity)
      Spacer(minLength: 0)
    }
    .padding(.horizontal, 22)
    .padding(.top, 22)
    .padding(.bottom, 24)
    .background(Color(red: 0.19, green: 0.20, blue: 0.24).ignoresSafeArea())
    .presentationDetents([.medium])
    .presentationDragIndicator(.hidden)
  }

  private var calibrationDays: [CardioLoadCalibrationDay] {
    [
      CardioLoadCalibrationDay(label: "Sat", systemImage: "bicycle", fill: .green.opacity(0.25), glow: .clear, isCurrent: false, isFuture: false),
      CardioLoadCalibrationDay(label: "Sun", systemImage: "figure.run", fill: .green.opacity(0.62), glow: .clear, isCurrent: false, isFuture: false),
      CardioLoadCalibrationDay(label: "Mon", systemImage: "bicycle", fill: .green, glow: .green.opacity(0.55), isCurrent: true, isFuture: false),
      CardioLoadCalibrationDay(label: "Tue", systemImage: nil, fill: .white.opacity(0.11), glow: .clear, isCurrent: false, isFuture: false),
      CardioLoadCalibrationDay(label: "Wed", systemImage: nil, fill: .clear, glow: .clear, isCurrent: false, isFuture: true),
      CardioLoadCalibrationDay(label: "Thu", systemImage: nil, fill: .clear, glow: .clear, isCurrent: false, isFuture: true),
    ]
  }
}

struct CardioLoadCalibrationDay: Identifiable {
  let id = UUID()
  let label: String
  let systemImage: String?
  let fill: Color
  let glow: Color
  let isCurrent: Bool
  let isFuture: Bool
}

struct CardioLoadStatusBreakdown: Identifiable {
  let status: String
  let days: Int
  let percent: Double
  let color: Color

  var id: String { status }
}

private let cardioLoadBackground = Color(red: 0.10, green: 0.11, blue: 0.14)

private func cardioLoadStatusColor(_ status: String) -> Color {
  switch status {
  case "Calibrating":
    return Color.white.opacity(0.58)
  case "Detraining":
    return Color(red: 1.0, green: 0.68, blue: 0.25)
  case "Maintaining":
    return Color(red: 0.75, green: 0.68, blue: 1.0)
  case "Peaking":
    return Color(red: 0.32, green: 0.72, blue: 1.0)
  case "Productive":
    return Color(red: 0.38, green: 0.76, blue: 0.30)
  case "Fatigued":
    return Color(red: 0.96, green: 0.48, blue: 0.70)
  case "Overtraining":
    return Color(red: 0.95, green: 0.34, blue: 0.55)
  default:
    return Color(red: 0.78, green: 0.72, blue: 1.0)
  }
}

private let cardioLoadDateFormatter: DateFormatter = {
  let formatter = DateFormatter()
  formatter.setLocalizedDateFormatFromTemplate("d MMM")
  return formatter
}()
