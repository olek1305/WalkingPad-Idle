//! Reguły gry pierwszego wycinka: dystans bieżni, wiedza, karty i ulepszenia.

use super::catalog::{self, Discovery, UpgradeKind};
use super::persistence::EvolutionSave;

pub const END_OF_SLICE_KM: f64 = 3.0;

pub fn sync_discoveries(state: &mut EvolutionSave) -> bool {
    let mut changed = false;
    for discovery in catalog::DISCOVERIES {
        if state.treadmill_distance_km + f64::EPSILON >= discovery.distance_km
            && !state
                .unlocked_discovery_ids
                .iter()
                .any(|id| id == discovery.id)
        {
            state.unlocked_discovery_ids.push(discovery.id.to_string());
            changed = true;
        }
    }
    changed
}

pub fn mark_read(state: &mut EvolutionSave, id: &str) -> bool {
    if !state.unlocked_discovery_ids.iter().any(|known| known == id)
        || state.read_discovery_ids.iter().any(|known| known == id)
    {
        return false;
    }
    state.read_discovery_ids.push(id.to_string());
    true
}

pub fn selected_discovery(state: &EvolutionSave, selected_id: &str) -> &'static Discovery {
    if let Some(discovery) = catalog::discovery(selected_id).filter(|discovery| {
        state
            .unlocked_discovery_ids
            .iter()
            .any(|id| id == discovery.id)
    }) {
        return discovery;
    }
    state
        .unlocked_discovery_ids
        .iter()
        .rev()
        .find_map(|id| catalog::discovery(id))
        .unwrap_or(&catalog::DISCOVERIES[0])
}

pub fn knowledge_per_minute(speed_kmh: f64, accuracy_level: u8) -> f64 {
    if !speed_kmh.is_finite() || speed_kmh <= 0.05 {
        return 0.0;
    }
    let level = accuracy_level.min(2) as f64;
    speed_kmh.max(0.0) * (1.0 + 0.25 * level)
}

pub fn upgrade_level(state: &EvolutionSave, kind: UpgradeKind) -> u8 {
    match kind {
        UpgradeKind::Accuracy => state.observation_accuracy_level,
        UpgradeKind::Range => state.observation_range_level,
    }
}

pub fn purchase_upgrade(state: &mut EvolutionSave, kind: UpgradeKind) -> PurchaseResult {
    let spec = catalog::upgrade(kind);
    let level = upgrade_level(state, kind);
    if level >= spec.max_level {
        return PurchaseResult::Capped;
    }
    let Some(&cost) = spec.costs.get(level as usize) else {
        return PurchaseResult::Capped;
    };
    if state.knowledge + f64::EPSILON < cost {
        return PurchaseResult::InsufficientKnowledge { cost };
    }
    state.knowledge -= cost;
    match kind {
        UpgradeKind::Accuracy => state.observation_accuracy_level += 1,
        UpgradeKind::Range => state.observation_range_level += 1,
    }
    PurchaseResult::Purchased { cost }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PurchaseResult {
    Purchased { cost: f64 },
    Capped,
    InsufficientKnowledge { cost: f64 },
}

pub fn unlocked_count(state: &EvolutionSave) -> usize {
    state
        .unlocked_discovery_ids
        .iter()
        .filter(|id| catalog::is_known_discovery(id))
        .count()
}
