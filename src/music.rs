//! Spokojna muzyka tła generowana proceduralnie — zero zewnętrznych plików.
//!
//! 16-sekundowa zapętlona fala: miękki pad (Am–F–C–G z gładkimi
//! przejściami) plus delikatne dźwięki pentatoniki. Wszystkie obwiednie
//! i LFO są okresowe względem długości pętli, więc zapętlenie nie klika.

use macroquad::audio::{self, PlaySoundParams, Sound};
use std::f32::consts::TAU;

const SAMPLE_RATE: u32 = 22_050;
const LOOP_SECS: f32 = 16.0;
const VOLUME: f32 = 0.30;
/// Szerokość okna akordu (4 s akordu + 1,5 s zakładki na wejście).
const CHORD_SPAN: f32 = 5.5;

/// Akordy pada: (start_s, [f1, f2, f3]). Progresja Am–F–C–G.
const CHORDS: [(f32, [f32; 3]); 4] = [
    (0.0, [110.00, 130.81, 164.81]),
    (4.0, [87.31, 110.00, 130.81]),
    (8.0, [98.00, 130.81, 164.81]),
    (12.0, [98.00, 123.47, 146.83]),
];

/// (start_s, częstotliwość). Ogony gasną na długo przed końcem pętli.
const PLUCKS: [(f32, f32); 8] = [
    (0.5, 440.00),
    (2.5, 523.25),
    (4.5, 587.33),
    (6.5, 659.25),
    (8.5, 783.99),
    (10.5, 659.25),
    (12.0, 587.33),
    (13.0, 523.25),
];

fn voice(phase_t: f32, freq: f32) -> f32 {
    let p = TAU * freq * phase_t;
    p.sin() + 0.25 * (2.0 * p).sin() + 0.10 * (3.0 * p + 0.5).sin()
}

/// Dostraja częstotliwość tak, by wykonała całkowitą liczbę cykli w pętli
/// (odchyłka rzędu tysięcznych herca, niesłyszalna), co domyka fazę na
/// szwie zapętlenia. Bez tego słychać ciche kliknięcie co 16 s.
fn snap(freq: f32) -> f32 {
    (freq * LOOP_SECS).round() / LOOP_SECS
}

/// Waga akordu w chwili `t`: podniesiony kosinus na oknie
/// [start − 1,5 s, start + 4 s) liczonym cyklicznie — zero na brzegach.
fn chord_weight(t: f32, start: f32) -> f32 {
    let u = (t - (start - 1.5)).rem_euclid(LOOP_SECS);
    if u < 0.0 || u >= CHORD_SPAN {
        return 0.0;
    }
    let s = (std::f32::consts::PI * u / CHORD_SPAN).sin();
    s * s
}

fn render_loop() -> Vec<f32> {
    let n = (SAMPLE_RATE as f32 * LOOP_SECS) as usize;
    let mut out = vec![0.0f32; n];
    for (i, sample) in out.iter_mut().enumerate() {
        let t = i as f32 / SAMPLE_RATE as f32;
        // Pad z powolnym tremolo (okres 8 s = 2 cykle na pętlę).
        let tremolo = 1.0 + 0.18 * (TAU * t / 8.0 + 1.0).sin();
        let mut pad = 0.0;
        for (start, freqs) in CHORDS {
            let w = chord_weight(t, start);
            if w > 0.0 {
                for f in freqs {
                    pad += w * voice(t, f);
                }
            }
        }
        // Delikatne dźwięki z wykładniczym gaszeniem.
        let mut pluck = 0.0;
        for (start, freq) in PLUCKS {
            let dt = t - start;
            if dt >= 0.0 {
                let env = (-3.0 * dt).exp();
                if env > 0.0002 {
                    let p = TAU * freq * dt;
                    pluck += env * (p.sin() + 0.25 * (2.0 * p).sin());
                }
            }
        }
        *sample = 0.16 * tremolo * pad + 0.30 * pluck;
    }
    // Normalizacja do spokojnego poziomu z zapasem.
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-6);
    let gain = 0.5 / peak;
    for sample in &mut out {
        *sample *= gain;
    }
    out
}

fn encode_wav_mono16(samples: &[f32]) -> Vec<u8> {
    let data_bytes = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + samples.len() * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        wav.extend_from_slice(&v.to_le_bytes());
    }
    wav
}

pub(crate) struct Music {
    sound: Option<Sound>,
    muted: bool,
}

impl Music {
    /// Gra dalej po cichu, gdy dźwięk jest niedostępny.
    pub(crate) fn off() -> Self {
        Self {
            sound: None,
            muted: true,
        }
    }

    /// Buduje pętlę i startuje odtwarzanie. Nie panikuje.
    pub(crate) async fn start() -> Self {
        let wav = encode_wav_mono16(&render_loop());
        match audio::load_sound_from_bytes(&wav).await {
            Ok(sound) => {
                audio::play_sound(
                    &sound,
                    PlaySoundParams {
                        looped: true,
                        volume: VOLUME,
                    },
                );
                Self {
                    sound: Some(sound),
                    muted: false,
                }
            }
            Err(error) => {
                eprintln!("music: no sound ({error}), running silently");
                Self::off()
            }
        }
    }

    pub(crate) fn toggle(&mut self) {
        let Some(sound) = &self.sound else {
            return;
        };
        if self.muted {
            self.muted = false;
            audio::play_sound(
                sound,
                PlaySoundParams {
                    looped: true,
                    volume: VOLUME,
                },
            );
        } else {
            self.muted = true;
            audio::stop_sound(sound);
        }
    }
}
