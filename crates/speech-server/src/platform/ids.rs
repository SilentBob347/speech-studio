pub(crate) fn new_id() -> String {
    let mut id = uuid::Uuid::new_v4().simple().to_string();
    id.truncate(10);
    id
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn generated_ids_keep_the_ten_lowercase_hex_character_format() {
        let ids: HashSet<_> = (0..128).map(|_| new_id()).collect();
        assert_eq!(ids.len(), 128);
        for id in ids {
            assert_eq!(id.len(), 10);
            assert!(id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        }
    }
}
