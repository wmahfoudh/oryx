# rarball against the rar crate

Design notes recording where rarball differs from the `rar` crate
(0.4.0, the existing Rust reader), kept as evidence for a possible
later publication. Measurements join as they are taken.

## Differences

- Both generations. rarball walks the RAR 1.5-to-4 header chain and
  RAR5's vint-framed blocks; the rar crate parses RAR5 only and
  recognizes the old signature without reading its archives.
- Honest extraction. The rar crate's unpack step is an open `// todo`
  in its extract loop, so a compressed entry writes its packed bytes
  to disk as if extracted; rarball extracts stored entries with the
  data CRC verified and refuses compressed ones in a typed error.
- No dependencies. The rar crate pulls nom, chrono, thiserror,
  lazy_static and an AES stack (aes, cbc, hmac, pbkdf2, sha2,
  generic-array); rarball needs nothing outside std, encryption being
  a refusal rather than a feature.
- Zero-copy. rarball borrows one byte slice and a stored extraction
  returns a verified subslice; the rar crate streams through reader
  adapters into files on disk.
- Typed refusals. Header encryption surfaces as `Error::Encrypted`
  before any entry is produced; truncation and corruption are distinct
  errors, every offset is bounds-checked, and a walk over every prefix
  of a fixture pins the no-panic promise. Header CRCs are verified on
  the walk, both the CRC16 of the old chain and RAR5's CRC32.
- Packed names. rarball decodes the RAR4 Unicode name packing (the
  high-byte page and two-bit opcodes); the rar crate reads RAR5's
  UTF-8 names only.

## Shared ground

- Both read RAR5 block headers, file metadata and stored data, and
  both detect the two signatures.
- The rar crate decrypts password-protected entries when given the
  password; rarball refuses encrypted archives and entries by design.
- Neither decompresses. The one complete, clean-licensed RAR
  decompression found in the wild is nwaples/rardecode (Go,
  BSD-2-Clause), the raw material if a decompressor is ever ported;
  unrar itself and everything derived from it carries the no-archiver
  license restriction that keeps it out of an MIT/Apache crate.

## Validation

- 19/08/2026, the library corpus: the four CBRs (RAR 1.5 era, stored)
  walk and extract with every CRC verifying; one book compared file
  by file against unrar 7.23's extraction, 67 files byte-identical.
  Three mixed archives (RAR 2.9 compression, up to 109,124 entries)
  walk in 48ms with stored entries verified and the entry accounting
  matching unrar's listing exactly.
