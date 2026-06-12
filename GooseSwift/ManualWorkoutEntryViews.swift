import SwiftUI

// MARK: - View Model

@MainActor
final class WorkoutEntryViewModel: ObservableObject {
  @Published var selectedKind: ActivityKind = .run
  @Published var durationMinutes: Int = 30
  @Published var effortValue: Int = 5
  @Published var isSubmitting = false
  @Published var errorMessage: String? = nil

  var isFormValid: Bool { durationMinutes >= 1 }

  // Use GooseRustBridge directly — GooseRustBridging protocol lands in Plan 72-03
  // TODO(72-03): switch to GooseRustBridging protocol for mock injection
  var bridge: GooseRustBridge
  var databasePath: String

  init(bridge: GooseRustBridge, databasePath: String) {
    self.bridge = bridge
    self.databasePath = databasePath
  }

  func submitWorkout() async {
    guard isFormValid else { return }
    isSubmitting = true
    errorMessage = nil
    let now = Date()
    let formatter = DateFormatter()
    formatter.dateFormat = "yyyy-MM-dd"
    let dateStr = formatter.string(from: now)
    let isoFormatter = ISO8601DateFormatter()
    isoFormatter.formatOptions = [.withInternetDateTime]
    let startTime = Calendar.current.date(byAdding: .minute, value: -durationMinutes, to: now) ?? now
    do {
      _ = try await bridge.requestAsync(
        method: "workout.upsert",
        args: [
          "database_path": databasePath,
          "date": dateStr,
          "source": "manual",
          "sport": selectedKind.rawValue,
          "start_time": isoFormatter.string(from: startTime),
          "end_time": isoFormatter.string(from: now),
          "duration_s": Double(durationMinutes) * 60.0,
          "notes": "perceived_effort: \(effortValue)",
        ]
      )
      // success — caller dismisses
    } catch {
      errorMessage = "Could not save workout. Please try again."
      isSubmitting = false
    }
  }
}

// MARK: - Sheet View

struct ManualWorkoutEntrySheet: View {
  @Environment(\.dismiss) private var dismiss
  @StateObject private var vm: WorkoutEntryViewModel

  init(store: HealthDataStore) {
    _vm = StateObject(wrappedValue: WorkoutEntryViewModel(
      bridge: store.bridge,
      databasePath: store.databasePath
    ))
  }

  var body: some View {
    NavigationStack {
      Form {
        Section("Activity") {
          Picker("Sport", selection: $vm.selectedKind) {
            ForEach(ActivityKind.allCases) { kind in
              Text(kind.title).tag(kind)
            }
          }
          .pickerStyle(.menu)
        }

        Section("Duration") {
          Stepper("\(vm.durationMinutes) min", value: $vm.durationMinutes, in: 1...600, step: 5)
        }

        Section {
          VStack(alignment: .leading, spacing: 12) {
            HStack {
              Text("Perceived Effort")
                .font(.subheadline.weight(.semibold))
              Spacer()
              Text("\(vm.effortValue) / 10")
                .font(.subheadline.weight(.semibold))
                .fontDesign(.rounded)
                .foregroundStyle(Color.orange)
            }
            EffortScaleSelector(selectedValue: $vm.effortValue)
          }
          .padding(.vertical, 4)
        } header: {
          Text("Effort")
        }

        if let err = vm.errorMessage {
          Section {
            Text(err)
              .font(.caption)
              .foregroundStyle(.red)
          }
        }
      }
      .navigationTitle("Log Workout")
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .topBarLeading) {
          Button("Cancel") { dismiss() }
            .accessibilityLabel("Cancel workout entry")
        }
        ToolbarItem(placement: .topBarTrailing) {
          Button("Log") {
            Task {
              await vm.submitWorkout()
              if vm.errorMessage == nil { dismiss() }
            }
          }
          .fontWeight(.semibold)
          .foregroundStyle(vm.isFormValid ? Color.orange : Color.secondary)
          .disabled(!vm.isFormValid || vm.isSubmitting)
          .accessibilityLabel("Log workout")
        }
      }
    }
  }
}

// MARK: - Effort Scale Selector

private struct EffortScaleSelector: View {
  @Binding var selectedValue: Int

  var body: some View {
    HStack(spacing: 6) {
      ForEach(1...10, id: \.self) { value in
        Button {
          selectedValue = value
        } label: {
          Text("\(value)")
            .font(.caption.weight(.semibold))
            .frame(maxWidth: .infinity)
            .frame(height: 32)
            .background(
              selectedValue == value ? Color.orange : Color.orange.opacity(0.12),
              in: RoundedRectangle(cornerRadius: 8, style: .continuous)
            )
            .foregroundStyle(selectedValue == value ? Color.white : Color.orange)
        }
        .buttonStyle(.plain)
        .contentShape(Rectangle())
        .frame(minWidth: 44, minHeight: 44)
        .accessibilityLabel("Effort \(value) of 10")
      }
    }
  }
}
