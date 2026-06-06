//! OpenWhoop-derived WHOOP protocol references.
//!
//! Source snapshot:
//! <https://github.com/bWanShiTong/openwhoop/tree/55c5c1e2e02d3822c33e258838a57bb7d9e2ca53>
//!
//! License caveat: the cited snapshot did not include a license file at the
//! referenced commit. Treat the data below as reverse-engineering prior art and
//! behavioral reference, not copied implementation code.

use std::fmt::{Display, Formatter};

use crate::protocol::DeviceType;

pub const OPENWHOOP_REFERENCE_REPOSITORY: &str = "https://github.com/bWanShiTong/openwhoop";
pub const OPENWHOOP_REFERENCE_COMMIT: &str = "55c5c1e2e02d3822c33e258838a57bb7d9e2ca53";
pub const OPENWHOOP_REFERENCE_SNAPSHOT_URL: &str =
    "https://github.com/bWanShiTong/openwhoop/tree/55c5c1e2e02d3822c33e258838a57bb7d9e2ca53";
pub const OPENWHOOP_REFERENCE_ATTRIBUTION: &str =
    "OpenWhoop snapshot used as a behavioral reference for WHOOP Gen4/Gen5 BLE protocol layout.";
pub const OPENWHOOP_REFERENCE_LICENSE_CAVEAT: &str = "The referenced OpenWhoop snapshot did not include a license file at the cited commit; use it as reverse-engineering prior art only.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhoopGeneration {
    Gen4,
    Gen5,
}

impl WhoopGeneration {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gen4 => "Gen4",
            Self::Gen5 => "Gen5",
        }
    }
}

impl Display for WhoopGeneration {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhoopCharacteristicRole {
    CommandToStrap,
    CommandFromStrap,
    EventsFromStrap,
    DataFromStrap,
    Memfault,
}

impl WhoopCharacteristicRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CommandToStrap => "command_to_strap",
            Self::CommandFromStrap => "command_from_strap",
            Self::EventsFromStrap => "events_from_strap",
            Self::DataFromStrap => "data_from_strap",
            Self::Memfault => "memfault",
        }
    }
}

impl Display for WhoopCharacteristicRole {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub const WHOOP_SERVICE_GEN4: &str = "61080001-8d6d-82b8-614a-1c8cb0f8dcc6";
pub const WHOOP_SERVICE_GEN5: &str = "fd4b0001-cce1-4033-93ce-002d5875f58a";

pub const WHOOP_COMMAND_TO_STRAP_GEN4: &str = "61080002-8d6d-82b8-614a-1c8cb0f8dcc6";
pub const WHOOP_COMMAND_FROM_STRAP_GEN4: &str = "61080003-8d6d-82b8-614a-1c8cb0f8dcc6";
pub const WHOOP_EVENTS_FROM_STRAP_GEN4: &str = "61080004-8d6d-82b8-614a-1c8cb0f8dcc6";
pub const WHOOP_DATA_FROM_STRAP_GEN4: &str = "61080005-8d6d-82b8-614a-1c8cb0f8dcc6";
pub const WHOOP_MEMFAULT_GEN4: &str = "61080007-8d6d-82b8-614a-1c8cb0f8dcc6";

pub const WHOOP_COMMAND_TO_STRAP_GEN5: &str = "fd4b0002-cce1-4033-93ce-002d5875f58a";
pub const WHOOP_COMMAND_FROM_STRAP_GEN5: &str = "fd4b0003-cce1-4033-93ce-002d5875f58a";
pub const WHOOP_EVENTS_FROM_STRAP_GEN5: &str = "fd4b0004-cce1-4033-93ce-002d5875f58a";
pub const WHOOP_DATA_FROM_STRAP_GEN5: &str = "fd4b0005-cce1-4033-93ce-002d5875f58a";
pub const WHOOP_MEMFAULT_GEN5: &str = "fd4b0007-cce1-4033-93ce-002d5875f58a";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhoopGenerationReference {
    pub generation: WhoopGeneration,
    pub service_uuid: &'static str,
    pub command_to_strap_uuid: &'static str,
    pub command_from_strap_uuid: &'static str,
    pub events_from_strap_uuid: &'static str,
    pub data_from_strap_uuid: &'static str,
    pub memfault_uuid: &'static str,
}

impl WhoopGenerationReference {
    pub const fn characteristic_uuid(self, role: WhoopCharacteristicRole) -> &'static str {
        match role {
            WhoopCharacteristicRole::CommandToStrap => self.command_to_strap_uuid,
            WhoopCharacteristicRole::CommandFromStrap => self.command_from_strap_uuid,
            WhoopCharacteristicRole::EventsFromStrap => self.events_from_strap_uuid,
            WhoopCharacteristicRole::DataFromStrap => self.data_from_strap_uuid,
            WhoopCharacteristicRole::Memfault => self.memfault_uuid,
        }
    }
}

pub const WHOOP_REFERENCE_GEN4: WhoopGenerationReference = WhoopGenerationReference {
    generation: WhoopGeneration::Gen4,
    service_uuid: WHOOP_SERVICE_GEN4,
    command_to_strap_uuid: WHOOP_COMMAND_TO_STRAP_GEN4,
    command_from_strap_uuid: WHOOP_COMMAND_FROM_STRAP_GEN4,
    events_from_strap_uuid: WHOOP_EVENTS_FROM_STRAP_GEN4,
    data_from_strap_uuid: WHOOP_DATA_FROM_STRAP_GEN4,
    memfault_uuid: WHOOP_MEMFAULT_GEN4,
};

pub const WHOOP_REFERENCE_GEN5: WhoopGenerationReference = WhoopGenerationReference {
    generation: WhoopGeneration::Gen5,
    service_uuid: WHOOP_SERVICE_GEN5,
    command_to_strap_uuid: WHOOP_COMMAND_TO_STRAP_GEN5,
    command_from_strap_uuid: WHOOP_COMMAND_FROM_STRAP_GEN5,
    events_from_strap_uuid: WHOOP_EVENTS_FROM_STRAP_GEN5,
    data_from_strap_uuid: WHOOP_DATA_FROM_STRAP_GEN5,
    memfault_uuid: WHOOP_MEMFAULT_GEN5,
};

pub const WHOOP_REFERENCE_TABLE: [WhoopGenerationReference; 2] =
    [WHOOP_REFERENCE_GEN4, WHOOP_REFERENCE_GEN5];

pub fn whoop_generation_reference(
    generation: WhoopGeneration,
) -> &'static WhoopGenerationReference {
    match generation {
        WhoopGeneration::Gen4 => &WHOOP_REFERENCE_GEN4,
        WhoopGeneration::Gen5 => &WHOOP_REFERENCE_GEN5,
    }
}

pub fn whoop_generation_references() -> &'static [WhoopGenerationReference] {
    &WHOOP_REFERENCE_TABLE
}

pub fn whoop_service_uuid(generation: WhoopGeneration) -> &'static str {
    whoop_generation_reference(generation).service_uuid
}

pub fn whoop_characteristic_uuid(
    generation: WhoopGeneration,
    role: WhoopCharacteristicRole,
) -> &'static str {
    whoop_generation_reference(generation).characteristic_uuid(role)
}

pub fn whoop_generation_from_service_uuid(service_uuid: &str) -> Option<WhoopGeneration> {
    let service_uuid = service_uuid.trim();
    if service_uuid.eq_ignore_ascii_case(WHOOP_SERVICE_GEN4) {
        Some(WhoopGeneration::Gen4)
    } else if service_uuid.eq_ignore_ascii_case(WHOOP_SERVICE_GEN5) {
        Some(WhoopGeneration::Gen5)
    } else {
        None
    }
}

pub fn whoop_generation_from_device_type(device_type: DeviceType) -> Option<WhoopGeneration> {
    match device_type {
        DeviceType::Gen4 => Some(WhoopGeneration::Gen4),
        DeviceType::Maverick | DeviceType::Goose => Some(WhoopGeneration::Gen5),
        // HrMonitor is not a WHOOP device; Puffin has no known generation mapping.
        DeviceType::Puffin | DeviceType::HrMonitor => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GooseSummaryStatus {
    Matched,
    Candidate,
    Conflicting,
    NotDecoded,
}

impl GooseSummaryStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::Candidate => "candidate",
            Self::Conflicting => "conflicting",
            Self::NotDecoded => "not_decoded",
        }
    }
}

impl Display for GooseSummaryStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenWhoopHistoryField {
    Bpm,
    Rr,
    Imu,
    Ppg,
    RawSpo2RedIr,
    RawSkinTemp,
    RespiratoryRaw,
    SignalQuality,
    SkinContact,
    Gravity,
    Gen5Spo2Percentage,
}

impl OpenWhoopHistoryField {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bpm => "BPM",
            Self::Rr => "RR",
            Self::Imu => "IMU",
            Self::Ppg => "PPG",
            Self::RawSpo2RedIr => "raw SpO2 red/IR",
            Self::RawSkinTemp => "raw skin temp",
            Self::RespiratoryRaw => "respiratory raw",
            Self::SignalQuality => "signal quality",
            Self::SkinContact => "skin contact",
            Self::Gravity => "gravity",
            Self::Gen5Spo2Percentage => "Gen5 SpO2 percentage",
        }
    }
}

impl Display for OpenWhoopHistoryField {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub const GOOSE_SUMMARIES_NONE: [&str; 0] = [];
pub const GOOSE_SUMMARIES_NORMAL_HISTORY: [&str; 1] = ["normal_history"];
pub const GOOSE_SUMMARIES_R17: [&str; 1] = ["r17_optical_or_labrador_filtered"];
pub const GOOSE_SUMMARIES_RAW_MOTION: [&str; 2] = ["raw_motion_k10", "raw_motion_k21"];
pub const GOOSE_SUMMARIES_EVENT_TEMPERATURE: [&str; 1] = ["event_temperature_level"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryFieldReference {
    pub field: OpenWhoopHistoryField,
    pub gen4: bool,
    pub gen5: bool,
    pub goose_summary_kinds: &'static [&'static str],
    pub status: GooseSummaryStatus,
    pub note: &'static str,
}

impl HistoryFieldReference {
    pub const fn applies_to(self, generation: WhoopGeneration) -> bool {
        match generation {
            WhoopGeneration::Gen4 => self.gen4,
            WhoopGeneration::Gen5 => self.gen5,
        }
    }
}

pub const OPENWHOOP_HISTORY_FIELD_REFERENCES: [HistoryFieldReference; 11] = [
    HistoryFieldReference {
        field: OpenWhoopHistoryField::Bpm,
        gen4: true,
        gen5: true,
        goose_summary_kinds: &GOOSE_SUMMARIES_NORMAL_HISTORY,
        status: GooseSummaryStatus::Matched,
        note: "Goose already promotes the normal_history heart-rate marker into heart_rate_bpm; the byte-level source is not the same, but the summary-level concept matches.",
    },
    HistoryFieldReference {
        field: OpenWhoopHistoryField::Rr,
        gen4: true,
        gen5: true,
        goose_summary_kinds: &GOOSE_SUMMARIES_R17,
        status: GooseSummaryStatus::Candidate,
        note: "Goose currently treats RR intervals as preliminary candidates from r17_optical_or_labrador_filtered rather than a fully decoded strap-history field.",
    },
    HistoryFieldReference {
        field: OpenWhoopHistoryField::Imu,
        gen4: true,
        gen5: true,
        goose_summary_kinds: &GOOSE_SUMMARIES_RAW_MOTION,
        status: GooseSummaryStatus::Matched,
        note: "Goose already exposes raw motion summaries for K10/K21 motion payloads, which are the closest direct match to OpenWhoop IMU samples.",
    },
    HistoryFieldReference {
        field: OpenWhoopHistoryField::Ppg,
        gen4: true,
        gen5: true,
        goose_summary_kinds: &GOOSE_SUMMARIES_R17,
        status: GooseSummaryStatus::Candidate,
        note: "Goose keeps the optical stream as r17_optical_or_labrador_filtered candidates, but does not yet expose a dedicated PPG history field.",
    },
    HistoryFieldReference {
        field: OpenWhoopHistoryField::RawSpo2RedIr,
        gen4: true,
        gen5: true,
        goose_summary_kinds: &GOOSE_SUMMARIES_NONE,
        status: GooseSummaryStatus::NotDecoded,
        note: "Goose does not currently expose a raw SpO2 red/IR decoder or summary.",
    },
    HistoryFieldReference {
        field: OpenWhoopHistoryField::RawSkinTemp,
        gen4: true,
        gen5: true,
        goose_summary_kinds: &GOOSE_SUMMARIES_EVENT_TEMPERATURE,
        status: GooseSummaryStatus::Candidate,
        note: "Goose has an event_temperature_level candidate path, but metric_readiness still blocks skin-temperature extraction until the units and semantics are verified.",
    },
    HistoryFieldReference {
        field: OpenWhoopHistoryField::RespiratoryRaw,
        gen4: true,
        gen5: true,
        goose_summary_kinds: &GOOSE_SUMMARIES_NONE,
        status: GooseSummaryStatus::NotDecoded,
        note: "Goose's metric readiness explicitly marks respiratory-rate extraction as not implemented.",
    },
    HistoryFieldReference {
        field: OpenWhoopHistoryField::SignalQuality,
        gen4: true,
        gen5: true,
        goose_summary_kinds: &GOOSE_SUMMARIES_NONE,
        status: GooseSummaryStatus::NotDecoded,
        note: "Goose does not currently surface a signal-quality summary for this field.",
    },
    HistoryFieldReference {
        field: OpenWhoopHistoryField::SkinContact,
        gen4: true,
        gen5: true,
        goose_summary_kinds: &GOOSE_SUMMARIES_NONE,
        status: GooseSummaryStatus::NotDecoded,
        note: "Goose does not currently surface a skin-contact summary for this field.",
    },
    HistoryFieldReference {
        field: OpenWhoopHistoryField::Gravity,
        gen4: true,
        gen5: true,
        goose_summary_kinds: &GOOSE_SUMMARIES_RAW_MOTION,
        status: GooseSummaryStatus::Conflicting,
        note: "Goose raw motion summaries expose signed axes rather than the derived gravity vector that OpenWhoop stores here, so the semantics do not line up cleanly.",
    },
    HistoryFieldReference {
        field: OpenWhoopHistoryField::Gen5Spo2Percentage,
        gen4: false,
        gen5: true,
        goose_summary_kinds: &GOOSE_SUMMARIES_NONE,
        status: GooseSummaryStatus::NotDecoded,
        note: "OpenWhoop marks this as a Gen5-only field, but Goose does not yet have a summary or decoder for it.",
    },
];

pub fn openwhoop_history_field_references() -> &'static [HistoryFieldReference] {
    &OPENWHOOP_HISTORY_FIELD_REFERENCES
}

pub fn openwhoop_history_field_reference(
    field: OpenWhoopHistoryField,
) -> Option<&'static HistoryFieldReference> {
    OPENWHOOP_HISTORY_FIELD_REFERENCES
        .iter()
        .find(|reference| reference.field == field)
}

pub fn openwhoop_history_field_status(field: OpenWhoopHistoryField) -> Option<GooseSummaryStatus> {
    openwhoop_history_field_reference(field).map(|reference| reference.status)
}

pub fn openwhoop_history_field_goose_summary_kinds(
    field: OpenWhoopHistoryField,
) -> Option<&'static [&'static str]> {
    openwhoop_history_field_reference(field).map(|reference| reference.goose_summary_kinds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_uuids_match_reference_snapshot() {
        let gen4 = whoop_generation_reference(WhoopGeneration::Gen4);
        assert_eq!(gen4.service_uuid, WHOOP_SERVICE_GEN4);
        assert_eq!(
            gen4.characteristic_uuid(WhoopCharacteristicRole::CommandToStrap),
            WHOOP_COMMAND_TO_STRAP_GEN4
        );
        assert_eq!(
            gen4.characteristic_uuid(WhoopCharacteristicRole::CommandFromStrap),
            WHOOP_COMMAND_FROM_STRAP_GEN4
        );
        assert_eq!(
            gen4.characteristic_uuid(WhoopCharacteristicRole::EventsFromStrap),
            WHOOP_EVENTS_FROM_STRAP_GEN4
        );
        assert_eq!(
            gen4.characteristic_uuid(WhoopCharacteristicRole::DataFromStrap),
            WHOOP_DATA_FROM_STRAP_GEN4
        );
        assert_eq!(
            gen4.characteristic_uuid(WhoopCharacteristicRole::Memfault),
            WHOOP_MEMFAULT_GEN4
        );

        let gen5 = whoop_generation_reference(WhoopGeneration::Gen5);
        assert_eq!(gen5.service_uuid, WHOOP_SERVICE_GEN5);
        assert_eq!(
            gen5.characteristic_uuid(WhoopCharacteristicRole::CommandToStrap),
            WHOOP_COMMAND_TO_STRAP_GEN5
        );
        assert_eq!(
            gen5.characteristic_uuid(WhoopCharacteristicRole::CommandFromStrap),
            WHOOP_COMMAND_FROM_STRAP_GEN5
        );
        assert_eq!(
            gen5.characteristic_uuid(WhoopCharacteristicRole::EventsFromStrap),
            WHOOP_EVENTS_FROM_STRAP_GEN5
        );
        assert_eq!(
            gen5.characteristic_uuid(WhoopCharacteristicRole::DataFromStrap),
            WHOOP_DATA_FROM_STRAP_GEN5
        );
        assert_eq!(
            gen5.characteristic_uuid(WhoopCharacteristicRole::Memfault),
            WHOOP_MEMFAULT_GEN5
        );
    }

    #[test]
    fn service_uuid_lookup_is_generation_aware() {
        assert_eq!(
            whoop_generation_from_service_uuid(WHOOP_SERVICE_GEN4),
            Some(WhoopGeneration::Gen4)
        );
        assert_eq!(
            whoop_generation_from_service_uuid(WHOOP_SERVICE_GEN5),
            Some(WhoopGeneration::Gen5)
        );
        assert_eq!(
            whoop_generation_from_device_type(DeviceType::Gen4),
            Some(WhoopGeneration::Gen4)
        );
        assert_eq!(
            whoop_generation_from_device_type(DeviceType::Goose),
            Some(WhoopGeneration::Gen5)
        );
        assert_eq!(whoop_generation_from_device_type(DeviceType::Puffin), None);
    }

    #[test]
    fn history_field_table_marks_goose_statuses() {
        let bpm = openwhoop_history_field_reference(OpenWhoopHistoryField::Bpm).unwrap();
        assert_eq!(bpm.status, GooseSummaryStatus::Matched);
        assert_eq!(bpm.goose_summary_kinds, &GOOSE_SUMMARIES_NORMAL_HISTORY);

        let gravity = openwhoop_history_field_reference(OpenWhoopHistoryField::Gravity).unwrap();
        assert_eq!(gravity.status, GooseSummaryStatus::Conflicting);
        assert_eq!(gravity.goose_summary_kinds, &GOOSE_SUMMARIES_RAW_MOTION);

        let spo2 =
            openwhoop_history_field_reference(OpenWhoopHistoryField::Gen5Spo2Percentage).unwrap();
        assert!(!spo2.gen4);
        assert!(spo2.gen5);
        assert_eq!(spo2.status, GooseSummaryStatus::NotDecoded);
    }

    #[test]
    fn attribution_note_mentions_reference_caveat() {
        assert!(OPENWHOOP_REFERENCE_LICENSE_CAVEAT.contains("license file"));
        assert!(OPENWHOOP_REFERENCE_ATTRIBUTION.contains("OpenWhoop"));
    }
}
