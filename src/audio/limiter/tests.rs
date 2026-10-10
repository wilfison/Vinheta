use std::f32::consts::TAU;
use std::time::Duration;

use super::Limiter;

const RATE: u32 = 48000;
// A WAV buffer at the end of the call branch: 40 ms.
const BUFFER: usize = 1920;

fn limiter() -> Limiter {
    Limiter::new(0.5, Duration::from_millis(200), RATE)
}

fn tone(amplitude: f32, length: usize) -> Vec<f32> {
    (0..length)
        .map(|i| amplitude * (TAU * 1000.0 * i as f32 / RATE as f32).sin())
        .collect()
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

#[test]
fn a_loud_buffer_is_scaled_to_the_ceiling() {
    let mut limiter = limiter();
    let input = tone(1.0, BUFFER);
    let mut output = input.clone();
    limiter.process(&mut output);
    assert!((peak(&output) - 0.5).abs() < 0.001, "{}", peak(&output));
    for (input, output) in input.iter().zip(&output) {
        assert_eq!(*output, input * limiter.gain());
    }
}

#[test]
fn a_buffer_under_the_ceiling_is_untouched() {
    let mut limiter = limiter();
    let input = tone(0.4, BUFFER);
    let mut output = input.clone();
    limiter.process(&mut output);
    assert_eq!(output, input);
    assert_eq!(limiter.gain(), 1.0);
}

#[test]
fn the_gain_recovers_with_the_release() {
    let mut limiter = limiter();
    limiter.process(&mut tone(1.0, BUFFER));
    let mut quiet = tone(0.1, BUFFER);
    limiter.process(&mut quiet);
    assert!(peak(&quiet) <= 0.1);
    let expected = 0.5 + 0.5 * (1.0 - (-0.04f32 / 0.2).exp());
    assert!(
        (limiter.gain() - expected).abs() < expected * 0.01,
        "{} instead of {expected}",
        limiter.gain()
    );
}

#[test]
fn a_second_of_silence_restores_the_gain() {
    let mut limiter = limiter();
    limiter.process(&mut tone(1.0, BUFFER));
    for _ in 0..25 {
        limiter.process(&mut [0.0; BUFFER]);
    }
    assert!(limiter.gain() > 0.99, "{}", limiter.gain());
}

#[test]
fn no_sample_ever_exceeds_the_ceiling() {
    let mut limiter = limiter();
    let mut state: u32 = 12345;
    let mut next = move || {
        state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (state >> 8) as f32 / (1 << 24) as f32
    };
    for _ in 0..200 {
        let length = 64 + (next() * (4096 - 64) as f32) as usize;
        let amplitude = next() * 1.2;
        let mut samples: Vec<f32> = (0..length)
            .map(|_| amplitude * (next() * 2.0 - 1.0))
            .collect();
        limiter.process(&mut samples);
        assert!(peak(&samples) <= 0.5 + 1e-6, "{}", peak(&samples));
    }
}

#[test]
fn reset_restores_the_gain() {
    let mut limiter = limiter();
    limiter.process(&mut tone(1.0, BUFFER));
    limiter.reset();
    assert_eq!(limiter.gain(), 1.0);
}

#[test]
fn bad_constants_are_clamped() {
    let mut limiter = Limiter::new(2.0, Duration::ZERO, RATE);
    let mut samples = [0.9, -0.9];
    limiter.process(&mut samples);
    assert_eq!(samples, [0.9, -0.9]);

    let mut limiter = Limiter::new(0.0, Duration::ZERO, 0);
    let mut samples = [1.0, -1.0];
    limiter.process(&mut samples);
    assert!(samples.iter().all(|sample| sample.is_finite()));
    limiter.process(&mut [0.0; 4]);
    assert!(limiter.gain().is_finite());
}
