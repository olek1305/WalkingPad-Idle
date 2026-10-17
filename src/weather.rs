//! Pogoda z regionu: deterministyczna, płynne przejścia na granicach.
//! POGODA 70 / DESZCZ 20 / MGŁA 10. Przejście fade 30 m od granicy regionu.

use crate::rng::{REGION_M, hash1, region_of};

/// Szerokość pasa przejścia przy granicy regionu (metry).
pub const FADE_M: f64 = 30.0;

#[derive(PartialEq, Clone, Copy)]
pub enum Pogoda {
    Pogodnie,
    Deszcz,
    Mgla,
}

/// Typ pogody regionu (stabilny, z hasha).
pub fn pogoda_regionu(region: i64) -> Pogoda {
    match hash1(region as u64 ^ 0xEA7) % 100 {
        0..70 => Pogoda::Pogodnie,
        70..90 => Pogoda::Deszcz,
        _ => Pogoda::Mgla,
    }
}

fn smooth01(x: f64) -> f64 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Intensywności (deszcz 0..1, mgła 0..1) w punkcie `m`.
/// W głębi regionu pełnia, w pasie FADE_M przy granicy miks z sąsiadem.
pub fn intensity_at(m: f64) -> (f32, f32) {
    let r = region_of(m);
    let base = pogoda_regionu(r);
    let edge0 = r as f64 * REGION_M;
    let d_left = m - edge0;
    let d_right = edge0 + REGION_M - m;
    let d_edge = d_left.min(d_right);
    if d_edge >= FADE_M {
        return full(base);
    }
    // sąsiad zza bliższej granicy
    let rn = if d_left < d_right { r - 1 } else { r + 1 };
    let (rb, fb) = to_pair(base);
    let (rn_b, fn_b) = to_pair(pogoda_regionu(rn));
    let t = smooth01(d_edge / FADE_M) as f32; // 0 na granicy, 1 w głębi
    (rb + (rn_b - rb) * (1.0 - t), fb + (fn_b - fb) * (1.0 - t))
}

fn to_pair(p: Pogoda) -> (f32, f32) {
    match p {
        Pogoda::Pogodnie => (0.0, 0.0),
        Pogoda::Deszcz => (1.0, 0.0),
        Pogoda::Mgla => (0.0, 1.0),
    }
}
fn full(p: Pogoda) -> (f32, f32) {
    to_pair(p)
}

pub fn pogoda_name(rain: f32, fog: f32) -> &'static str {
    if rain > 0.5 {
        "rain"
    } else if fog > 0.5 {
        "fog"
    } else if rain > 0.15 {
        "drizzle"
    } else if fog > 0.15 {
        "mist"
    } else {
        "clear"
    }
}
