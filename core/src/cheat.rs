use std::str::FromStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameGenieCode {
    address: u16,
    replacement: u8,
    compare: Option<u8>,
}

impl GameGenieCode {
    pub const fn address(self) -> u16 {
        self.address
    }

    pub const fn replacement(self) -> u8 {
        self.replacement
    }

    pub const fn compare(self) -> Option<u8> {
        self.compare
    }

    pub fn apply(self, address: u16, original: u8) -> Option<u8> {
        if address != self.address {
            return None;
        }

        if self.compare.is_some_and(|compare| compare != original) {
            return None;
        }

        Some(self.replacement)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GameGenieCodeError {
    #[error("Game Genie codes must contain 6 or 8 letters, got {0}")]
    InvalidLength(usize),
    #[error("invalid Game Genie character {character:?} at position {position}")]
    InvalidCharacter { position: usize, character: char },
}

impl FromStr for GameGenieCode {
    type Err = GameGenieCodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let symbols: Vec<char> = s
            .chars()
            .filter(|ch| *ch != '-' && !ch.is_ascii_whitespace())
            .collect();

        let length = symbols.len();

        if !matches!(length, 6 | 8) {
            return Err(GameGenieCodeError::InvalidLength(length));
        }

        let mut nibbles = [0u8; 8];

        for (position, character) in symbols.into_iter().enumerate() {
            nibbles[position] =
                decode_character(character).ok_or(GameGenieCodeError::InvalidCharacter {
                    position,
                    character,
                })?
        }

        let n = nibbles;

        let address = 0x8000
            | ((n[3] & 7) as u16) << 12
            | ((n[5] & 7) as u16) << 8
            | ((n[4] & 8) as u16) << 8
            | ((n[2] & 7) as u16) << 4
            | ((n[1] & 8) as u16) << 4
            | (n[4] & 7) as u16
            | (n[3] & 8) as u16;

        let final_nibble = if length == 6 { n[5] } else { n[7] };

        let replacement = ((n[1] & 7) << 4) | ((n[0] & 8) << 4) | (n[0] & 7) | (final_nibble & 8);

        let compare =
            (length == 8).then(|| ((n[7] & 7) << 4) | ((n[6] & 8) << 4) | (n[6] & 7) | (n[5] & 8));

        Ok(Self {
            address,
            replacement,
            compare,
        })
    }
}

fn decode_character(character: char) -> Option<u8> {
    match character.to_ascii_uppercase() {
        'A' => Some(0x0),
        'P' => Some(0x1),
        'Z' => Some(0x2),
        'L' => Some(0x3),
        'G' => Some(0x4),
        'I' => Some(0x5),
        'T' => Some(0x6),
        'Y' => Some(0x7),
        'E' => Some(0x8),
        'O' => Some(0x9),
        'X' => Some(0xA),
        'U' => Some(0xB),
        'K' => Some(0xC),
        'S' => Some(0xD),
        'V' => Some(0xE),
        'N' => Some(0xF),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_six_character_code() {
        let code: GameGenieCode = "GOSSIP".parse().unwrap();

        assert_eq!(code.address(), 0xD1DD);
        assert_eq!(code.replacement(), 0x14);
        assert_eq!(code.compare(), None);
    }

    #[test]
    fn decodes_eight_character_code() {
        let code: GameGenieCode = "ZEXPYGLA".parse().unwrap();

        assert_eq!(code.address(), 0x94A7);
        assert_eq!(code.replacement(), 0x02);
        assert_eq!(code.compare(), Some(0x03));
    }

    #[test]
    fn comparison_controls_application() {
        let code: GameGenieCode = "ZEXPYGLA".parse().unwrap();

        assert_eq!(code.apply(0x94A7, 0x03), Some(0x02));
        assert_eq!(code.apply(0x94A7, 0x04), None);
        assert_eq!(code.apply(0x94A8, 0x03), None);
    }

    #[test]
    fn parsing_is_case_insensitive_and_ignores_display_separators() {
        assert_eq!(
            "gos-sip".parse::<GameGenieCode>(),
            "GOSSIP".parse::<GameGenieCode>()
        );
        assert_eq!(
            "zexp ygla".parse::<GameGenieCode>(),
            "ZEXPYGLA".parse::<GameGenieCode>()
        );
    }

    #[test]
    fn rejects_invalid_lengths_and_characters() {
        assert_eq!(
            "GOSSI".parse::<GameGenieCode>(),
            Err(GameGenieCodeError::InvalidLength(5))
        );
        assert_eq!(
            "GOSSIQ".parse::<GameGenieCode>(),
            Err(GameGenieCodeError::InvalidCharacter {
                position: 5,
                character: 'Q',
            })
        );
    }
}
