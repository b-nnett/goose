use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::{GooseError, GooseResult};

pub const PRIVACY_LINT_REPORT_SCHEMA: &str = "goose.privacy-lint-report.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyLintReport {
    pub schema: String,
    pub generated_by: String,
    pub input_path: String,
    pub pass: bool,
    #[serde(default)]
    pub input_valid: bool,
    #[serde(default)]
    pub files_readable: bool,
    #[serde(default)]
    pub scan_coverage_ready: bool,
    #[serde(default)]
    pub auth_tokens_clear: bool,
    #[serde(default)]
    pub debug_tokens_clear: bool,
    #[serde(default)]
    pub private_api_clear: bool,
    #[serde(default)]
    pub direct_identifiers_clear: bool,
    #[serde(default)]
    pub privacy_ready: bool,
    pub files: Vec<PrivacyLintFileReport>,
    pub issues: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<PrivacyLintNextAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyLintFileReport {
    pub path: String,
    pub format: String,
    pub byte_len: u64,
    pub scanned: bool,
    pub skipped: bool,
    pub findings: Vec<PrivacyFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct PrivacyLintNextAction {
    pub scope: String,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivacyFinding {
    pub rule: String,
    pub message: String,
    pub line: Option<usize>,
    pub snippet: String,
}

pub fn lint_privacy_path(path: &Path) -> GooseResult<PrivacyLintReport> {
    if !path.exists() {
        return Err(GooseError::message(format!(
            "input path does not exist: {}",
            path.display()
        )));
    }

    let mut files = Vec::new();
    if path.is_dir() {
        let mut paths = Vec::new();
        collect_files(path, &mut paths)?;
        paths.sort();
        for file_path in paths {
            let relative = relative_path(path, &file_path);
            lint_file_path(&file_path, &slash_path(&relative), &mut files)?;
        }
    } else {
        lint_file_path(path, &file_name(path), &mut files)?;
    }

    let issues = files
        .iter()
        .flat_map(|file| {
            file.findings.iter().map(|finding| {
                if let Some(line) = finding.line {
                    format!("{}:{line}: {}", file.path, finding.message)
                } else {
                    format!("{}: {}", file.path, finding.message)
                }
            })
        })
        .collect::<Vec<_>>();
    let next_actions = privacy_lint_next_actions(&files);
    let input_valid = true;
    let files_readable = true;
    let scan_coverage_ready = files.iter().all(|file| file.scanned || file.skipped);
    let auth_tokens_clear = privacy_rules_clear(
        &files,
        &[
            "authorization_header",
            "bearer_token",
            "jwt_token",
            "json_token",
        ],
    );
    let debug_tokens_clear = privacy_rules_clear(&files, &["debug_query_token"]);
    let private_api_clear = privacy_rules_clear(&files, &["private_whoop_api_material"]);
    let direct_identifiers_clear =
        privacy_rules_clear(&files, &["direct_identifier", "email", "mac_address"]);
    let privacy_ready = input_valid
        && files_readable
        && scan_coverage_ready
        && auth_tokens_clear
        && debug_tokens_clear
        && private_api_clear
        && direct_identifiers_clear
        && issues.is_empty();

    Ok(PrivacyLintReport {
        schema: PRIVACY_LINT_REPORT_SCHEMA.to_string(),
        generated_by: "goose-privacy-lint".to_string(),
        input_path: path.display().to_string(),
        pass: privacy_ready,
        input_valid,
        files_readable,
        scan_coverage_ready,
        auth_tokens_clear,
        debug_tokens_clear,
        private_api_clear,
        direct_identifiers_clear,
        privacy_ready,
        files,
        issues,
        next_actions,
    })
}

fn privacy_rules_clear(files: &[PrivacyLintFileReport], rules: &[&str]) -> bool {
    files.iter().all(|file| {
        file.findings
            .iter()
            .all(|finding| !rules.contains(&finding.rule.as_str()))
    })
}

fn privacy_lint_next_actions(files: &[PrivacyLintFileReport]) -> Vec<PrivacyLintNextAction> {
    let mut actions = Vec::new();
    let mut seen = BTreeSet::new();
    for file in files {
        for finding in &file.findings {
            let scope = match finding.line {
                Some(line) => format!("{}:{line}", file.path),
                None => file.path.clone(),
            };
            let action = PrivacyLintNextAction {
                scope,
                reason: finding.rule.clone(),
                action: privacy_lint_action_for_rule(&finding.rule).to_string(),
            };
            let key = format!("{}:{}:{}", action.scope, action.reason, action.action);
            if seen.insert(key) {
                actions.push(action);
            }
        }
    }
    actions
}

fn privacy_lint_action_for_rule(rule: &str) -> &'static str {
    match rule {
        "authorization_header" => {
            "Regenerate the artifact after redacting Authorization headers before writing logs or exports."
        }
        "bearer_token" | "jwt_token" => {
            "Regenerate the artifact after replacing bearer/JWT values with redaction markers before writing logs or exports."
        }
        "debug_query_token" => {
            "Redact local debug WebSocket token query values before persisting debug URLs into logs or exports."
        }
        "json_token" => {
            "Replace JSON token field values with redaction or pseudonym markers before export."
        }
        "private_whoop_api_material" => {
            "Remove private WHOOP API replay material from Goose artifacts; keep official-app traces only as redacted capture fixtures with provenance."
        }
        "direct_identifier" => {
            "Replace direct user/device identifiers with Goose-owned pseudonyms before export and keep any mapping outside shareable artifacts."
        }
        "email" => {
            "Remove or pseudonymize email-like values before writing shareable logs or exports."
        }
        "mac_address" => {
            "Replace MAC-address-like values with a stable pseudonym or redaction marker before export."
        }
        _ => "Inspect and redact the flagged value before sharing the artifact.",
    }
}

fn lint_file_path(
    path: &Path,
    display_path: &str,
    files: &mut Vec<PrivacyLintFileReport>,
) -> GooseResult<()> {
    if is_zip_path(path) {
        let file = File::open(path).map_err(|source| GooseError::io(path, source))?;
        let mut archive = ZipArchive::new(file)
            .map_err(|source| GooseError::message(format!("cannot open zip archive: {source}")))?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|source| {
                GooseError::message(format!("cannot read zip entry {index}: {source}"))
            })?;
            if entry.is_dir() {
                continue;
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|source| {
                GooseError::message(format!("cannot read zip bytes: {source}"))
            })?;
            let entry_path = format!("{display_path}!{}", entry.name());
            files.push(lint_bytes(&entry_path, &bytes));
        }
        return Ok(());
    }

    let bytes = fs::read(path).map_err(|source| GooseError::io(path, source))?;
    files.push(lint_bytes(display_path, &bytes));
    Ok(())
}

fn lint_bytes(path: &str, bytes: &[u8]) -> PrivacyLintFileReport {
    let format = file_format(path, bytes);
    if format == "binary" {
        return PrivacyLintFileReport {
            path: path.to_string(),
            format: format.to_string(),
            byte_len: bytes.len() as u64,
            scanned: false,
            skipped: true,
            findings: Vec::new(),
        };
    }

    let text = String::from_utf8_lossy(bytes);
    let findings = lint_text(&text);
    PrivacyLintFileReport {
        path: path.to_string(),
        format: format.to_string(),
        byte_len: bytes.len() as u64,
        scanned: true,
        skipped: false,
        findings,
    }
}

pub fn lint_text(text: &str) -> Vec<PrivacyFinding> {
    let mut findings = Vec::new();
    let mut seen = BTreeSet::new();

    for (index, line) in text.lines().enumerate() {
        let line_no = index + 1;
        let lower = line.to_ascii_lowercase();
        push_line_findings(line, &lower, line_no, &mut seen, &mut findings);
    }

    findings
}

fn push_line_findings(
    line: &str,
    lower: &str,
    line_no: usize,
    seen: &mut BTreeSet<String>,
    findings: &mut Vec<PrivacyFinding>,
) {
    if lower.contains("authorization:")
        && !lower.contains("authorization: <redacted:authorization>")
    {
        push_finding(
            "authorization_header",
            "unredacted Authorization header",
            Some(line_no),
            line,
            seen,
            findings,
        );
    }
    if lower.contains("bearer ") && !lower.contains("<redacted:bearer-token>") {
        push_finding(
            "bearer_token",
            "unredacted Bearer token marker",
            Some(line_no),
            line,
            seen,
            findings,
        );
    }
    if contains_unredacted_query_token(line) {
        push_finding(
            "debug_query_token",
            "unredacted token= query parameter",
            Some(line_no),
            line,
            seen,
            findings,
        );
    }
    if contains_unredacted_json_token(line) {
        push_finding(
            "json_token",
            "unredacted JSON token field",
            Some(line_no),
            line,
            seen,
            findings,
        );
    }
    if lower.contains("metrics-service/v1/")
        || lower.contains("sensorapi")
        || lower.contains("sensor_data")
        || lower.contains("api-7.whoop.com")
        || lower.contains("api.prod.whoop.com")
        || lower.contains("x-whoop-")
    {
        push_finding(
            "private_whoop_api_material",
            "private WHOOP API replay material",
            Some(line_no),
            line,
            seen,
            findings,
        );
    }
    if lower.contains("userid")
        || lower.contains("user_id")
        || lower.contains("strap_serial")
        || lower.contains("strap serial")
        || lower.contains("strap-id")
        || lower.contains("x-whoop-strap-id")
    {
        push_finding(
            "direct_identifier",
            "unredacted direct user/device identifier",
            Some(line_no),
            line,
            seen,
            findings,
        );
    }

    for token in tokens(line) {
        if looks_like_jwt(&token) {
            push_finding(
                "jwt_token",
                "unredacted JWT-like token",
                Some(line_no),
                &token,
                seen,
                findings,
            );
        }
        if looks_like_email(&token) {
            push_finding(
                "email",
                "unredacted email-like token",
                Some(line_no),
                &token,
                seen,
                findings,
            );
        }
    }

    if contains_mac_address(line) {
        push_finding(
            "mac_address",
            "unredacted MAC-address-like token",
            Some(line_no),
            line,
            seen,
            findings,
        );
    }
}

fn push_finding(
    rule: &str,
    message: &str,
    line: Option<usize>,
    snippet: &str,
    seen: &mut BTreeSet<String>,
    findings: &mut Vec<PrivacyFinding>,
) {
    let snippet = compact_snippet(snippet);
    let key = format!("{rule}:{line:?}:{snippet}");
    if seen.insert(key) {
        findings.push(PrivacyFinding {
            rule: rule.to_string(),
            message: message.to_string(),
            line,
            snippet,
        });
    }
}

fn contains_unredacted_query_token(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let mut search_start = 0;
    while let Some(relative_index) = lower[search_start..].find("token=") {
        let value_start = search_start + relative_index + "token=".len();
        let value = &line[value_start..];
        let value_end = value
            .find(['&', '"', '\'', ' ', '}', ']'])
            .unwrap_or(value.len());
        let value = &value[..value_end];
        if !value.is_empty() && !value.starts_with("<redacted>") {
            return true;
        }
        search_start = value_start + value_end;
    }
    false
}

fn contains_unredacted_json_token(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    for needle in ["\"token\"", "'token'"] {
        let Some(index) = lower.find(needle) else {
            continue;
        };
        let after = &line[index + needle.len()..];
        let Some(separator_index) = after.find(':') else {
            continue;
        };
        let value = after[separator_index + 1..].trim_start();
        if value.starts_with('"') || value.starts_with('\'') {
            let value = value.trim_start_matches(['"', '\'']);
            if !value.starts_with("<redacted") && !value.starts_with("<pseudonym") {
                return true;
            }
        }
    }
    false
}

fn tokens(text: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut token = String::new();
    for ch in text.chars() {
        if is_token_char(ch) {
            token.push(ch);
        } else if !token.is_empty() {
            values.push(std::mem::take(&mut token));
        }
    }
    if !token.is_empty() {
        values.push(token);
    }
    values
}

fn is_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '%' | '+' | '@' | '/' | '=')
}

fn looks_like_email(token: &str) -> bool {
    let Some(at_index) = token.find('@') else {
        return false;
    };
    at_index > 0
        && token[at_index + 1..].contains('.')
        && !token.ends_with('@')
        && !token.starts_with('@')
}

fn looks_like_jwt(token: &str) -> bool {
    token.len() > 60 && token.starts_with("eyJ") && token.matches('.').count() == 2
}

fn contains_mac_address(text: &str) -> bool {
    let mut index = 0;
    while index < text.len() {
        let remaining = &text[index..];
        if remaining.len() >= 17 && looks_like_mac_prefix(remaining) {
            return true;
        }
        let Some(ch) = remaining.chars().next() else {
            break;
        };
        index += ch.len_utf8();
    }
    false
}

fn looks_like_mac_prefix(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() < 17 {
        return false;
    }
    for index in 0..17 {
        if matches!(index, 2 | 5 | 8 | 11 | 14) {
            if bytes[index] != b':' {
                return false;
            }
        } else if !bytes[index].is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

fn compact_snippet(text: &str) -> String {
    let value = text.trim().replace('\t', " ");
    if value.len() <= 160 {
        value
    } else {
        format!("{}...", &value[..160])
    }
}

fn file_format(path: &str, bytes: &[u8]) -> &'static str {
    if bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
        return "binary";
    }
    if path.ends_with(".json") {
        "json"
    } else if path.ends_with(".jsonl") || path.ends_with(".ndjson") {
        "jsonl"
    } else if path.ends_with(".csv") {
        "csv"
    } else {
        "text"
    }
}

fn is_zip_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>) -> GooseResult<()> {
    for entry in fs::read_dir(root).map_err(|source| GooseError::io(root, source))? {
        let entry = entry.map_err(|source| GooseError::io(root, source))?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn relative_path(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

fn slash_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => part.to_str().map(ToOwned::to_owned),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("input")
        .to_string()
}
