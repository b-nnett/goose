import SwiftUI
import UIKit

struct OnboardingHeader: View {
  let step: OnboardingStep

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      HStack {
        Text(step.stepLabel)
        Spacer()
        Text("\(Int((step.progress * 100).rounded()))%")
      }
      .font(.caption.weight(.semibold))
      .foregroundStyle(.secondary)

      Text(step.title)
        .font(.system(size: 34, weight: .bold, design: .rounded))
        .foregroundStyle(.primary)
      ProgressView(value: step.progress)
        .tint(.blue)
    }
  }
}

struct OnboardingProfileStep: View {
  @Binding var firstName: String
  @Binding var dateOfBirth: Date
  @Binding var unitSystemRaw: String
  @Binding var heightInput: String
  @Binding var heightFeetInput: String
  @Binding var heightInchesInput: String
  @Binding var weightInput: String
  @Binding var genderRaw: String
  let validationMessage: String?
  let focusedField: FocusState<OnboardingInputField?>.Binding

  private var unitSystem: OnboardingUnitSystem {
    OnboardingUnitSystem(rawValue: unitSystemRaw) ?? .imperial
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 18) {
      Text("These basics help Goose calculate your local metrics.")
        .font(.body)
        .foregroundStyle(.secondary)

      OnboardingGroupedSection {
        OnboardingTextFieldRow(
          label: "First name",
          text: $firstName,
          prompt: "First name",
          keyboardType: .default,
          textContentType: .givenName,
          field: .firstName,
          focusedField: focusedField
        )
        OnboardingDivider()
        DatePicker(
          "Date of birth",
          selection: $dateOfBirth,
          in: OnboardingDate.minimumDateOfBirth()...OnboardingDate.maximumDateOfBirth(),
          displayedComponents: .date
        )
        .font(.body)
        .padding(.horizontal, 16)
        .frame(minHeight: 50)
      }

      VStack(alignment: .leading, spacing: 10) {
        OnboardingSectionLabel("Units")
        Picker("Units", selection: $unitSystemRaw) {
          ForEach(OnboardingUnitSystem.allCases) { unit in
            Text(unit.title).tag(unit.rawValue)
          }
        }
        .pickerStyle(.segmented)
      }

      VStack(alignment: .leading, spacing: 10) {
        OnboardingSectionLabel("Measurements")
        OnboardingGroupedSection {
          if unitSystem == .metric {
            OnboardingTextFieldRow(
              label: "Height",
              text: $heightInput,
              prompt: "cm",
              keyboardType: .decimalPad,
              suffix: "cm",
              field: .heightCentimeters,
              focusedField: focusedField
            )
          } else {
            OnboardingImperialHeightRow(
              feet: $heightFeetInput,
              inches: $heightInchesInput,
              focusedField: focusedField
            )
          }
          OnboardingDivider()
          OnboardingTextFieldRow(
            label: "Weight",
            text: $weightInput,
            prompt: unitSystem == .metric ? "kg" : "lb",
            keyboardType: .decimalPad,
            suffix: unitSystem == .metric ? "kg" : "lb",
            field: .weight,
            focusedField: focusedField
          )
        }
      }

      VStack(alignment: .leading, spacing: 10) {
        OnboardingSectionLabel("Gender")
        OnboardingGroupedSection {
          Picker("Gender", selection: $genderRaw) {
            Text("Select").tag("")
            ForEach(OnboardingGender.allCases) { gender in
              Text(gender.title).tag(gender.rawValue)
            }
          }
          .pickerStyle(.menu)
          .font(.body)
          .padding(.horizontal, 16)
          .frame(minHeight: 50)
        }
      }

      if let validationMessage {
        Text(validationMessage)
          .font(.footnote)
          .foregroundStyle(.red)
          .padding(.horizontal, 4)
      }
    }
  }
}

struct OnboardingPermissionStep: View {
  let systemImage: String
  let title: String
  let bodyText: String
  let details: [String]
  let buttonTitle: String
  let isRequesting: Bool
  let tint: Color
  let action: () -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 18) {
      Text(bodyText)
        .font(.body)
        .foregroundStyle(.secondary)

      OnboardingGroupedSection {
        VStack(alignment: .leading, spacing: 16) {
          HStack(spacing: 12) {
            Image(systemName: systemImage)
              .font(.headline)
              .foregroundStyle(tint)
              .frame(width: 36, height: 36)
              .background(tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            Text(title)
              .font(.headline)
          }

          VStack(alignment: .leading, spacing: 10) {
            ForEach(details, id: \.self) { detail in
              Label(detail, systemImage: "checkmark.circle.fill")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .labelStyle(.titleAndIcon)
            }
          }

          Button(action: action) {
            HStack {
              if isRequesting {
                ProgressView()
              }
              Text(buttonTitle)
                .frame(maxWidth: .infinity)
            }
          }
          .buttonStyle(.borderedProminent)
          .controlSize(.large)
          .disabled(isRequesting)
        }
        .padding(16)
      }
    }
  }
}

struct OnboardingConnectStep: View {
  var ble: GooseBLEClient

  var body: some View {
    VStack(alignment: .leading, spacing: 18) {
      RoundedRectangle(cornerRadius: 8, style: .continuous)
        .fill(Color(.secondarySystemGroupedBackground))
        .aspectRatio(1.7, contentMode: .fit)
        .overlay {
          Image("onboarding_pairing_help")
            .resizable()
            .scaledToFit()
            .padding(18)
        }
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))

      VStack(alignment: .leading, spacing: 8) {
        Text(connectHeading)
          .font(.title2.weight(.bold))
        Text(connectBody)
          .font(.body)
          .foregroundStyle(.secondary)
      }

      OnboardingStateRow(systemImage: connectIcon, label: connectStateLabel, detail: ble.connectionState)

      if !ble.discoveredDevices.isEmpty {
        VStack(alignment: .leading, spacing: 10) {
          OnboardingSectionLabel("Choose your strap")
          VStack(spacing: 8) {
            ForEach(ble.discoveredDevices.prefix(4)) { device in
              OnboardingDiscoveredStrapRow(
                device: device,
                selected: ble.selectedDeviceID == device.id
              ) {
                ble.select(device)
              }
            }
          }
        }
      }
    }
  }

  private var hasDiscoveredStraps: Bool {
    !ble.discoveredDevices.isEmpty
  }

  private var connected: Bool {
    ["connecting", "discovering", "connected", "ready"].contains(ble.connectionState)
  }

  private var canVerify: Bool {
    ble.connectionState == "ready"
      || ble.liveHeartRateBPM != nil
      || ble.batteryLevelPercent != nil
      || ble.firmwareVersion != nil
      || ble.modelNumber != nil
  }

  private var searching: Bool {
    ble.isScanning || ble.bluetoothState == "waiting for bluetooth"
  }

  private var connectHeading: String {
    if canVerify {
      return "WHOOP is connected"
    }
    if connected {
      return "Reading strap data"
    }
    if hasDiscoveredStraps {
      return "We found a WHOOP nearby"
    }
    if searching {
      return "Looking for your WHOOP"
    }
    return "Pair your WHOOP strap"
  }

  private var connectBody: String {
    if canVerify {
      return "Finish setup to start using Goose with this strap."
    }
    if connected {
      return "Keep the strap close while Goose confirms it can read data."
    }
    if hasDiscoveredStraps {
      return "Select the strap you want to use with Goose."
    }
    if searching {
      return "Keep Bluetooth on and keep the strap close to this phone."
    }
    return "Supports WHOOP 4.0 and WHOOP 5.0. Take the strap off your wrist, keep it nearby, then start pairing."
  }

  private var connectStateLabel: String {
    if canVerify {
      return "Connected and ready"
    }
    if connected {
      return "Connected"
    }
    if hasDiscoveredStraps {
      return "Strap found"
    }
    if searching {
      return "Searching"
    }
    return "Ready to pair"
  }

  private var connectIcon: String {
    if canVerify || connected {
      return "checkmark.circle.fill"
    }
    if searching {
      return "antenna.radiowaves.left.and.right"
    }
    return "bluetooth"
  }
}

struct OnboardingStandardActionBar: View {
  let showBack: Bool
  let primaryTitle: String
  let onBack: () -> Void
  let onPrimary: () -> Void

  var body: some View {
    HStack(spacing: 12) {
      if showBack {
        Button(action: onBack) {
          Label("Back", systemImage: "chevron.left")
            .labelStyle(.titleAndIcon)
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.bordered)
        .controlSize(.large)
      }
      Button(action: onPrimary) {
        Text(primaryTitle)
          .frame(maxWidth: .infinity)
      }
      .buttonStyle(.borderedProminent)
      .controlSize(.large)
    }
    .padding(16)
    .background(.regularMaterial)
  }
}

struct OnboardingConnectActionBar: View {
  var ble: GooseBLEClient
  let onBack: () -> Void
  let readyTitle: String
  let onComplete: () -> Void

  var body: some View {
    VStack(spacing: 10) {
      Button(action: primaryAction) {
        Text(primaryTitle)
          .frame(maxWidth: .infinity)
      }
      .buttonStyle(.borderedProminent)
      .controlSize(.large)
      .disabled(primaryDisabled)

      if hasDiscoveredStraps && !connected {
        Button("Search again", action: startPairing)
          .buttonStyle(.bordered)
          .controlSize(.large)
          .frame(maxWidth: .infinity)
      }

      Button(action: onBack) {
        Label("Back", systemImage: "chevron.left")
          .labelStyle(.titleAndIcon)
          .frame(maxWidth: .infinity)
      }
      .buttonStyle(.bordered)
      .controlSize(.large)
    }
    .padding(16)
    .background(.regularMaterial)
  }

  private var hasDiscoveredStraps: Bool {
    !ble.discoveredDevices.isEmpty
  }

  private var connected: Bool {
    ["connecting", "discovering", "connected", "ready"].contains(ble.connectionState)
  }

  private var canVerify: Bool {
    ble.connectionState == "ready"
      || ble.liveHeartRateBPM != nil
      || ble.batteryLevelPercent != nil
      || ble.firmwareVersion != nil
      || ble.modelNumber != nil
  }

  private var searching: Bool {
    ble.isScanning
  }

  private var primaryTitle: String {
    if canVerify {
      return readyTitle
    }
    if connected {
      return "Waiting for strap data"
    }
    if hasDiscoveredStraps {
      return "Connect selected strap"
    }
    return searching ? "Searching..." : "Find my WHOOP"
  }

  private var primaryDisabled: Bool {
    if connected && !canVerify {
      return true
    }
    if searching && !hasDiscoveredStraps {
      return true
    }
    return false
  }

  private func primaryAction() {
    if canVerify {
      onComplete()
    } else if hasDiscoveredStraps {
      ble.connectSelected()
    } else {
      startPairing()
    }
  }

  private func startPairing() {
    ble.requestBluetooth()
    ble.startScan()
  }
}

struct OnboardingGroupedSection<Content: View>: View {
  let content: Content

  init(@ViewBuilder content: () -> Content) {
    self.content = content()
  }

  var body: some View {
    VStack(spacing: 0) {
      content
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .background(Color(.secondarySystemGroupedBackground))
    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
    .overlay {
      RoundedRectangle(cornerRadius: 8, style: .continuous)
        .strokeBorder(Color(.separator).opacity(0.35))
    }
  }
}

struct OnboardingImperialHeightRow: View {
  @Binding var feet: String
  @Binding var inches: String
  let focusedField: FocusState<OnboardingInputField?>.Binding

  var body: some View {
    VStack(spacing: 0) {
      OnboardingTextFieldRow(
        label: "Height",
        text: $feet,
        prompt: "ft",
        keyboardType: .numberPad,
        suffix: "ft",
        field: .heightFeet,
        focusedField: focusedField
      )
      OnboardingDivider()
      OnboardingTextFieldRow(
        label: "Inches",
        text: $inches,
        prompt: "in",
        keyboardType: .decimalPad,
        suffix: "in",
        field: .heightInches,
        focusedField: focusedField
      )
    }
  }
}

struct OnboardingTextFieldRow: View {
  let label: String
  @Binding var text: String
  let prompt: String
  let keyboardType: UIKeyboardType
  var textContentType: UITextContentType?
  var suffix: String? = nil
  let field: OnboardingInputField
  let focusedField: FocusState<OnboardingInputField?>.Binding

  var body: some View {
    HStack(spacing: 12) {
      Text(label)
        .foregroundStyle(.primary)
      TextField(suffix == nil ? prompt : "0", text: $text)
        .multilineTextAlignment(.trailing)
        .keyboardType(keyboardType)
        .textContentType(textContentType)
        .focused(focusedField, equals: field)
        .submitLabel(.done)
        .onSubmit {
          focusedField.wrappedValue = nil
        }
      if let suffix {
        Text(suffix)
          .foregroundStyle(.secondary)
      }
    }
    .font(.body)
    .padding(.horizontal, 16)
    .frame(minHeight: 50)
  }
}

struct OnboardingDivider: View {
  var body: some View {
    Divider()
      .padding(.leading, 16)
  }
}

struct OnboardingSectionLabel: View {
  let text: String

  init(_ text: String) {
    self.text = text
  }

  var body: some View {
    Text(text.uppercased())
      .font(.caption.weight(.semibold))
      .foregroundStyle(.secondary)
      .padding(.horizontal, 4)
  }
}

struct OnboardingStateRow: View {
  let systemImage: String
  let label: String
  let detail: String

  var body: some View {
    HStack(spacing: 12) {
      Image(systemName: systemImage)
        .font(.title3)
        .foregroundStyle(.blue)
        .frame(width: 28)
      VStack(alignment: .leading, spacing: 2) {
        Text(label)
          .font(.headline)
        Text(detail)
          .font(.subheadline)
          .foregroundStyle(.secondary)
      }
      Spacer()
    }
    .padding(14)
    .frame(maxWidth: .infinity, alignment: .leading)
    .background(Color(.secondarySystemGroupedBackground))
    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
  }
}

struct OnboardingDiscoveredStrapRow: View {
  let device: GooseDiscoveredDevice
  let selected: Bool
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      HStack(spacing: 12) {
        Image(systemName: selected ? "checkmark.circle.fill" : "circle")
          .foregroundStyle(selected ? .blue : .secondary)
        VStack(alignment: .leading, spacing: 2) {
          Text(device.name)
            .font(.headline)
            .foregroundStyle(.primary)
          Text("\(device.generation == "unknown" ? "" : "Gen \(device.generation.prefix(1)) · ")RSSI \(device.rssi)")
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
        Spacer()
      }
      .padding(14)
      .frame(maxWidth: .infinity, alignment: .leading)
      .background(Color(.secondarySystemGroupedBackground))
      .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
    }
    .buttonStyle(.plain)
  }
}

