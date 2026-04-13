//! PublicKey and Fingerprint (first 6 bytes of SHA-256(pubkey)).

use std::fmt;

use sha2::{Digest, Sha256};

use crate::emojilist::EMOJI;
use crate::wordlist::WORDS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicKey(pub [u8; 32]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub [u8; 6]);

impl PublicKey {
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let bytes = hex::decode(s).ok()?;
        if bytes.len() != 32 {
            return None;
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        Some(PublicKey(out))
    }

    pub fn fingerprint(&self) -> Fingerprint {
        let hash = Sha256::digest(self.0);
        let mut fp = [0u8; 6];
        fp.copy_from_slice(&hash[..6]);
        Fingerprint(fp)
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl Fingerprint {
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Three BIP-39 words derived from bytes [0..2], [2..4], [4..6] taken as
    /// little-endian `u16` values, each modulo 2048.
    pub fn to_words(&self) -> [&'static str; 3] {
        let a = u16::from_le_bytes([self.0[0], self.0[1]]) as usize % 2048;
        let b = u16::from_le_bytes([self.0[2], self.0[3]]) as usize % 2048;
        let c = u16::from_le_bytes([self.0[4], self.0[5]]) as usize % 2048;
        [WORDS[a], WORDS[b], WORDS[c]]
    }

    /// Six emoji, indexed by each byte.
    pub fn to_emoji(&self) -> [&'static str; 6] {
        [
            EMOJI[self.0[0] as usize],
            EMOJI[self.0[1] as usize],
            EMOJI[self.0[2] as usize],
            EMOJI[self.0[3] as usize],
            EMOJI[self.0[4] as usize],
            EMOJI[self.0[5] as usize],
        ]
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let w = self.to_words();
        write!(f, "{}-{}-{}", w[0], w[1], w[2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZEROS_PK: PublicKey = PublicKey([0u8; 32]);

    #[test]
    fn fingerprint_is_deterministic() {
        let a = ZEROS_PK.fingerprint();
        let b = ZEROS_PK.fingerprint();
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_of_zero_pubkey_is_known() {
        // SHA-256 of 32 zero bytes = 66687aadf862bd776c8fc18b8e9f8e20...
        // First 6 bytes:
        let fp = ZEROS_PK.fingerprint();
        assert_eq!(fp.0, [0x66, 0x68, 0x7a, 0xad, 0xf8, 0x62]);
    }

    #[test]
    fn words_are_from_wordlist() {
        let fp = ZEROS_PK.fingerprint();
        let words = fp.to_words();
        assert!(super::WORDS.contains(&words[0]));
        assert!(super::WORDS.contains(&words[1]));
        assert!(super::WORDS.contains(&words[2]));
    }

    #[test]
    fn emoji_six_entries() {
        let fp = ZEROS_PK.fingerprint();
        let e = fp.to_emoji();
        assert_eq!(e.len(), 6);
    }

    #[test]
    fn display_is_three_dash_separated_words() {
        let fp = ZEROS_PK.fingerprint();
        let s = fp.to_string();
        assert_eq!(s.matches('-').count(), 2);
    }

    #[test]
    fn hex_round_trip() {
        let fp = ZEROS_PK.fingerprint();
        let hex = fp.to_hex();
        assert_eq!(hex.len(), 12);
        let pk_hex = ZEROS_PK.to_hex();
        assert_eq!(pk_hex.len(), 64);
        assert_eq!(PublicKey::from_hex(&pk_hex).unwrap(), ZEROS_PK);
    }
}
