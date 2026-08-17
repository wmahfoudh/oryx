# palmbook against the mobi crate

Design notes recording where palmbook differs from the `mobi` crate
(0.8.0, the existing Rust reader) and why, kept as evidence for a
possible later publication. Measurements join as they are taken.

## Differences

- No dependencies. The mobi crate pulls `encoding`, `indexmap` and
  `thiserror`, with `chrono` behind a feature; palmbook decodes cp1252
  through a 32-entry table and needs nothing else.
- Zero-copy. palmbook borrows one byte slice and every record is a
  subslice; the mobi crate reads through an owned reader structure.
- Typed refusals. DRM surfaces as `Error::Drm` before any content is
  produced; truncation and corruption are distinct errors, and every
  offset is bounds-checked, verified by a walk over every prefix of a
  fixture. The mobi crate panics on some malformed inputs.
- KF8. palmbook reads the KF8 payload (skeleton and fragment
  reassembly, FDST parts, resources); the mobi crate stops at MOBI6.
- Round-trip tests. A writer in the test fixtures builds containers
  record by record, so both decompressors and every header field are
  pinned against hand-built bytes, not against sample files.

## Shared ground

- Both implement PalmDOC LZ77 and HuffCdic decompression and read EXTH
  metadata.
- The mobi crate reads more EXTH kinds into named accessors; palmbook
  exposes the raw record list and one string helper, and leaves
  interpretation to the caller.
