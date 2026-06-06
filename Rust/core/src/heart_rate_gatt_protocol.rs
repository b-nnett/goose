// Standard Bluetooth 0x2A37 HR Measurement characteristic parser.

/// Decoded Heart Rate Measurement from a standard Bluetooth 0x2A37 GATT notification.
#[derive(Debug, Clone, PartialEq)]
pub struct HrMeasurement {
    /// Heart rate in beats per minute.
    pub hr_bpm: u16,
    /// RR intervals in milliseconds, converted from raw 1/1024-second units.
    /// Each raw value is converted via: raw as f64 * 1000.0 / 1024.0
    pub rr_intervals_ms: Vec<f64>,
    /// Energy expended in kilojoules, present when flags bit 3 is set.
    pub energy_expended_kj: Option<u16>,
    /// Sensor contact status: Some(true) when contact detected (bits 1–2 == 0b11),
    /// Some(false) when not detected (bits 1–2 == 0b10), None when not supported
    /// (bits 1–2 == 0b00 or 0b01).
    pub sensor_contact: Option<bool>,
}

/// Parse a standard Bluetooth 0x2A37 HR Measurement characteristic value.
///
/// Returns `Err` if the input is too short or truncated for the declared fields.
pub fn parse_hr_measurement(data: &[u8]) -> Result<HrMeasurement, String> {
    if data.len() < 2 {
        return Err("hr_measurement: data too short".to_string());
    }

    let flags = data[0];
    let mut offset = 1usize;

    // HR value: bit 0 selects format (0 = uint8, 1 = uint16 LE)
    let hr_bpm: u16 = if flags & 0x01 == 0 {
        let bpm = data[offset] as u16;
        offset += 1;
        bpm
    } else {
        if data.len() < offset + 2 {
            return Err("hr_measurement: truncated 16-bit hr".to_string());
        }
        let bpm = u16::from_le_bytes([data[offset], data[offset + 1]]);
        offset += 2;
        bpm
    };

    // Sensor contact status: bits 1–2
    // Bits 1–2 == 0b11 → detected (Some(true))
    // Bits 1–2 == 0b10 → not detected (Some(false))
    // Bits 1–2 == 0b00 or 0b01 → not supported (None)
    let sc_bits = (flags >> 1) & 0x03;
    let sensor_contact: Option<bool> = match sc_bits {
        0b11 => Some(true),
        0b10 => Some(false),
        _ => None,
    };

    // Energy expended: bit 3
    let energy_expended_kj: Option<u16> = if flags & 0x08 != 0 {
        if data.len() >= offset + 2 {
            let energy = u16::from_le_bytes([data[offset], data[offset + 1]]);
            offset += 2;
            Some(energy)
        } else {
            // Truncated — advance past and set None
            offset = data.len();
            None
        }
    } else {
        None
    };

    // RR intervals: bit 4
    let mut rr_intervals_ms: Vec<f64> = Vec::new();
    if flags & 0x10 != 0 {
        while data.len() >= offset + 2 {
            let raw = u16::from_le_bytes([data[offset], data[offset + 1]]);
            rr_intervals_ms.push(raw as f64 * 1000.0 / 1024.0);
            offset += 2;
        }
    }

    Ok(HrMeasurement {
        hr_bpm,
        rr_intervals_ms,
        energy_expended_kj,
        sensor_contact,
    })
}
