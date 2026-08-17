//! HuffCdic decompression: a canonical Huffman code over a phrase
//! dictionary, the HUFF record carrying the code tables and the CDIC
//! records the phrases. A phrase not marked final is itself a code
//! stream and expands recursively.

use crate::{be32, Error};

/// How deep phrase recursion may nest before the table reads as
/// corrupt; real books use one level.
const MAX_DEPTH: usize = 32;

pub struct HuffCdic {
    /// Per leading byte: code length in the low five bits, the terminal
    /// flag at 0x80, and the maxcode seed above.
    cache: [u32; 256],
    /// Per code length 1..=32, the smallest and largest code left-aligned
    /// in 32 bits; consulted when the cache entry is not terminal.
    mincode: [u32; 33],
    maxcode: [u32; 33],
    /// The phrases with their final flag.
    dictionary: Vec<(Vec<u8>, bool)>,
}

impl HuffCdic {
    pub fn new(huff: &[u8], cdics: &[&[u8]]) -> Result<HuffCdic, Error> {
        if huff.get(..4) != Some(b"HUFF") {
            return Err(Error::Corrupt("no HUFF magic"));
        }
        let cache_at = be32(huff, 8)? as usize;
        let base_at = be32(huff, 12)? as usize;
        let mut cache = [0u32; 256];
        for (index, entry) in cache.iter_mut().enumerate() {
            *entry = be32(huff, cache_at + index * 4)?;
        }
        let mut mincode = [0u32; 33];
        let mut maxcode = [0u32; 33];
        maxcode[0] = 0xFFFF_FFFF;
        for length in 1..=32usize {
            let min = be32(huff, base_at + (length - 1) * 8)? as u64;
            let max = be32(huff, base_at + (length - 1) * 8 + 4)? as u64;
            mincode[length] = (min << (32 - length)) as u32;
            maxcode[length] = (((max + 1) << (32 - length)) - 1) as u32;
        }

        let mut dictionary = Vec::new();
        for cdic in cdics {
            if cdic.get(..4) != Some(b"CDIC") {
                return Err(Error::Corrupt("no CDIC magic"));
            }
            let total = be32(cdic, 8)? as usize;
            let bits = be32(cdic, 12)?;
            if bits > 16 {
                return Err(Error::Corrupt("an oversized CDIC code length"));
            }
            let here = (1usize << bits).min(total - dictionary.len());
            for index in 0..here {
                let offset = crate::be16(cdic, 16 + index * 2)? as usize;
                let flagged = crate::be16(cdic, 16 + offset)? as usize;
                let length = flagged & 0x7FFF;
                let phrase = cdic
                    .get(18 + offset..18 + offset + length)
                    .ok_or(Error::Truncated)?;
                dictionary.push((phrase.to_vec(), flagged & 0x8000 != 0));
            }
        }
        Ok(HuffCdic {
            cache,
            mincode,
            maxcode,
            dictionary,
        })
    }

    /// Decodes one record's body into text.
    pub fn unpack(&self, data: &[u8]) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        self.unpack_into(data, &mut out, 0)?;
        Ok(out)
    }

    fn unpack_into(&self, data: &[u8], out: &mut Vec<u8>, depth: usize) -> Result<(), Error> {
        if depth > MAX_DEPTH {
            return Err(Error::Corrupt("phrase recursion runs away"));
        }
        let mut padded = data.to_vec();
        padded.extend_from_slice(&[0u8; 8]);
        let mut bits_left = (data.len() * 8) as i64;
        let mut pos = 0usize;
        let word = |at: usize| -> Result<u64, Error> {
            let slice = padded.get(at..at + 8).ok_or(Error::Truncated)?;
            Ok(u64::from_be_bytes(slice.try_into().expect("eight bytes")))
        };
        let mut x = word(pos)?;
        let mut n = 32i32;
        loop {
            if n <= 0 {
                pos += 4;
                x = word(pos)?;
                n += 32;
            }
            let code = ((x >> n) & 0xFFFF_FFFF) as u32;
            let entry = self.cache[(code >> 24) as usize];
            let mut length = (entry & 0x1F) as usize;
            if length == 0 {
                return Err(Error::Corrupt("a code with no length"));
            }
            let max = if entry & 0x80 != 0 {
                let seed = (entry >> 8) as u64;
                (((seed + 1) << (32 - length)) - 1) as u32
            } else {
                while length < 32 && code < self.mincode[length] {
                    length += 1;
                }
                self.maxcode[length]
            };
            n -= length as i32;
            bits_left -= length as i64;
            if bits_left < 0 {
                break;
            }
            let index = (max
                .checked_sub(code)
                .ok_or(Error::Corrupt("a code above its table"))?
                >> (32 - length)) as usize;
            let (phrase, done) = self
                .dictionary
                .get(index)
                .ok_or(Error::Corrupt("a code past the dictionary"))?;
            if *done {
                out.extend_from_slice(phrase);
            } else {
                let phrase = phrase.clone();
                self.unpack_into(&phrase, out, depth + 1)?;
            }
        }
        Ok(())
    }
}
