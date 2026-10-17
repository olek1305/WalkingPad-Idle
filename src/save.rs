//! Zapis gry. Nowe pola ZAWSZE z #[serde(default)] — stare save.json muszą się wczytać.

use serde::{Deserialize, Serialize};
use std::fs;

pub const SAVE_PATH: &str = "save.json";

#[derive(Serialize, Deserialize, Clone)]
pub struct QuestState {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub progress: f64,
    #[serde(default = "default_goal")]
    pub goal: f64,
    #[serde(default)]
    pub done: bool,
}

/// Cel questu, gdy pole zniknęło z JSON-a (zero = quest skończony od razu).
fn default_goal() -> f64 {
    1.0
}

impl Default for QuestState {
    fn default() -> Self {
        QuestState {
            id: String::new(),
            progress: 0.0,
            goal: 1.0,
            done: false,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Save {
    pub gold: f64,
    pub total_gold: f64,
    pub total_km: f64,
    pub shoes: u32,
    pub bank: u32,
    #[serde(default)]
    pub pulse: u32,
    #[serde(default)]
    pub route: u32,
    #[serde(default)]
    pub next_km: f64,
    // --- Wędrówka v2 (wszystko z default = kompatybilność wstecz) ---
    /// Offset doby w metrach. Ustawiany raz przy pierwszym starcie z godziny
    /// systemowej (patrz daynight::init_offset). 0 + doba_init=false = niezainicjowane.
    #[serde(default)]
    pub doba_off: f64,
    #[serde(default)]
    pub doba_init: bool,
    /// Poziom Wędrowca (prestiż, zero mnożnika).
    #[serde(default)]
    pub level: u32,
    /// Album: 20 typów grzybów (typ = index), 5 zwierząt licznikowo.
    #[serde(default)]
    pub album_shrooms: [bool; 20],
    #[serde(default)]
    pub album_animals: [u32; 5],
    #[serde(default)]
    pub osady: u32,
    /// Statystyki do questów: km nocą / w deszczu.
    #[serde(default)]
    pub night_km: f64,
    #[serde(default)]
    pub rain_km: f64,
    /// Aktywne questy Wędrówki.
    #[serde(default)]
    pub quests: Vec<QuestState>,
    /// Cursor rotacji szablonów questów.
    #[serde(default)]
    pub quest_cursor: u32,
    /// Kurier: aktywne paczki + liczba udanych dostaw.
    #[serde(default)]
    pub parcels: Vec<Parcel>,
    #[serde(default)]
    pub deliveries: u32,
    /// Latarnie drogowe zapalone przez gracza (stabilne ID z generatora).
    #[serde(default)]
    pub lit_lamps: Vec<u64>,
}

/// Limit aktywnych paczek kuriera (służy też normalizacji starych zapisów).
pub const MAX_PARCELS: usize = 2;

/// Aktywna przesyłka kuriera. Świeżość jest odległościowa (metr absolutny),
/// więc nie istnieje wyścig z czasem — postój nie karze (zasada zero offline).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Parcel {
    /// Region osady odbioru (tożsamość stab. dla przyszłych tabliczek).
    pub id: u64,
    /// Region osady docelowej.
    pub target_region: i64,
    /// 0 = letter, 1 = parcel, 2 = crate.
    pub kind: u32,
    /// Bazowy gold za dostawę (świeżość mnoży osobno).
    pub reward: f64,
    /// Metr absolutny odbioru.
    pub pickup_m: f64,
    /// Limit świeżości (metr absolutny): poniżej = bonus.
    pub fresh_until_m: f64,
}

impl Default for Save {
    fn default() -> Self {
        Save {
            gold: 0.0,
            total_gold: 0.0,
            total_km: 0.0,
            shoes: 0,
            bank: 0,
            pulse: 0,
            route: 0,
            next_km: 1.0,
            doba_off: 0.0,
            doba_init: false,
            level: 0,
            album_shrooms: [false; 20],
            album_animals: [0; 5],
            osady: 0,
            night_km: 0.0,
            rain_km: 0.0,
            quests: Vec::new(),
            quest_cursor: 0,
            parcels: Vec::new(),
            deliveries: 0,
            lit_lamps: Vec::new(),
        }
    }
}

/// Mnożnik golda z Butów. Poziomy 0–3 (starsze zapisy przycięte do III).
pub const BUTY_MULT: [f64; 4] = [1.0, 1.25, 1.5, 2.0];
/// Skarbonka: stały przychód gold/min, TYLKO w marszu.
pub const BANK_RATE: [f64; 4] = [0.0, 0.5, 1.5, 4.0];
/// Pulsometr: szansa na ×5 golda w sekundowym ticku.
pub const CRIT_CH: [f64; 4] = [0.0, 0.10, 0.20, 0.30];
/// Trasa: premia za każdy pełny kilometr.
pub const ROUTE_BON: [f64; 4] = [25.0, 40.0, 80.0, 175.0];

pub fn lvl3(l: u32) -> usize {
    l.min(3) as usize
}
pub fn shoes_mult(s: &Save) -> f64 {
    BUTY_MULT[lvl3(s.shoes)]
}
pub fn bank_rate(s: &Save) -> f64 {
    BANK_RATE[lvl3(s.bank)]
}
pub fn crit_chance(s: &Save) -> f64 {
    CRIT_CH[lvl3(s.pulse)]
}
pub fn route_bonus(s: &Save) -> f64 {
    ROUTE_BON[lvl3(s.route)]
}
/// Poziom Wędrowca z dystansu (progi w km). Czysty prestiż.
pub fn level_for_km(km: f64) -> u32 {
    const PROGI: [f64; 7] = [1.0, 3.0, 6.0, 10.0, 15.0, 21.0, 30.0];
    PROGI.iter().filter(|&&p| km >= p).count() as u32
}
pub fn level_title(lvl: u32) -> &'static str {
    match lvl {
        0 => "Rambler",
        1 => "Vagabond",
        2 => "Hiker",
        3 => "Wayfarer",
        4 => "Forest Spirit",
        5 => "Pilgrim",
        6 => "Trail Master",
        _ => "Treadmill Legend",
    }
}

/// Stawka gold/min: baza (prędkość × Buty) + Skarbonka. NA POSTOJU skarbonka = 0
/// (anty-exploit: stoi = zero, nawet z kupioną skarbonką).
pub fn income_per_min(s: &Save, speed: f64) -> f64 {
    let bank = if speed >= 0.1 { bank_rate(s) } else { 0.0 };
    speed * shoes_mult(s) + bank
}

/// Jeden tick ekonomii (1 s). `roll` = wynik losowania (0..1, rosnąco).
/// Zwraca (gold z ticka, czy pokazać popup KRYT). Baza 1:1: 4 km/h = 4 gold/min.
pub fn tick_gold(s: &Save, speed: f64, roll: f64) -> (f64, bool) {
    let base = income_per_min(s, speed) / 60.0;
    let crit = roll < crit_chance(s);
    (if crit { base * 5.0 } else { base }, crit && base > 0.0)
}

/// Kamień milowy: przekroczony pełny km = premia z Trasy (raz na km, next_km +1).
pub fn take_milestone(s: &mut Save) -> Option<f64> {
    if s.total_km >= s.next_km {
        let bonus = route_bonus(s);
        s.next_km += 1.0;
        Some(bonus)
    } else {
        None
    }
}

pub fn load_save() -> Save {
    let mut s: Save = fs::read_to_string(SAVE_PATH)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if s.next_km < 1.0 {
        s.next_km = s.total_km.floor() + 1.0;
    }
    // normalizacja starych zapisów: limit paczek + historia latarni bez wzrostu
    s.parcels.truncate(MAX_PARCELS);
    while s.lit_lamps.len() > 4096 {
        s.lit_lamps.remove(0);
    }
    s
}
pub fn store_save(s: &Save) {
    if let Ok(t) = serde_json::to_string_pretty(s) {
        let _ = fs::write(SAVE_PATH, t);
    }
}
