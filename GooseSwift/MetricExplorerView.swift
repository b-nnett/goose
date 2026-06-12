import SwiftUI

// MARK: - MetricRow

private struct MetricRow: Identifiable {
  let id: String
  let name: String
  let systemImage: String
  let tint: Color
  let displayValue: String
  let timestamp: String
}

// MARK: - MetricExplorerView

struct MetricExplorerView: View {
  var healthStore: HealthDataStore

  // MARK: - Data

  private var readinessRows: [MetricRow] {
    [
      row(for: .recovery),
      row(for: .strain),
    ]
  }

  private var sleepRows: [MetricRow] {
    [
      row(for: .sleep),
    ]
  }

  private var stressRows: [MetricRow] {
    [
      row(for: .stress),
    ]
  }

  private var cardiovascularRows: [MetricRow] {
    var rows: [MetricRow] = []
    // HRV — via energyBank proxy isn't right; use cardioLoad for cardiovascular signal
    rows.append(row(for: .cardioLoad))
    // RHR — derived from HeartRateSeriesStore, not a HealthRoute
    if let estimate = HeartRateSeriesStore.shared.restingEstimate() {
      let bpm = Int(estimate.bpm.rounded())
      rows.append(MetricRow(
        id: "rhr",
        name: "Resting HR",
        systemImage: "heart.fill",
        tint: .red,
        displayValue: "\(bpm) bpm",
        timestamp: estimate.updatedAt.map { timestampLabel(for: $0) } ?? "No data"
      ))
    } else {
      rows.append(MetricRow(
        id: "rhr",
        name: "Resting HR",
        systemImage: "heart.fill",
        tint: .red,
        displayValue: "—",
        timestamp: "No data"
      ))
    }
    return rows
  }

  private var energyRows: [MetricRow] {
    [
      row(for: .energyBank),
    ]
  }

  private var allRows: [MetricRow] {
    readinessRows + sleepRows + stressRows + cardiovascularRows + energyRows
  }

  // MARK: - Body

  var body: some View {
    Group {
      if allRows.isEmpty {
        ContentUnavailableView(
          "No Metrics Yet",
          systemImage: "chart.bar.xaxis",
          description: Text("Sync your WHOOP band to populate metrics.")
        )
      } else {
        List {
          Section {
            ForEach(readinessRows) { metricRowView($0) }
          } header: {
            sectionHeader("READINESS")
          }

          Section {
            ForEach(sleepRows) { metricRowView($0) }
          } header: {
            sectionHeader("SLEEP")
          }

          Section {
            ForEach(stressRows) { metricRowView($0) }
          } header: {
            sectionHeader("STRESS")
          }

          Section {
            ForEach(cardiovascularRows) { metricRowView($0) }
          } header: {
            sectionHeader("CARDIOVASCULAR")
          }

          Section {
            ForEach(energyRows) { metricRowView($0) }
          } header: {
            sectionHeader("ENERGY")
          }
        }
        .listStyle(.insetGrouped)
        .gooseListBackground()
      }
    }
    .navigationTitle("Metric Explorer")
    .navigationBarTitleDisplayMode(.inline)
    .toolbarBackground(.hidden, for: .navigationBar)
    .toolbar(.hidden, for: .tabBar)
  }

  // MARK: - Row View

  @ViewBuilder
  private func metricRowView(_ row: MetricRow) -> some View {
    HStack(spacing: 12) {
      Image(systemName: row.systemImage)
        .font(.caption.weight(.bold))
        .foregroundStyle(row.tint)
        .frame(width: 28, height: 28)
        .background(row.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 6))

      VStack(alignment: .leading, spacing: 2) {
        Text(row.name)
          .font(.subheadline)
        Text(row.timestamp)
          .font(.caption2)
          .foregroundStyle(.tertiary)
      }

      Spacer()

      Text(row.displayValue)
        .font(.subheadline.weight(.semibold))
        .fontDesign(.rounded)
        .foregroundStyle(row.displayValue == "—" ? Color.secondary : Color.primary)
    }
    .padding(.vertical, 4)
    .accessibilityElement(children: .combine)
    .accessibilityLabel("\(row.name), \(row.displayValue), \(row.timestamp)")
  }

  // MARK: - Section Header

  private func sectionHeader(_ title: String) -> some View {
    Text(title)
      .font(.system(size: 11, weight: .black))
      .foregroundStyle(.secondary)
  }

  // MARK: - Helpers

  private func row(for route: HealthRoute) -> MetricRow {
    let snap = healthStore.snapshot(for: route)
    let display: String
    if snap.value == "--" || snap.value.isEmpty {
      display = "—"
    } else {
      display = snap.displayValue.isEmpty ? snap.value : snap.displayValue
    }
    return MetricRow(
      id: snap.id,
      name: snap.title,
      systemImage: snap.systemImage,
      tint: snap.tint,
      displayValue: display,
      timestamp: "No data"
    )
  }

  private func timestampLabel(for date: Date) -> String {
    let hours = Int(Date().timeIntervalSince(date) / 3600)
    if hours < 1 {
      return "Updated just now"
    } else if hours == 1 {
      return "Updated 1h ago"
    } else {
      return "Updated \(hours)h ago"
    }
  }
}
