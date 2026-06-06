use goose_core::heart_rate_gatt_protocol::parse_hr_measurement;

// Test vectors derived from the Swift parseStandardHeartRateMeasurement reference implementation
// in GooseBLEClient+Parsing.swift lines 502–535.

#[test]
fn test_parses_8bit_hr_only() {
    // flags=0x00: 8-bit HR format, no sensor contact, no energy, no RR
    let data = [0x00u8, 72];
    let result = parse_hr_measurement(&data).unwrap();
    assert_eq!(result.hr_bpm, 72);
    assert!(result.rr_intervals_ms.is_empty());
    assert_eq!(result.energy_expended_kj, None);
    assert_eq!(result.sensor_contact, None);
}

#[test]
fn test_parses_16bit_hr() {
    // flags=0x01: 16-bit HR format; HR = 0x00C4 = 196 in little-endian
    let data = [0x01u8, 0xC4, 0x00];
    let result = parse_hr_measurement(&data).unwrap();
    assert_eq!(result.hr_bpm, 196);
    assert!(result.rr_intervals_ms.is_empty());
    assert_eq!(result.energy_expended_kj, None);
    assert_eq!(result.sensor_contact, None);
}

#[test]
fn test_parses_hr_with_rr_intervals() {
    // flags=0x10: 8-bit HR, RR intervals present
    // HR = 60 (1 byte), then RR = 0x0400 LE (1024 raw) = 1000.0 ms
    // Last two bytes (0x00, 0xFF) are incomplete (only 1 byte for 2nd RR) — ignored
    let data = [0x10u8, 60, 0x00, 0x04, 0xFF];
    let result = parse_hr_measurement(&data).unwrap();
    assert_eq!(result.hr_bpm, 60);
    assert_eq!(result.rr_intervals_ms.len(), 1);
    let expected_ms = 1024.0f64 * 1000.0 / 1024.0; // = 1000.0
    assert!((result.rr_intervals_ms[0] - expected_ms).abs() < 0.01);
}

#[test]
fn test_parses_hr_with_energy_expended() {
    // flags=0x08: 8-bit HR, energy expended present
    // HR = 75, energy = 0x03E8 LE = 1000 kJ
    let data = [0x08u8, 75, 0xE8, 0x03];
    let result = parse_hr_measurement(&data).unwrap();
    assert_eq!(result.hr_bpm, 75);
    assert_eq!(result.energy_expended_kj, Some(1000));
    assert!(result.rr_intervals_ms.is_empty());
}

#[test]
fn test_parses_all_fields() {
    // flags=0b00011111 = 0x1F:
    //   bit 0: 1 → 16-bit HR
    //   bits 1-2: 11 → sensor contact detected (Some(true))
    //   bit 3: 1 → energy expended present
    //   bit 4: 1 → RR intervals present
    // Layout: [flags=0x1F, HR_lo=0xC8, HR_hi=0x00 (200 bpm), energy_lo=0x64, energy_hi=0x00 (100 kJ),
    //          RR_lo=0x00, RR_hi=0x02 (512 raw → 500.0 ms)]
    let data = [0x1Fu8, 0xC8, 0x00, 0x64, 0x00, 0x00, 0x02];
    let result = parse_hr_measurement(&data).unwrap();
    assert_eq!(result.hr_bpm, 200);
    assert_eq!(result.sensor_contact, Some(true));
    assert_eq!(result.energy_expended_kj, Some(100));
    assert_eq!(result.rr_intervals_ms.len(), 1);
    let expected_ms = 512.0f64 * 1000.0 / 1024.0; // = 500.0
    assert!((result.rr_intervals_ms[0] - expected_ms).abs() < 0.01);
}

#[test]
fn test_returns_error_on_empty_data() {
    assert!(parse_hr_measurement(&[]).is_err());
}

#[test]
fn test_returns_error_on_single_byte() {
    assert!(parse_hr_measurement(&[0x00]).is_err());
}

#[test]
fn test_16bit_hr_truncated_returns_error() {
    // flags=0x01 (16-bit HR), but only 1 byte available for HR value (truncated)
    let data = [0x01u8, 0xC4];
    let result = parse_hr_measurement(&data);
    assert!(result.is_err());
}

#[test]
fn test_sensor_contact_not_detected() {
    // flags: bits 1-2 = 0b10 → sensor contact supported but NOT detected
    // bits 1-2 = 0b10 → flags & 0x06 = 0x04 → (flags >> 1) & 0x03 = 0b10
    let flags = 0x04u8; // bits: 0b00000100 → sc_bits = 0b10
    let data = [flags, 80];
    let result = parse_hr_measurement(&data).unwrap();
    assert_eq!(result.sensor_contact, Some(false));
}

#[test]
fn test_sensor_contact_detected() {
    // flags: bits 1-2 = 0b11 → sensor contact supported AND detected
    // 0b00000110 = 0x06 → sc_bits = 0b11
    let flags = 0x06u8;
    let data = [flags, 85];
    let result = parse_hr_measurement(&data).unwrap();
    assert_eq!(result.sensor_contact, Some(true));
}
