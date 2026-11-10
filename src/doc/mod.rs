pub mod comic;
pub mod count;
pub mod epub;
pub mod fb2;
pub mod fetch;
pub mod html;
pub mod images;
pub mod kindle;
pub mod load;
pub mod markdown;
pub mod model;
pub mod stream;

/// Reads an archive's entry to its end, at most `ceiling` bytes: None
/// when it holds more. The size an entry's header declares is not the
/// size it unpacks to, and a reader bounded by the compressed bytes
/// alone can unpack a thousand times as much.
pub(crate) fn read_capped(
    reader: impl std::io::Read,
    ceiling: u64,
) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    reader
        .take(ceiling.saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= ceiling).then_some(bytes))
}

#[cfg(test)]
mod tests {
    use super::read_capped;

    #[test]
    fn an_entry_larger_than_the_ceiling_is_not_read_whole() {
        let data = vec![7u8; 1000];
        assert_eq!(read_capped(&data[..], 1000).unwrap(), Some(data.clone()));
        assert_eq!(read_capped(&data[..], 999).unwrap(), None);
        assert_eq!(read_capped(&data[..0], 0).unwrap(), Some(Vec::new()));
    }
}
