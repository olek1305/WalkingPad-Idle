//! Doba z dystansu: pełna doba = 2000 m (~24 min @5 km/h).
//! Faza 0 = świt (6h), 0.25 = dzień (12h), 0.5 = zmierzch (18h), 0.75 = noc (0h).
//! Start mapowany z godziny systemowej, potem jedzie z marszem.
//! Na postoju doba stoi.

use macroquad::prelude::*;
use std::time::{SystemTime, UNIX_EPOCH};

/// Metry na pełną dobę.
pub const DAY_M: f64 = 2000.0;

pub struct Palette {
    pub sky_top: Color,
    pub sky_bot: Color,
    pub sun: Color,
    pub hill: Color,
}

const KEY_T: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
// świt / dzień / zmierzch (oryginalny synthwave) / noc / świt(zapętlenie)
const KEY_TOP: [(f32, f32, f32); 5] = [
    (0.10, 0.12, 0.30), // świt
    (0.25, 0.50, 0.95), // dzień
    (0.04, 0.06, 0.16), // zmierzch (= stare SKY_TOP)
    (0.02, 0.03, 0.10), // noc
    (0.10, 0.12, 0.30), // świt
];
const KEY_BOT: [(f32, f32, f32); 5] = [
    (0.90, 0.45, 0.35), // świt
    (0.65, 0.85, 0.95), // dzień
    (0.35, 0.16, 0.35), // zmierzch (= stare SKY_BOT)
    (0.10, 0.08, 0.20), // noc
    (0.90, 0.45, 0.35), // świt
];
const KEY_SUN: [(f32, f32, f32); 5] = [
    (1.00, 0.55, 0.25), // świt: nisko, ciepło
    (1.00, 0.90, 0.60), // dzień
    (1.00, 0.62, 0.15), // zmierzch (= stare SUN)
    (0.85, 0.88, 1.00), // noc: księżyc
    (1.00, 0.55, 0.25), // świt
];
const KEY_HILL: [(f32, f32, f32); 5] = [
    (0.12, 0.30, 0.28), // świt
    (0.15, 0.45, 0.35), // dzień
    (0.08, 0.35, 0.30), // zmierzch (= stare HILL_NEAR)
    (0.03, 0.08, 0.10), // noc
    (0.12, 0.30, 0.28), // świt
];

fn lerp3(a: (f32, f32, f32), b: (f32, f32, f32), t: f32) -> (f32, f32, f32) {
    (
        a.0 + (b.0 - a.0) * t,
        a.1 + (b.1 - a.1) * t,
        a.2 + (b.2 - a.2) * t,
    )
}

pub fn palette_at(phase: f64) -> Palette {
    let p = phase.rem_euclid(1.0);
    let mut k = 0;
    while k + 1 < KEY_T.len() && p > KEY_T[k + 1] {
        k += 1;
    }
    let k2 = (k + 1).min(KEY_T.len() - 1);
    let span = (KEY_T[k2] - KEY_T[k]).max(1e-9);
    let raw_t = ((p - KEY_T[k]) / span) as f32;
    // Hermite interpolation avoids visible colour-rate kinks at dawn/day and
    // dusk/night keyframes while preserving the exact palette at every key.
    let t = raw_t * raw_t * (3.0 - 2.0 * raw_t);
    let top = lerp3(KEY_TOP[k], KEY_TOP[k2], t);
    let bot = lerp3(KEY_BOT[k], KEY_BOT[k2], t);
    let sun = lerp3(KEY_SUN[k], KEY_SUN[k2], t);
    let hill = lerp3(KEY_HILL[k], KEY_HILL[k2], t);
    Palette {
        sky_top: Color::new(top.0, top.1, top.2, 1.0),
        sky_bot: Color::new(bot.0, bot.1, bot.2, 1.0),
        sun: Color::new(sun.0, sun.1, sun.2, 1.0),
        hill: Color::new(hill.0, hill.1, hill.2, 1.0),
    }
}

/// Faza doby 0..1 z kamery (metry) + offsetu.
pub fn doba_phase(cam_m: f64, doba_off: f64) -> f64 {
    ((cam_m + doba_off) % DAY_M + DAY_M) % DAY_M / DAY_M
}

/// 1 = pełna noc (faza 0.75), 0 = pełny dzień (faza 0.25).
pub fn nightness(phase: f64) -> f32 {
    let c = ((phase - 0.75) * std::f64::consts::TAU).cos();
    ((0.5 + 0.5 * c) as f32).powi(2)
}

/// Jasność świata 0.35 (noc) .. 1.0 (dzień).
pub fn brightness(phase: f64) -> f32 {
    0.35 + 0.65 * (1.0 - nightness(phase))
}

/// Harmonogram latarni „przełącznik": świecą nocą (od ~18:39), gasną od rana
/// (do ~7:54) i pozostają zgaszone przez cały dzień i południe. Krótki fade
/// ~24 m na obu brzegach zamiast łagodnego skalowania z `nightness`.
pub fn lamp_schedule(phase: f64) -> f32 {
    let phase = phase.rem_euclid(1.0);
    const DUSK: f64 = 0.527; // ~18:39
    const DAWN: f64 = 0.079; // ~7:54
    const RAMP: f64 = 0.012;
    let night_len = (1.0 + DAWN) - DUSK;
    let t = (phase - DUSK + 1.0).rem_euclid(1.0);
    let intro = smooth01((t / RAMP).clamp(0.0, 1.0));
    let outro = smooth01(((night_len - t) / RAMP).clamp(0.0, 1.0));
    (intro * outro).clamp(0.0, 1.0) as f32
}

fn smooth01(x: f64) -> f64 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn phase_name(phase: f64) -> &'static str {
    match phase.rem_euclid(1.0) {
        p if p < 0.125 => "dawn",
        p if p < 0.375 => "day",
        p if p < 0.625 => "dusk",
        p if p < 0.875 => "night",
        _ => "dawn",
    }
}

/// Offset na pierwszy start: żeby faza zgadzała się z godziną systemową.
/// Mapowanie liniowe: 6h -> 0, 12h -> 0.25, 18h -> 0.5, 0h -> 0.75.
pub fn init_offset(cam_m: f64) -> f64 {
    let hour = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs() % 86400;
            secs as f64 / 3600.0
        })
        .unwrap_or(12.0);
    let want = (hour - 6.0).rem_euclid(24.0) / 24.0;
    want * DAY_M - cam_m
}
