import Foundation
import SwiftUI

enum BreathePhase {
  case inhale, hold, exhale

  var label: String {
    switch self {
    case .inhale: "INHALE"
    case .hold:   "HOLD"
    case .exhale: "EXHALE"
    }
  }

  static let duration: TimeInterval = 4.0
}

struct BreatheView: View {
  @State private var isRunning = false
  @State private var currentPhase: BreathePhase = .inhale
  @State private var circleScale: CGFloat = 0.6
  @State private var phaseTask: Task<Void, Never>? = nil

  @Environment(GooseAppModel.self) private var model
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    ZStack {
      FitnessColor.background
        .ignoresSafeArea()

      VStack(spacing: 0) {
        Spacer()

        ZStack {
          Circle()
            .fill(FitnessColor.panel)
            .frame(width: 220, height: 220)
            .scaleEffect(circleScale)
          Circle()
            .strokeBorder(FitnessColor.standCyan.opacity(0.72), lineWidth: 3)
            .frame(width: 220, height: 220)
            .scaleEffect(circleScale)
        }
        .accessibilityLabel("Breathing circle, \(currentPhase.label.lowercased()) phase")

        Spacer().frame(height: 16)

        Text(currentPhase.label)
          .font(.system(size: 20, weight: .semibold))
          .tracking(2.0)
          .foregroundStyle(isRunning ? FitnessColor.standCyan : FitnessColor.secondaryText)
          .animation(.easeInOut(duration: 0.25), value: currentPhase)
          .contentTransition(.opacity)
          .accessibilityAddTraits(.updatesFrequently)

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
            .foregroundStyle(.white)
            .frame(width: 160, height: 48)
            .background(FitnessColor.panel, in: Capsule())
            .padding(.bottom, 32)
            .accessibilityLabel("Stop breathing session")
        } else {
          Button("Start") { startSession() }
            .font(.body.weight(.semibold))
            .foregroundStyle(FitnessColor.standCyan)
            .frame(width: 160, height: 48)
            .background(FitnessColor.standCyan.opacity(0.14), in: Capsule())
            .padding(.bottom, 32)
            .accessibilityLabel("Start breathing session")
        }
      }
    }
    .navigationTitle("Breathe")
    .navigationBarTitleDisplayMode(.inline)
    .toolbar(.hidden, for: .tabBar)
    .background(FitnessColor.background.ignoresSafeArea())
    .toolbarBackground(FitnessColor.background, for: .navigationBar)
    .toolbarColorScheme(.dark, for: .navigationBar)
    .onDisappear { stopSession() }
  }

  private func startSession() {
    isRunning = true
    phaseTask = Task { @MainActor in
      repeat {
        currentPhase = .inhale
        model.ble.buzz(loops: 1)
        if reduceMotion {
          circleScale = 1.0
        } else {
          withAnimation(.easeInOut(duration: BreathePhase.duration)) { circleScale = 1.0 }
        }
        try? await Task.sleep(for: .seconds(BreathePhase.duration))
        guard !Task.isCancelled else { break }

        currentPhase = .hold
        model.ble.buzz(loops: 1)
        try? await Task.sleep(for: .seconds(BreathePhase.duration))
        guard !Task.isCancelled else { break }

        currentPhase = .exhale
        model.ble.buzz(loops: 1)
        if reduceMotion {
          circleScale = 0.6
        } else {
          withAnimation(.easeInOut(duration: BreathePhase.duration)) { circleScale = 0.6 }
        }
        try? await Task.sleep(for: .seconds(BreathePhase.duration))
      } while !Task.isCancelled
    }
  }

  private func stopSession() {
    phaseTask?.cancel()
    phaseTask = nil
    isRunning = false
    currentPhase = .inhale
    withAnimation(.easeInOut(duration: 0.4)) { circleScale = 0.6 }
  }
}
