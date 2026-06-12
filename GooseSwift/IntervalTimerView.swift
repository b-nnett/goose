import Foundation
import SwiftUI

private enum IntervalPhase {
  case work, rest
}

struct IntervalTimerView: View {
  @Environment(GooseAppModel.self) private var model

  @State private var isRunning = false
  @State private var phaseTask: Task<Void, Never>? = nil
  @State private var currentPhase: IntervalPhase = .work
  @State private var countdownSeconds: Int = 30
  @State private var workSeconds: Int = 30
  @State private var restSeconds: Int = 10

  // MARK: - Computed

  private var currentPhaseLabel: String {
    switch currentPhase {
    case .work: "WORK"
    case .rest: "REST"
    }
  }

  private var currentPhaseColor: Color {
    switch currentPhase {
    case .work: FitnessColor.standCyan
    case .rest: FitnessColor.workoutYellow
    }
  }

  private var countdownText: String {
    String(format: "%02d:%02d", countdownSeconds / 60, countdownSeconds % 60)
  }

  // MARK: - Body

  var body: some View {
    ZStack {
      FitnessColor.background
        .ignoresSafeArea()

      VStack(spacing: 0) {
        Spacer()

        if !isRunning {
          VStack(spacing: 24) {
            IntervalStepperRow(
              label: "Work",
              seconds: $workSeconds,
              min: 5,
              max: 300,
              step: 5
            )
            IntervalStepperRow(
              label: "Rest",
              seconds: $restSeconds,
              min: 5,
              max: 120,
              step: 5
            )
          }
          .padding(.horizontal, 32)
        } else {
          VStack(spacing: 8) {
            Text(currentPhaseLabel)
              .font(.system(size: 20, weight: .semibold))
              .tracking(2.0)
              .foregroundStyle(currentPhaseColor)
              .animation(.easeInOut(duration: 0.25), value: currentPhase)
              .contentTransition(.opacity)
              .accessibilityLabel("\(currentPhaseLabel) — \(countdownText) remaining")
              .accessibilityAddTraits(.updatesFrequently)

            Text(countdownText)
              .font(.system(size: 64, weight: .semibold, design: .rounded))
              .foregroundStyle(currentPhaseColor)
              .monospacedDigit()
              .contentTransition(.numericText(countsDown: true))
          }
        }

        Spacer()

        if !isRunning && model.ble.connectionState != "ready" {
          HStack(spacing: 8) {
            Image(systemName: "sensor.tag.radiowaves.forward")
              .foregroundStyle(FitnessColor.secondaryText)
            Text("Connect WHOOP to enable haptics")
              .font(.system(size: 16, weight: .regular))
              .foregroundStyle(FitnessColor.secondaryText)
          }
          .padding(.horizontal, 16)
          .padding(.vertical, 10)
          .background(FitnessColor.panel, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
          .padding(.bottom, 16)
          .accessibilityElement(children: .combine)
        }

        if isRunning {
          Button("Stop") { stopSession() }
            .font(.body.weight(.semibold))
            .foregroundStyle(FitnessColor.endRed)
            .frame(width: 160, height: 48)
            .background(FitnessColor.endRed.opacity(0.14), in: Capsule())
            .padding(.bottom, 32)
            .accessibilityLabel("Stop interval timer")
        } else {
          Button("Start") { startSession() }
            .font(.body.weight(.semibold))
            .foregroundStyle(FitnessColor.standCyan)
            .frame(width: 160, height: 48)
            .background(FitnessColor.standCyan.opacity(0.14), in: Capsule())
            .padding(.bottom, 32)
            .accessibilityLabel("Start interval timer")
        }
      }
    }
    .navigationTitle("Interval Timer")
    .navigationBarTitleDisplayMode(.inline)
    .toolbar(.hidden, for: .tabBar)
    .background(FitnessColor.background.ignoresSafeArea())
    .toolbarBackground(FitnessColor.background, for: .navigationBar)
    .toolbarColorScheme(.dark, for: .navigationBar)
    .onDisappear { stopSession() }
  }

  // MARK: - Session Control

  private func startSession() {
    isRunning = true
    countdownSeconds = workSeconds
    phaseTask = Task { @MainActor in
      repeat {
        currentPhase = .work
        model.ble.buzz(loops: 1)
        countdownSeconds = workSeconds
        for _ in 0..<workSeconds {
          try? await Task.sleep(for: .seconds(1))
          guard !Task.isCancelled else { break }
          countdownSeconds -= 1
        }
        guard !Task.isCancelled else { break }

        currentPhase = .rest
        model.ble.buzz(loops: 1)
        countdownSeconds = restSeconds
        for _ in 0..<restSeconds {
          try? await Task.sleep(for: .seconds(1))
          guard !Task.isCancelled else { break }
          countdownSeconds -= 1
        }
      } while !Task.isCancelled
    }
  }

  private func stopSession() {
    phaseTask?.cancel()
    phaseTask = nil
    isRunning = false
    currentPhase = .work
  }
}

// MARK: - IntervalStepperRow

private struct IntervalStepperRow: View {
  let label: String
  @Binding var seconds: Int
  let min: Int
  let max: Int
  let step: Int

  var body: some View {
    HStack {
      Text(label)
        .font(.subheadline.weight(.semibold))
      Spacer()
      HStack(spacing: 16) {
        Button {
          seconds = Swift.max(min, seconds - step)
        } label: {
          Image(systemName: "minus.circle.fill")
            .font(.title2)
        }
        .foregroundStyle(FitnessColor.standCyan)

        Text("\(seconds)s")
          .font(.subheadline.weight(.semibold))
          .monospacedDigit()
          .frame(minWidth: 44, alignment: .center)

        Button {
          seconds = Swift.min(max, seconds + step)
        } label: {
          Image(systemName: "plus.circle.fill")
            .font(.title2)
        }
        .foregroundStyle(FitnessColor.standCyan)
      }
    }
    .padding(14)
    .background(FitnessColor.panel, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
  }
}
