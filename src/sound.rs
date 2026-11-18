//! The short confirmation tones played when capture is switched on or off.
//!
//! They are tiny WAV files built in memory and played with `PlaySound`, which goes through the
//! normal Windows audio mixer, so they play *alongside* game and system audio like any
//! notification sound. (The old `Beep` call grabbed the sound device directly and was the prime
//! suspect for game/system audio cutting out.)

use std::sync::OnceLock;

const RATE: u32 = 44_100;

/// A mono 16-bit PCM WAV containing a sine tone with short fades (no clicks).
pub fn tone_wav(freq: f64, ms: u32, volume: f64) -> Vec<u8> {
    let samples = (RATE as u64 * ms as u64 / 1000) as usize;
    let fade = (RATE as usize / 200).min(samples / 2).max(1); // 5 ms
    let amp = volume.clamp(0.0, 1.0) * f64::from(i16::MAX);
    let data_len = (samples * 2) as u32;
    let mut w = Vec::with_capacity(44 + samples * 2);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&RATE.to_le_bytes());
    w.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    w.extend_from_slice(&2u16.to_le_bytes()); // block align
    w.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..samples {
        let env = (i.min(samples - 1 - i) as f64 / fade as f64).min(1.0);
        let t = i as f64 / f64::from(RATE);
        let v = (2.0 * std::f64::consts::PI * freq * t).sin() * amp * env;
        w.extend_from_slice(&(v.round() as i16).to_le_bytes());
    }
    w
}

static TONES: OnceLock<[Vec<u8>; 2]> = OnceLock::new();

/// Plays the "capture on" (higher) or "capture off" (lower) tone without waiting for it.
pub fn play_toggle(on: bool) {
    if !cfg!(windows) {
        return;
    }
    // Kept for the life of the program: an asynchronous PlaySound reads the buffer while playing.
    let tones = TONES.get_or_init(|| [tone_wav(440.0, 70, 0.25), tone_wav(880.0, 70, 0.25)]);
    let wav = &tones[usize::from(on)];
    unsafe {
        crate::sys::PlaySoundW(
            wav.as_ptr().cast(),
            std::ptr::null_mut(),
            crate::sys::SND_ASYNC | crate::sys::SND_MEMORY | crate::sys::SND_NODEFAULT,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_valid_pcm() {
        let w = tone_wav(880.0, 70, 0.25);
        let samples = (RATE * 70 / 1000) as usize;
        assert_eq!(w.len(), 44 + samples * 2);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes(w[4..8].try_into().unwrap()) as usize,
            w.len() - 8
        );
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([w[20], w[21]]), 1, "PCM");
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), RATE);
        assert_eq!(&w[36..40], b"data");
        assert_eq!(
            u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize,
            samples * 2
        );
    }

    #[test]
    fn tone_is_quiet_and_starts_and_ends_silent() {
        let w = tone_wav(440.0, 70, 0.25);
        let pcm: Vec<i16> = w[44..]
            .chunks(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(
            peak <= (0.25 * f64::from(i16::MAX)) as u16 + 1,
            "peak {peak}"
        );
        assert!(peak > 1000, "audible");
        assert_eq!(pcm[0], 0);
        assert!(
            pcm.last().unwrap().unsigned_abs() < 200,
            "fades out without a click"
        );
    }
}
