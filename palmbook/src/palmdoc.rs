//! PalmDOC LZ77 decompression: literals, short runs, an 11-bit-distance
//! backreference, and the space fold, per the MobileRead description.

use crate::Error;

/// Decompresses one record's body onto `out`. A backreference pointing
/// before the output that exists so far is corrupt; copies may overlap
/// themselves and extend byte by byte.
pub fn decompress(src: &[u8], out: &mut Vec<u8>) -> Result<(), Error> {
    let base = out.len();
    let mut i = 0;
    while i < src.len() {
        let byte = src[i];
        i += 1;
        match byte {
            0x00 => out.push(0),
            0x01..=0x08 => {
                let run = src
                    .get(i..i + byte as usize)
                    .ok_or(Error::Corrupt("a literal run ends early"))?;
                out.extend_from_slice(run);
                i += byte as usize;
            }
            0x09..=0x7F => out.push(byte),
            0x80..=0xBF => {
                let second = *src
                    .get(i)
                    .ok_or(Error::Corrupt("a backreference ends early"))?;
                i += 1;
                let pair = ((byte as usize & 0x3F) << 8) | second as usize;
                let distance = pair >> 3;
                let length = (pair & 7) + 3;
                if distance == 0 || distance > out.len() - base {
                    return Err(Error::Corrupt("a backreference points before the text"));
                }
                for _ in 0..length {
                    out.push(out[out.len() - distance]);
                }
            }
            0xC0..=0xFF => {
                out.push(b' ');
                out.push(byte ^ 0x80);
            }
        }
    }
    Ok(())
}
