import Darwin
import Foundation
import SwiftUI
import UIKit

struct SleepV2SleepNeededSheet: View {
  let palette: SleepV2Palette
  @Environment(\.dismiss) private var dismiss
  @State private var targetSleepMinutes = 7 * 60 + 30

	  var body: some View {
	    NavigationStack {
	      ScrollView {
	        VStack(alignment: .leading, spacing: 18) {
	          VStack(alignment: .center, spacing: 14) {
	            Image(systemName: "moon.zzz.fill")
	              .font(.title2.weight(.semibold))
	              .foregroundStyle(palette.accent)
	              .frame(width: 50, height: 50)
	              .background(palette.accent.opacity(0.12), in: Circle())
	            VStack(spacing: 4) {
	              Text("Tonight's sleep needed")
	                .font(.headline.weight(.semibold))
	                .foregroundStyle(palette.secondaryText)
	              Text(sleepNeededText)
	                .font(.system(size: 52, weight: .semibold, design: .rounded))
	                .foregroundStyle(palette.text)
	                .lineLimit(1)
	                .minimumScaleFactor(0.70)
	              Text("Target time in bed for the next sleep window.")
	                .font(.subheadline)
	                .multilineTextAlignment(.center)
	                .foregroundStyle(palette.secondaryText)
	            }
	          }
	          .frame(maxWidth: .infinity)
	          .padding(24)
	          .background(palette.surface, in: RoundedRectangle(cornerRadius: 30, style: .continuous))
	          .overlay(RoundedRectangle(cornerRadius: 30, style: .continuous).stroke(palette.separator.opacity(0.70), lineWidth: 1))

            VStack(alignment: .leading, spacing: 14) {
              HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 3) {
                  Text("Target amount")
                    .font(.headline.weight(.semibold))
                    .foregroundStyle(palette.text)
                  Text("Your preferred sleep duration before buffers.")
                    .font(.caption.weight(.medium))
                    .foregroundStyle(palette.secondaryText)
                }
                Spacer()
                Text(targetSleepText)
                  .font(.title3.weight(.semibold))
                  .fontDesign(.rounded)
                  .foregroundStyle(palette.text)
              }

              Stepper("Target sleep", value: $targetSleepMinutes, in: 5 * 60...10 * 60, step: 15)
                .labelsHidden()
            }
            .padding(20)
            .background(palette.surface, in: RoundedRectangle(cornerRadius: 24, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 24, style: .continuous).stroke(palette.separator.opacity(0.70), lineWidth: 1))

	          VStack(alignment: .leading, spacing: 12) {
	            Text("Calculation")
	              .font(.headline.weight(.semibold))
	              .foregroundStyle(palette.text)
	            VStack(spacing: 10) {
	              SleepV2SleepNeedFactorRow(
	                palette: palette,
	                systemImage: "target",
	                title: "Sleep goal",
	                detail: "Base target before adjustments",
	                value: targetSleepText,
	                tint: palette.accent
	              )
	              SleepV2SleepNeedFactorRow(
	                palette: palette,
	                systemImage: "figure.run",
	                title: "Recent strain",
	                detail: "No extra recovery time added",
	                value: "+0m",
	                tint: Color(red: 0.94, green: 0.45, blue: 0.30)
	              )
	              SleepV2SleepNeedFactorRow(
	                palette: palette,
	                systemImage: "banknote",
	                title: "Sleep debt",
	                detail: "No repayment needed tonight",
	                value: "+0m",
	                tint: palette.success
	              )
	              SleepV2SleepNeedFactorRow(
	                palette: palette,
	                systemImage: "clock.badge.checkmark",
	                title: "Efficiency buffer",
	                detail: "Covers awake time in bed",
	                value: "+9m",
	                tint: Color(red: 0.42, green: 0.78, blue: 0.86)
	              )
	            }
	          }
	          .padding(20)
	          .background(palette.surface, in: RoundedRectangle(cornerRadius: 24, style: .continuous))
	          .overlay(RoundedRectangle(cornerRadius: 24, style: .continuous).stroke(palette.separator.opacity(0.70), lineWidth: 1))

	          VStack(alignment: .leading, spacing: 10) {
	            HStack {
	              Text("Total")
	                .font(.headline.weight(.semibold))
	                .foregroundStyle(palette.text)
	              Spacer()
	              Text(sleepNeededText)
	                .font(.title2.weight(.semibold))
	                .fontDesign(.rounded)
	                .foregroundStyle(palette.text)
	            }
	            Text("Use this as the time-in-bed target. Your actual sleep score still depends on sleep continuity, wake time, and stage balance.")
	              .font(.subheadline)
	              .foregroundStyle(palette.secondaryText)
	              .fixedSize(horizontal: false, vertical: true)
	          }
	          .padding(20)
	          .background(palette.surfaceHeader.opacity(0.72), in: RoundedRectangle(cornerRadius: 24, style: .continuous))
	        }
	        .padding(.horizontal, 18)
	        .padding(.top, 18)
	        .padding(.bottom, 30)
      }
      .background(palette.background.ignoresSafeArea())
      .navigationTitle("Sleep Needed")
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .topBarLeading) {
          Button {
            dismiss()
          } label: {
            Image(systemName: "xmark")
          }
          .foregroundStyle(palette.text)
        }
      }
      .toolbarBackground(.hidden, for: .navigationBar)
    }
	    .presentationDetents([.large])
	  }

  private var sleepNeededText: String {
    Self.durationText(targetSleepMinutes + 9)
  }

  private var targetSleepText: String {
    Self.durationText(targetSleepMinutes)
  }

  private static func durationText(_ minutes: Int) -> String {
    "\(minutes / 60)h \(minutes % 60)m"
  }
	}

struct SleepV2SleepNeedFactorRow: View {
  let palette: SleepV2Palette
  let systemImage: String
  let title: String
  let detail: String
  let value: String
  let tint: Color

  var body: some View {
    HStack(spacing: 12) {
      Image(systemName: systemImage)
        .font(.subheadline.weight(.semibold))
        .foregroundStyle(tint)
        .frame(width: 34, height: 34)
        .background(tint.opacity(0.12), in: Circle())
      VStack(alignment: .leading, spacing: 3) {
        Text(title)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(palette.text)
        Text(detail)
          .font(.caption.weight(.medium))
          .foregroundStyle(palette.secondaryText)
          .lineLimit(1)
          .minimumScaleFactor(0.72)
      }
      Spacer(minLength: 8)
      Text(value)
        .font(.subheadline.weight(.semibold))
        .fontDesign(.rounded)
        .foregroundStyle(palette.text)
    }
    .padding(12)
    .background(palette.surfaceElevated.opacity(0.48), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
  }
}

struct SleepV2CalculationRow: View {
  let label: String
  let value: String
  var muted = false
  var prominent = false
  let palette: SleepV2Palette

  var body: some View {
    HStack {
      Text(label)
      Spacer()
      Text(value)
        .foregroundStyle(muted ? palette.mutedText : palette.text)
    }
    .font(.system(size: prominent ? 17 : 15, weight: prominent ? .semibold : .regular, design: .rounded))
    .foregroundStyle(palette.text)
    .padding(.horizontal, 16)
    .padding(.vertical, 14)
    .background(prominent ? palette.surfaceHeader : .clear)
    .overlay(alignment: .bottom) {
      Rectangle().fill(palette.separator).frame(height: prominent ? 0 : 1)
    }
  }
}

struct SleepV2AlarmSheet: View {
  var ble: GooseBLEClient
  @Environment(\.dismiss) private var dismiss
  @Environment(\.colorScheme) private var colorScheme
  @State private var alarmTime = Self.defaultWakeTime()
  @State private var alarmType = "Regular"
  @State private var haptic = "Progressive"
  @State private var targetSleepMinutes = 7 * 60 + 30
  @State private var showWheelPicker = false
  @State private var showingHapticOptions = false
  @State private var showingDiagnostics = false
  @State private var pendingConfirmation: SleepAlarmConfirmation?
  private let alarmID = 1

  var body: some View {
    let palette = SleepV2Palette(colorScheme: colorScheme)
    NavigationStack {
      ScrollView {
        VStack(alignment: .leading, spacing: 18) {
          alarmConfigCard(palette: palette)
          hapticCard(palette: palette)
          bandControlsCard(palette: palette)
        }
        .padding(.horizontal, 18)
        .padding(.top, 18)
        .padding(.bottom, 98)
      }
      .background(palette.background.ignoresSafeArea())
      .navigationTitle("Sleep Alarm")
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .topBarLeading) {
          Button {
            dismiss()
          } label: {
            Image(systemName: "xmark")
          }
          .foregroundStyle(palette.text)
        }
      }
      .toolbarBackground(.hidden, for: .navigationBar)
      .safeAreaInset(edge: .bottom) {
        Button {
          pendingConfirmation = alarmType == "No alarm" ? .disable : .set(nextWakeDate, alarmID)
        } label: {
          Text(alarmType == "No alarm" ? "Disable alarm on band" : "Save to band")
            .font(.headline.weight(.semibold))
            .foregroundStyle(.white)
            .frame(maxWidth: .infinity, minHeight: 56)
            .background(Capsule().fill(palette.accent))
        }
        .buttonStyle(.plain)
        .disabled(!ble.canWriteAlarm)
        .opacity(ble.canWriteAlarm ? 1 : 0.52)
        .padding(.horizontal, 18)
        .padding(.vertical, 12)
        .background(.ultraThinMaterial)
      }
    }
    .presentationDetents([.large])
    .alert(item: $pendingConfirmation) { confirmation in
      switch confirmation {
      case .set(let date, let alarmID):
        return Alert(
          title: Text(confirmation.title),
          message: Text(confirmation.message),
          primaryButton: .default(Text("Save to Band")) {
            ble.setWhoopAlarm(at: date, alarmID: alarmID)
          },
          secondaryButton: .cancel()
        )
      case .run(let alarmID):
        return Alert(
          title: Text(confirmation.title),
          message: Text(confirmation.message),
          primaryButton: .default(Text("Run Now")) {
            ble.runWhoopAlarmNow(alarmID: alarmID)
          },
          secondaryButton: .cancel()
        )
      case .disable:
        return Alert(
          title: Text(confirmation.title),
          message: Text(confirmation.message),
          primaryButton: .destructive(Text("Disable on Band")) {
            ble.disableWhoopAlarms()
          },
          secondaryButton: .cancel()
        )
      }
    }
  }

  private func alarmConfigCard(palette: SleepV2Palette) -> some View {
    VStack(alignment: .leading, spacing: 18) {
      HStack(spacing: 12) {
        Image(systemName: "alarm.fill")
          .font(.headline.weight(.semibold))
          .foregroundStyle(palette.accent)
          .frame(width: 38, height: 38)
          .background(palette.accent.opacity(0.12), in: Circle())
        VStack(alignment: .leading, spacing: 2) {
          Text("Alarm config")
            .font(.title3.weight(.semibold))
            .foregroundStyle(palette.text)
          Text(ble.canWriteAlarm ? "Ready to write to band" : "Connect a band to write alarms")
            .font(.caption.weight(.medium))
            .foregroundStyle(palette.secondaryText)
        }
      }

      Button {
        withAnimation(.easeOut(duration: 0.20)) {
          showWheelPicker.toggle()
        }
      } label: {
        HStack(alignment: .center, spacing: 12) {
          VStack(alignment: .leading, spacing: 4) {
            Text("Wake up at")
              .font(.caption.weight(.semibold))
              .foregroundStyle(palette.secondaryText)
            Text(alarmTimeLabel)
              .font(.system(size: 44, weight: .semibold, design: .rounded))
              .foregroundStyle(palette.text)
          }
          Spacer()
          Image(systemName: showWheelPicker ? "chevron.up" : "chevron.down")
            .font(.headline.weight(.semibold))
            .foregroundStyle(palette.mutedText)
            .frame(width: 28, height: 28)
        }
        .padding(16)
        .background(palette.surfaceElevated.opacity(0.50), in: RoundedRectangle(cornerRadius: 20, style: .continuous))
      }
      .buttonStyle(.plain)

      if showWheelPicker {
        DatePicker("Wake-up time", selection: $alarmTime, displayedComponents: .hourAndMinute)
          .datePickerStyle(.wheel)
          .labelsHidden()
          .frame(maxWidth: .infinity)
          .padding(.vertical, 2)
          .transition(.opacity.combined(with: .move(edge: .top)))
      }

      VStack(alignment: .leading, spacing: 10) {
        Text("Alarm mode")
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(palette.text)
        Picker("Alarm mode", selection: $alarmType) {
          Text("Smart").tag("Smart alarm")
          Text("Regular").tag("Regular")
          Text("Needed").tag("Sleep needed")
          Text("Off").tag("No alarm")
        }
        .pickerStyle(.segmented)
        SleepV2AlarmModeHelp(
          palette: palette,
          systemImage: alarmModeIcon,
          title: alarmModeTitle,
          detail: alarmModeDetail,
          tint: alarmModeTint
        )
      }

      Stepper(value: $targetSleepMinutes, in: 5 * 60...10 * 60, step: 15) {
        HStack(alignment: .firstTextBaseline) {
          VStack(alignment: .leading, spacing: 3) {
            Text("Target amount")
              .font(.subheadline.weight(.semibold))
              .foregroundStyle(palette.text)
            Text("Used by Needed mode and sleep-needed planning.")
              .font(.caption.weight(.medium))
              .foregroundStyle(palette.secondaryText)
          }
          Spacer(minLength: 10)
          Text(targetSleepText)
            .font(.subheadline.weight(.semibold))
            .fontDesign(.rounded)
            .foregroundStyle(palette.text)
        }
      }
      .padding(14)
      .background(palette.surfaceElevated.opacity(0.50), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
    }
    .padding(20)
    .background(palette.surface, in: RoundedRectangle(cornerRadius: 28, style: .continuous))
    .overlay(RoundedRectangle(cornerRadius: 28, style: .continuous).stroke(palette.separator.opacity(0.70), lineWidth: 1))
  }

  private func hapticCard(palette: SleepV2Palette) -> some View {
    DisclosureGroup(isExpanded: $showingHapticOptions) {
      VStack(alignment: .leading, spacing: 10) {
        Text("Choose the vibration pattern Goose should use for this alarm profile.")
          .font(.footnote)
          .foregroundStyle(palette.secondaryText)
          .fixedSize(horizontal: false, vertical: true)
        VStack(spacing: 8) {
          ForEach(["Progressive", "Gentle", "Medium", "Intense"], id: \.self) { option in
            SleepV2AlarmOptionRow(
              palette: palette,
              title: option,
              selected: haptic == option
            ) {
              haptic = option
            }
          }
        }
      }
      .padding(.top, 12)
    } label: {
      HStack(spacing: 12) {
        Image(systemName: "waveform")
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(palette.accent)
          .frame(width: 32, height: 32)
          .background(palette.accent.opacity(0.12), in: Circle())
        VStack(alignment: .leading, spacing: 2) {
          Text("Haptic")
            .font(.headline.weight(.semibold))
            .foregroundStyle(palette.text)
          Text(haptic)
            .font(.caption.weight(.medium))
            .foregroundStyle(palette.secondaryText)
        }
      }
    }
    .padding(20)
    .background(palette.surface, in: RoundedRectangle(cornerRadius: 24, style: .continuous))
    .overlay(RoundedRectangle(cornerRadius: 24, style: .continuous).stroke(palette.separator.opacity(0.70), lineWidth: 1))
  }

  private func bandControlsCard(palette: SleepV2Palette) -> some View {
    VStack(alignment: .leading, spacing: 16) {
      VStack(alignment: .leading, spacing: 4) {
        Text("Band controls")
          .font(.headline.weight(.semibold))
          .foregroundStyle(palette.text)
        Text("Test the vibration or clear the alarm from the connected band.")
          .font(.footnote)
          .foregroundStyle(palette.secondaryText)
          .fixedSize(horizontal: false, vertical: true)
      }

      HStack(spacing: 10) {
        SleepV2AlarmControlButton(
          palette: palette,
          title: "Test haptic",
          detail: "Run now",
          systemImage: "waveform",
          destructive: false
        ) {
          pendingConfirmation = .run(alarmID)
        }
        .disabled(!ble.canWriteAlarm)

        SleepV2AlarmControlButton(
          palette: palette,
          title: "Turn off",
          detail: "Disable on band",
          systemImage: "bell.slash",
          destructive: true
        ) {
          pendingConfirmation = .disable
        }
        .disabled(!ble.canWriteAlarm)
      }
      .opacity(ble.canWriteAlarm ? 1 : 0.52)

      DisclosureGroup(isExpanded: $showingDiagnostics) {
        SleepV2AlarmDiagnostics(ble: ble, palette: palette)
          .padding(.top, 10)
      } label: {
        Label("Band write diagnostics", systemImage: "stethoscope")
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(palette.text)
      }
    }
    .padding(20)
    .background(palette.surface, in: RoundedRectangle(cornerRadius: 24, style: .continuous))
    .overlay(RoundedRectangle(cornerRadius: 24, style: .continuous).stroke(palette.separator.opacity(0.70), lineWidth: 1))
  }

  private var alarmTimeLabel: String {
    let formatter = DateFormatter()
    formatter.dateFormat = "HH:mm"
    return formatter.string(from: alarmTime)
  }

  private var targetSleepText: String {
    Self.durationText(targetSleepMinutes)
  }

  private var alarmModeTitle: String {
    switch alarmType {
    case "Smart alarm": return "Smart wake"
    case "Sleep needed": return "Needed"
    case "No alarm": return "Off"
    default: return "Regular"
    }
  }

  private var alarmModeDetail: String {
    switch alarmType {
    case "Smart alarm":
      return "Uses your selected time as the latest wake target. Sleep-stage wake-window logic can be layered on when the band data is available."
    case "Sleep needed":
      return "Uses your target amount to plan tonight's sleep need, then saves the selected wake alarm to the band."
    case "No alarm":
      return "Disables the alarm on the connected band."
    default:
      return "Goes off at the wake time you choose."
    }
  }

  private var alarmModeIcon: String {
    switch alarmType {
    case "Smart alarm": return "sparkles"
    case "Sleep needed": return "moon.zzz.fill"
    case "No alarm": return "bell.slash.fill"
    default: return "alarm.fill"
    }
  }

  private var alarmModeTint: Color {
    switch alarmType {
    case "No alarm": return Color(red: 1.0, green: 0.50, blue: 0.28)
    case "Sleep needed": return Color(red: 0.42, green: 0.78, blue: 0.86)
    default: return Color(red: 0.48, green: 0.49, blue: 1.0)
    }
  }

  private var nextWakeDate: Date {
    let calendar = Calendar.current
    let components = calendar.dateComponents([.hour, .minute], from: alarmTime)
    let now = Date()
    guard let next = calendar.nextDate(
      after: now,
      matching: components,
      matchingPolicy: .nextTime,
      repeatedTimePolicy: .first,
      direction: .forward
    ) else {
      return now.addingTimeInterval(60)
    }
    return next <= now.addingTimeInterval(10)
      ? (calendar.date(byAdding: .day, value: 1, to: next) ?? now.addingTimeInterval(60))
      : next
  }

	  private static func defaultWakeTime() -> Date {
	    Calendar.current.date(bySettingHour: 7, minute: 0, second: 0, of: Date()) ?? Date()
	  }

	  private static func durationText(_ minutes: Int) -> String {
    "\(minutes / 60)h \(minutes % 60)m"
  }
	}

struct SleepV2AlarmModeHelp: View {
  let palette: SleepV2Palette
  let systemImage: String
  let title: String
  let detail: String
  let tint: Color

  var body: some View {
    HStack(alignment: .top, spacing: 10) {
      Image(systemName: systemImage)
        .font(.caption.weight(.semibold))
        .foregroundStyle(tint)
        .frame(width: 28, height: 28)
        .background(tint.opacity(0.12), in: Circle())
      VStack(alignment: .leading, spacing: 3) {
        Text(title)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(palette.text)
        Text(detail)
          .font(.caption.weight(.medium))
          .foregroundStyle(palette.secondaryText)
          .fixedSize(horizontal: false, vertical: true)
      }
    }
    .padding(12)
    .frame(maxWidth: .infinity, alignment: .leading)
    .background(palette.surfaceElevated.opacity(0.46), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
  }
}

struct SleepV2AlarmControlButton: View {
  let palette: SleepV2Palette
  let title: String
  let detail: String
  let systemImage: String
  let destructive: Bool
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      VStack(alignment: .leading, spacing: 8) {
        Image(systemName: systemImage)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(tint)
          .frame(width: 30, height: 30)
          .background(tint.opacity(0.12), in: Circle())
        VStack(alignment: .leading, spacing: 2) {
          Text(title)
            .font(.subheadline.weight(.semibold))
            .foregroundStyle(palette.text)
          Text(detail)
            .font(.caption.weight(.medium))
            .foregroundStyle(palette.secondaryText)
        }
      }
      .frame(maxWidth: .infinity, alignment: .leading)
      .padding(14)
      .background(palette.surfaceElevated.opacity(0.50), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
    }
    .buttonStyle(.plain)
  }

  private var tint: Color {
    destructive ? Color(red: 1.0, green: 0.50, blue: 0.28) : palette.accent
  }
}

struct SleepV2AlarmOptionRow: View {
  let palette: SleepV2Palette
  let title: String
  let selected: Bool
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      HStack {
        Text(title)
          .font(.subheadline.weight(.semibold))
          .foregroundStyle(palette.text)
        Spacer()
        Image(systemName: selected ? "checkmark.circle.fill" : "circle")
          .font(.headline.weight(.semibold))
          .foregroundStyle(selected ? palette.accent : palette.mutedText.opacity(0.72))
      }
      .padding(.horizontal, 14)
      .padding(.vertical, 12)
      .background(
        palette.surfaceElevated.opacity(selected ? 0.74 : 0.42),
        in: RoundedRectangle(cornerRadius: 16, style: .continuous)
      )
    }
    .buttonStyle(.plain)
  }
}

struct SleepV2AlarmBackdrop: View {
  var body: some View {
    Canvas { context, size in
      let rect = CGRect(origin: .zero, size: size)
      context.fill(
        Path(rect),
        with: .linearGradient(
          Gradient(colors: [
            Color(red: 0.11, green: 0.13, blue: 0.22),
            Color(red: 0.27, green: 0.28, blue: 0.38),
            Color(red: 0.19, green: 0.22, blue: 0.33),
          ]),
          startPoint: .zero,
          endPoint: CGPoint(x: 0, y: size.height)
        )
      )
      for index in 0..<48 {
        let x = CGFloat((index * 53 + 19) % max(1, Int(size.width)))
        let y = size.height * 0.12 + CGFloat((index * 37) % 250)
        let radius = index % 7 == 0 ? CGFloat(1.5) : CGFloat(0.9)
        context.fill(Path(ellipseIn: CGRect(x: x, y: y, width: radius * 2, height: radius * 2)), with: .color(.white.opacity(0.22)))
      }
      context.fill(
        Path(ellipseIn: CGRect(x: size.width * -0.10, y: size.height * 0.02, width: size.width * 1.2, height: size.height * 0.54)),
        with: .radialGradient(
          Gradient(colors: [.white.opacity(0.12), .white.opacity(0.02), .clear]),
          center: CGPoint(x: size.width * 0.50, y: size.height * 0.20),
          startRadius: 10,
          endRadius: size.width * 0.58
        )
      )
    }
  }
}

struct SleepV2AlarmWakeIcon: View {
  var body: some View {
    ZStack {
      Image(systemName: "arrow.up")
        .font(.system(size: 31, weight: .bold))
        .offset(y: -9)
      RoundedRectangle(cornerRadius: 18, style: .continuous)
        .stroke(.white, lineWidth: 2.4)
        .frame(width: 34, height: 16)
        .offset(y: 10)
      Rectangle()
        .fill(.white)
        .frame(width: 46, height: 3)
        .offset(y: 18)
    }
    .foregroundStyle(.white)
    .frame(height: 42)
  }
}

struct SleepV2AlarmSectionLabel: View {
  let text: String

  init(_ text: String) {
    self.text = text
  }

  var body: some View {
    Text(text)
      .font(.system(size: 18, weight: .heavy))
      .foregroundStyle(.white.opacity(0.50))
  }
}

struct SleepV2AlarmTileBackground: View {
  let selected: Bool

  var body: some View {
    RoundedRectangle(cornerRadius: 16, style: .continuous)
      .fill(.white.opacity(0.055))
      .overlay(
        RoundedRectangle(cornerRadius: 16, style: .continuous)
          .stroke(.white.opacity(selected ? 0.96 : 0.18), lineWidth: selected ? 1.45 : 1)
      )
  }
}

struct SleepV2AlarmDiagnostics: View {
  var ble: GooseBLEClient
  let palette: SleepV2Palette

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      Text("Diagnostics")
        .font(.system(size: 14, weight: .heavy))
        .foregroundStyle(palette.secondaryText)
      SleepV2AlarmDiagnosticRow(label: "Write support", value: ble.alarmWriteSupportSummary, palette: palette)
      SleepV2AlarmDiagnosticRow(label: "Last response", value: ble.lastAlarmResponseSummary, palette: palette)
      SleepV2AlarmDiagnosticRow(label: "Last event", value: ble.lastAlarmEventSummary, palette: palette)
      if !ble.lastAlarmCommandFrameHex.isEmpty {
        SleepV2AlarmDiagnosticRow(label: "Last frame", value: String(ble.lastAlarmCommandFrameHex.prefix(38)), palette: palette)
      }
      if !ble.lastAlarmResponsePayloadHex.isEmpty {
        SleepV2AlarmDiagnosticRow(label: "Response hex", value: String(ble.lastAlarmResponsePayloadHex.prefix(38)), palette: palette)
      }
      if !ble.lastAlarmEventPayloadHex.isEmpty {
        SleepV2AlarmDiagnosticRow(label: "Event hex", value: String(ble.lastAlarmEventPayloadHex.prefix(38)), palette: palette)
      }
    }
    .padding(14)
    .background(palette.surfaceElevated.opacity(0.48), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
  }
}

struct SleepV2AlarmDiagnosticRow: View {
  let label: String
  let value: String
  let palette: SleepV2Palette

  var body: some View {
    HStack(alignment: .top, spacing: 12) {
      Text(label)
        .font(.caption.weight(.bold))
        .foregroundStyle(palette.secondaryText)
        .frame(width: 92, alignment: .leading)
      Text(value)
        .font(.caption.weight(.semibold))
        .foregroundStyle(palette.text)
        .lineLimit(2)
        .minimumScaleFactor(0.75)
      Spacer(minLength: 0)
    }
  }
}
