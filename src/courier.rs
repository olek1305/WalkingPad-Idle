//! Kurier: paczki zbierane przy przejściu osady, dostawa w następnej osadzie.
//! Premia za „świeżość" jest odległościowa (ZERO liczenia czasu): nie da się jej
//! ominąć czekaniem, bo `runner_m` rośnie tylko w marszu. Gold za dostawę to
//! premia questowa — baza 1:1 (km → gold) pozostaje nietknięta.

use crate::rng::{Biom, biom, hash1, place_name, settle_m};
use crate::save::{MAX_PARCELS, Parcel, Save};
use crate::world::SettlementDesc;

///// Poszukiwanie następnej osady (WIEŚ/MIASTO) w kolejnych regionach.
pub fn next_settlement_after(region: i64) -> Option<(i64, f64, Biom)> {
    for r in region + 1..=region + 64 {
        let kind = biom(r);
        if matches!(kind, Biom::Wies | Biom::Miasto) {
            return Some((r, settle_m(r), kind));
        }
    }
    None
}

/// Flavour paczki wg hasha regionu odbioru: (etykieta, mnożnik nagrody).
pub fn parcel_kind(kind: u32) -> (&'static str, f64) {
    match kind {
        0 => ("letter", 0.9),
        1 => ("parcel", 1.0),
        _ => ("crate", 1.15),
    }
}

/// Nagroda za dostawę: ~75 gold/km + 20 stałej. Dla porównania baza 1:1:
/// 1 km przy 4 km/h = 15 min = 60 gold za sam marsz, paczka dodaje bonus.
pub fn parcel_reward(dist_m: f64, kind: u32) -> f64 {
    (dist_m * 0.075 + 20.0) * parcel_kind(kind).1
}

/// Zdarzenia przekazywane pętli gry (tylko teksty popupów; gold jest
/// rozliczony już tutaj — pętla NIE dodaje go drugi raz).
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Pickup { text: String },
    Deliver { text: String },
}

fn spawn_parcel(s: &mut Save, from: &SettlementDesc) -> Option<Event> {
    let (target_region, target_m, _) = next_settlement_after(from.region)?;
    let dist = target_m - from.position_m;
    let roll = hash1(from.region as u64 ^ 0x0ACEC) % 100;
    let kind = if roll < 55 {
        0
    } else if roll < 92 {
        1
    } else {
        2
    };
    let reward = parcel_reward(dist, kind);
    s.parcels.push(Parcel {
        id: hash1(from.region as u64 ^ 0xAB3),
        target_region,
        kind,
        reward,
        pickup_m: from.position_m,
        fresh_until_m: from.position_m + dist * 1.35 + 550.0,
    });
    Some(Event::Pickup {
        text: format!(
            "Parcel → {} · {:.1} km",
            place_name(target_region),
            dist / 1000.0
        ),
    })
}

/// Rozliczenie osad przekraczanych w tej klatce. Jedna osada może skleić
/// łącznie jedną dostawę i jeden odbiór; limit aktywnych paczek = MAX_PARCELS.
pub fn handle_crossings(s: &mut Save, crossings: &[SettlementDesc], runner_m: f64) -> Vec<Event> {
    let mut events = Vec::new();
    for sett in crossings {
        // dostawa: wszystkie paczki, których celem jest ta osada
        let mut i = 0;
        while i < s.parcels.len() {
            if s.parcels[i].target_region == sett.region {
                let p = s.parcels.remove(i);
                let fresh = runner_m <= p.fresh_until_m;
                let pay = if fresh { p.reward * 1.4 } else { p.reward };
                s.gold += pay;
                s.total_gold += pay;
                s.deliveries += 1;
                events.push(Event::Deliver {
                    text: format!(
                        "Delivered {} · {} · +{:.0} gold{}",
                        parcel_kind(p.kind).0,
                        place_name(sett.region),
                        pay,
                        if fresh { " (fresh)" } else { " (stale)" }
                    ),
                });
            } else {
                i += 1;
            }
        }
        // odbiór nowej paczki (max jedna na osadę, MAX_PARCELS aktywnych)
        if s.parcels.len() < MAX_PARCELS {
            if let Some(event) = spawn_parcel(s, sett) {
                events.push(event);
            }
        }
    }
    events
}
