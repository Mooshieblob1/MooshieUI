// Ported from Adobe's TrustMark Rust crate (trustmark 0.4.0, `src/bits.rs`,
// MIT License, Copyright (c) 2024 Adobe, https://github.com/adobe/trustmark).
// The ONNX tensor conversions are dropped (the caller builds plain `f32`
// buffers); the bit-string logic and its tests are unchanged.

//! TrustMark's 100-bit payload: data bits, BCH error-correction bits and a
//! 4-bit schema version, in that order.

use super::bch;

const VERSION_BITS: u16 = 4;

/// Total payload length the models read and write.
pub const PAYLOAD_BITS: usize = 100;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// Only '0' and '1' are allowed in a bit string.
    InvalidChar,
    /// More data bits than the version carries.
    InvalidDataLength { version_allows: usize, bits: usize },
    /// A raw payload must be exactly 100 bits.
    InvalidLength,
    /// No watermark, or too many bit flips to correct.
    CorruptWatermark,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidChar => write!(f, "allowed chars are '0' and '1'"),
            Error::InvalidDataLength {
                version_allows,
                bits,
            } => write!(
                f,
                "input bitstring ({bits} bits) has more bits than version allows ({version_allows} bits)"
            ),
            Error::InvalidLength => write!(f, "must be of length 100"),
            Error::CorruptWatermark => write!(f, "corrupt watermark"),
        }
    }
}

#[derive(Debug)]
pub(super) struct Bits(String);

impl Bits {
    /// Add error correction and the schema to `input` (a bit string).
    pub(super) fn apply_error_correction_and_schema(
        mut input: String,
        version: Version,
    ) -> Result<Self, Error> {
        let data_bits: usize = version.data_bits().into();

        if input.chars().any(|c| c != '0' && c != '1') {
            return Err(Error::InvalidChar);
        }
        if input.len() > data_bits {
            return Err(Error::InvalidDataLength {
                bits: input.len(),
                version_allows: data_bits,
            });
        }

        input.push_str(&"0".repeat(data_bits - input.len() + (8 - data_bits % 8)));
        let data = pack(&input);

        let mut ecc_state = bch::bch_init(version.allowed_bit_flips() as u32, bch::POLYNOMIAL);
        let ecc = bch::bch_encode(&mut ecc_state, &data);
        let mut error_correction = unpack(&ecc);

        input.truncate(data_bits);
        error_correction.truncate(version.ecc_bits().into());

        Ok(Self(format!(
            "{input}{error_correction}{}",
            version.bitstring()
        )))
    }

    /// The payload as model input: one 0.0/1.0 float per bit.
    pub(super) fn to_floats(&self) -> Vec<f32> {
        self.0
            .chars()
            .map(|c| if c == '1' { 1.0 } else { 0.0 })
            .collect()
    }

    /// Read a decoder output (one logit per bit; negative = 0), correcting
    /// bit flips where the error correction allows.
    pub(super) fn from_logits(logits: &[f32]) -> Result<Self, Error> {
        let s: String = logits
            .iter()
            .map(|v| if *v < 0. { '0' } else { '1' })
            .collect();
        Bits::new(s)
    }

    /// The data bits, without error correction or schema.
    pub(super) fn get_data(self) -> String {
        let version = self.get_version();
        let Self(mut s) = self;
        s.truncate(version.data_bits().into());
        s
    }

    fn get_version(&self) -> Version {
        match &self.0[98..100] {
            "00" => Version::BchSuper,
            "01" => Version::Bch5,
            "10" => Version::Bch4,
            _ => Version::Bch3,
        }
    }

    /// Check (and fix) bit flips with the error-correcting bits; as a last
    /// resort, try every version in case the version bits themselves flipped.
    fn new(s: String) -> Result<Self, Error> {
        if s.chars().any(|c| c != '0' && c != '1') {
            return Err(Error::InvalidChar);
        }
        if s.len() != PAYLOAD_BITS {
            return Err(Error::InvalidLength);
        }

        let version: Version = Version::from_bitstring(&s[96..]).unwrap_or_default();
        if let Ok(bits) = Bits::new_with_version(&s, version) {
            return Ok(bits);
        }
        let mut last = Err(Error::CorruptWatermark);
        for other in [
            Version::Bch3,
            Version::Bch4,
            Version::Bch5,
            Version::BchSuper,
        ] {
            if other == version {
                continue;
            }
            last = Bits::new_with_version(&s, other);
            if last.is_ok() {
                return last;
            }
        }
        last
    }

    fn new_with_version(s: &str, version: Version) -> Result<Self, Error> {
        let data_bits: usize = version.data_bits().into();
        let ecc_bits: usize = version.ecc_bits().into();

        let mut data = s[..data_bits].to_string();
        let mut ecc = s[data_bits..data_bits + ecc_bits].to_string();

        data.push_str(&"0".repeat(data_bits - data.len() + (8 - data_bits % 8)));
        ecc.push_str(&"0".repeat(ecc_bits - ecc.len() + (8 - ecc_bits % 8)));

        let mut data = pack(&data);
        let ecc = pack(&ecc);

        let mut ecc_state = bch::bch_init(version.allowed_bit_flips() as u32, bch::POLYNOMIAL);
        let bitflips = bch::bch_decode(&mut ecc_state, &mut data, &ecc);
        if bitflips > version.allowed_bit_flips() {
            return Err(Error::CorruptWatermark);
        }

        let mut data = unpack(&data);
        let mut ecc = unpack(&ecc);
        data.truncate(data_bits);
        ecc.truncate(ecc_bits);

        Ok(Bits(format!("{data}{ecc}{}", version.bitstring())))
    }
}

/// Pack a bit string whose length is a multiple of 8 into bytes.
fn pack(bits: &str) -> Vec<u8> {
    bits.as_bytes()
        .chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .fold(0u8, |acc, b| (acc << 1) | (*b == b'1') as u8)
        })
        .collect()
}

fn unpack(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:08b}")).collect()
}

/// The error correction schema.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub enum Version {
    /// Tolerates 8 bit flips, 40 data bits.
    #[default]
    BchSuper,
    /// Tolerates 5 bit flips, 61 data bits.
    Bch5,
    /// Tolerates 4 bit flips, 68 data bits.
    Bch4,
    /// Tolerates 3 bit flips, 75 data bits.
    Bch3,
}

impl Version {
    fn allowed_bit_flips(&self) -> u8 {
        match self {
            Version::BchSuper => 8,
            Version::Bch5 => 5,
            Version::Bch4 => 4,
            Version::Bch3 => 3,
        }
    }

    /// Number of data bits this version carries.
    pub fn data_bits(&self) -> u16 {
        match self {
            Version::BchSuper => 40,
            Version::Bch5 => 61,
            Version::Bch4 => 68,
            Version::Bch3 => 75,
        }
    }

    fn bitstring(&self) -> &'static str {
        match self {
            Version::BchSuper => "0000",
            Version::Bch5 => "0001",
            Version::Bch4 => "0010",
            Version::Bch3 => "0011",
        }
    }

    fn from_bitstring(s: &str) -> Result<Self, Error> {
        Ok(match s {
            "0000" => Version::BchSuper,
            "0001" => Version::Bch5,
            "0010" => Version::Bch4,
            "0011" => Version::Bch3,
            _ => return Err(Error::CorruptWatermark),
        })
    }

    fn ecc_bits(&self) -> u16 {
        PAYLOAD_BITS as u16 - VERSION_BITS - self.data_bits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENCODED: &str = "1011011110011000111111000000011111011111011100000110110110111000110010101101111010011011000010000001";
    const DATA: &str = "1011011110011000111111000000011111011111011100000110110110111";

    #[test]
    fn get_version() {
        assert_eq!(Bits(ENCODED.to_owned()).get_version(), Version::Bch5);
    }

    #[test]
    fn get_data() {
        assert_eq!(Bits(ENCODED.to_owned()).get_data(), DATA);
    }

    #[test]
    fn encode_matches_the_reference_payload() {
        let bits = Bits::apply_error_correction_and_schema(DATA.to_owned(), Version::Bch5).unwrap();
        assert_eq!(bits.0, ENCODED);
    }

    #[test]
    fn new() {
        assert_eq!(Bits::new(ENCODED.to_owned()).unwrap().get_data(), DATA);
    }

    #[test]
    fn fully_corrupted() {
        let input = "0000000000000000000000000000000000000000000100000110110110111000110010101101111010011011000010000001".to_owned();
        assert_eq!(Bits::new(input).unwrap_err(), Error::CorruptWatermark);
    }

    #[test]
    fn single_bitflip() {
        let input = "0011011110011000111111000000011111011111011100000110110110111000110010101101111010011011000010000001".to_owned();
        assert_eq!(Bits::new(input).unwrap().get_data(), DATA);
    }

    #[test]
    fn single_bitflip_and_corrupted_version() {
        let input = "0011011110011000111111000000011111011111011100000110110110111000110010101101111010011011000010000011".to_owned();
        assert_eq!(Bits::new(input).unwrap().get_data(), DATA);
    }

    #[test]
    fn five_bitflips_are_corrected() {
        let mut flipped: Vec<u8> = ENCODED.bytes().collect();
        for i in [0, 13, 27, 44, 60] {
            flipped[i] = if flipped[i] == b'1' { b'0' } else { b'1' };
        }
        let s = String::from_utf8(flipped).unwrap();
        assert_eq!(Bits::new(s).unwrap().get_data(), DATA);
    }

    #[test]
    fn invalid_bitstring() {
        let err = Bits::apply_error_correction_and_schema("hello".to_string(), Version::Bch5)
            .unwrap_err();
        assert_eq!(err, Error::InvalidChar);
    }

    #[test]
    fn too_long_input() {
        let err =
            Bits::apply_error_correction_and_schema("0".repeat(200), Version::Bch5).unwrap_err();
        assert!(matches!(err, Error::InvalidDataLength { .. }));
    }

    #[test]
    fn corrupt() {
        assert_eq!(
            Bits::new("1".repeat(100)).unwrap_err(),
            Error::CorruptWatermark
        );
    }

    #[test]
    fn logits_round_trip() {
        let bits = Bits::apply_error_correction_and_schema(DATA.to_owned(), Version::Bch5).unwrap();
        let logits: Vec<f32> = bits.to_floats().iter().map(|b| b * 2.0 - 1.0).collect();
        assert_eq!(Bits::from_logits(&logits).unwrap().get_data(), DATA);
    }
}
