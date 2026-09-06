//! Original B / warm resonance cues, embedded as headerless mono PCM.
//! Conversion to playback buffers happens once on the audio worker; no file
//! access, audio decoder, or synthesis is needed at runtime.

pub(super) const SAMPLE_RATE: u32 = 48_000;
pub(super) const TONE_COUNT: usize = 6;

#[derive(Clone, Copy)]
pub(super) enum Tone {
    Tap,
    Arrival,
    Start,
    Success,
    Failure,
    Request,
}

impl Tone {
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn samples(self) -> Vec<f32> {
        self.pcm()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|bytes| f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32768.0)
            .collect()
    }

    pub(super) fn pcm(self) -> &'static [u8] {
        match self {
            Self::Tap => include_bytes!("../../assets/sounds/tap.pcm"),
            Self::Arrival => include_bytes!("../../assets/sounds/arrival.pcm"),
            Self::Start => include_bytes!("../../assets/sounds/start.pcm"),
            Self::Success => include_bytes!("../../assets/sounds/success.pcm"),
            Self::Failure => include_bytes!("../../assets/sounds/failure.pcm"),
            Self::Request => include_bytes!("../../assets/sounds/request.pcm"),
        }
    }
}

pub(super) fn bank() -> [Vec<f32>; TONE_COUNT] {
    [
        Tone::Tap,
        Tone::Arrival,
        Tone::Start,
        Tone::Success,
        Tone::Failure,
        Tone::Request,
    ]
    .map(Tone::samples)
}
