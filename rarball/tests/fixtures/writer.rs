//! Builds RAR containers in memory for the rarball tests, so every
//! fixture stays readable Rust instead of a binary file in the tree.
//! The layouts follow the rarlab format notes; RAR4 headers carry a
//! CRC16 (the low half of a CRC32), RAR5 headers a full CRC32 over the
//! size field and the header data. The writer keeps its own CRC so the
//! fixtures never depend on the code under test.
#![allow(dead_code)]

/// Standard CRC32, bitwise; fixture-sized inputs make speed irrelevant.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// One entry under construction.
pub struct FileSpec {
    pub name: String,
    pub data: Vec<u8>,
    /// 0 stored; 1 to 5 the compressed methods.
    pub method: u8,
    pub directory: bool,
    pub encrypted: bool,
    /// Overrides the declared data CRC, for corruption tests.
    pub declared_crc: Option<u32>,
    /// RAR4: write the 64-bit size fields.
    pub large: bool,
    /// RAR4: pack this Unicode name beside the plain one.
    pub unicode: Option<String>,
}

pub fn file(name: &str, data: &[u8]) -> FileSpec {
    FileSpec {
        name: name.to_string(),
        data: data.to_vec(),
        method: 0,
        directory: false,
        encrypted: false,
        declared_crc: None,
        large: false,
        unicode: None,
    }
}

pub fn directory(name: &str) -> FileSpec {
    FileSpec {
        directory: true,
        ..file(name, b"")
    }
}

/// A RAR4 block: the CRC16 seals the header from the type byte on.
fn rar4_block(head_type: u8, flags: u16, body: &[u8]) -> Vec<u8> {
    let head_size = (7 + body.len()) as u16;
    let mut block = vec![0, 0, head_type];
    block.extend_from_slice(&flags.to_le_bytes());
    block.extend_from_slice(&head_size.to_le_bytes());
    block.extend_from_slice(body);
    let crc = (crc32(&block[2..]) & 0xFFFF) as u16;
    block[0..2].copy_from_slice(&crc.to_le_bytes());
    block
}

/// The RAR4 Unicode name packing: a high-byte page, then two-bit
/// opcodes. Page zero with plain bytes (opcode 0) and full pairs
/// (opcode 2) covers every character the fixtures need.
pub fn pack_unicode(target: &str) -> Vec<u8> {
    let mut ops: Vec<(u8, Vec<u8>)> = Vec::new();
    for ch in target.chars() {
        let unit = ch as u32;
        if unit < 0x100 {
            ops.push((0, vec![unit as u8]));
        } else {
            ops.push((2, vec![(unit & 0xFF) as u8, (unit >> 8) as u8]));
        }
    }
    let mut out = vec![0u8];
    for chunk in ops.chunks(4) {
        let mut flags = 0u8;
        for (slot, (op, _)) in chunk.iter().enumerate() {
            flags |= op << (6 - 2 * slot as u8);
        }
        out.push(flags);
        for (_, bytes) in chunk {
            out.extend_from_slice(bytes);
        }
    }
    out
}

/// A RAR4 archive of the given entries. `main_flags` 0x0080 marks the
/// whole archive password-protected.
pub fn rar4(files: &[FileSpec], main_flags: u16) -> Vec<u8> {
    let mut out = b"Rar!\x1a\x07\x00".to_vec();
    out.extend(rar4_block(0x73, main_flags, &[0u8; 6]));
    for spec in files {
        let data = if spec.directory { &[][..] } else { &spec.data };
        let crc = spec.declared_crc.unwrap_or_else(|| crc32(data));
        let name_bytes = match &spec.unicode {
            Some(target) => {
                let mut n = spec.name.as_bytes().to_vec();
                n.push(0);
                n.extend(pack_unicode(target));
                n
            }
            None => spec.name.as_bytes().to_vec(),
        };
        let mut flags = 0x8000u16;
        if spec.directory {
            flags |= 0x00E0;
        }
        if spec.encrypted {
            flags |= 0x0004;
        }
        if spec.large {
            flags |= 0x0100;
        }
        if spec.unicode.is_some() {
            flags |= 0x0200;
        }
        let mut body = Vec::new();
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.push(2);
        body.extend_from_slice(&crc.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.push(if spec.method == 0 { 20 } else { 29 });
        body.push(0x30 + spec.method);
        body.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        if spec.large {
            body.extend_from_slice(&0u32.to_le_bytes());
            body.extend_from_slice(&0u32.to_le_bytes());
        }
        body.extend_from_slice(&name_bytes);
        out.extend(rar4_block(0x74, flags, &body));
        out.extend_from_slice(data);
    }
    out.extend(rar4_block(0x7B, 0, &[]));
    out
}

/// A one-entry RAR4 archive whose name field is written verbatim with
/// the Unicode flag set, for pinning the packed-name opcodes.
pub fn rar4_with_raw_name(name_bytes: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = b"Rar!\x1a\x07\x00".to_vec();
    out.extend(rar4_block(0x73, 0, &[0u8; 6]));
    let mut body = Vec::new();
    body.extend_from_slice(&(data.len() as u32).to_le_bytes());
    body.extend_from_slice(&(data.len() as u32).to_le_bytes());
    body.push(2);
    body.extend_from_slice(&crc32(data).to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.push(20);
    body.push(0x30);
    body.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(name_bytes);
    out.extend(rar4_block(0x74, 0x8000 | 0x0200, &body));
    out.extend_from_slice(data);
    out.extend(rar4_block(0x7B, 0, &[]));
    out
}

/// Little-endian base-128, the RAR5 vint.
pub fn vint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
    out
}

/// A RAR5 block from the bytes after the size field; the CRC32 seals
/// the size field and the header data together.
fn rar5_block(header: &[u8]) -> Vec<u8> {
    let size = vint(header.len() as u64);
    let mut sealed = size.clone();
    sealed.extend_from_slice(header);
    let crc = crc32(&sealed);
    let mut out = crc.to_le_bytes().to_vec();
    out.extend(sealed);
    out
}

/// A RAR5 archive of the given entries. An encrypted archive leads
/// with the encryption block, the whole-archive password form.
pub fn rar5(files: &[FileSpec], encrypted_archive: bool) -> Vec<u8> {
    let mut out = b"Rar!\x1a\x07\x01\x00".to_vec();
    if encrypted_archive {
        let mut h = Vec::new();
        h.extend(vint(4));
        h.extend(vint(0));
        h.extend(vint(0));
        h.extend(vint(0));
        out.extend(rar5_block(&h));
        return out;
    }
    let mut main = Vec::new();
    main.extend(vint(1));
    main.extend(vint(0));
    main.extend(vint(0));
    out.extend(rar5_block(&main));
    for spec in files {
        let data = if spec.directory { &[][..] } else { &spec.data };
        let crc = spec.declared_crc.unwrap_or_else(|| crc32(data));
        // A file-encryption record in the extra area marks the entry.
        let extra: Vec<u8> = if spec.encrypted {
            let mut record = vint(1);
            record.extend_from_slice(&[0, 0, 0]);
            let mut area = vint(record.len() as u64);
            area.extend(record);
            area
        } else {
            Vec::new()
        };
        let mut file_flags = 0u64;
        if spec.directory {
            file_flags |= 0x1;
        }
        if !spec.directory {
            file_flags |= 0x4;
        }
        let compression = (spec.method as u64) << 7;
        let mut h = Vec::new();
        h.extend(vint(2));
        let mut header_flags = 0u64;
        if !extra.is_empty() {
            header_flags |= 0x1;
        }
        if !data.is_empty() {
            header_flags |= 0x2;
        }
        h.extend(vint(header_flags));
        if !extra.is_empty() {
            h.extend(vint(extra.len() as u64));
        }
        if !data.is_empty() {
            h.extend(vint(data.len() as u64));
        }
        h.extend(vint(file_flags));
        h.extend(vint(data.len() as u64));
        h.extend(vint(0));
        if file_flags & 0x4 != 0 {
            h.extend_from_slice(&crc.to_le_bytes());
        }
        h.extend(vint(compression));
        h.extend(vint(1));
        h.extend(vint(spec.name.len() as u64));
        h.extend_from_slice(spec.name.as_bytes());
        h.extend_from_slice(&extra);
        out.extend(rar5_block(&h));
        out.extend_from_slice(data);
    }
    let mut end = Vec::new();
    end.extend(vint(5));
    end.extend(vint(0));
    end.extend(vint(0));
    out.extend(rar5_block(&end));
    out
}
