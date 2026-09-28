//! Live spectrum and oscilloscope frames from the local receiver.
//!
//! Frames arrive only while this computer is the one playing. A background
//! thread keeps the newest one, and the scopes read it while they draw, so 30
//! frames a second never go through the app's update loop.

use std::io::Read;
use std::os::unix::net::UnixStream;
use std::sync::{Mutex, Once, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use crate::player;

/// Mirrors `player/src/scope.rs`.
pub const BANDS: usize = 32;
pub const WAVE: usize = 64;
const MAGIC: u8 = b'S';
const FRAME_LEN: usize = 1 + BANDS + WAVE;

/// Longer than a few missed frames, short enough to hand back to the idle
/// animation soon after the music stops.
const STALE: Duration = Duration::from_millis(250);
const RETRY: Duration = Duration::from_secs(1);

#[derive(Clone, Copy)]
pub struct Frame {
    /// `0.0..=1.0`, bass first.
    pub bands: [f32; BANDS],
    /// `-1.0..=1.0`.
    pub wave: [f32; WAVE],
}

static LATEST: Mutex<Option<(Instant, Frame)>> = Mutex::new(None);
static START: Once = Once::new();

/// The frame sounding right now, if the receiver is playing.
pub fn latest() -> Option<Frame> {
    START.call_once(|| {
        let _ = thread::Builder::new()
            .name("scope-feed".into())
            .spawn(listen);
    });
    let latest = LATEST.lock().unwrap_or_else(PoisonError::into_inner);
    latest
        .filter(|(at, _)| at.elapsed() < STALE)
        .map(|(_, frame)| frame)
}

fn listen() {
    let mut buffer = [0; FRAME_LEN];
    loop {
        if let Ok(mut stream) = UnixStream::connect(player::scope_socket()) {
            while stream.read_exact(&mut buffer).is_ok() {
                let Some(frame) = parse(&buffer) else {
                    break;
                };
                *LATEST.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some((Instant::now(), frame));
            }
        }
        thread::sleep(RETRY);
    }
}

fn parse(bytes: &[u8; FRAME_LEN]) -> Option<Frame> {
    let (&magic, rest) = bytes.split_first()?;
    if magic != MAGIC {
        return None;
    }
    let (bands, wave) = rest.split_at(BANDS);
    let mut frame = Frame {
        bands: [0.0; BANDS],
        wave: [0.0; WAVE],
    };
    for (slot, byte) in frame.bands.iter_mut().zip(bands) {
        *slot = f32::from(*byte) / 255.0;
    }
    for (slot, byte) in frame.wave.iter_mut().zip(wave) {
        *slot = f32::from(i8::from_ne_bytes([*byte])) / 127.0;
    }
    Some(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_decode_to_unit_ranges() {
        let mut bytes = [0; FRAME_LEN];
        bytes[0] = MAGIC;
        bytes[1] = 255;
        bytes[1 + BANDS] = i8::MIN.to_ne_bytes()[0].wrapping_add(1);
        bytes[2 + BANDS] = 127;
        let frame = parse(&bytes).unwrap();
        assert!((frame.bands[0] - 1.0).abs() < f32::EPSILON);
        assert!((frame.wave[0] + 1.0).abs() < f32::EPSILON);
        assert!((frame.wave[1] - 1.0).abs() < f32::EPSILON);

        bytes[0] = 0;
        assert!(parse(&bytes).is_none());
    }
}
