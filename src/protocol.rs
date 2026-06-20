//! Pure Stream Deck wire-protocol builders and parsers.
//!
//! Every constant here is taken from the MK.2 HID report descriptor:
//! - Output report id `0x02`, 1023-byte payload -> 1024-byte image packets.
//! - Input report id `0x01`, 511-byte payload, key states at offset 4.
//! - Feature reports id `0x03`, 31-byte payload (32 incl. report id).
//!
//! These functions are deliberately free of any I/O so they can be unit
//! tested without the hardware present.

/// Total length of one image output packet (incl. report id byte).
pub const IMAGE_PACKET_LEN: usize = 1024;
/// Header length inside an image packet (report id + 7 control bytes).
pub const IMAGE_HEADER_LEN: usize = 8;
/// Maximum JPEG payload bytes carried by a single image packet.
pub const IMAGE_PAYLOAD_LEN: usize = IMAGE_PACKET_LEN - IMAGE_HEADER_LEN;

/// Length of a feature report payload sent to the device (incl. report id).
pub const FEATURE_LEN: usize = 32;

/// Report id used to read button state input reports.
pub const INPUT_REPORT_ID: u8 = 0x01;
/// Offset within an input report where the per-key state bytes begin.
pub const KEY_STATE_OFFSET: usize = 4;
/// Full size of a button input report (report id + 511 payload bytes).
pub const INPUT_REPORT_LEN: usize = 512;

/// Build the feature report that sets display brightness (`0..=100`).
pub fn brightness_feature(percent: u8) -> [u8; FEATURE_LEN] {
    let mut report = [0u8; FEATURE_LEN];
    report[0] = 0x03;
    report[1] = 0x08;
    report[2] = percent.min(100);
    report
}

/// Build the feature report that resets the device to its standby logo.
pub fn reset_feature() -> [u8; FEATURE_LEN] {
    let mut report = [0u8; FEATURE_LEN];
    report[0] = 0x03;
    report[1] = 0x02;
    report
}

/// Split an encoded key image into a sequence of 1024-byte output packets.
///
/// `key` is the hardware key index. `image` is the already-encoded image
/// payload (JPEG for the MK.2). Each returned packet is exactly
/// [`IMAGE_PACKET_LEN`] bytes and begins with output report id `0x02`.
pub fn image_packets(key: u8, image: &[u8]) -> Vec<[u8; IMAGE_PACKET_LEN]> {
    let mut packets = Vec::new();
    let mut remaining = image.len();
    let mut page: u16 = 0;

    // Always emit at least one packet, even for an empty image, so a key can
    // be explicitly cleared.
    loop {
        let sent = page as usize * IMAGE_PAYLOAD_LEN;
        let this_len = remaining.min(IMAGE_PAYLOAD_LEN);
        let is_last = this_len == remaining;

        let mut packet = [0u8; IMAGE_PACKET_LEN];
        packet[0] = 0x02;
        packet[1] = 0x07;
        packet[2] = key;
        packet[3] = if is_last { 1 } else { 0 };
        packet[4] = (this_len & 0xff) as u8;
        packet[5] = (this_len >> 8) as u8;
        packet[6] = (page & 0xff) as u8;
        packet[7] = (page >> 8) as u8;
        packet[IMAGE_HEADER_LEN..IMAGE_HEADER_LEN + this_len]
            .copy_from_slice(&image[sent..sent + this_len]);

        packets.push(packet);

        remaining -= this_len;
        page += 1;
        if remaining == 0 {
            break;
        }
    }

    packets
}

/// Parse the pressed/released state of each key from an input report.
///
/// `report` is the raw bytes read from the hidraw node (first byte is the
/// report id). `key_count` is how many keys the device has. Missing/short
/// reports yield all-released so callers never panic on a partial read.
pub fn parse_key_states(report: &[u8], key_count: usize) -> Vec<bool> {
    (0..key_count)
        .map(|i| {
            report
                .get(KEY_STATE_OFFSET + i)
                .map(|&b| b != 0)
                .unwrap_or(false)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_is_clamped_and_framed() {
        let report = brightness_feature(60);
        assert_eq!(report.len(), FEATURE_LEN);
        assert_eq!(&report[..3], &[0x03, 0x08, 60]);

        // Over-100 values clamp to 100 rather than wrapping.
        assert_eq!(brightness_feature(250)[2], 100);
        assert_eq!(brightness_feature(0)[2], 0);
    }

    #[test]
    fn reset_report_is_framed() {
        let report = reset_feature();
        assert_eq!(report.len(), FEATURE_LEN);
        assert_eq!(&report[..2], &[0x03, 0x02]);
    }

    #[test]
    fn small_image_fits_in_one_packet() {
        let image = vec![0xABu8; 100];
        let packets = image_packets(7, &image);

        assert_eq!(packets.len(), 1);
        let p = &packets[0];
        assert_eq!(p.len(), IMAGE_PACKET_LEN);
        assert_eq!(p[0], 0x02); // report id
        assert_eq!(p[1], 0x07);
        assert_eq!(p[2], 7); // key index
        assert_eq!(p[3], 1); // is_last
        assert_eq!(p[4], 100); // length low byte
        assert_eq!(p[5], 0); // length high byte
        assert_eq!(p[6], 0); // page low
        assert_eq!(p[7], 0); // page high
        assert_eq!(&p[IMAGE_HEADER_LEN..IMAGE_HEADER_LEN + 100], &image[..]);
        // Remainder is zero padding.
        assert!(p[IMAGE_HEADER_LEN + 100..].iter().all(|&b| b == 0));
    }

    #[test]
    fn large_image_splits_into_paged_packets() {
        // 2.5 payloads worth of data -> 3 packets.
        let len = IMAGE_PAYLOAD_LEN * 2 + 10;
        let image: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let packets = image_packets(0, &image);

        assert_eq!(packets.len(), 3);

        // First two packets are full and not flagged last.
        for (page, packet) in packets.iter().take(2).enumerate() {
            assert_eq!(packet[3], 0, "page {page} should not be last");
            let this_len = u16::from_le_bytes([packet[4], packet[5]]) as usize;
            assert_eq!(this_len, IMAGE_PAYLOAD_LEN);
            assert_eq!(u16::from_le_bytes([packet[6], packet[7]]), page as u16);
        }

        // Last packet carries the remainder and is flagged.
        let last = &packets[2];
        assert_eq!(last[3], 1);
        assert_eq!(u16::from_le_bytes([last[4], last[5]]) as usize, 10);
        assert_eq!(u16::from_le_bytes([last[6], last[7]]), 2);

        // Reassembling the payloads reproduces the original image.
        let mut reassembled = Vec::new();
        for packet in &packets {
            let n = u16::from_le_bytes([packet[4], packet[5]]) as usize;
            reassembled.extend_from_slice(&packet[IMAGE_HEADER_LEN..IMAGE_HEADER_LEN + n]);
        }
        assert_eq!(reassembled, image);
    }

    #[test]
    fn key_states_parse_from_offset_four() {
        // report id, 3 header bytes, then 15 key states.
        let mut report = vec![0x01, 0, 0, 0];
        report.extend_from_slice(&[
            0, 1, 0, 0, 0, // keys 0..4 (key 1 pressed)
            0, 0, 0, 0, 0, // keys 5..9
            0, 0, 1, 0, 0, // keys 10..14 (key 12 pressed)
        ]);

        let states = parse_key_states(&report, 15);
        assert_eq!(states.len(), 15);
        assert!(states[1]);
        assert!(states[12]);
        assert_eq!(states.iter().filter(|&&s| s).count(), 2);
    }

    #[test]
    fn short_report_yields_all_released() {
        let states = parse_key_states(&[0x01, 0, 0, 0], 15);
        assert_eq!(states.len(), 15);
        assert!(states.iter().all(|&s| !s));
    }
}
