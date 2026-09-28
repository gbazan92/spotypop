//! `PulseAudio` output (`PipeWire` serves it too) that also keeps a copy of what
//! is about to be heard, so the panel scopes can follow the music.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use libpulse_binding::def::BufferAttr;
use libpulse_binding::error::PAErr;
use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;
use librespot_playback::audio_backend::{Sink, SinkError, SinkResult};
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::{NUM_CHANNELS, SAMPLE_RATE};

/// Short enough that pausing feels immediate, long enough to ride out hiccups.
const TARGET_LATENCY: Duration = Duration::from_millis(200);
/// Must exceed the server latency, which may ignore [`TARGET_LATENCY`].
const TAP_CAPACITY: usize = 1 << 17;

/// Mono samples in playback order, plus when the newest one reaches the speakers.
#[derive(Default)]
pub struct Tap {
    state: Mutex<TapState>,
}

#[derive(Default)]
struct TapState {
    samples: VecDeque<f32>,
    heard_until: Option<Instant>,
}

impl Tap {
    fn push(&self, interleaved: &[f64], latency: Duration) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        #[allow(clippy::cast_possible_truncation)]
        state.samples.extend(
            interleaved
                .chunks_exact(usize::from(NUM_CHANNELS))
                .map(|frame| (frame.iter().sum::<f64>() / f64::from(NUM_CHANNELS)) as f32),
        );
        let excess = state.samples.len().saturating_sub(TAP_CAPACITY);
        state.samples.drain(..excess);
        state.heard_until = Some(Instant::now() + latency);
    }

    fn clear(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.samples.clear();
        state.heard_until = None;
    }

    /// Fills `out` with the samples sounding at `now`; false when nothing is.
    pub fn audible(&self, now: Instant, out: &mut [f32]) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(until) = state.heard_until.filter(|until| *until > now) else {
            return false;
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pending = ((until - now).as_secs_f64() * f64::from(SAMPLE_RATE)) as usize;
        let Some(end) = state.samples.len().checked_sub(pending) else {
            return false;
        };
        let Some(start) = end.checked_sub(out.len()) else {
            return false;
        };
        for (slot, sample) in out.iter_mut().zip(state.samples.range(start..end)) {
            *slot = *sample;
        }
        true
    }
}

pub struct PulseTap {
    stream: Option<Simple>,
    tap: Arc<Tap>,
    bytes: Vec<u8>,
    /// Last latency we asked `PulseAudio` for. Asking on every packet stalls playback.
    latency: Duration,
    latency_at: Option<Instant>,
}

impl PulseTap {
    pub fn new(tap: Arc<Tap>) -> Self {
        Self {
            stream: None,
            tap,
            bytes: Vec::new(),
            latency: TARGET_LATENCY,
            latency_at: None,
        }
    }

    /// `PipeWire` often reports a buffer much larger than the one we asked for.
    /// Chasing that number puts the scope a beat behind the speakers.
    fn refresh_latency(&mut self) -> Duration {
        let now = Instant::now();
        let fresh = self
            .latency_at
            .is_none_or(|at| now.saturating_duration_since(at) >= Duration::from_millis(250));
        if fresh {
            self.latency_at = Some(now);
            if let Some(stream) = self.stream.as_ref()
                && let Ok(reported) = stream.get_latency()
            {
                let reported = Duration::from_micros(reported.0);
                self.latency = reported.clamp(Duration::from_millis(40), TARGET_LATENCY);
            }
        }
        self.latency
    }
}

fn describe(error: PAErr) -> String {
    error
        .to_string()
        .unwrap_or_else(|| format!("PulseAudio error {}", error.0))
}

fn target_bytes() -> u32 {
    let per_second = SAMPLE_RATE * u32::from(NUM_CHANNELS) * 2;
    #[allow(clippy::cast_possible_truncation)]
    let bytes = (u128::from(per_second) * TARGET_LATENCY.as_millis() / 1000) as u32;
    bytes
}

impl Sink for PulseTap {
    fn start(&mut self) -> SinkResult<()> {
        if self.stream.is_some() {
            return Ok(());
        }
        let spec = Spec {
            format: Format::S16NE,
            channels: NUM_CHANNELS,
            rate: SAMPLE_RATE,
        };
        let buffer = BufferAttr {
            maxlength: u32::MAX,
            tlength: target_bytes(),
            prebuf: u32::MAX,
            minreq: u32::MAX,
            fragsize: u32::MAX,
        };
        let stream = Simple::new(
            None,
            "Spotify (COSMIC)",
            Direction::Playback,
            None,
            "Música",
            &spec,
            None,
            Some(&buffer),
        )
        .map_err(|error| SinkError::ConnectionRefused(describe(error)))?;
        self.stream = Some(stream);
        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        self.tap.clear();
        let Some(stream) = self.stream.take() else {
            return Ok(());
        };
        stream
            .drain()
            .map_err(|error| SinkError::StateChange(describe(error)))
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let AudioPacket::Samples(samples) = packet else {
            return Err(SinkError::InvalidParams(
                "raw audio is not supported".into(),
            ));
        };
        self.bytes.clear();
        self.bytes.extend(
            converter
                .f64_to_s16(&samples)
                .iter()
                .flat_map(|sample| sample.to_ne_bytes()),
        );
        let latency = self.refresh_latency();
        let stream = self
            .stream
            .as_ref()
            .ok_or_else(|| SinkError::NotConnected("the audio stream is closed".into()))?;
        stream
            .write(&self.bytes)
            .map_err(|error| SinkError::OnWrite(describe(error)))?;
        self.tap.push(&samples, latency);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audible_skips_what_is_still_buffered() {
        let tap = Tap::default();
        let second = usize::try_from(SAMPLE_RATE).unwrap();
        let ramp: Vec<f64> = (0..second * 2)
            .flat_map(|i| {
                let v = f64::from(u32::try_from(i).unwrap());
                [v, v]
            })
            .collect();
        tap.push(&ramp, Duration::from_millis(500));

        let mut out = [0.0; 4];
        assert!(tap.audible(Instant::now(), &mut out));
        let newest = out[3];
        let buffered = f64::from(u32::try_from(second * 2).unwrap()) - f64::from(newest);
        assert!(buffered > f64::from(SAMPLE_RATE) * 0.45);
        assert!(buffered < f64::from(SAMPLE_RATE) * 0.51);

        tap.clear();
        assert!(!tap.audible(Instant::now(), &mut out));
    }
}
