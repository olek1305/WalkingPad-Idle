//! RNG + współrzędne świata w metrach (f64). Bez wrapu, nieskończony świat.

/// Długość regionu (biomu) w metrach.
pub const REGION_M: f64 = 300.0;

/// splitmix64 — jeden hash dla całego świata.
pub fn hash1(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// Mały RNG: next / range / chance. Koniec z wrapping_* po kodzie.
pub struct Rng(pub u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(hash1(seed))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = hash1(self.0);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    pub fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (self.next() as f64 / u64::MAX as f64) * (b - a)
    }
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum Biom {
    Las,
    Laka,
    Wies,
    Miasto,
}

/// Region co 300 m: LAS 35 / ŁĄKA 25 / WIEŚ 20 / MIASTO 20.
pub fn biom(region: i64) -> Biom {
    match hash1(region as u64) % 100 {
        0..35 => Biom::Las,
        35..60 => Biom::Laka,
        60..80 => Biom::Wies,
        _ => Biom::Miasto,
    }
}
pub fn region_of(m: f64) -> i64 {
    (m / REGION_M).floor() as i64
}
pub fn biom_at(m: f64) -> Biom {
    biom(region_of(m))
}
/// Pozycja osady w regionie WIEŚ/MIASTO: start + 50–150 m.
pub fn settle_m(region: i64) -> f64 {
    region as f64 * REGION_M + 50.0 + (hash1(region as u64 ^ 0x1234) % 100) as f64
}

/// Typy 4 grzybów regionu — DOKŁADNIE ta sama kolejność losowań co
/// `world::region_description` (range przed below, inaczej typy by się
/// rozjechały z rysowanymi obiektami).
pub fn shroom_types(region: i64) -> [u32; 4] {
    let mut out = [0u32; 4];
    for j in 0..4u64 {
        let mut jrng = Rng::new(region as u64 ^ 0x9A17 ^ j.wrapping_mul(0x85EBCA6B));
        let _ = jrng.range(0.0, 120.0);
        out[j as usize] = jrng.below(20) as u32;
    }
    out
}
/// Indeks zwierzyny regionu 0..4 (sowa/jeleń/jeż/niedźwiedź/wilk) — jak w
/// `world::region_description`.
pub fn animal_idx(region: i64) -> usize {
    let mut arng = Rng::new(region as u64 ^ 0xA41E);
    let _ = arng.range(300.0, 1200.0);
    match arng.below(100) {
        a if a < 40 => 0,
        a if a < 55 => 1,
        a if a < 80 => 2,
        a if a < 92 => 3,
        _ => 4,
    }
}

/// Nazwa miejscowości po angielsku (zasada języka z CLAUDE.md):
/// deterministyczna per region, np. "Birchbrook", "Millgate". Używana przez
/// kuriera w popupach, docelowo przez tabliczki osad w 3D.
pub fn place_name(region: i64) -> String {
    const A: [&str; 12] = [
        "Birch", "Stone", "Mill", "Ash", "Hollow", "Fern", "Oak", "Elm", "Ridge", "Copper",
        "Willow", "Hazel",
    ];
    const B: [&str; 12] = [
        "brook", "field", "ford", "mere", "gate", "wood", "ton", "burn", "cross", "vale", "haven",
        "stead",
    ];
    let h = hash1(region as u64 ^ 0x7EC5_C4FE);
    format!("{}{}", A[(h % 12) as usize], B[((h >> 8) % 12) as usize])
}
