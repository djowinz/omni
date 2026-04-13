//! BIP-39 English wordlist (2048 words).

pub(crate) const WORDS: &[&str; 2048] = &include!("wordlist_data.rs");

#[cfg(test)]
mod tests {
    use super::WORDS;

    #[test]
    fn has_2048_entries() {
        assert_eq!(WORDS.len(), 2048);
    }

    #[test]
    fn first_and_last_match_bip39() {
        assert_eq!(WORDS[0], "abandon");
        assert_eq!(WORDS[2047], "zoo");
    }

    #[test]
    fn all_lowercase_ascii() {
        for w in WORDS.iter() {
            assert!(
                w.chars().all(|c| c.is_ascii_lowercase()),
                "{w} not ascii-lower"
            );
        }
    }
}
