import SwiftUI

struct MoreRawExportView: View {
  @ObservedObject var store: MoreDataStore

  var body: some View {
    List {
      Section("Window") {
        MoreInfoRow(title: "Export Window", value: store.rawExportWindowSummary(), systemImage: "calendar", status: store.rawExportWindowIssueSummary() == nil ? .ready : .blocked)
        MoreInfoRow(title: "Window Issue", value: store.rawExportWindowIssueSummary() ?? "Window is valid", systemImage: "checkmark.seal", status: store.rawExportWindowIssueSummary() == nil ? .ready : .blocked)
        MoreInfoRow(title: "Scope", value: store.rawExportScopeSummary(), systemImage: "square.stack.3d.up", status: store.selectedRawFamilies.isEmpty ? .blocked : .ready)
        TextField("Start", text: $store.rawExportStart)
          .textInputAutocapitalization(.never)
          .keyboardType(.numbersAndPunctuation)
        TextField("End", text: $store.rawExportEnd)
          .textInputAutocapitalization(.never)
          .keyboardType(.numbersAndPunctuation)
      }

      Section("Filters") {
        TextField("Capture sessions", text: $store.rawCaptureSessions)
          .textInputAutocapitalization(.never)
        TextField("Packet types", text: $store.rawPacketTypes)
          .textInputAutocapitalization(.never)
        TextField("Sensor signals", text: $store.rawSensorSignals)
          .textInputAutocapitalization(.never)
        TextField("Metric families", text: $store.rawMetricFamilies)
          .textInputAutocapitalization(.never)
        TextField("Algorithm ids", text: $store.rawAlgorithmIDs)
          .textInputAutocapitalization(.never)
        TextField("Algorithm versions", text: $store.rawAlgorithmVersions)
          .textInputAutocapitalization(.never)
        Toggle("Include Raw Bytes", isOn: $store.includeRawBytes)
      }

      Section("Data Families") {
        ForEach(MoreDataStore.rawFamilies, id: \.self) { family in
          Toggle(family, isOn: Binding(
            get: { store.selectedRawFamilies.contains(family) },
            set: { store.setRawFamily(family, enabled: $0) }
          ))
        }
      }

      Section("Recent Capture Shortcuts") {
        ForEach(store.recentCaptureSessions, id: \.self) { session in
          Button {
            store.rawCaptureSessions = session.components(separatedBy: "|").first?.trimmingCharacters(in: .whitespacesAndNewlines) ?? store.rawCaptureSessions
          } label: {
            Label(session, systemImage: "clock.arrow.circlepath")
          }
        }
      }

      Section("Export") {
        Button {
          store.saveLocalDataBundle()
        } label: {
          Label("Save Local Data File", systemImage: "externaldrive.badge.plus")
        }
        .disabled(store.localExportInProgress)

        Button {
          store.runRawExport()
        } label: {
          Label("Export", systemImage: "square.and.arrow.up")
        }
        .disabled(!store.canRunRawExport || store.rawExportInProgress)

        Button {
          store.validateExportArtifacts()
        } label: {
          Label("Validate Export And Lint", systemImage: "checkmark.seal")
        }
        .disabled(store.rawBundlePath == "No bundle" || store.rawExportInProgress)

        if store.rawExportInProgress {
          ProgressView("Saving export")
        }

        if store.localExportInProgress {
          ProgressView("Saving local data file")
        }

        if let localExportURL = store.localExportURL {
          ShareLink(item: localExportURL) {
            Label("AirDrop Local Data File", systemImage: "square.and.arrow.up")
          }
        }

        if let localExportManifestURL = store.localExportManifestURL {
          ShareLink(item: localExportManifestURL) {
            Label("AirDrop Export Manifest", systemImage: "list.bullet.rectangle")
          }
        }

        if let rawZipURL = store.rawZipURL {
          ShareLink(item: rawZipURL) {
            Label("AirDrop Latest Zip", systemImage: "square.and.arrow.up")
          }
        }

        if let rawValidationManifestURL = store.rawValidationManifestURL {
          ShareLink(item: rawValidationManifestURL) {
            Label("AirDrop Validation Manifest", systemImage: "list.bullet.rectangle")
          }
        }

        if let rawValidationReviewURL = store.rawValidationReviewURL {
          ShareLink(item: rawValidationReviewURL) {
            Label("AirDrop Validation Review", systemImage: "checklist")
          }
        }

        if let rawValidationRunbookURL = store.rawValidationRunbookURL {
          ShareLink(item: rawValidationRunbookURL) {
            Label("AirDrop Validation Runbook", systemImage: "doc.text")
          }
        }

        MoreInfoRow(title: "Status", value: store.rawExportStatus, systemImage: "shippingbox", status: store.canRunRawExport ? .pending : .unavailable)
        MoreInfoRow(title: "Local File", value: store.localExportStatus, systemImage: "doc", status: store.localExportURL == nil ? .pending : .ready)
        MoreInfoRow(title: "Bundle Path", value: store.rawBundlePath, systemImage: "folder", status: store.rawBundlePath == "No bundle" ? .pending : .ready)
        MoreInfoRow(title: "Zip Path", value: store.rawZipPath, systemImage: "doc.zipper", status: store.rawZipPath == "No zip" ? .pending : .ready)
        MoreInfoRow(title: "Row Counts", value: store.rawRowCounts, systemImage: "number", status: .pending)
        MoreInfoRow(title: "Validation Manifest", value: store.rawValidationManifestStatus, systemImage: "list.bullet.rectangle", status: store.rawValidationManifestURL == nil ? .pending : .ready)
        MoreInfoRow(title: "Validation Review", value: store.rawValidationReviewStatus, systemImage: "checklist", status: store.validationStatusKind(store.rawValidationReviewStatus))
        MoreInfoRow(title: "Validation Runbook", value: store.rawValidationRunbookStatus, systemImage: "doc.text", status: store.rawValidationRunbookURL == nil ? .pending : .ready)
        MoreInfoRow(title: "Bundle Validation", value: store.rawBundleValidation, systemImage: "checkmark.seal", status: store.validationStatusKind(store.rawBundleValidation))
        MoreInfoRow(title: "Zip Validation", value: store.rawZipValidation, systemImage: "checkmark.seal", status: store.validationStatusKind(store.rawZipValidation))
        MoreInfoRow(title: "Privacy Lint", value: store.privacyLintStatus, systemImage: "hand.raised", status: store.validationStatusKind(store.privacyLintStatus))
        MoreInfoRow(title: "Sanitized Privacy", value: store.sanitizedPrivacyStatus, systemImage: "sparkles.rectangle.stack", status: .pending)
      }
    }
    .gooseListBackground()
    .navigationTitle("Raw Export")
    .onAppear {
      store.refreshRecentCaptureSessions()
    }
  }
}

struct MoreAlgorithmsView: View {
  @ObservedObject var store: MoreDataStore
  var healthStore: HealthDataStore
  let openHealthAlgorithms: () -> Void

  var body: some View {
    List {
      Section("Preferences") {
        ForEach(healthStore.algorithmFamilies, id: \.self) { family in
          let algorithms = healthStore.algorithms(for: family)
          if algorithms.isEmpty {
            MoreInfoRow(title: family.uppercased(), value: "No algorithm registered", systemImage: "function", status: .unavailable)
          } else {
            Picker(family.uppercased(), selection: Binding(
              get: { healthStore.selectedAlgorithmByFamily[family] ?? algorithms[0].id },
              set: { id in
                healthStore.selectAlgorithm(id, for: family)
                if let selected = algorithms.first(where: { $0.id == id }) {
                  store.persistAlgorithmPreference(family: family, algorithm: selected)
                }
              }
            )) {
              ForEach(algorithms) { algorithm in
                Text(algorithm.displayName).tag(algorithm.id)
              }
            }
          }
        }

        Button {
          store.applyRecommendedAlgorithmDefaults(healthStore: healthStore)
        } label: {
          Label("Defaults", systemImage: "arrow.counterclockwise.circle")
        }

        MoreInfoRow(title: "Preference Status", value: store.algorithmPreferenceStatus, systemImage: "gearshape.2", status: .pending)
      }

      Section("Reference Benchmarks") {
        ForEach(healthStore.referenceDefinitions) { definition in
          MoreInfoRow(title: definition.family.uppercased(), value: "\(definition.displayName) | \(definition.status) | \(definition.provider)", systemImage: "scalemass", status: .ready)
        }
      }

      Section("Metric Context") {
        Button {
          openHealthAlgorithms()
        } label: {
          Label("Open Health > Algorithms", systemImage: "heart.text.square")
        }
        MoreInfoRow(title: "Boundary", value: "Operational selection lives here; metric explanations stay in Health.", systemImage: "rectangle.split.2x1", status: .ready)
      }
    }
    .gooseListBackground()
    .navigationTitle("Algorithms")
    .onAppear {
      healthStore.loadBridgeCatalogsIfNeeded()
    }
  }
}

