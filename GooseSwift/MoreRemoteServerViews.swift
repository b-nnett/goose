import Foundation
import SwiftUI
import UIKit

@MainActor
final class MoreRemoteServerViewModel: ObservableObject {
  @Published var serverURL: String
  @Published var bearerToken: String
  @Published var uploadEnabled: Bool
  @Published var urlValidationError: String?
  @Published var saveSuccess: Bool = false

  init() {
    serverURL = UserDefaults.standard.string(forKey: RemoteServerStorage.serverURL) ?? ""
    bearerToken = (try? RemoteServerKeychain.loadToken()) ?? ""
    uploadEnabled = UserDefaults.standard.bool(forKey: RemoteServerStorage.uploadEnabled)
  }

  func save() {
    guard RemoteServerURLValidator.validate(serverURL) else {
      urlValidationError = "Invalid URL. Use https://hostname for public servers, or http:// for local IPs and .local hostnames."
      return
    }
    urlValidationError = nil
    UserDefaults.standard.set(serverURL, forKey: RemoteServerStorage.serverURL)
    UserDefaults.standard.set(uploadEnabled, forKey: RemoteServerStorage.uploadEnabled)
    try? RemoteServerKeychain.saveToken(bearerToken)
    saveSuccess = true
  }
}

struct MoreRemoteServerView: View {
  @StateObject private var vm = MoreRemoteServerViewModel()
  @Environment(GooseAppModel.self) private var model

  private static let relativeDateFormatter: RelativeDateTimeFormatter = {
    let f = RelativeDateTimeFormatter()
    f.unitsStyle = .short
    return f
  }()

  private var uploadIsActive: Bool {
    vm.uploadEnabled && !vm.serverURL.isEmpty
  }

  var body: some View {
    Form {
      Section("Server") {
        TextField("https://hostname:8770", text: $vm.serverURL)
          .keyboardType(.URL)
          .autocorrectionDisabled()
          .textInputAutocapitalization(.never)
        if let error = vm.urlValidationError {
          Text(error)
            .font(.caption)
            .foregroundStyle(.red)
        }
      }

      Section("Authentication") {
        SecureField("Bearer token (API key)", text: $vm.bearerToken)
          .autocorrectionDisabled()
          .textInputAutocapitalization(.never)
      }

      Section("Upload") {
        Toggle("Enable Upload", isOn: $vm.uploadEnabled)
      }

      if uploadIsActive {
        Section("Status") {
          // Row 1: Server reachability
          Label {
            switch model.serverReachable {
            case .none:
              Text("Checking...").foregroundStyle(.secondary)
            case .some(true):
              Text("Server reachable").foregroundStyle(.green)
            case .some(false):
              Text("Server unreachable").foregroundStyle(.red)
            }
          } icon: {
            switch model.serverReachable {
            case .none:
              ProgressView().scaleEffect(0.7)
            case .some(true):
              Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
            case .some(false):
              Image(systemName: "xmark.circle.fill").foregroundStyle(.red)
            }
          }

          // Row 2: Last sync + ACK count + manual trigger
          LabeledContent("Last sync") {
            HStack(spacing: 8) {
              if let lastUpload = model.lastUploadAt {
                VStack(alignment: .trailing, spacing: 1) {
                  Text(Self.relativeDateFormatter.localizedString(for: lastUpload, relativeTo: Date()))
                    .foregroundStyle(.secondary)
                  if let count = model.lastSyncedCount {
                    Text("\(count) records acked")
                      .font(.caption2)
                      .foregroundStyle(.green)
                  }
                }
              } else {
                Text("Never").foregroundStyle(.secondary)
              }
              Button("Now") {
                model.triggerManualUpload()
              }
              .buttonStyle(.bordered)
              .controlSize(.mini)
            }
          }

          // Row 3: Pending batch count
          LabeledContent("Pending batches") {
            Text("\(model.pendingBatchCount)")
              .foregroundStyle(model.pendingBatchCount > 0 ? .orange : .secondary)
          }
        }
      }

      Section {
        Button("Save") {
          vm.save()
          if vm.urlValidationError == nil {
            model.checkServerHealth()
          }
        }
        .frame(maxWidth: .infinity)
        .foregroundStyle(.white)
      }
    }
    .navigationTitle("Remote Server")
    .navigationBarTitleDisplayMode(.inline)
    .listStyle(.insetGrouped)
    .gooseListBackground()
    .alert("Settings saved", isPresented: $vm.saveSuccess) {
      Button("OK") {}
    }
  }
}

// MARK: - Previews

#Preview("Status — Checking") {
  NavigationStack {
    MoreRemoteServerView()
  }
  .environment({
    let m = GooseAppModel()
    m.serverReachable = nil
    m.lastUploadAt = nil
    m.pendingBatchCount = 0
    return m
  }())
}

#Preview("Status — Reachable") {
  NavigationStack {
    MoreRemoteServerView()
  }
  .environment({
    let m = GooseAppModel()
    m.serverReachable = true
    m.lastUploadAt = Date().addingTimeInterval(-120)
    m.pendingBatchCount = 0
    return m
  }())
}

#Preview("Status — Unreachable") {
  NavigationStack {
    MoreRemoteServerView()
  }
  .environment({
    let m = GooseAppModel()
    m.serverReachable = false
    m.lastUploadAt = nil
    m.pendingBatchCount = 2
    return m
  }())
}
