import SwiftUI

struct CoachInlineTip: Identifiable {
  let id: String
  let title: String
  let message: String
  let source: String
  let prompt: String
  let systemImage: String
  let tint: Color
}

@MainActor
enum CoachTipFactory {
  static func homeTip(healthStore: HealthDataStore, appModel: GooseAppModel) -> CoachInlineTip {
    let readiness = healthStore.metricInputReadinessSummary()
    let inputNextAction = healthStore.metricInputReadinessNextActionSummary()
    let scoreNextAction = healthStore.packetDerivedScoreNextActionSummary()
    let liveHeartRate = healthStore.latestHeartRateSummary(
      bpm: appModel.ble.liveHeartRateBPM,
      source: appModel.ble.liveHeartRateSource,
      updatedAt: appModel.ble.liveHeartRateUpdatedAt
    )
    let sleep = healthStore.snapshot(for: .sleep)
    let recovery = healthStore.snapshot(for: .recovery)
    let strain = healthStore.snapshot(for: .strain)

    return CoachInlineTip(
      id: "home",
      title: "Coach",
      message: firstUseful(
        inputNextAction,
        scoreNextAction,
        "Readiness: \(readiness). Sleep \(sleep.displayValue), recovery \(recovery.displayValue), strain \(strain.displayValue)."
      ),
      source: "Local readiness, scores, and live HR",
      prompt: """
      Give me today's coaching priority from my local Goose context. Use readiness, sleep, recovery, strain, stress, live heart rate, and missing-data gaps. Cite the local tool outputs and keep it to one concrete next action.

      Current local highlights:
      - Readiness: \(readiness)
      - Input next action: \(inputNextAction)
      - Score next action: \(scoreNextAction)
      - Sleep: \(sleep.displayValue) | \(sleep.status) | \(sleep.freshness)
      - Recovery: \(recovery.displayValue) | \(recovery.status) | \(recovery.freshness)
      - Strain: \(strain.displayValue) | \(strain.status) | \(strain.freshness)
      - Live HR: \(liveHeartRate)
      """,
      systemImage: "sparkles",
      tint: .purple
    )
  }

  static func metricTip(
    route: HealthRoute,
    healthStore: HealthDataStore,
    appModel: GooseAppModel
  ) -> CoachInlineTip {
    switch route {
    case .sleep:
      return sleepTip(healthStore: healthStore, ble: appModel.ble)
    case .recovery:
      return recoveryTip(healthStore: healthStore)
    case .strain:
      return strainTip(healthStore: healthStore, appModel: appModel)
    case .stress:
      return stressTip(healthStore: healthStore, appModel: appModel)
    default:
      let snapshot = healthStore.snapshot(for: route)
      return CoachInlineTip(
        id: route.rawValue,
        title: "\(route.title) Coach",
        message: "\(snapshot.title): \(snapshot.displayValue) | \(snapshot.status).",
        source: snapshot.provenance,
        prompt: "Explain my \(route.title.lowercased()) page using the local Goose context. Cite the tool outputs and call out stale or missing data.",
        systemImage: "sparkles",
        tint: snapshot.tint
      )
    }
  }

  static func sleepTip(healthStore: HealthDataStore, ble: GooseBLEClient) -> CoachInlineTip {
    let snapshot = healthStore.snapshot(for: .sleep)
    let schedule = healthStore.sleepV1ScheduleSummary()
    let debt = healthStore.sleepV1DebtSummary()
    let confidence = healthStore.sleepV1ConfidenceSummary()
    let nextAction = healthStore.packetDerivedScoreNextActionSummary()

    return CoachInlineTip(
      id: "sleep",
      title: "Sleep Coach",
      message: firstUseful(
        sentence("Sleep \(snapshot.displayValue)", snapshot.status, schedule),
        sentence("Sleep \(snapshot.displayValue)", debt, confidence),
        nextAction
      ),
      source: "Local sleep score and schedule",
      prompt: """
      Explain my sleep page and give one practical next action. Use only local Goose context and call out missing data and provenance.

      Current local highlights:
      - Sleep score: \(snapshot.displayValue) | \(snapshot.status) | \(snapshot.freshness)
      - Schedule: \(schedule)
      - Sleep debt: \(debt)
      - Confidence: \(confidence)
      - Alarm: \(ble.alarmDisplaySummary)
      - Score next action: \(nextAction)
      """,
      systemImage: "moon.zzz.fill",
      tint: .indigo
    )
  }

  private static func recoveryTip(healthStore: HealthDataStore) -> CoachInlineTip {
    let snapshot = healthStore.snapshot(for: .recovery)
    let recovery = "\(healthStore.recoveryScoreDisplayValue())% recovery"
    let hrv = healthStore.recoveryHRVDisplayText()
    let restingHeartRate = healthStore.recoveryRestingHRDisplayText()
    let vitals = [
      healthStore.recoveryRespiratoryRateDisplayText(),
      healthStore.recoveryOxygenSaturationDisplayText(),
      healthStore.recoveryWristTemperatureDisplayText(),
    ].joined(separator: " | ")
    let optimalStrain = healthStore.strainTargetDisplayText()

    return CoachInlineTip(
      id: "recovery",
      title: "Recovery Coach",
      message: sentence(recovery, "Target strain today: \(optimalStrain)", "HRV: \(hrv)"),
      source: "Local recovery, HRV, RHR, vitals, and optimal strain",
      prompt: """
      Explain my recovery and give one practical next action. Use recovery score, HRV, resting HR, vitals, and the optimal strain target. Cite local tool outputs.

      Current local highlights:
      - Recovery snapshot: \(snapshot.displayValue) | \(snapshot.status) | \(snapshot.freshness)
      - Recovery score: \(recovery)
      - Optimal strain target today: \(optimalStrain) (0–21 scale)
      - HRV: \(hrv)
      - Resting HR: \(restingHeartRate)
      - Provided vitals: \(vitals)
      - Provenance: \(healthStore.packetScoreProvenanceSummary("recovery"))
      """,
      systemImage: "battery.100percent",
      tint: .green
    )
  }

  private static func strainTip(healthStore: HealthDataStore, appModel: GooseAppModel) -> CoachInlineTip {
    let snapshot = healthStore.snapshot(for: .strain)
    let strain = healthStore.strainFeatureScoreSummary()
    let target = healthStore.strainTargetDisplayText()
    let motion = healthStore.motionFeatureSummary()
    let activity = appModel.activitySession.statusText
    let nextAction = healthStore.packetDerivedScoreNextActionSummary()

    return CoachInlineTip(
      id: "strain",
      title: "Strain Coach",
      message: sentence("Current: \(strain)", "Target today: \(target)", firstUseful(nextAction, motion)),
      source: "Local strain, optimal target, motion, and activity",
      prompt: """
      Explain my strain page and give one practical training-load next action. Use WHOOP's 0-21 strain scale, today's optimal target, and cite local tool outputs.

      Current local highlights:
      - Strain snapshot: \(snapshot.displayValue) | \(snapshot.status) | \(snapshot.freshness)
      - Current strain: \(strain)
      - Optimal strain target today: \(target) (0–21 scale)
      - Motion: \(motion)
      - Activity session: \(activity)
      - Score next action: \(nextAction)
      - Provenance: \(healthStore.packetScoreProvenanceSummary("strain"))
      """,
      systemImage: "figure.run",
      tint: .orange
    )
  }

  private static func stressTip(healthStore: HealthDataStore, appModel: GooseAppModel) -> CoachInlineTip {
    let snapshot = healthStore.snapshot(for: .stress)
    let stress = healthStore.stressFeatureScoreSummary()
    let hrv = healthStore.hrvFeatureSummary()
    let liveHeartRate = healthStore.latestHeartRateSummary(
      bpm: appModel.ble.liveHeartRateBPM,
      source: appModel.ble.liveHeartRateSource,
      updatedAt: appModel.ble.liveHeartRateUpdatedAt
    )

    return CoachInlineTip(
      id: "stress",
      title: "Stress Coach",
      message: sentence(stress, "HRV: \(hrv)", "Latest HR: \(liveHeartRate)"),
      source: "Local stress, HRV, and live HR",
      prompt: """
      Explain my stress page and give one practical next action. Use stress score, HRV, latest heart rate, and missing time-series data. Cite local tool outputs.

      Current local highlights:
      - Stress snapshot: \(snapshot.displayValue) | \(snapshot.status) | \(snapshot.freshness)
      - Stress score: \(stress)
      - HRV: \(hrv)
      - Latest HR: \(liveHeartRate)
      - Provenance: \(healthStore.packetScoreProvenanceSummary("stress"))
      """,
      systemImage: "waveform.path.ecg",
      tint: .yellow
    )
  }

  private static func firstUseful(_ values: String...) -> String {
    values
      .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
      .first { !$0.isEmpty } ?? "Open Coach for the next local-data recommendation."
  }

  private static func sentence(_ parts: String...) -> String {
    parts
      .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
      .filter { !$0.isEmpty }
      .joined(separator: ". ")
  }
}

struct CoachTipCard: View {
  let tip: CoachInlineTip
  var actionTitle = "Ask Coach"
  let action: () -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack(alignment: .top, spacing: 10) {
        Image(systemName: tip.systemImage)
          .font(.system(size: 17, weight: .semibold))
          .foregroundStyle(tip.tint)
          .frame(width: 32, height: 32)
          .background(tip.tint.opacity(0.10), in: RoundedRectangle(cornerRadius: 8, style: .continuous))

        VStack(alignment: .leading, spacing: 4) {
          Text(tip.title)
            .font(.headline)
            .foregroundStyle(.primary)
          Text(tip.message)
            .font(.subheadline)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
            .lineLimit(4)
        }

        Spacer(minLength: 0)
      }

      HStack(spacing: 8) {
        Text(tip.source)
          .font(.caption2.weight(.semibold))
          .foregroundStyle(.tertiary)
          .lineLimit(1)
          .minimumScaleFactor(0.78)
        Spacer(minLength: 8)
        Button(action: action) {
          Label(actionTitle, systemImage: "bubble.left.and.bubble.right")
            .font(.caption.weight(.semibold))
        }
        .buttonStyle(.bordered)
        .controlSize(.small)
      }
    }
    .padding(14)
    .background(Color(.secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
    .overlay {
      RoundedRectangle(cornerRadius: 12, style: .continuous)
        .strokeBorder(tip.tint.opacity(0.13), lineWidth: 1)
    }
    .accessibilityElement(children: .combine)
  }
}
