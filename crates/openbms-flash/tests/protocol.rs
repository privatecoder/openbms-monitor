use openbms_flash::{boot, ehex};

fn record(kind: u8, addr: u16, data: &[u8], header: bool) -> String {
    let mut b = vec![data.len() as u8, (addr >> 8) as u8, addr as u8, kind];
    b.extend_from_slice(data);
    let checksum = if header {
        0xaa
    } else {
        (0u8).wrapping_sub(b.iter().fold(0u8, |s, x| s.wrapping_add(*x)))
    };
    b.push(checksum);
    format!(
        ":{}",
        b.iter().map(|x| format!("{x:02X}")).collect::<String>()
    )
}

fn image() -> Vec<u8> {
    let mut lines = Vec::new();
    for text in ["SWM181RCT6_64_M0", "EMU1101_10D_HW16", "RS485Y19200N9600"] {
        lines.push(record(0, 0, text.as_bytes(), true));
    }
    for _ in 3..16 {
        lines.push(record(0, 0, &[0x55; 16], true));
    }
    let mut code = [0u8; 16];
    code[0..4].copy_from_slice(&0x2000_1000u32.to_le_bytes());
    code[4..8].copy_from_slice(&0x0000_00c1u32.to_le_bytes());
    lines.push(record(0, 0, &code, false));
    lines.push(record(5, 0, &[0, 0, 0, 0xc1], false));
    lines.push(record(1, 0, &[], false));
    lines.join("\n").into_bytes()
}

#[test]
fn boot_vectors_and_rejection() {
    let request = boot::encode(b'R', b'L', 0, &[]).unwrap();
    assert_eq!(
        request,
        hex::decode("3A3A3A3A52464C00000000FF1C0D0D0D0D").unwrap()
    );
    let reply =
        boot::decode(&hex::decode("3A3A3A3A524642000200000100FF230D0D0D0D").unwrap()).unwrap();
    assert_eq!(reply.data, vec![1, 0]);
    let mut bad = request;
    bad[11] ^= 1;
    assert!(boot::decode(&bad).is_err());
}

#[test]
fn ehex_stream_includes_header_and_start_record() {
    let image = ehex::parse(&image()).unwrap();
    assert_eq!(image.header.raw.len(), 256);
    assert_eq!(image.code.len(), 16);
    assert_eq!(image.stream().len(), 276);
    assert_eq!(&image.stream()[272..], &[0, 0, 0, 0xc1]);
}

#[test]
fn ehex_rejects_gap_and_bad_checksum() {
    let text = String::from_utf8(image()).unwrap();
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let code = hex::decode(&lines[16][9..41]).unwrap();
    lines[16] = record(0, 16, &code, false);
    assert!(ehex::parse(lines.join("\n").as_bytes()).is_err());
    let bad = text.replacen(":04000005000000C136", ":04000005000000C137", 1);
    assert!(ehex::parse(bad.as_bytes()).is_err());
}

#[test]
fn ehex_rejects_changed_header_padding_and_start_record() {
    let text = String::from_utf8(image()).unwrap();
    let changed_header = text.replacen(
        &record(0, 0, &[0x55; 16], true),
        &record(0, 0, &[0x56; 16], true),
        1,
    );
    assert!(ehex::parse(changed_header.as_bytes()).is_err());
    let changed_start = text.replacen(":04000005000000C136", ":04000005000000C235", 1);
    assert!(ehex::parse(changed_start.as_bytes()).is_err());
}

#[test]
fn boot_rejects_unverified_high_checksum_sum() {
    assert!(boot::encode(b'W', b'D', 0xffff, &[0xff; 256]).is_err());
}

#[test]
fn ehex_rejects_address_extension_and_missing_eof() {
    let text = String::from_utf8(image()).unwrap();
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    lines.insert(16, record(4, 0, &[0, 1], false));
    assert!(ehex::parse(lines.join("\n").as_bytes()).is_err());
    let no_eof = text
        .lines()
        .take(text.lines().count() - 1)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(ehex::parse(no_eof.as_bytes()).is_err());
}
