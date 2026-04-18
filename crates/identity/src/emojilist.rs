//! 256 visually-distinct emoji, indexed by byte value 0..=255.

pub(crate) const EMOJI: &[&str; 256] = &include!("emojilist_data.rs");

#[cfg(test)]
mod tests {
    use super::EMOJI;
    use std::collections::HashSet;

    #[test]
    fn has_256_entries() {
        assert_eq!(EMOJI.len(), 256);
    }

    #[test]
    fn all_unique() {
        let set: HashSet<&&str> = EMOJI.iter().collect();
        assert_eq!(set.len(), 256, "emoji must be distinct");
    }

    #[test]
    fn all_nonempty() {
        for e in EMOJI.iter() {
            assert!(!e.is_empty());
        }
    }
}
