import CoreBluetooth
import CoreLocation
import HealthKit
import SwiftUI
import UIKit
import UserNotifications

struct OnboardingView: View {
  @Environment(GooseAppModel.self) private var model
  let onComplete: () -> Void
  @StateObject private var locationPermissionRequester = OnboardingLocationPermissionRequester()

  @State private var step = OnboardingStep.healthKit
  @State private var dateOfBirth = OnboardingDate.defaultDateOfBirth()
  @State private var validationMessage: String?
  @State private var healthKitStatus = "Not requested"
  @State private var healthKitRequesting = false
  @State private var locationStatus = "Not requested"
  @State private var locationRequesting = false
  @State private var notificationStatus = "Not requested"
  @State private var notificationRequesting = false
  @State private var bluetoothPermissionResolved = OnboardingPermissionState.bluetoothResolved()
  @State private var locationPermissionResolved = OnboardingPermissionState.locationResolved()
  @State private var notificationPermissionResolved = false
  @FocusState private var focusedField: OnboardingInputField?

  @AppStorage(OnboardingStorage.firstName) private var firstName = ""
  @AppStorage(OnboardingStorage.dateOfBirth) private var dateOfBirthString = ""
  @AppStorage(OnboardingStorage.unitSystem) private var unitSystemRaw = OnboardingUnitSystem.imperial.rawValue
  @AppStorage(OnboardingStorage.heightInput) private var heightInput = ""
  @AppStorage(OnboardingStorage.heightFeetInput) private var heightFeetInput = ""
  @AppStorage(OnboardingStorage.heightInchesInput) private var heightInchesInput = ""
  @AppStorage(OnboardingStorage.weightInput) private var weightInput = ""
  @AppStorage(OnboardingStorage.gender) private var genderRaw = ""
  @AppStorage(OnboardingStorage.heightMm) private var heightMm = 0
  @AppStorage(OnboardingStorage.weightGrams) private var weightGrams = 0
  @AppStorage(OnboardingStorage.createdAtUnixMs) private var createdAtUnixMs = 0
  @AppStorage(OnboardingStorage.timezoneID) private var timezoneID = ""
  @AppStorage(OnboardingStorage.healthKitPermissionHandled) private var healthKitPermissionHandled = false
  @AppStorage(OnboardingStorage.locationPermissionHandled) private var locationPermissionHandled = false
  @AppStorage(OnboardingStorage.notificationPermissionHandled) private var notificationPermissionHandled = false

  var body: some View {
    VStack(spacing: 0) {
      ScrollView {
        VStack(alignment: .leading, spacing: 22) {
          OnboardingHeader(step: step)
          content
        }
        .padding(.horizontal, 20)
        .padding(.top, 18)
        .padding(.bottom, 28)
      }
    }
    .background {
      ZStack {
        GooseTheme.appBackground
          .ignoresSafeArea()
          .onTapGesture {
            focusedField = nil
          }
        OnboardingKeyboardDismissTapCatcher(isEnabled: focusedField != nil) {
          focusedField = nil
        }
      }
    }
    .scrollDismissesKeyboard(.interactively)
    .toolbar(.hidden, for: .navigationBar)
    .toolbar {
      ToolbarItemGroup(placement: .keyboard) {
        Spacer()
        Button("Done") {
          focusedField = nil
        }
      }
    }
    .safeAreaInset(edge: .bottom) {
      if focusedField == nil {
        footer
          .transition(.move(edge: .bottom).combined(with: .opacity))
      }
    }
    .onAppear(perform: hydrateOnAppear)
    .onChange(of: dateOfBirth) { _, newValue in
      dateOfBirthString = OnboardingDate.dateOnlyString(OnboardingDate.clamp(newValue))
    }
    .onChange(of: unitSystemRaw) { oldValue, newValue in
      convertDisplayedMeasurements(from: oldValue, to: newValue)
    }
    .onChange(of: model.ble.bluetoothState) { _, _ in
      bluetoothPermissionResolved = OnboardingPermissionState.bluetoothResolved()
      if step == .bluetooth, shouldSkip(.bluetooth) {
        moveForward()
      }
    }
  }

  @ViewBuilder
  private var content: some View {
    switch step {
    case .profile:
      OnboardingProfileStep(
        firstName: $firstName,
        dateOfBirth: $dateOfBirth,
        unitSystemRaw: $unitSystemRaw,
        heightInput: $heightInput,
        heightFeetInput: $heightFeetInput,
        heightInchesInput: $heightInchesInput,
        weightInput: $weightInput,
        genderRaw: $genderRaw,
        validationMessage: validationMessage,
        focusedField: $focusedField
      )
    case .healthKit:
      OnboardingPermissionStep(
        systemImage: "heart.fill",
        title: "HealthKit",
        bodyText: "Goose uses HealthKit only to prefill profile values.",
        details: [
          "Body weight to prefill your profile",
          "No steps, calories, workouts, sleep, or recovery metrics imported",
        ],
        buttonTitle: "Import Weight",
        isRequesting: healthKitRequesting,
        tint: .red,
        action: requestHealthKitAccess
      )
    case .location:
      OnboardingPermissionStep(
        systemImage: "location.fill",
        title: "Location",
        bodyText: "Goose uses location to track outdoor workouts, routes, pace, and distance while an activity is running.",
        details: [
          "Active workout GPS while Goose is open",
          "Background route tracking when you minimize Goose",
          "Distance, pace, elevation, and route points",
        ],
        buttonTitle: "Enable Location",
        isRequesting: locationRequesting,
        tint: .green,
        action: requestLocationAccess
      )
    case .bluetooth:
      OnboardingPermissionStep(
        systemImage: "bluetooth",
        title: "Bluetooth",
        bodyText: "Goose needs Bluetooth to find your owned WHOOP strap and keep the local connection live.",
        details: [
          "Scan for nearby WHOOP services",
          "Connect to the selected strap",
          "Read live battery, firmware, and strap notifications",
        ],
        buttonTitle: "Enable Bluetooth",
        isRequesting: false,
        tint: .blue,
        action: requestBluetoothAccess
      )
    case .notifications:
      OnboardingPermissionStep(
        systemImage: "bell.badge.fill",
        title: "Notifications",
        bodyText: "Goose can notify you when the strap connects, disconnects, or needs attention.",
        details: [
          "Connection and reconnect status",
          "Battery and sync reminders",
          "Local alerts only",
        ],
        buttonTitle: "Enable Notifications",
        isRequesting: notificationRequesting,
        tint: .orange,
        action: requestNotificationAccess
      )
    case .connect:
      OnboardingConnectStep(ble: model.ble)
    }
  }

  @ViewBuilder
  private var footer: some View {
    VStack(spacing: 0) {
      if step == .connect {
        OnboardingConnectActionBar(
          ble: model.ble,
          onBack: moveBack,
          readyTitle: nextAvailableStep(after: step) == nil ? "Finish setup" : "Continue",
          onComplete: moveForward
        )
      } else {
        OnboardingStandardActionBar(
          showBack: step.previous != nil,
          primaryTitle: standardPrimaryTitle,
          onBack: moveBack,
          onPrimary: continueFromCurrentStep
        )
      }
      Button(String(localized: "Skip setup")) {
        onComplete()
      }
      .font(.footnote)
      .foregroundStyle(.secondary)
      .padding(.vertical, 12)
    }
  }

  private func hydrateOnAppear() {
    restorePersistedProfileIfNeeded()
    hydrateDateOfBirth()
    hydrateMeasurementsIfNeeded()
    refreshPermissionState()
    if shouldSkip(step), let next = nextAvailableStep(after: step) {
      step = next
    }
  }

  private func hydrateDateOfBirth() {
    if let saved = OnboardingDate.parse(dateOfBirthString) {
      dateOfBirth = OnboardingDate.clamp(saved)
    } else {
      dateOfBirth = OnboardingDate.defaultDateOfBirth()
      dateOfBirthString = OnboardingDate.dateOnlyString(dateOfBirth)
    }
  }

  private func hydrateMeasurementsIfNeeded() {
    let unitSystem = OnboardingUnitSystem(rawValue: unitSystemRaw) ?? .imperial
    if unitSystem == .imperial,
       heightFeetInput.isEmpty,
       heightInchesInput.isEmpty {
      if let totalInches = measurementValue(heightInput), totalInches > 0 {
        applyHeightMillimeters(Int((totalInches * 25.4).rounded()), for: .imperial)
      } else if heightMm > 0 {
        applyHeightMillimeters(heightMm, for: .imperial)
      }
    }
    if unitSystem == .metric, heightInput.isEmpty, heightMm > 0 {
      applyHeightMillimeters(heightMm, for: .metric)
    }
    if weightInput.isEmpty, weightGrams > 0 {
      applyWeightGrams(weightGrams, for: unitSystem)
    }
  }

  private func continueFromCurrentStep() {
    if step == .healthKit {
      requestHealthKitAccess()
      return
    }
    if step == .location {
      requestLocationAccess()
      return
    }
    if step == .profile {
      saveProfileAndContinue()
      return
    }
    moveForward()
  }

  private var standardPrimaryTitle: String {
    if step == .healthKit {
      return "Import Weight"
    }
    if step == .location {
      return "Enable Location"
    }
    return nextAvailableStep(after: step) == nil ? "Finish setup" : "Continue"
  }

  private func saveProfileAndContinue() {
    validationMessage = nil
    let trimmedName = firstName.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmedName.isEmpty else {
      validationMessage = "Enter your first name."
      return
    }
    guard trimmedName.count <= 40 else {
      validationMessage = "Use 40 characters or fewer."
      return
    }
    guard !genderRaw.isEmpty else {
      validationMessage = "Select a gender."
      return
    }
    let unitSystem = OnboardingUnitSystem(rawValue: unitSystemRaw) ?? .imperial
    guard let parsedHeightMm = heightMillimeters(for: unitSystem) else {
      validationMessage = "Enter height."
      return
    }
    guard let parsedWeightGrams = weightGrams(for: unitSystem) else {
      validationMessage = "Enter weight."
      return
    }

    let heightCentimeters = Double(parsedHeightMm) / 10
    guard (90...245).contains(heightCentimeters) else {
      validationMessage = "Check height."
      return
    }
    let weightKilograms = Double(parsedWeightGrams) / 1000
    guard (30...320).contains(weightKilograms) else {
      validationMessage = "Check weight."
      return
    }

    firstName = trimmedName
    dateOfBirthString = OnboardingDate.dateOnlyString(dateOfBirth)
    heightMm = parsedHeightMm
    weightGrams = parsedWeightGrams
    createdAtUnixMs = Int((Date().timeIntervalSince1970 * 1000).rounded())
    timezoneID = TimeZone.current.identifier
    OnboardingProfilePersistence.saveProfile(
      currentProfileSnapshot(),
      onboardingComplete: false
    )
    model.recordUIAction(
      "onboarding.profile.saved",
      detail: "\(unitSystem.rawValue) height_mm=\(heightMm) weight_g=\(weightGrams)"
    )
    moveForward()
  }

  private func measurementValue(_ rawValue: String) -> Double? {
    let normalized = rawValue
      .trimmingCharacters(in: .whitespacesAndNewlines)
      .replacingOccurrences(of: ",", with: ".")
    return Double(normalized)
  }

  private func convertDisplayedMeasurements(from oldRawValue: String, to newRawValue: String) {
    guard
      let oldUnitSystem = OnboardingUnitSystem(rawValue: oldRawValue),
      let newUnitSystem = OnboardingUnitSystem(rawValue: newRawValue),
      oldUnitSystem != newUnitSystem
    else {
      return
    }
    if let currentHeightMm = heightMillimeters(for: oldUnitSystem) {
      applyHeightMillimeters(currentHeightMm, for: newUnitSystem)
    }
    if let currentWeightGrams = weightGrams(for: oldUnitSystem) {
      applyWeightGrams(currentWeightGrams, for: newUnitSystem)
    }
  }

  private func heightMillimeters(for unitSystem: OnboardingUnitSystem) -> Int? {
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

  private func weightGrams(for unitSystem: OnboardingUnitSystem) -> Int? {
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

  private func applyHeightMillimeters(_ millimeters: Int, for unitSystem: OnboardingUnitSystem) {
    switch unitSystem {
    case .metric:
      heightInput = Self.formatted(Double(millimeters) / 10, maxFractionDigits: 1)
    case .imperial:
      let totalInches = Double(millimeters) / 25.4
      let feet = Int(totalInches / 12)
      let inches = totalInches - Double(feet * 12)
      heightFeetInput = String(feet)
      heightInchesInput = Self.formatted(inches, maxFractionDigits: 1)
      heightInput = Self.formatted(totalInches, maxFractionDigits: 1)
    }
  }

  private func applyWeightGrams(_ grams: Int, for unitSystem: OnboardingUnitSystem) {
    switch unitSystem {
    case .metric:
      weightInput = Self.formatted(Double(grams) / 1000, maxFractionDigits: 1)
    case .imperial:
      weightInput = Self.formatted(Double(grams) / 453.59237, maxFractionDigits: 1)
    }
  }

  private static func formatted(_ value: Double, maxFractionDigits: Int) -> String {
    let formatter = NumberFormatter()
    formatter.locale = Locale(identifier: "en_US_POSIX")
    formatter.numberStyle = .decimal
    formatter.minimumFractionDigits = 0
    formatter.maximumFractionDigits = maxFractionDigits
    return formatter.string(from: NSNumber(value: value)) ?? String(format: "%.\(maxFractionDigits)f", value)
  }

  private func requestHealthKitAccess() {
    guard !healthKitRequesting else {
      return
    }
    healthKitRequesting = true
    healthKitStatus = "Requesting..."
    model.recordUIAction("onboarding.healthkit.requested")

    Task {
      let result = await HealthKitPermissionRequester.requestAccess()
      await MainActor.run {
        healthKitStatus = result.status
        applyHealthKitProfileAutofill(result.autofill, overwrite: false)
        healthKitRequesting = false
        healthKitPermissionHandled = true
        model.recordUIAction("onboarding.healthkit.result", detail: result.status)
        moveForward()
      }
    }
  }

  private func applyHealthKitProfileAutofill(_ autofill: HealthKitProfileAutofill, overwrite: Bool) {
    let unitSystem = OnboardingUnitSystem(rawValue: unitSystemRaw) ?? .imperial
    if let grams = autofill.weightGrams,
       overwrite || (weightGrams == 0 && weightInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty) {
      weightGrams = grams
      applyWeightGrams(grams, for: unitSystem)
    }
  }

  private func requestLocationAccess() {
    guard !locationRequesting else {
      return
    }
    locationRequesting = true
    locationStatus = "Requesting..."
    model.recordUIAction("onboarding.location.requested")

    Task {
      let result = await locationPermissionRequester.requestAccess()
      await MainActor.run {
        locationStatus = result.status
        locationRequesting = false
        locationPermissionHandled = result.isResolved
        locationPermissionResolved = result.isResolved || OnboardingPermissionState.locationResolved()
        model.recordUIAction("onboarding.location.result", detail: result.status)
        moveForward()
      }
    }
  }

  private func requestBluetoothAccess() {
    model.ble.requestBluetooth()
    bluetoothPermissionResolved = OnboardingPermissionState.bluetoothResolved()
    model.recordUIAction("onboarding.bluetooth.requested")
    if shouldSkip(.bluetooth) {
      moveForward()
    }
  }

  private func requestNotificationAccess() {
    guard !notificationRequesting else {
      return
    }
    notificationRequesting = true
    notificationStatus = "Requesting..."
    model.recordUIAction("onboarding.notifications.requested")

    Task {
      let status: String
      do {
        let granted = try await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound])
        status = granted ? "Allowed" : "Not allowed"
      } catch {
        status = "Failed: \(error.localizedDescription)"
      }
      await MainActor.run {
        notificationStatus = status
        notificationRequesting = false
        notificationPermissionHandled = true
        notificationPermissionResolved = true
        model.recordUIAction("onboarding.notifications.result", detail: status)
        moveForward()
      }
    }
  }

  private func moveForward() {
    refreshPermissionState()
    guard let next = nextAvailableStep(after: step) else {
      finishOnboarding()
      return
    }
    validationMessage = nil
    withAnimation(.snappy) {
      step = next
    }
  }

  private func moveBack() {
    refreshPermissionState()
    guard let previous = previousAvailableStep(before: step) else {
      return
    }
    validationMessage = nil
    withAnimation(.snappy) {
      step = previous
    }
  }

  private func finishOnboarding() {
    OnboardingProfilePersistence.markCompleteFromDefaults()
    model.recordUIAction("onboarding.finish", detail: "step=\(step.rawValue)")
    onComplete()
  }

  private func restorePersistedProfileIfNeeded() {
    guard
      let state = OnboardingProfilePersistence.restoreIntoDefaultsIfAvailable(restoreCompletion: false),
      state.profile.hasRequiredDetails
    else {
      return
    }
    applyPersistedProfile(state.profile)
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

  private func applyPersistedProfile(_ profile: OnboardingProfileSnapshot) {
    if firstName.isEmpty {
      firstName = profile.firstName
    }
    if dateOfBirthString.isEmpty {
      dateOfBirthString = profile.dateOfBirthString
    }
    if unitSystemRaw.isEmpty {
      unitSystemRaw = profile.unitSystemRaw
    }
    if heightInput.isEmpty {
      heightInput = profile.heightInput
    }
    if heightFeetInput.isEmpty {
      heightFeetInput = profile.heightFeetInput
    }
    if heightInchesInput.isEmpty {
      heightInchesInput = profile.heightInchesInput
    }
    if weightInput.isEmpty {
      weightInput = profile.weightInput
    }
    if genderRaw.isEmpty {
      genderRaw = profile.genderRaw
    }
    if heightMm == 0 {
      heightMm = profile.heightMm
    }
    if weightGrams == 0 {
      weightGrams = profile.weightGrams
    }
    if createdAtUnixMs == 0 {
      createdAtUnixMs = profile.createdAtUnixMs
    }
    if timezoneID.isEmpty {
      timezoneID = profile.timezoneID
    }
  }

  private func refreshPermissionState() {
    bluetoothPermissionResolved = OnboardingPermissionState.bluetoothResolved()
    locationPermissionResolved = OnboardingPermissionState.locationResolved()
    if locationPermissionResolved {
      locationPermissionHandled = true
    }
    Task {
      let resolved = await OnboardingPermissionState.notificationResolved()
      await MainActor.run {
        notificationPermissionResolved = resolved
        if resolved {
          notificationPermissionHandled = true
        }
      }
    }
  }

  private func shouldSkip(_ candidate: OnboardingStep) -> Bool {
    switch candidate {
    case .profile, .connect:
      return false
    case .healthKit:
      return healthKitPermissionHandled || !HKHealthStore.isHealthDataAvailable()
    case .location:
      return locationPermissionHandled || locationPermissionResolved
    case .bluetooth:
      return bluetoothPermissionResolved || bluetoothStateIsResolved
    case .notifications:
      return notificationPermissionHandled || notificationPermissionResolved
    }
  }

  private var bluetoothStateIsResolved: Bool {
    switch model.ble.bluetoothState {
    case "powered on", "powered off", "unauthorized", "unsupported", "bluetooth unavailable":
      return true
    default:
      return false
    }
  }

  private func nextAvailableStep(after currentStep: OnboardingStep) -> OnboardingStep? {
    var candidate = currentStep.next
    while let step = candidate, shouldSkip(step) {
      candidate = step.next
    }
    return candidate
  }

  private func previousAvailableStep(before currentStep: OnboardingStep) -> OnboardingStep? {
    var candidate = currentStep.previous
    while let step = candidate, shouldSkip(step) {
      candidate = step.previous
    }
    return candidate
  }
}
