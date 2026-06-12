import SwiftUI

struct TrendsDashboardView: View {
  var store: HealthDataStore

  @State private var recoveryPoints: [(date: String, value: Double)] = []
  @State private var hrvPoints: [(date: String, value: Double)] = []
  @State private var strainPoints: [(date: String, value: Double)] = []
  @State private var isLoading = true

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 24) {
        Text("7-Day Trends")
          .font(.headline.weight(.semibold))
          .foregroundStyle(.primary)

        if isLoading {
          ProgressView()
            .frame(maxWidth: .infinity, minHeight: 200)
        } else {
          TrendsSparklineCard(metric: .recovery, points: recoveryPoints, tint: .green)
          TrendsSparklineCard(metric: .hrv, points: hrvPoints, tint: Color(red: 0.33, green: 0.72, blue: 0.70))
          TrendsSparklineCard(metric: .strain, points: strainPoints, tint: .orange)
        }
      }
      .padding(.horizontal, 16)
      .padding(.vertical, 16)
    }
    .gooseScreenBackground()
    .navigationTitle("Trends")
    .navigationBarTitleDisplayMode(.inline)
    .task {
      await loadTrends()
    }
  }

  private func loadTrends() async {
    async let recovery = (try? await store.fetchTrendsSeries(metricName: "recovery")) ?? []
    async let hrv = (try? await store.fetchTrendsSeries(metricName: "hrv")) ?? []
    async let strain = (try? await store.fetchTrendsSeries(metricName: "strain")) ?? []
    recoveryPoints = await recovery
    hrvPoints = await hrv
    strainPoints = await strain
    isLoading = false
  }
}

// MARK: - Metric Definition

private enum TrendsMetric {
  case recovery, hrv, strain

  var title: String {
    switch self {
    case .recovery: "Recovery"
    case .hrv: "HRV"
    case .strain: "Strain"
    }
  }

  var systemImage: String {
    switch self {
    case .recovery: "battery.100percent"
    case .hrv: "waveform.path.ecg"
    case .strain: "figure.run"
    }
  }
}

// MARK: - SparklineCard

private struct TrendsSparklineCard: View {
  let metric: TrendsMetric
  let points: [(date: String, value: Double)]
  let tint: Color

  private var latestValueFormatted: String {
    guard let last = points.last else { return "--" }
    return String(format: "%.0f", last.value)
  }

  private var dateLabels: [String] {
    let formatter = DateFormatter()
    formatter.dateFormat = "yyyy-MM-dd"
    let display = DateFormatter()
    display.dateFormat = "M/d"
    return points.compactMap { formatter.date(from: $0.date) }.map { display.string(from: $0) }
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack(spacing: 8) {
        Image(systemName: metric.systemImage)
          .font(.system(size: 14, weight: .semibold))
          .foregroundStyle(tint)
          .frame(width: 28, height: 28)
          .background(tint.opacity(0.14), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        Text(metric.title)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(.primary)
        Spacer()
        Text(latestValueFormatted)
          .font(.title3.weight(.semibold))
          .fontDesign(.rounded)
          .foregroundStyle(tint)
      }

      if points.isEmpty {
        Text("No data for the last 7 days")
          .font(.caption)
          .foregroundStyle(.secondary)
          .frame(maxWidth: .infinity, minHeight: 52)
      } else {
        TrendsSparklineShape(points: points.map(\.value))
          .stroke(tint, style: StrokeStyle(lineWidth: 2.5, lineCap: .round, lineJoin: .round))
          .frame(height: 52)
          .background(alignment: .bottom) {
            TrendsSparklineFillShape(points: points.map(\.value))
              .fill(tint.opacity(0.07))
          }
          .accessibilityHidden(true)
          .accessibilityLabel("\(metric.title) trend over 7 days")

        if dateLabels.count >= 2 {
          HStack {
            Text(dateLabels.first ?? "")
            Spacer()
            if dateLabels.count >= 4 { Text(dateLabels[dateLabels.count / 2]) }
            Spacer()
            Text(dateLabels.last ?? "")
          }
          .font(.caption.weight(.semibold))
          .foregroundStyle(.secondary)
        }
      }
    }
    .padding(16)
    .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
  }
}

// MARK: - Sparkline Shapes

private struct TrendsSparklineShape: Shape {
  let points: [Double]

  func path(in rect: CGRect) -> Path {
    guard points.count > 1 else { return Path() }
    let minVal = points.min() ?? 0
    let maxVal = points.max() ?? 1
    let range = max(maxVal - minVal, 1)
    var path = Path()
    for (i, value) in points.enumerated() {
      let x = rect.width * CGFloat(i) / CGFloat(points.count - 1)
      let y = rect.height * CGFloat(1.0 - (value - minVal) / range)
      let pt = CGPoint(x: x, y: max(0, min(rect.height, y)))
      if i == 0 { path.move(to: pt) } else { path.addLine(to: pt) }
    }
    return path
  }
}

private struct TrendsSparklineFillShape: Shape {
  let points: [Double]

  func path(in rect: CGRect) -> Path {
    guard points.count > 1 else { return Path() }
    let minVal = points.min() ?? 0
    let maxVal = points.max() ?? 1
    let range = max(maxVal - minVal, 1)
    var path = Path()
    for (i, value) in points.enumerated() {
      let x = rect.width * CGFloat(i) / CGFloat(points.count - 1)
      let y = rect.height * CGFloat(1.0 - (value - minVal) / range)
      let pt = CGPoint(x: x, y: max(0, min(rect.height, y)))
      if i == 0 { path.move(to: pt) } else { path.addLine(to: pt) }
    }
    // Close fill area to bottom
    path.addLine(to: CGPoint(x: rect.width, y: rect.height))
    path.addLine(to: CGPoint(x: 0, y: rect.height))
    path.closeSubpath()
    return path
  }
}
