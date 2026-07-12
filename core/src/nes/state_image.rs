use crate::nes::{FrameCheckpoint, FrameCheckpointError, MachineState, Nes};

const STATE_IMAGE_SIZE: usize = 32 * 1024;
const HEADER_SIZE: usize = 88;

const MAGIC: &[u8; 8] = b"SENSTATE";
const FORMAT_VERSION: u16 = 1;
const EMULATION_REVISION: u16 = 1;

const FORMAT_VERSION_OFFSET: usize = 8;
const EMULATION_REVISION_OFFSET: usize = 10;
const CARTRIDGE_ID_OFFSET: usize = 12;
const SAMPLE_RATE_OFFSET: usize = 44;
const PAYLOAD_LENGTH_OFFSET: usize = 52;
const CHECKSUM_OFFSET: usize = 56;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StateImageError {
    #[error("not at a frame boundary")]
    NotAtFrameBoundary,
    #[error("invalid buffer size: expected {expected}, got {actual}")]
    InvalidBufferSize { expected: usize, actual: usize },
    #[error("invalid state-image magic")]
    InvalidMagic,
    #[error("unsupported state-image format version {0}")]
    UnsupportedFormatVersion(u16),
    #[error("unsupported emulation revision {0}")]
    UnsupportedEmulationRevision(u16),
    #[error("state image belongs to an incompatible machine")]
    IncompatibleMachine,
    #[error("invalid payload length {0}")]
    InvalidPayloadLength(u32),
    #[error("state-image checksum mismatch")]
    ChecksumMismatch,
    #[error("machine state does not fit in the state image")]
    PayloadTooLarge,
    #[error("machine-state decoding failed")]
    DecodingFailed,
    #[error("machine-state payload contains trailing bytes")]
    TrailingPayload,
}

fn codec_config() -> impl bincode::config::Config {
    bincode::config::standard()
        .with_fixed_int_encoding()
        .with_little_endian()
        .with_limit::<STATE_IMAGE_SIZE>()
}

fn checksum(image: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&image[..CHECKSUM_OFFSET]);
    hasher.update(&image[HEADER_SIZE..]);
    *hasher.finalize().as_bytes()
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        bytes[offset..offset + 2]
            .try_into()
            .expect("fixed state-image field"),
    )
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("fixed state-image field"),
    )
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("fixed state-image field"),
    )
}

impl Nes {
    pub fn serialized_state_size(&self) -> usize {
        STATE_IMAGE_SIZE
    }

    pub fn serialize_state(&self, output: &mut [u8]) -> Result<(), StateImageError> {
        if output.len() != STATE_IMAGE_SIZE {
            return Err(StateImageError::InvalidBufferSize {
                expected: STATE_IMAGE_SIZE,
                actual: output.len(),
            });
        }

        let checkpoint = self
            .capture_frame_checkpoint()
            .map_err(|error| match error {
                FrameCheckpointError::NotAtFrameBoundary => StateImageError::NotAtFrameBoundary,
                FrameCheckpointError::IncompatibleMachine => {
                    unreachable!("capture cannot be incompatible")
                }
            })?;

        output.fill(0);

        output[..8].copy_from_slice(MAGIC);
        output[FORMAT_VERSION_OFFSET..FORMAT_VERSION_OFFSET + 2]
            .copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        output[EMULATION_REVISION_OFFSET..EMULATION_REVISION_OFFSET + 2]
            .copy_from_slice(&EMULATION_REVISION.to_le_bytes());
        output[CARTRIDGE_ID_OFFSET..CARTRIDGE_ID_OFFSET + 32]
            .copy_from_slice(checkpoint.compatibility.cartridge.as_bytes());
        output[SAMPLE_RATE_OFFSET..SAMPLE_RATE_OFFSET + 8]
            .copy_from_slice(&checkpoint.compatibility.sample_rate_bits.to_le_bytes());

        let payload_len = bincode::encode_into_slice(
            &checkpoint.state,
            &mut output[HEADER_SIZE..],
            codec_config(),
        )
        .map_err(|_| StateImageError::PayloadTooLarge)?;

        let payload_len =
            u32::try_from(payload_len).map_err(|_| StateImageError::PayloadTooLarge)?;

        output[PAYLOAD_LENGTH_OFFSET..PAYLOAD_LENGTH_OFFSET + 4]
            .copy_from_slice(&payload_len.to_le_bytes());

        let checksum = checksum(output);
        output[CHECKSUM_OFFSET..HEADER_SIZE].copy_from_slice(&checksum);

        Ok(())
    }

    pub fn unserialize_state(&mut self, input: &[u8]) -> Result<(), StateImageError> {
        if input.len() != STATE_IMAGE_SIZE {
            return Err(StateImageError::InvalidBufferSize {
                expected: STATE_IMAGE_SIZE,
                actual: input.len(),
            });
        }

        if &input[..8] != MAGIC {
            return Err(StateImageError::InvalidMagic);
        }

        let format_version = read_u16(input, FORMAT_VERSION_OFFSET);
        if format_version != FORMAT_VERSION {
            return Err(StateImageError::UnsupportedFormatVersion(format_version));
        }

        let emulation_revision = read_u16(input, EMULATION_REVISION_OFFSET);
        if emulation_revision != EMULATION_REVISION {
            return Err(StateImageError::UnsupportedEmulationRevision(
                emulation_revision,
            ));
        }

        let cartridge_id = &input[CARTRIDGE_ID_OFFSET..CARTRIDGE_ID_OFFSET + 32];
        let sample_rate_bits = read_u64(input, SAMPLE_RATE_OFFSET);

        if cartridge_id != self.compatibility.cartridge.as_bytes()
            || sample_rate_bits != self.compatibility.sample_rate_bits
        {
            return Err(StateImageError::IncompatibleMachine);
        }

        let payload_len = read_u32(input, PAYLOAD_LENGTH_OFFSET);

        let payload_len_usize = payload_len as usize;
        if payload_len_usize > STATE_IMAGE_SIZE - HEADER_SIZE {
            return Err(StateImageError::InvalidPayloadLength(payload_len));
        }

        let expected_checksum = checksum(input);
        if input[CHECKSUM_OFFSET..HEADER_SIZE] != expected_checksum {
            return Err(StateImageError::ChecksumMismatch);
        }

        let payload = &input[HEADER_SIZE..HEADER_SIZE + payload_len_usize];

        let (state, consumed): (MachineState, usize) =
            bincode::decode_from_slice(payload, codec_config())
                .map_err(|_| StateImageError::DecodingFailed)?;

        if consumed != payload.len() {
            return Err(StateImageError::TrailingPayload);
        }

        let checkpoint = FrameCheckpoint {
            compatibility: self.compatibility,
            state,
        };

        self.restore_frame_checkpoint(&checkpoint)
            .map_err(|_| StateImageError::IncompatibleMachine)
    }
}
