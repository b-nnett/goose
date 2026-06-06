import Foundation
import CryptoKit
import SwiftUI
import UIKit

#if canImport(HealthKit)
import HealthKit
#endif

struct MoreGreetingHeader: View {
  let firstName: String
  let profileSummary: String

  var body: some View {
    HStack(alignment: .center, spacing: 12) {
      VStack(alignment: .leading, spacing: 3) {
        Text(greeting)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(.secondary)
        Text(displayName)
          .font(.headline)
        Text(profileSummary)
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(1)
      }

      Spacer(minLength: 8)

      Label("Update Profile", systemImage: "person.crop.circle")
        .font(.caption.weight(.semibold))
        .labelStyle(.titleAndIcon)
        .foregroundStyle(.blue)
    }
    .padding(.vertical, 5)
  }

  private var displayName: String {
    let trimmed = firstName.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? "there" : trimmed
  }

  private var greeting: String {
    let hour = Calendar.current.component(.hour, from: Date())
    switch hour {
    case 5..<12:
      return "Good morning,"
    case 12..<17:
      return "Good afternoon,"
    default:
      return "Good evening,"
    }
  }
}

struct MoreDeveloperView: View {
  let routes: [MoreRoute]
  let routeStatus: MoreRouteStatus

  var body: some View {
    List {
      Section("Tools") {
        ForEach(routes) { route in
          NavigationLink(value: route) {
            MoreRouteRow(route: route, status: routeStatus[keyPath: route.statusKeyPath], showsStatus: true)
          }
          .accessibilityLabel(route.title)
        }
      }

      Section("Scope") {
        MoreInfoRow(
          title: "Developer Surface",
          value: "Internal capture, export, storage, bridge, and algorithm tools live here instead of the main More page.",
          systemImage: "hammer",
          status: .pending
        )
      }
    }
    .listStyle(.insetGrouped)
    .gooseListBackground()
    .navigationTitle("Developer")
    .navigationBarTitleDisplayMode(.inline)
    .toolbarBackground(.hidden, for: .navigationBar)
  }
}

struct MoreProfileView: View {
  @Environment(GooseAppModel.self) private var model
  @AppStorage(OnboardingStorage.firstName) private var firstName = ""
  @AppStorage(OnboardingStorage.dateOfBirth) private var dateOfBirthString = ""
  @AppStorage(OnboardingStorage.unitSystem) private var unitSystemRaw = MoreProfileUnitSystem.imperial.rawValue
  @AppStorage(OnboardingStorage.heightInput) private var heightInput = ""
  @AppStorage(OnboardingStorage.heightFeetInput) private var heightFeetInput = ""
  @AppStorage(OnboardingStorage.heightInchesInput) private var heightInchesInput = ""
  @AppStorage(OnboardingStorage.weightInput) private var weightInput = ""
  @AppStorage(OnboardingStorage.gender) private var genderRaw = ""
  @AppStorage(OnboardingStorage.heightMm) private var heightMm = 0
  @AppStorage(OnboardingStorage.weightGrams) private var weightGrams = 0
  @AppStorage(OnboardingStorage.createdAtUnixMs) private var createdAtUnixMs = 0
  @AppStorage(OnboardingStorage.timezoneID) private var timezoneID = ""
  @State private var dateOfBirth = MoreProfileDate.defaultDateOfBirth()
  @State private var statusMessage: String?
  @State private var healthKitImporting = false
  @FocusState private var focusedField: MoreProfileField?

  private var unitSystem: MoreProfileUnitSystem {
    MoreProfileUnitSystem(rawValue: unitSystemRaw) ?? .imperial
  }

  var body: some View {
    List {
      Section("Personal") {
        HStack {
          Text("First name")
          TextField("First name", text: $firstName)
            .multilineTextAlignment(.trailing)
            .textContentType(.givenName)
            .focused($focusedField, equals: .firstName)
        }

        DatePicker(
          "Date of birth",
          selection: $dateOfBirth,
          in: MoreProfileDate.minimumDateOfBirth()...MoreProfileDate.maximumDateOfBirth(),
          displayedComponents: .date
        )

        Picker("Gender", selection: $genderRaw) {
          Text("Select").tag("")
          ForEach(MoreProfileGender.allCases) { gender in
            Text(gender.title).tag(gender.rawValue)
          }
        }
      }

      Section("Measurements") {
        Picker("Units", selection: $unitSystemRaw) {
          ForEach(MoreProfileUnitSystem.allCases) { unit in
            Text(unit.title).tag(unit.rawValue)
          }
        }

        if unitSystem == .metric {
          MoreProfileTextFieldRow(
            label: "Height",
            text: $heightInput,
            prompt: "cm",
            suffix: "cm",
            field: .heightCentimeters,
            focusedField: $focusedField
          )
        } else {
          MoreProfileTextFieldRow(
            label: "Height",
            text: $heightFeetInput,
            prompt: "ft",
            suffix: "ft",
            keyboardType: .numberPad,
            field: .heightFeet,
            focusedField: $focusedField
          )
          MoreProfileTextFieldRow(
            label: "Inches",
            text: $heightInchesInput,
            prompt: "in",
            suffix: "in",
            field: .heightInches,
            focusedField: $focusedField
          )
        }

        MoreProfileTextFieldRow(
          label: "Weight",
          text: $weightInput,
          prompt: unitSystem == .metric ? "kg" : "lb",
          suffix: unitSystem == .metric ? "kg" : "lb",
          field: .weight,
          focusedField: $focusedField
        )
      }

      Section("Apple Health") {
        Button {
          updateFromHealthKit()
        } label: {
          HStack {
            Label("Update weight", systemImage: "heart.text.square")
            Spacer()
            if healthKitImporting {
              ProgressView()
            }
          }
        }
        .disabled(healthKitImporting)
      }

      if let statusMessage {
        Section {
          Text(statusMessage)
            .font(.footnote)
            .foregroundStyle(statusMessage.hasPrefix("Profile") || statusMessage.hasPrefix("Updated") ? .green : .red)
        }
      }
    }
    .listStyle(.insetGrouped)
    .gooseListBackground()
    .navigationTitle("Profile")
    .navigationBarTitleDisplayMode(.inline)
    .toolbarBackground(.hidden, for: .navigationBar)
    .toolbar {
      ToolbarItem(placement: .topBarTrailing) {
        Button("Save") {
          saveProfile()
        }
        .fontWeight(.semibold)
      }
    }
    .toolbar {
      ToolbarItemGroup(placement: .keyboard) {
        Spacer()
        Button("Done") {
          focusedField = nil
        }
      }
    }
    .onAppear {
      hydrateDateOfBirth()
      hydrateMeasurementsIfNeeded()
    }
    .onChange(of: dateOfBirth) { _, newValue in
      dateOfBirthString = MoreProfileDate.dateOnlyString(MoreProfileDate.clamp(newValue))
    }
    .onChange(of: unitSystemRaw) { oldValue, newValue in
      convertDisplayedMeasurements(from: oldValue, to: newValue)
    }
  }

  private func hydrateDateOfBirth() {
    if let saved = MoreProfileDate.parse(dateOfBirthString) {
      dateOfBirth = MoreProfileDate.clamp(saved)
    } else {
      dateOfBirth = MoreProfileDate.defaultDateOfBirth()
      dateOfBirthString = MoreProfileDate.dateOnlyString(dateOfBirth)
    }
  }

  private func hydrateMeasurementsIfNeeded() {
    if unitSystem == .imperial,
       heightFeetInput.isEmpty,
       heightInchesInput.isEmpty,
       heightMm > 0 {
      applyHeightMillimeters(heightMm, for: .imperial)
    }
    if unitSystem == .metric, heightInput.isEmpty, heightMm > 0 {
      applyHeightMillimeters(heightMm, for: .metric)
    }
    if weightInput.isEmpty, weightGrams > 0 {
      applyWeightGrams(weightGrams, for: unitSystem)
    }
  }

  private func saveProfile() {
    focusedField = nil
    statusMessage = nil
    let trimmedName = firstName.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmedName.isEmpty else {
      statusMessage = "Enter your first name."
      return
    }
    guard trimmedName.count <= 40 else {
      statusMessage = "Use 40 characters or fewer."
      return
    }
    guard !genderRaw.isEmpty else {
      statusMessage = "Select a gender."
      return
    }
    guard let parsedHeightMm = heightMillimeters(for: unitSystem) else {
      statusMessage = "Enter height."
      return
    }
    guard let parsedWeightGrams = parsedWeightGrams(for: unitSystem) else {
      statusMessage = "Enter weight."
      return
    }

    let heightCentimeters = Double(parsedHeightMm) / 10
    guard (90...245).contains(heightCentimeters) else {
      statusMessage = "Check height."
      return
    }
    let weightKilograms = Double(parsedWeightGrams) / 1000
    guard (30...320).contains(weightKilograms) else {
      statusMessage = "Check weight."
      return
    }

    firstName = trimmedName
    dateOfBirthString = MoreProfileDate.dateOnlyString(dateOfBirth)
    heightMm = parsedHeightMm
    weightGrams = parsedWeightGrams
    if createdAtUnixMs == 0 {
      createdAtUnixMs = Int((Date().timeIntervalSince1970 * 1000).rounded())
    }
    timezoneID = TimeZone.current.identifier
    OnboardingProfilePersistence.saveProfile(
      currentProfileSnapshot(),
      onboardingComplete: UserDefaults.standard.bool(forKey: OnboardingStorage.onboardingComplete)
    )
    model.recordUIAction("profile.saved", detail: "\(unitSystem.rawValue) height_mm=\(heightMm) weight_g=\(weightGrams)")
    statusMessage = "Profile updated."
  }

  private func updateFromHealthKit() {
    guard !healthKitImporting else {
      return
    }
    healthKitImporting = true
    statusMessage = nil
    Task {
      let result = await HealthKitProfileImporter.requestProfileAccess()
      await MainActor.run {
        applyHealthKitProfileAutofill(result.autofill, overwrite: true)
        healthKitImporting = false
        statusMessage = result.autofill.hasValues ? "Updated from Apple Health." : result.status
        model.recordUIAction("profile.healthkit_autofill", detail: result.status)
      }
    }
  }

  private func applyHealthKitProfileAutofill(_ autofill: HealthKitProfileAutofill, overwrite: Bool) {
    if let grams = autofill.weightGrams,
       overwrite || weightGrams == 0 {
      weightGrams = grams
      applyWeightGrams(grams, for: unitSystem)
    }
  }

  private func currentProfileSnapshot() -> OnboardingProfileSnapshot {
    OnboardingProfileSnapshot(
      firstName: firstName,
      dateOfBirthString: dateOfBirthString,
      unitSystemRaw: unitSystemRaw,
      heightInput: heightInput,
      heightFeetInput: heightFeetInput,
      heightInchesInput: heightInchesInput,
      weightInput: weightInput,
      genderRaw: genderRaw,
      heightMm: heightMm,
      weightGrams: weightGrams,
      createdAtUnixMs: createdAtUnixMs,
      timezoneID: timezoneID
    )
  }

  private func convertDisplayedMeasurements(from oldRawValue: String, to newRawValue: String) {
    guard
      let oldUnitSystem = MoreProfileUnitSystem(rawValue: oldRawValue),
      let newUnitSystem = MoreProfileUnitSystem(rawValue: newRawValue),
      oldUnitSystem != newUnitSystem
    else {
      return
    }
    if let currentHeightMm = heightMillimeters(for: oldUnitSystem) {
      applyHeightMillimeters(currentHeightMm, for: newUnitSystem)
    }
    if let currentWeightGrams = parsedWeightGrams(for: oldUnitSystem) {
      applyWeightGrams(currentWeightGrams, for: newUnitSystem)
    }
  }

  private func heightMillimeters(for unitSystem: MoreProfileUnitSystem) -> Int? {
    switch unitSystem {
    case .metric:
      guard let centimeters = measurementValue(heightInput), centimeters > 0 else {
        return nil
      }
      return Int((centimeters * 10).rounded())
    case .imperial:
      let feet = measurementValue(heightFeetInput) ?? 0
      let inches = measurementValue(heightInchesInput) ?? 0
      let totalInches = feet * 12 + inches
      guard totalInches > 0 else {
        return nil
      }
      return Int((totalInches * 25.4).rounded())
    }
  }

  private func parsedWeightGrams(for unitSystem: MoreProfileUnitSystem) -> Int? {
    guard let weight = measurementValue(weightInput), weight > 0 else {
      return nil
    }
    switch unitSystem {
    case .metric:
      return Int((weight * 1000).rounded())
    case .imperial:
      return Int((weight * 453.59237).rounded())
    }
  }

  private func applyHeightMillimeters(_ millimeters: Int, for unitSystem: MoreProfileUnitSystem) {
    switch unitSystem {
    case .metric:
      heightInput = MoreProfileFormatting.formatted(Double(millimeters) / 10, maxFractionDigits: 1)
    case .imperial:
      let totalInches = Double(millimeters) / 25.4
      let feet = Int(totalInches / 12)
      let inches = totalInches - Double(feet * 12)
      heightFeetInput = String(feet)
      heightInchesInput = MoreProfileFormatting.formatted(inches, maxFractionDigits: 1)
      heightInput = MoreProfileFormatting.formatted(totalInches, maxFractionDigits: 1)
    }
  }

  private func applyWeightGrams(_ grams: Int, for unitSystem: MoreProfileUnitSystem) {
    switch unitSystem {
    case .metric:
      weightInput = MoreProfileFormatting.formatted(Double(grams) / 1000, maxFractionDigits: 1)
    case .imperial:
      weightInput = MoreProfileFormatting.formatted(Double(grams) / 453.59237, maxFractionDigits: 1)
    }
  }

  private func measurementValue(_ rawValue: String) -> Double? {
    let normalized = rawValue
      .trimmingCharacters(in: .whitespacesAndNewlines)
      .replacingOccurrences(of: ",", with: ".")
    return Double(normalized)
  }
}

struct MoreProfileTextFieldRow: View {
  let label: String
  @Binding var text: String
  let prompt: String
  let suffix: String
  var keyboardType: UIKeyboardType = .decimalPad
  let field: MoreProfileField
  let focusedField: FocusState<MoreProfileField?>.Binding

  var body: some View {
    HStack(spacing: 10) {
      Text(label)
      TextField(prompt, text: $text)
        .multilineTextAlignment(.trailing)
        .keyboardType(keyboardType)
        .focused(focusedField, equals: field)
      Text(suffix)
        .foregroundStyle(.secondary)
    }
  }
}

enum MoreProfileField: Hashable {
  case firstName
  case heightCentimeters
  case heightFeet
  case heightInches
  case weight
}

enum MoreProfileUnitSystem: String, CaseIterable, Identifiable {
  case imperial
  case metric

  var id: String { rawValue }

  var title: String {
    switch self {
    case .imperial: "Imperial"
    case .metric: "Metric"
    }
  }
}

enum MoreProfileGender: String, CaseIterable, Identifiable {
  case female
  case male
  case nonBinary = "non_binary"
  case preferNotToSay = "prefer_not_to_say"

  var id: String { rawValue }

  var title: String {
    switch self {
    case .female: "Female"
    case .male: "Male"
    case .nonBinary: "Non-binary"
    case .preferNotToSay: "Prefer not to say"
    }
  }
}

enum MoreProfileDate {
  static func parse(_ value: String) -> Date? {
    let formatter = dateFormatter
    guard let date = formatter.date(from: value) else {
      return nil
    }
    return Calendar.current.startOfDay(for: date)
  }

  static func dateOnlyString(_ date: Date) -> String {
    dateFormatter.string(from: date)
  }

  static func defaultDateOfBirth() -> Date {
    clamp(Calendar.current.date(byAdding: .year, value: -30, to: Date()) ?? Date())
  }

  static func minimumDateOfBirth() -> Date {
    Calendar.current.date(byAdding: .year, value: -120, to: Date()) ?? Date.distantPast
  }

  static func maximumDateOfBirth() -> Date {
    Calendar.current.date(byAdding: .year, value: -13, to: Date()) ?? Date()
  }

  static func clamp(_ date: Date) -> Date {
    let normalized = Calendar.current.startOfDay(for: date)
    let minimum = Calendar.current.startOfDay(for: minimumDateOfBirth())
    let maximum = Calendar.current.startOfDay(for: maximumDateOfBirth())
    if normalized < minimum {
      return minimum
    }
    if normalized > maximum {
      return maximum
    }
    return normalized
  }

  private static var dateFormatter: DateFormatter {
    let formatter = DateFormatter()
    formatter.calendar = Calendar(identifier: .gregorian)
    formatter.locale = Locale(identifier: "en_US_POSIX")
    formatter.dateFormat = "yyyy-MM-dd"
    return formatter
  }
}

enum MoreProfileFormatting {
  static func heightText(millimeters: Int, unitSystemRaw: String) -> String {
    guard millimeters > 0 else {
      return ""
    }
    let unitSystem = MoreProfileUnitSystem(rawValue: unitSystemRaw) ?? .imperial
    switch unitSystem {
    case .metric:
      return "\(formatted(Double(millimeters) / 10, maxFractionDigits: 1)) cm"
    case .imperial:
      let totalInches = Double(millimeters) / 25.4
      let feet = Int(totalInches / 12)
      let inches = totalInches - Double(feet * 12)
      return "\(feet) ft \(formatted(inches, maxFractionDigits: 1)) in"
    }
  }

  static func weightText(grams: Int, unitSystemRaw: String) -> String {
    guard grams > 0 else {
      return ""
    }
    let unitSystem = MoreProfileUnitSystem(rawValue: unitSystemRaw) ?? .imperial
    switch unitSystem {
    case .metric:
      return "\(formatted(Double(grams) / 1000, maxFractionDigits: 1)) kg"
    case .imperial:
      return "\(formatted(Double(grams) / 453.59237, maxFractionDigits: 1)) lb"
    }
  }

  static func formatted(_ value: Double, maxFractionDigits: Int) -> String {
    let formatter = NumberFormatter()
    formatter.locale = Locale(identifier: "en_US_POSIX")
    formatter.numberStyle = .decimal
    formatter.minimumFractionDigits = 0
    formatter.maximumFractionDigits = maxFractionDigits
    return formatter.string(from: NSNumber(value: value)) ?? String(format: "%.\(maxFractionDigits)f", value)
  }
}

struct MoreRouteRow: View {
  let route: MoreRoute
  let status: MoreStatusKind
  var showsStatus = false

  var body: some View {
    HStack(spacing: 12) {
      Image(systemName: route.systemImage)
        .font(.title3)
        .foregroundStyle(status.tint)
        .frame(width: 28, height: 28)

      VStack(alignment: .leading, spacing: 3) {
        Text(route.title)
          .font(.body.weight(.semibold))
        Text(route.subtitle)
          .font(.caption)
          .foregroundStyle(.secondary)
          .lineLimit(2)
      }

      Spacer(minLength: 8)
      if showsStatus {
        MoreStatusBadge(status: status)
      }
    }
    .padding(.vertical, 4)
  }
}

struct MoreStatusBadge: View {
  let status: MoreStatusKind

  var body: some View {
    Label(status.title, systemImage: status.systemImage)
      .font(.caption2.weight(.semibold))
      .labelStyle(.titleAndIcon)
      .padding(.horizontal, 8)
      .padding(.vertical, 4)
      .background(status.tint.opacity(0.14), in: Capsule())
      .foregroundStyle(status.tint)
      .lineLimit(1)
      .minimumScaleFactor(0.8)
      .accessibilityLabel(status.title)
  }
}
