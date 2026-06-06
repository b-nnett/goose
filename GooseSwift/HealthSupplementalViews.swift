import Darwin
import Foundation
import SwiftUI
import UIKit

struct EnergyBankView: View {
  var store: HealthDataStore

  var body: some View {
    List {
      Section {
        HealthHero(snapshot: store.snapshot(for: .energyBank), subtitle: "Energy charge, drain, stress, and sleep contribution")
          .listRowInsets(EdgeInsets())
          .listRowBackground(Color.clear)
      }
      Section("Energy And Stress") {
        EnergyAndStressChart(points: summary.points, selectedPoint: selectedPoint)
          .frame(height: 190)
        if let selected = selectedPoint {
          HealthInfoRow(row: HealthSummaryRow("Selected window", value: "\(selected.timeLabel) | Energy \(Int(selected.energy)) | Stress \(Int(selected.stress))", source: summary.source, systemImage: "scope"))
        }
        HealthInfoRow(row: HealthSummaryRow("Total Charged", value: signedPercent(summary.totalCharged, prefix: "+"), source: summary.source, systemImage: "plus.circle"))
        HealthInfoRow(row: HealthSummaryRow("Total Drained", value: signedPercent(summary.totalDrained, prefix: "-"), source: summary.source, systemImage: "minus.circle"))
      }
      Section("Energy Usage") {
        HealthInfoRow(row: HealthSummaryRow("Primary sleep contribution", value: signedPercent(summary.primarySleepCharge, prefix: "+"), source: summary.source, systemImage: "bed.double"))
        if let selected = selectedPoint {
          HealthInfoRow(row: HealthSummaryRow("Stress usage window", value: "\(selected.timeLabel) | stress \(Int(selected.stress)) | load \(Int(selected.usage))", source: summary.source, systemImage: "waveform.path.ecg"))
        }
        HealthInfoRow(row: HealthSummaryRow("Confidence", value: summary.confidence.flatMap { HealthDataStore.numberText($0, fractionDigits: 2) } ?? "--", source: summary.source, systemImage: "checkmark.seal"))
        HealthInfoRow(row: HealthSummaryRow("Inputs", value: summary.inputSummary, source: summary.source, systemImage: "checklist"))
      }
    }
    .gooseListBackground()
    .navigationTitle("Energy Bank")
  }

  private var summary: EnergyBankAlgorithmSummary {
    store.energyBankAlgorithmSummary()
  }

  private var selectedPoint: EnergyStressPoint? {
    summary.points.last
  }

  private func signedPercent(_ value: Double, prefix: String) -> String {
    "\(prefix)\(Int(value.rounded()))%"
  }
}

struct AlgorithmsHealthView: View {
  var store: HealthDataStore

  var body: some View {
    List {
      Section("Primary Selection") {
        ForEach(store.algorithmFamilies, id: \.self) { family in
          let algorithms = store.algorithms(for: family)
          if algorithms.isEmpty {
            HealthInfoRow(row: HealthSummaryRow(family.uppercased(), value: "No algorithms registered", source: store.catalogSource, systemImage: "function"))
          } else {
            Picker(family.uppercased(), selection: Binding(
              get: { store.selectedAlgorithmByFamily[family] ?? algorithms[0].id },
              set: { store.selectAlgorithm($0, for: family) }
            )) {
              ForEach(algorithms) { algorithm in
                Text(algorithm.displayName).tag(algorithm.id)
              }
            }
          }
        }
      }

      Section("Algorithm Definitions") {
        ForEach(store.algorithmDefinitions) { definition in
          HealthInfoRow(row: HealthSummaryRow(definition.displayName, value: "\(definition.family) | \(definition.status) | \(definition.provider)", source: definition.source, systemImage: "function"))
        }
      }

      Section("Reference Definitions") {
        ForEach(store.referenceDefinitions) { definition in
          HealthInfoRow(row: HealthSummaryRow(definition.displayName, value: "\(definition.family) | \(definition.status)", source: definition.source, systemImage: "scalemass"))
        }
      }
    }
    .gooseListBackground()
    .navigationTitle("Algorithms")
  }
}

struct ReferenceComparisonsView: View {
  var store: HealthDataStore

  var body: some View {
    List {
      Section {
        Button {
          store.runReferenceComparisons()
        } label: {
          Label("Run Reference Comparisons", systemImage: "compare.arrows")
        }
      }
      Section("Policy") {
        HealthInfoRow(row: HealthSummaryRow("Pass/fail policy", value: "Reference comparisons need real captured inputs before they can run", source: .unavailable("reference comparison inputs not wired"), systemImage: "checkmark.seal"))
      }
      Section("Comparisons") {
        ForEach(["hrv", "sleep", "strain", "stress"], id: \.self) { family in
          HealthInfoRow(row: HealthSummaryRow(family.uppercased(), value: store.referenceComparisonSummary(family), source: store.referenceComparisonSource(family), systemImage: "scalemass"))
        }
      }
    }
    .gooseListBackground()
    .navigationTitle("References")
  }
}

struct CalibrationHealthView: View {
  @Bindable var store: HealthDataStore

  var body: some View {
    List {
      Section("Target") {
        Picker("Family", selection: $store.calibrationTargetFamily) {
          ForEach(["recovery", "sleep", "strain", "stress", "hrv"], id: \.self) { family in
            Text(family.uppercased()).tag(family)
          }
        }
        .pickerStyle(.segmented)
      }

      Section("Actions") {
        Button {
          store.importCalibrationLabels()
        } label: {
          Label("Import Labels", systemImage: "square.and.arrow.down")
        }
        Button {
          store.calibrate()
        } label: {
          Label("Calibrate", systemImage: "slider.horizontal.3")
        }
      }

      Section("Calibration") {
        HealthInfoRow(row: HealthSummaryRow("Dataset", value: "No calibration dataset", source: .unavailable("calibration dataset not wired"), systemImage: "folder"))
        HealthInfoRow(row: HealthSummaryRow("User labels", value: store.calibrationLabelSummary(), source: .unavailable("calibration labels not imported from real source"), systemImage: "tag"))
        HealthInfoRow(row: HealthSummaryRow("Holdout", value: store.calibrationSummary(), source: .unavailable("calibration holdout not computed"), systemImage: "chart.xyaxis.line"))
        HealthInfoRow(row: HealthSummaryRow("Calibrated score", value: store.calibratedScoreSummary(), source: .unavailable("calibrated score not computed"), systemImage: "checkmark.seal"))
        HealthInfoRow(row: HealthSummaryRow("Label policy", value: "No real label source wired", source: .unavailable("label policy pending"), systemImage: "text.badge.checkmark"))
        HealthInfoRow(row: HealthSummaryRow("Next action", value: store.calibrationNextActionSummary(), source: .unavailable("calibration action pending"), systemImage: "arrow.triangle.2.circlepath"))
        ForEach(store.calibrationIssues(), id: \.self) { issue in
          HealthInfoRow(row: HealthSummaryRow("Issue", value: issue, source: .unavailable("calibration issue"), systemImage: "exclamationmark.triangle"))
        }
      }
    }
    .gooseListBackground()
    .navigationTitle("Calibration")
  }
}

struct HealthHero: View {
  let snapshot: HealthMetricSnapshot
  let subtitle: String

  var body: some View {
    VStack(alignment: .leading, spacing: 14) {
      HStack(alignment: .top, spacing: 12) {
        Image(systemName: snapshot.systemImage)
          .font(.system(size: 28, weight: .bold))
          .foregroundStyle(snapshot.tint)
          .frame(width: 48, height: 48)
          .background(snapshot.tint.opacity(0.14), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        VStack(alignment: .leading, spacing: 4) {
          Text(snapshot.title)
            .font(.title2.bold())
          Text(subtitle)
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
        Spacer()
        HealthSourceBadge(source: snapshot.source)
      }

      HStack(alignment: .firstTextBaseline, spacing: 8) {
        Text(snapshot.displayValue)
          .font(.system(size: 36, weight: .bold))
          .lineLimit(1)
          .minimumScaleFactor(0.7)
        Text(snapshot.status)
          .font(.headline)
          .foregroundStyle(snapshot.tint)
      }
      Text("\(snapshot.freshness) | \(snapshot.provenance)")
        .font(.caption)
        .foregroundStyle(.secondary)
    }
    .padding(16)
    .healthCardSurface()
  }
}

struct HealthWideRouteCard: View {
  let title: String
  let value: String
  let status: String
  let systemImage: String
  let tint: Color
  let source: HealthDataSource

  var body: some View {
    HStack(spacing: 12) {
      Image(systemName: systemImage)
        .font(.system(size: 22, weight: .semibold))
        .foregroundStyle(tint)
        .frame(width: 38, height: 38)
        .background(tint.opacity(0.14), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
      VStack(alignment: .leading, spacing: 3) {
        Text(title)
          .font(.headline)
        Text("\(value) | \(status)")
          .font(.subheadline)
          .foregroundStyle(.secondary)
      }
      Spacer()
      HealthSourceBadge(source: source)
      Image(systemName: "chevron.right")
        .font(.caption.weight(.bold))
        .foregroundStyle(.tertiary)
    }
    .padding(14)
    .healthCardSurface()
  }
}

struct HealthInfoRow: View {
  let row: HealthSummaryRow

  var body: some View {
    HStack(alignment: .top, spacing: 12) {
      Image(systemName: row.systemImage)
        .font(.system(size: 17, weight: .semibold))
        .foregroundStyle(row.source.kind == .unavailable ? .orange : .secondary)
        .frame(width: 24)
      VStack(alignment: .leading, spacing: 4) {
        Text(row.label)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(.primary)
        Text(row.value)
          .font(.subheadline)
          .foregroundStyle(.secondary)
        if !row.status.isEmpty {
          Text(row.status)
            .font(.caption)
            .foregroundStyle(.tertiary)
        }
        Text(row.source.label)
          .font(.caption2)
          .foregroundStyle(.tertiary)
      }
    }
  }
}

struct HealthOptionalRow: View {
  let label: String
  let value: String
  let source: HealthDataSource
  let systemImage: String

  var body: some View {
    if !value.isEmpty {
      HealthInfoRow(row: HealthSummaryRow(label, value: value, source: source, systemImage: systemImage))
    }
  }
}

struct HealthTrendRow: View {
  let snapshot: HealthMetricSnapshot

  var body: some View {
    HStack(spacing: 12) {
      Image(systemName: snapshot.systemImage)
        .font(.system(size: 18, weight: .semibold))
        .foregroundStyle(snapshot.tint)
        .frame(width: 32, height: 32)
        .background(snapshot.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
      VStack(alignment: .leading, spacing: 3) {
        Text(snapshot.title)
          .font(.headline)
        Text("\(snapshot.displayValue) | \(snapshot.status)")
          .font(.caption)
          .foregroundStyle(.secondary)
        Text(snapshot.source.label)
          .font(.caption2)
          .foregroundStyle(.tertiary)
      }
      Spacer()
      HealthSparkline(points: snapshot.trend.points.map(\.value), tint: snapshot.tint)
        .frame(width: 76, height: 34)
      Image(systemName: "chevron.right")
        .font(.caption.weight(.bold))
        .foregroundStyle(.tertiary)
    }
    .padding(14)
    .healthCardSurface()
  }
}

struct HealthTrendSheet: View {
  let snapshot: HealthMetricSnapshot
  @Environment(\.dismiss) private var dismiss
  @State private var selectedRange = "30D"

  var body: some View {
    NavigationStack {
      List {
        Section {
          VStack(alignment: .leading, spacing: 12) {
            HStack {
              Text(snapshot.displayValue)
                .font(.system(size: 34, weight: .bold))
              Spacer()
              Text(snapshot.status)
                .font(.headline)
                .foregroundStyle(snapshot.tint)
            }
            Text(snapshot.trend.rangeLabel)
              .font(.subheadline)
              .foregroundStyle(.secondary)
            Text(snapshot.source.label)
              .font(.caption)
              .foregroundStyle(.tertiary)
          }
        }

        Section("Trend") {
          Picker("Range", selection: $selectedRange) {
            ForEach(["7D", "30D", "6M"], id: \.self) { range in
              Text(range).tag(range)
            }
          }
          .pickerStyle(.segmented)

          if snapshot.trend.hasData {
            HealthSparkline(points: snapshot.trend.points.map(\.value), tint: snapshot.tint)
              .frame(height: 160)
            Text(snapshot.trend.summary)
              .font(.caption)
              .foregroundStyle(.secondary)
          } else {
            ContentUnavailableView("No Trend Data", systemImage: "chart.line.uptrend.xyaxis", description: Text(snapshot.trend.analysis))
          }
        }

        Section("Analysis") {
          Text(snapshot.trend.analysis)
        }

        Section("Resources") {
          ForEach(snapshot.trend.resources, id: \.self) { resource in
            Label(resource, systemImage: "book")
          }
        }
      }
      .gooseListBackground()
      .navigationTitle(snapshot.title)
      .navigationBarTitleDisplayMode(.inline)
      .toolbarBackground(.hidden, for: .navigationBar)
      .toolbar {
        ToolbarItem(placement: .topBarLeading) {
          Button("Close") {
            dismiss()
          }
        }
      }
    }
  }
}

