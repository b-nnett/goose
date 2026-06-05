use std::{
    fs,
    path::{Path, PathBuf},
};

const FORBIDDEN_HEALTHKIT_TOKENS: &[(&str, &str)] = &[
    (
        ".stepCount",
        "steps must come from WHOOP packets or validated local estimates",
    ),
    (
        ".activeEnergyBurned",
        "calories must come from WHOOP packet-derived local estimates",
    ),
    (
        ".basalEnergyBurned",
        "calories must come from WHOOP packet-derived local estimates",
    ),
    (
        ".respiratoryRate",
        "respiratory rate must come from decoded WHOOP sensor evidence",
    ),
    (
        ".oxygenSaturation",
        "oxygen saturation must come from decoded WHOOP sensor evidence",
    ),
    (
        ".bodyTemperature",
        "temperature must come from decoded WHOOP sensor evidence",
    ),
    (
        ".basalBodyTemperature",
        "temperature must come from decoded WHOOP sensor evidence",
    ),
    (
        ".heartRate",
        "heart rate and RHR must come from WHOOP packets",
    ),
    (".restingHeartRate", "RHR must come from WHOOP packets"),
    (
        ".heartRateVariabilitySDNN",
        "HRV must come from true WHOOP beat-interval evidence",
    ),
    (
        ".sleepAnalysis",
        "sleep widgets must not be imported from HealthKit",
    ),
    (
        ".appleExerciseTime",
        "activity must not be imported from HealthKit",
    ),
    (
        ".distanceWalkingRunning",
        "activity must not be imported from HealthKit",
    ),
    (
        "HKObjectType.categoryType",
        "HealthKit category imports are outside the profile-only boundary",
    ),
    (
        "HKObjectType.workoutType",
        "HealthKit workouts are outside the profile-only boundary",
    ),
    (
        "HKWorkout",
        "HealthKit workouts are outside the profile-only boundary",
    ),
    (
        "HKStatistics",
        "HealthKit aggregate queries are outside the profile-only boundary",
    ),
];

#[test]
fn ios_healthkit_read_boundary_is_weight_only() {
    let swift_root = swift_source_root();
    let swift_files = swift_files_under(&swift_root);
    assert!(
        !swift_files.is_empty(),
        "expected Swift source files under {}",
        swift_root.display()
    );

    let mut saw_body_mass_request = false;
    let mut violations = Vec::new();

    for path in swift_files {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read Swift source {}: {error}", path.display()));
        let relative = path.strip_prefix(&swift_root).unwrap_or(&path);
        if !source_uses_healthkit_api(&source) {
            continue;
        }

        if source.contains(".bodyMass") {
            saw_body_mass_request = true;
        }

        for (token, reason) in FORBIDDEN_HEALTHKIT_TOKENS {
            if source.contains(token) {
                violations.push(format!(
                    "{} contains forbidden HealthKit token `{}`: {}",
                    relative.display(),
                    token,
                    reason
                ));
            }
        }

        for identifier in healthkit_quantity_identifiers(&source) {
            if identifier != "bodyMass" {
                violations.push(format!(
                    "{} requests HealthKit quantity `{}`; only `bodyMass` is allowed for profile autofill",
                    relative.display(),
                    identifier
                ));
            }
        }
    }

    assert!(
        saw_body_mass_request,
        "expected the iOS HealthKit profile importer to request bodyMass"
    );
    assert!(
        violations.is_empty(),
        "iOS HealthKit boundary violated:\n{}",
        violations.join("\n")
    );
}

#[test]
fn ios_health_metric_display_filters_forbidden_metric_sources() {
    let swift_root = swift_source_root();
    let utilities = swift_source(&swift_root, "HealthDataStore+Utilities.swift");
    let activity_snapshots = swift_source(&swift_root, "HealthDataStore+ActivitySnapshots.swift");
    let snapshots = swift_source(&swift_root, "HealthDataStore+Snapshots.swift");

    for token in [
        "localHealthMetricRowIsDisplaySafe",
        "localHealthMetricValueContainsForbiddenSourceMarker",
        "healthkit",
        "health_connect",
        "apple_health",
        "platform_import",
        "official_whoop",
        "whoop_app",
        "whoop_backend",
        "whoop_label",
    ] {
        assert!(
            utilities.contains(token),
            "Swift display boundary must scan for forbidden source marker `{token}`"
        );
    }

    assert!(
        activity_snapshots.contains(".filter { Self.localHealthMetricRowIsDisplaySafe($0) }"),
        "daily recovery metrics must be filtered before Health UI selection"
    );
    assert!(
        snapshots.contains(".filter { Self.localHealthMetricRowIsDisplaySafe($0) }"),
        "daily activity metrics must be filtered before Health UI selection"
    );
    assert!(
        activity_snapshots.contains(".filter { localHealthMetricRowIsDisplaySafe($0) }"),
        "preferred stored metric selection must defensively filter caller-provided rows"
    );
    assert!(
        activity_snapshots.contains("guard localHealthMetricRowIsDisplaySafe(metric) else"),
        "stored metric trend rows must skip unsafe metric provenance"
    );
}

fn swift_source_root() -> PathBuf {
    // The Swift app lives at `<repo-root>/GooseSwift`. `CARGO_MANIFEST_DIR`
    // points at `<repo-root>/Rust/core`, so two `parent()` hops reach the repo
    // root and the Swift sources sit directly under it.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core crate has parent")
        .parent()
        .expect("goose project has parent")
        .join("GooseSwift")
}

fn swift_source(root: &Path, filename: &str) -> String {
    let path = root.join(filename);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read Swift source {}: {error}", path.display()))
}

fn swift_files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_swift_files(root, &mut files);
    files.sort();
    files
}

fn collect_swift_files(path: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_swift_files(&path, files);
        } else if path.extension().and_then(|value| value.to_str()) == Some("swift") {
            files.push(path);
        }
    }
}

fn source_uses_healthkit_api(source: &str) -> bool {
    source.contains("import HealthKit")
        || source.contains("HKHealthStore")
        || source.contains("HKObjectType")
        || source.contains("HKSample")
        || source.contains("HKWorkout")
        || source.contains("HKStatistics")
        || source.contains("quantityType(forIdentifier:")
}

fn healthkit_quantity_identifiers(source: &str) -> Vec<String> {
    let marker = "quantityType(forIdentifier: .";
    let mut identifiers = Vec::new();
    let mut remainder = source;
    while let Some(index) = remainder.find(marker) {
        let after_marker = &remainder[index + marker.len()..];
        let identifier = after_marker
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect::<String>();
        if !identifier.is_empty() {
            identifiers.push(identifier);
        }
        remainder = after_marker;
    }
    identifiers
}
