//! Turns what is sounding into spectrum bands and an oscilloscope trace and
//! streams them to the applet over a unix socket, about 30 times a second.
//!
//! Each frame is [`FRAME_LEN`] bytes: [`MAGIC`], [`BANDS`] levels (0–255, bass
//! first) and [`WAVE`] signed samples. The applet mirrors this in `src/feed.rs`.

use std::f32::consts::TAU;
use std::io::ErrorKind;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use tokio::net::{UnixListener, UnixStream};

use crate::paths;
use crate::sink::Tap;

pub const BANDS: usize = 32;
pub const WAVE: usize = 64;
pub const MAGIC: u8 = b'S';
pub const FRAME_LEN: usize = 1 + BANDS + WAVE;

const INTERVAL: Duration = Duration::from_millis(33);
const FFT_SIZE: usize = 2048;
const SAMPLE_RATE: f32 = 44_100.0;
const LOWEST_HZ: f32 = 40.0;
const HIGHEST_HZ: f32 = 16_000.0;
/// Music loses energy with pitch; this keeps the treble bars alive.
const TILT_DB_PER_OCTAVE: f32 = 3.0;
const RANGE_DB: f32 = 45.0;
/// Quietest reference, so silence and hiss stay flat instead of being boosted.
const FLOOR_DB: f32 = -40.0;
const REFERENCE_FALL_DB: f32 = 0.25;
/// About one Winamp oscilloscope frame.
const TRACE_SPAN: usize = 576;
const TRACE_PEAK_FALL: f32 = 0.97;

pub struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    window_gain: f32,
    samples: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    edges: [usize; BANDS + 1],
    tilt: [f32; BANDS],
    reference_db: f32,
    trace_peak: f32,
}

impl Analyzer {
    pub fn new() -> Self {
        #[allow(clippy::cast_precision_loss)]
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| 0.5 - 0.5 * (TAU * i as f32 / FFT_SIZE as f32).cos())
            .collect();
        let window_gain = window.iter().sum::<f32>() / 2.0;
        let (edges, tilt) = bands();
        Self {
            fft: FftPlanner::new().plan_fft_forward(FFT_SIZE),
            window,
            window_gain,
            samples: vec![0.0; FFT_SIZE],
            spectrum: vec![Complex::default(); FFT_SIZE],
            edges,
            tilt,
            reference_db: FLOOR_DB,
            trace_peak: 0.0,
        }
    }

    pub fn frame(&mut self, tap: &Tap, now: Instant) -> Option<[u8; FRAME_LEN]> {
        if !tap.audible(now, &mut self.samples) {
            return None;
        }
        let mut frame = [0; FRAME_LEN];
        frame[0] = MAGIC;
        self.spectrum_into(&mut frame[1..=BANDS]);
        self.trace_into(&mut frame[1 + BANDS..]);
        Some(frame)
    }

    fn spectrum_into(&mut self, out: &mut [u8]) {
        for ((bin, sample), weight) in self
            .spectrum
            .iter_mut()
            .zip(&self.samples)
            .zip(&self.window)
        {
            *bin = Complex::new(sample * weight, 0.0);
        }
        self.fft.process(&mut self.spectrum);

        let mut levels = [0.0_f32; BANDS];
        for (band, level) in levels.iter_mut().enumerate() {
            let peak = self.spectrum[self.edges[band]..self.edges[band + 1]]
                .iter()
                .map(|bin| bin.norm())
                .fold(0.0, f32::max);
            *level = 20.0 * (peak / self.window_gain + 1e-9).log10() + self.tilt[band];
        }
        let loudest = levels.iter().copied().fold(f32::MIN, f32::max);
        self.reference_db = (self.reference_db - REFERENCE_FALL_DB)
            .max(loudest)
            .max(FLOOR_DB);
        for (slot, level) in out.iter_mut().zip(levels) {
            let unit = ((level - self.reference_db + RANGE_DB) / RANGE_DB).clamp(0.0, 1.0);
            *slot = to_byte(unit);
        }
    }

    /// Starts on a rising zero crossing so the trace stands still on steady tones.
    fn trace_into(&mut self, out: &mut [u8]) {
        let search = FFT_SIZE - TRACE_SPAN;
        let start = (1..search)
            .rev()
            .find(|&i| self.samples[i - 1] <= 0.0 && self.samples[i] > 0.0)
            .unwrap_or(search);
        let span = &self.samples[start..start + TRACE_SPAN];
        let peak = span
            .iter()
            .fold(0.0_f32, |max, sample| max.max(sample.abs()));
        self.trace_peak = (self.trace_peak * TRACE_PEAK_FALL).max(peak).max(0.05);
        let step = TRACE_SPAN / out.len();
        for (slot, chunk) in out.iter_mut().zip(span.chunks_exact(step)) {
            #[allow(clippy::cast_precision_loss)]
            let mean = chunk.iter().sum::<f32>() / chunk.len() as f32;
            #[allow(clippy::cast_possible_truncation)]
            let signed = (mean / self.trace_peak * 127.0)
                .clamp(-127.0, 127.0)
                .round() as i8;
            *slot = signed.to_ne_bytes()[0];
        }
    }
}

fn to_byte(unit: f32) -> u8 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = (unit * 255.0).round() as u8;
    byte
}

/// Log-spaced FFT bin ranges, each at least one bin wide, and their tilt.
fn bands() -> ([usize; BANDS + 1], [f32; BANDS]) {
    #[allow(clippy::cast_precision_loss)]
    let bin_hz = SAMPLE_RATE / FFT_SIZE as f32;
    let ratio = HIGHEST_HZ / LOWEST_HZ;
    let mut edges = [0; BANDS + 1];
    let mut tilt = [0.0; BANDS];
    for (i, edge) in edges.iter_mut().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let hz = LOWEST_HZ * ratio.powf(i as f32 / BANDS as f32);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            *edge = (hz / bin_hz).round() as usize;
        }
    }
    for i in 1..=BANDS {
        edges[i] = edges[i].max(edges[i - 1] + 1);
    }
    for (band, gain) in tilt.iter_mut().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let center = (edges[band] + edges[band + 1]) as f32 / 2.0 * bin_hz;
        *gain = TILT_DB_PER_OCTAVE * (center / 1000.0).log2();
    }
    (edges, tilt)
}

/// Serves frames until the receiver exits; clients come and go freely.
pub async fn serve(tap: Arc<Tap>) {
    let path = paths::scope_socket();
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) => {
            log::warn!("scope socket unavailable at {}: {error}", path.display());
            return;
        }
    };
    let mut clients: Vec<UnixStream> = Vec::new();
    let mut analyzer = Analyzer::new();
    let mut tick = tokio::time::interval(INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                if let Ok((client, _)) = accepted {
                    clients.push(client);
                }
            }
            _ = tick.tick() => {
                if clients.is_empty() {
                    continue;
                }
                let Some(frame) = analyzer.frame(&tap, Instant::now()) else {
                    continue;
                };
                // A partial write would break the framing, so that client
                // is dropped and reconnects.
                clients.retain(|client| match client.try_write(&frame) {
                    Ok(written) => written == frame.len(),
                    Err(error) => error.kind() == ErrorKind::WouldBlock,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_are_ordered_and_in_range() {
        let (edges, _) = bands();
        assert!(edges.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(edges[BANDS] <= FFT_SIZE / 2);
    }

    #[test]
    fn a_tone_lights_its_own_band() {
        let (edges, _) = bands();
        #[allow(clippy::cast_precision_loss)]
        let bin_hz = SAMPLE_RATE / FFT_SIZE as f32;
        let band = 20;
        #[allow(clippy::cast_precision_loss)]
        let hz = (edges[band] as f32 + 0.5) * bin_hz;

        let mut analyzer = Analyzer::new();
        #[allow(clippy::cast_precision_loss)]
        for (i, sample) in analyzer.samples.iter_mut().enumerate() {
            *sample = 0.5 * (TAU * hz * i as f32 / SAMPLE_RATE).sin();
        }
        let mut levels = [0; BANDS];
        analyzer.spectrum_into(&mut levels);
        let loudest = levels
            .iter()
            .enumerate()
            .max_by_key(|(_, level)| **level)
            .map(|(index, _)| index);
        assert_eq!(loudest, Some(band));
        assert!(levels[2] < levels[band] / 2);
    }
}
