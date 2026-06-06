import Darwin
import Foundation
import SwiftUI
import UIKit

struct HealthDashboardStatusHeader: View {
  let catalogStatus: String
  let usesSampleData: Bool

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      HStack(spacing: 10) {
        Image(systemName: usesSampleData ? "testtube.2" : "checkmark.seal")
          .font(.system(size: 16, weight: .semibold))
          .foregroundStyle(usesSampleData ? .orange : .green)
          .frame(width: 30, height: 30)
          .background((usesSampleData ? Color.orange : Color.green).opacity(0.14), in: Circle())
        VStack(alignment: .leading, spacing: 2) {
          Text("Health Sources")
            .font(.headline.weight(.semibold))
          Text(catalogStatus)
            .font(.caption)
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .minimumScaleFactor(0.74)
        }
        Spacer()
        Text(usesSampleData ? "Preview" : "Live")
          .font(.caption.weight(.bold))
          .foregroundStyle(usesSampleData ? .orange : .green)
          .padding(.horizontal, 9)
          .padding(.vertical, 5)
          .background((usesSampleData ? Color.orange : Color.green).opacity(0.12), in: Capsule())
      }

    }
    .padding(16)
    .healthDashboardSurface(tint: usesSampleData ? .orange : .green, tintOpacity: 0.05)
  }
}

struct HealthTodayFocusSection: View {
  let snapshots: [HealthMetricSnapshot]

  private var columns: [GridItem] {
    [GridItem(.adaptive(minimum: 148), spacing: 12)]
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HealthSectionTitle("Today")
      LazyVGrid(columns: columns, spacing: 12) {
        ForEach(snapshots) { snapshot in
          NavigationLink(value: snapshot.route) {
            HealthTodayFocusCard(snapshot: snapshot)
          }
          .buttonStyle(.plain)
        }
      }
    }
  }
}

struct HealthTodayFocusCard: View {
  let snapshot: HealthMetricSnapshot

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack {
        Image(systemName: snapshot.systemImage)
          .font(.system(size: 16, weight: .semibold))
          .foregroundStyle(snapshot.tint)
          .frame(width: 30, height: 30)
          .background(snapshot.tint.opacity(0.14), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        Spacer()
        HealthSourceBadge(source: snapshot.source)
      }

      VStack(alignment: .leading, spacing: 4) {
        Text(snapshot.title)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(.secondary)
          .lineLimit(1)
        Text(snapshot.displayValue)
          .font(.system(size: 30, weight: .bold, design: .rounded))
          .foregroundStyle(.primary)
          .lineLimit(1)
          .minimumScaleFactor(0.62)
        Text(snapshot.status)
          .font(.caption.weight(.semibold))
          .foregroundStyle(snapshot.tint)
          .lineLimit(1)
      }
      Spacer(minLength: 0)
    }
    .frame(maxWidth: .infinity, minHeight: 154, alignment: .topLeading)
    .padding(16)
    .healthDashboardSurface(tint: snapshot.tint, tintOpacity: 0.08)
  }
}

struct HealthActivityOverviewSection: View {
  let steps: String
  let activeEnergy: String
  let stepsFreshness: String
  let stepsSource: HealthDataSource
  let activeEnergyFreshness: String
  let activeEnergySource: HealthDataSource
  let heartRateValue: String
  let heartRateStatus: String
  let heartRateSource: HealthDataSource

  private var columns: [GridItem] {
    [GridItem(.adaptive(minimum: 156), spacing: 12)]
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HealthSectionTitle("Activity")
      LazyVGrid(columns: columns, spacing: 12) {
        NavigationLink(value: HealthRoute.strain) {
          HealthDashboardMetricCard(
            title: "Steps",
            value: steps,
            subtitle: stepsFreshness,
            systemImage: "shoeprints.fill",
            tint: .green,
            source: stepsSource
          )
        }
        .buttonStyle(.plain)

        NavigationLink(value: HealthRoute.strain) {
          HealthDashboardMetricCard(
            title: "Active Calories",
            value: activeEnergy,
            subtitle: activeEnergyFreshness,
            systemImage: "flame.fill",
            tint: .orange,
            source: activeEnergySource
          )
        }
        .buttonStyle(.plain)

        NavigationLink(value: HealthRoute.healthMonitor) {
          HealthDashboardMetricCard(
            title: "Heart Rate",
            value: heartRateValue,
            subtitle: heartRateStatus,
            systemImage: "heart.fill",
            tint: .red,
            source: heartRateSource
          )
        }
        .buttonStyle(.plain)
      }
    }
  }
}

struct HealthDashboardMetricCard: View {
  let title: String
  let value: String
  let subtitle: String
  let systemImage: String
  let tint: Color
  let source: HealthDataSource

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack {
        Image(systemName: systemImage)
          .font(.system(size: 16, weight: .semibold))
          .foregroundStyle(tint)
          .frame(width: 30, height: 30)
          .background(tint.opacity(0.14), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        Spacer()
        HealthSourceBadge(source: source)
      }

      VStack(alignment: .leading, spacing: 4) {
        Text(value)
          .font(.system(size: 28, weight: .bold, design: .rounded))
          .foregroundStyle(.primary)
          .lineLimit(1)
          .minimumScaleFactor(0.62)
        Text(title)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(.primary)
          .lineLimit(1)
        Text(subtitle)
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(2)
          .fixedSize(horizontal: false, vertical: true)
      }
      Spacer(minLength: 0)
    }
    .frame(maxWidth: .infinity, minHeight: 148, alignment: .topLeading)
    .padding(16)
    .healthDashboardSurface(tint: tint, tintOpacity: 0.08)
  }
}

struct HealthVitalsPreviewSection: View {
  let snapshots: [HealthMetricSnapshot]

  private var columns: [GridItem] {
    [GridItem(.adaptive(minimum: 148), spacing: 12)]
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack {
        HealthSectionTitle("Vitals")
        NavigationLink(value: HealthRoute.healthMonitor) {
          Image(systemName: "chevron.right")
            .font(.caption.weight(.bold))
            .foregroundStyle(.secondary)
            .frame(width: 30, height: 30)
            .background(Color(.tertiarySystemGroupedBackground), in: Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Open Health Monitor")
      }

      LazyVGrid(columns: columns, spacing: 12) {
        ForEach(snapshots) { snapshot in
          NavigationLink(value: HealthRoute.healthMonitor) {
            HealthVitalsPreviewCard(snapshot: snapshot)
          }
          .buttonStyle(.plain)
        }
      }
    }
  }
}

struct HealthVitalsPreviewCard: View {
  let snapshot: HealthMetricSnapshot

  var body: some View {
    HStack(alignment: .top, spacing: 10) {
      Image(systemName: snapshot.systemImage)
        .font(.system(size: 15, weight: .semibold))
        .foregroundStyle(snapshot.tint)
        .frame(width: 28, height: 28)
        .background(snapshot.tint.opacity(0.13), in: RoundedRectangle(cornerRadius: 9, style: .continuous))
      VStack(alignment: .leading, spacing: 4) {
        Text(snapshot.title)
          .font(.caption.weight(.semibold))
          .foregroundStyle(.secondary)
          .lineLimit(1)
        Text(snapshot.displayValue)
          .font(.headline.weight(.bold))
          .foregroundStyle(.primary)
          .lineLimit(1)
          .minimumScaleFactor(0.7)
        Text(snapshot.status)
          .font(.caption2)
          .foregroundStyle(.secondary)
          .lineLimit(1)
      }
      Spacer(minLength: 0)
    }
    .frame(maxWidth: .infinity, minHeight: 92, alignment: .topLeading)
    .padding(14)
    .healthDashboardSurface(tint: snapshot.tint, tintOpacity: 0.05)
  }
}

struct HealthRouteShortcutSection: View {
  let title: String
  let snapshots: [HealthMetricSnapshot]

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HealthSectionTitle(title)
      VStack(spacing: 10) {
        ForEach(snapshots) { snapshot in
          NavigationLink(value: snapshot.route) {
            HealthRouteShortcutCard(snapshot: snapshot)
          }
          .buttonStyle(.plain)
        }
      }
    }
  }
}

struct HealthRouteShortcutCard: View {
  let snapshot: HealthMetricSnapshot

  var body: some View {
    HStack(spacing: 12) {
      Image(systemName: snapshot.systemImage)
        .font(.system(size: 17, weight: .semibold))
        .foregroundStyle(snapshot.tint)
        .frame(width: 34, height: 34)
        .background(snapshot.tint.opacity(0.13), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
      VStack(alignment: .leading, spacing: 3) {
        Text(snapshot.title)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(.primary)
        Text("\(snapshot.displayValue) | \(snapshot.status)")
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(1)
          .minimumScaleFactor(0.78)
      }
      Spacer()
      HealthSourceBadge(source: snapshot.source)
      Image(systemName: "chevron.right")
        .font(.caption.weight(.bold))
        .foregroundStyle(.tertiary)
    }
    .padding(14)
    .healthDashboardSurface(tint: snapshot.tint, tintOpacity: 0.04)
  }
}

struct HealthRouteDetailView: View {
  let route: HealthRoute
  @State private var store: HealthDataStore

  init(route: HealthRoute, previewState: HealthPreviewState? = nil) {
    self.route = route
    let store = HealthDataStore()
    if let previewState {
      store.applyPreviewState(previewState)
    }
    _store = State(initialValue: store)
  }

  var body: some View {
    HealthRouteDestinationView(route: route, store: store)
  }
}

struct HealthRouteDestinationView: View {
  let route: HealthRoute
  var store: HealthDataStore
  var selectedDate: Binding<Date>?

  init(route: HealthRoute, store: HealthDataStore, selectedDate: Binding<Date>? = nil) {
    self.route = route
    self.store = store
    self.selectedDate = selectedDate
  }

  var body: some View {
    HealthRouteContentView(route: route, store: store, selectedDate: selectedDate)
      .task {
        store.loadBridgeCatalogsIfNeeded()
      }
  }
}

struct HealthRouteContentView: View {
  let route: HealthRoute
  var store: HealthDataStore
  var selectedDate: Binding<Date>? = nil

  var body: some View {
    switch route {
    case .healthMonitor:
      HealthMonitorView(store: store)
    case .sleep, .recovery, .strain, .stress:
      HealthMetricFamilyView(route: route, store: store, externalSelectedDate: selectedDate)
    case .cardioLoad:
      CardioLoadView(store: store)
    case .energyBank:
      EnergyBankView(store: store)
    case .packetInputs:
      PacketHealthView(store: store)
    case .algorithms:
      AlgorithmsHealthView(store: store)
    case .referenceComparisons:
      ReferenceComparisonsView(store: store)
    case .calibration:
      CalibrationHealthView(store: store)
    }
  }
}

struct HealthStatusBanner: View {
  var store: HealthDataStore

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      HStack(spacing: 10) {
        Image(systemName: store.usesSampleData ? "testtube.2" : "checkmark.seal")
          .foregroundStyle(store.usesSampleData ? .orange : .green)
        Text(store.catalogStatus)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(.primary)
        Spacer()
      }
      Text("Every row below declares bridge, live, local, or unavailable provenance.")
        .font(.caption)
        .foregroundStyle(.secondary)
    }
    .padding(14)
    .healthCardSurface()
  }
}

struct HealthCardGroup: View {
  let title: String
  let snapshots: [HealthMetricSnapshot]

  private let columns = [
    GridItem(.flexible(), spacing: 10),
    GridItem(.flexible(), spacing: 10),
  ]

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      HealthSectionTitle(title)
      LazyVGrid(columns: columns, spacing: 10) {
        ForEach(snapshots) { snapshot in
          NavigationLink(value: snapshot.route) {
            HealthMetricCard(snapshot: snapshot)
          }
          .buttonStyle(.plain)
        }
      }
    }
  }
}

struct HealthMetricCard: View {
  let snapshot: HealthMetricSnapshot

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      HStack {
        Image(systemName: snapshot.systemImage)
          .font(.system(size: 16, weight: .semibold))
          .foregroundStyle(snapshot.tint)
        Spacer()
        HealthSourceBadge(source: snapshot.source)
      }

      Text(snapshot.displayValue)
        .font(.title2.bold())
        .foregroundStyle(.primary)
        .lineLimit(1)
        .minimumScaleFactor(0.7)

      VStack(alignment: .leading, spacing: 3) {
        Text(snapshot.title)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(.primary)
          .lineLimit(1)
        Text("\(snapshot.status) | \(snapshot.freshness)")
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(2)
        Text(snapshot.provenance)
          .font(.caption2)
          .foregroundStyle(.tertiary)
          .lineLimit(1)
      }
    }
    .frame(maxWidth: .infinity, minHeight: 144, alignment: .topLeading)
    .padding(14)
    .healthCardSurface()
  }
}

struct HealthMonitorView: View {
  var store: HealthDataStore
  @State private var selectedTrend: HealthMetricSnapshot?
  @State private var cachedMonitorSnapshots: [HealthMetricSnapshot] = []

  private let columns = [
    GridItem(.flexible(), spacing: 10),
    GridItem(.flexible(), spacing: 10),
  ]

  var body: some View {
    ScrollView {
      LazyVStack(alignment: .leading, spacing: 18) {
        HealthHero(snapshot: store.snapshot(for: .healthMonitor), subtitle: "Vitals, timeline, and primary sleep inputs")

        LazyVGrid(columns: columns, spacing: 10) {
          ForEach(cachedMonitorSnapshots) { snapshot in
            Button {
              selectedTrend = snapshot
            } label: {
              HealthMetricCard(snapshot: snapshot)
            }
            .buttonStyle(.plain)
          }
        }

        let cardioLoadSnapshot = store.snapshot(for: .cardioLoad)
        NavigationLink(value: HealthRoute.cardioLoad) {
          HealthWideRouteCard(
            title: cardioLoadSnapshot.title,
            value: cardioLoadSnapshot.displayValue,
            status: cardioLoadSnapshot.status,
            systemImage: cardioLoadSnapshot.systemImage,
            tint: cardioLoadSnapshot.tint,
            source: cardioLoadSnapshot.source
          )
        }
        .buttonStyle(.plain)

        HealthSectionTitle("Timeline")
        VStack(spacing: 8) {
          if let sleep = store.primarySleep() {
            HealthInfoRow(row: HealthSummaryRow("Primary sleep", value: "\(sleep.startLabel) - \(sleep.endLabel) | \(sleep.durationText) | \(sleep.scoreDisplayText)", source: sleep.source, systemImage: "bed.double"))
          } else {
            HealthInfoRow(row: HealthSummaryRow("Primary sleep", value: "No band sleep data", source: .unavailable("band sleep import not available"), systemImage: "bed.double"))
          }
          HealthInfoRow(row: HealthSummaryRow("Heart rate 1D", value: store.heartRateTimelineStatus, source: store.heartRateHourlyRanges.isEmpty ? .unavailable("BLE heart-rate sample store") : .live("BLE heart-rate sample store"), systemImage: "heart"))
          ForEach(store.heartRateHourlyTimelineRows()) { row in
            HealthInfoRow(row: row)
          }
        }

        if store.localDataSupportsExport {
          HealthSectionTitle("Export")
          VStack(alignment: .leading, spacing: 10) {
            ForEach(store.healthMonitorExportRows()) { row in
              HealthInfoRow(row: row)
            }
            ShareLink(item: store.localHealthExportText) {
              Label("Share Local Health Snapshot", systemImage: "square.and.arrow.up")
                .font(.subheadline.weight(.semibold))
            }
          }
          .padding(14)
          .healthCardSurface()
        }
      }
      .padding(16)
    }
    .gooseScreenBackground()
    .navigationTitle("Health Monitor")
    .task {
      cachedMonitorSnapshots = store.healthMonitorSnapshots()
      store.refreshHeartRateTimeline()
      store.refreshPacketInputsIfNeeded()
    }
    .onChange(of: store.packetScoreStatus) { cachedMonitorSnapshots = store.healthMonitorSnapshots() }
    .onChange(of: store.hkHRVSDNNMs) { cachedMonitorSnapshots = store.healthMonitorSnapshots() }
    .onChange(of: store.hkRestingHR) { cachedMonitorSnapshots = store.healthMonitorSnapshots() }
    .sheet(item: $selectedTrend) { snapshot in
      if snapshot.id == "resting-hr" || snapshot.id == "resting-hrv" {
        SleepV2BevelTrendSheet(snapshot: snapshot)
      } else {
        HealthTrendSheet(snapshot: snapshot)
      }
    }
  }
}

struct PacketHealthView: View {
  @Environment(GooseAppModel.self) private var model
  var store: HealthDataStore

  var body: some View {
    List {
      Section {
        Button {
          store.runPacketInputs()
        } label: {
          Label("Extract Packet-Derived Inputs", systemImage: "square.stack.3d.up")
        }
      }

      Section("Packet-Derived Inputs") {
        HealthInfoRow(row: HealthSummaryRow("Readiness", value: store.metricInputReadinessSummary(), source: store.packetInputSource("metrics.input_readiness"), systemImage: "checklist"))
        HealthInfoRow(row: HealthSummaryRow("Latest HR", value: store.latestHeartRateSummary(bpm: model.ble.liveHeartRateBPM, source: model.ble.liveHeartRateSource, updatedAt: model.ble.liveHeartRateUpdatedAt), source: model.ble.liveHeartRateBPM == nil ? .unavailable("BLE latest HR unavailable") : .live("BLE latest HR"), systemImage: "heart"))
        if !store.latestHeartRateProvenanceSummary(source: model.ble.liveHeartRateSource).isEmpty {
          HealthInfoRow(row: HealthSummaryRow("HR provenance", value: store.latestHeartRateProvenanceSummary(source: model.ble.liveHeartRateSource), source: .live("latestHeartRateProvenanceSummary()"), systemImage: "doc.text.magnifyingglass"))
        }
        HealthInfoRow(row: HealthSummaryRow("Motion", value: store.motionFeatureSummary(), source: store.packetInputSource("metrics.motion_features"), systemImage: "figure.walk.motion"))
        if !store.motionFeatureProvenanceSummary().isEmpty {
          HealthInfoRow(row: HealthSummaryRow("Motion provenance", value: store.motionFeatureProvenanceSummary(), source: store.packetInputSource("metrics.motion_features"), systemImage: "doc.text.magnifyingglass"))
        }
        HealthInfoRow(row: HealthSummaryRow("Step discovery", value: store.stepDiscoverySummary(), source: store.packetInputSource("metrics.step_packet_discovery"), systemImage: "shoeprints.fill"))
        if !store.stepDiscoveryProvenanceSummary().isEmpty {
          HealthInfoRow(row: HealthSummaryRow("Step discovery provenance", value: store.stepDiscoveryProvenanceSummary(), source: store.packetInputSource("metrics.step_packet_discovery"), systemImage: "doc.text.magnifyingglass"))
        }
        HealthInfoRow(row: HealthSummaryRow("Unavailable steps", value: store.activityUnavailableStatusSummary(), source: store.packetInputSource("metrics.activity_unavailable_daily_status"), systemImage: "minus.circle"))
        HealthInfoRow(row: HealthSummaryRow("HRV", value: store.hrvFeatureSummary(), source: store.packetInputSource("metrics.hrv_features"), systemImage: "waveform.path.ecg"))
        if !store.hrvFeatureProvenanceSummary().isEmpty {
          HealthInfoRow(row: HealthSummaryRow("HRV provenance", value: store.hrvFeatureProvenanceSummary(), source: store.packetInputSource("metrics.hrv_features"), systemImage: "doc.text.magnifyingglass"))
        }
        HealthInfoRow(row: HealthSummaryRow("Resting HR", value: store.restingHeartRateFeatureSummary(), source: store.packetInputSource("metrics.resting_hr_features"), systemImage: "heart"))
        if !store.restingHeartRateFeatureProvenanceSummary().isEmpty {
          HealthInfoRow(row: HealthSummaryRow("Resting HR provenance", value: store.restingHeartRateFeatureProvenanceSummary(), source: store.packetInputSource("metrics.resting_hr_features"), systemImage: "doc.text.magnifyingglass"))
        }
        HealthInfoRow(row: HealthSummaryRow("Energy", value: store.energyRollupSummary(), source: store.whoopActiveCaloriesSource(), systemImage: "flame.fill"))
        if !store.energyRollupProvenanceSummary().isEmpty {
          HealthInfoRow(row: HealthSummaryRow("Energy provenance", value: store.energyRollupProvenanceSummary(), source: store.whoopActiveCaloriesSource(), systemImage: "doc.text.magnifyingglass"))
        }
        HealthInfoRow(row: HealthSummaryRow("Unavailable energy", value: store.energyUnavailableStatusSummary(), source: store.packetInputSource("metrics.energy_unavailable_daily_status"), systemImage: "minus.circle"))
        HealthInfoRow(row: HealthSummaryRow("Window", value: store.windowFeatureSummary(), source: store.packetInputSource("metrics.window_features"), systemImage: "rectangle.dashed"))
        if !store.windowFeatureProvenanceSummary().isEmpty {
          HealthInfoRow(row: HealthSummaryRow("Window provenance", value: store.windowFeatureProvenanceSummary(), source: store.packetInputSource("metrics.window_features"), systemImage: "doc.text.magnifyingglass"))
        }
        HealthInfoRow(row: HealthSummaryRow("Vitals", value: store.vitalEventFeatureSummary(), source: store.packetInputSource("metrics.vital_event_features"), systemImage: "thermometer.medium"))
        if !store.vitalEventFeatureProvenanceSummary().isEmpty {
          HealthInfoRow(row: HealthSummaryRow("Vitals provenance", value: store.vitalEventFeatureProvenanceSummary(), source: store.packetInputSource("metrics.vital_event_features"), systemImage: "doc.text.magnifyingglass"))
        }
        HealthInfoRow(row: HealthSummaryRow("Recovery sensor rollup", value: store.recoverySensorDailyRollupSummary(), source: store.packetInputSource("metrics.recovery_sensor_daily_rollup"), systemImage: "heart.text.square"))
        HealthInfoRow(row: HealthSummaryRow("Unavailable recovery", value: store.recoveryUnavailableStatusSummary(), source: store.packetInputSource("metrics.recovery_unavailable_daily_status"), systemImage: "minus.circle"))
        HealthInfoRow(row: HealthSummaryRow("Next action", value: store.packetDerivedFeatureNextActionSummary(), source: store.packetInputSource("packetDerivedFeatureNextActionSummary()"), systemImage: "arrow.triangle.2.circlepath"))
      }

      Section {
        Button {
          store.runPacketScores()
        } label: {
          Label("Run Packet-Derived Scores", systemImage: "chart.xyaxis.line")
        }
      }

      Section("Packet-Derived Scores") {
        HealthInfoRow(row: HealthSummaryRow("Sleep", value: store.sleepFeatureScoreSummary(), source: store.packetScoreSource("metrics.sleep_score_from_features"), systemImage: "bed.double"))
        HealthOptionalRow(label: "Sleep model", value: store.sleepV1ModelStatusSummary(), source: store.packetScoreSource("sleepV1ModelStatusSummary()"), systemImage: "brain.head.profile")
        HealthOptionalRow(label: "Sleep confidence", value: store.sleepV1ConfidenceSummary(), source: store.packetScoreSource("sleepV1ConfidenceSummary()"), systemImage: "checkmark.seal")
        HealthOptionalRow(label: "Sleep data", value: store.sleepV1DataNotesSummary(), source: store.packetScoreSource("sleepV1DataNotesSummary()"), systemImage: "info.circle")
        HealthOptionalRow(label: "Sleep schedule", value: store.sleepV1ScheduleSummary(), source: store.packetScoreSource("sleepV1ScheduleSummary()"), systemImage: "calendar")
        HealthOptionalRow(label: "Sleep debt", value: store.sleepV1DebtSummary(), source: store.packetScoreSource("sleepV1DebtSummary()"), systemImage: "minus.circle")
        HealthOptionalRow(label: "Sleep HR", value: store.sleepV1HeartRateSummary(), source: store.packetScoreSource("sleepV1HeartRateSummary()"), systemImage: "heart")
        HealthOptionalRow(label: "Sleep stages", value: store.sleepV1StagesSummary(), source: store.packetScoreSource("sleepV1StagesSummary()"), systemImage: "chart.bar")
        HealthOptionalRow(label: "Sleep architecture", value: store.sleepV1ArchitectureCalibrationSummary(), source: store.packetScoreSource("sleepV1ArchitectureCalibrationSummary()"), systemImage: "point.3.connected.trianglepath.dotted")
        HealthOptionalRow(label: "Sleep change", value: store.sleepV1WhyChangedSummary(), source: store.packetScoreSource("sleepV1WhyChangedSummary()"), systemImage: "arrow.left.arrow.right")
        ForEach(store.sleepV1ComponentBreakdownRows()) { row in
          HealthInfoRow(row: row)
        }
        HealthOptionalRow(label: "Sleep provenance", value: store.packetScoreProvenanceSummary("sleep"), source: store.packetScoreSource("packetScoreProvenanceSummary(sleep)"), systemImage: "doc.text.magnifyingglass")
        HealthInfoRow(row: HealthSummaryRow("Recovery", value: store.recoveryFeatureScoreSummary(), source: store.packetScoreSource("metrics.recovery_score_from_features"), systemImage: "battery.100percent"))
        HealthInfoRow(row: HealthSummaryRow("Recovery vitals", value: store.recoveryProvidedVitalsSummary(), source: store.packetScoreSource("recoveryProvidedVitalsSummary()"), systemImage: "lungs"))
        HealthOptionalRow(label: "Recovery provenance", value: store.packetScoreProvenanceSummary("recovery"), source: store.packetScoreSource("packetScoreProvenanceSummary(recovery)"), systemImage: "doc.text.magnifyingglass")
        HealthInfoRow(row: HealthSummaryRow("Strain", value: store.strainFeatureScoreSummary(), source: store.packetScoreSource("metrics.strain_score_from_features"), systemImage: "figure.run"))
        HealthOptionalRow(label: "Strain provenance", value: store.packetScoreProvenanceSummary("strain"), source: store.packetScoreSource("packetScoreProvenanceSummary(strain)"), systemImage: "doc.text.magnifyingglass")
        HealthInfoRow(row: HealthSummaryRow("Stress", value: store.stressFeatureScoreSummary(), source: store.packetScoreSource("metrics.stress_score_from_features"), systemImage: "waveform.path.ecg"))
        HealthOptionalRow(label: "Stress provenance", value: store.packetScoreProvenanceSummary("stress"), source: store.packetScoreSource("packetScoreProvenanceSummary(stress)"), systemImage: "doc.text.magnifyingglass")
        HealthInfoRow(row: HealthSummaryRow("Next action", value: store.packetDerivedScoreNextActionSummary(), source: store.packetScoreSource("packetDerivedScoreNextActionSummary()"), systemImage: "arrow.triangle.2.circlepath"))
      }
    }
    .gooseListBackground()
    .navigationTitle("Packet Inputs")
  }
}
