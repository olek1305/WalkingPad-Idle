//! Questy Wędrówki: zawsze 3 aktywne, nagroda jednorazowa (zero wpływu na stawkę/min).
//! Baza 1:1 nietknięta — quest to premia jak kamień milowy Trasy.

use crate::save::Save;

pub struct QuestDef {
    pub id: &'static str,
    pub desc: &'static str,
    pub goal: f64,
    pub reward: f64,
    pub unit: &'static str, // "km" | "m" | "s" | ""
}

/// Pula 8 szablonów, rotacja po cursorze. Nagrody ≈ 4–15 min marszu @4 km/h.
pub const QUESTS: [QuestDef; 8] = [
    QuestDef {
        id: "walk05",
        desc: "Warm-up: walk 0.5 km",
        goal: 0.5,
        reward: 15.0,
        unit: "km",
    },
    QuestDef {
        id: "tempo5",
        desc: "Steady pace: 5 min at 3–5 km/h",
        goal: 300.0,
        reward: 40.0,
        unit: "s",
    },
    QuestDef {
        id: "walk2",
        desc: "Pilgrim: walk 2 km",
        goal: 2.0,
        reward: 60.0,
        unit: "km",
    },
    QuestDef {
        id: "village2",
        desc: "Visit 2 villages",
        goal: 2.0,
        reward: 50.0,
        unit: "",
    },
    QuestDef {
        id: "forest300",
        desc: "Forester: 300 m of forest",
        goal: 300.0,
        reward: 35.0,
        unit: "m",
    },
    QuestDef {
        id: "night1",
        desc: "Night watch: 1 km at night",
        goal: 1.0,
        reward: 50.0,
        unit: "km",
    },
    QuestDef {
        id: "rain500",
        desc: "Rain walker: 500 m in rain",
        goal: 500.0,
        reward: 45.0,
        unit: "m",
    },
    QuestDef {
        id: "shroom3",
        desc: "Collector: 3 new mushrooms",
        goal: 3.0,
        reward: 30.0,
        unit: "",
    },
];

pub fn quest_def(id: &str) -> Option<&'static QuestDef> {
    QUESTS.iter().find(|q| q.id == id)
}

/// Dobierz do 3 aktywnych z rotacji (cursor w Save, distinct bo 8 > 3).
pub fn deal(s: &mut Save) {
    while s.quests.len() < 3 {
        let d = &QUESTS[(s.quest_cursor % QUESTS.len() as u32) as usize];
        s.quest_cursor += 1;
        if s.quests.iter().any(|q| q.id == d.id) {
            continue;
        }
        s.quests.push(crate::save::QuestState {
            id: d.id.to_string(),
            progress: 0.0,
            goal: d.goal,
            done: false,
        });
    }
}

fn add(s: &mut Save, id: &str, amount: f64) {
    for q in s.quests.iter_mut() {
        if q.id == id && !q.done {
            q.progress = (q.progress + amount).min(q.goal);
            if q.progress >= q.goal {
                q.done = true;
            }
        }
    }
}

pub fn add_walk_km(s: &mut Save, dkm: f64) {
    add(s, "walk05", dkm);
    add(s, "walk2", dkm);
}
pub fn add_forest_m(s: &mut Save, dm: f64) {
    add(s, "forest300", dm);
}
pub fn add_night_km(s: &mut Save, dkm: f64) {
    add(s, "night1", dkm);
}
pub fn add_rain_m(s: &mut Save, dm: f64) {
    add(s, "rain500", dm);
}
pub fn add_village(s: &mut Save) {
    add(s, "village2", 1.0);
}
pub fn add_shrooms(s: &mut Save, n: f64) {
    add(s, "shroom3", n);
}
/// Równe tempo: w zakresie nabija, poza nim ZERUJE (wyzwanie, nie akumulacja).
pub fn tempo_tick(s: &mut Save, dt: f32, ok: bool) {
    for q in s.quests.iter_mut() {
        if q.id == "tempo5" && !q.done {
            if ok {
                q.progress = (q.progress + dt as f64).min(q.goal);
                if q.progress >= q.goal {
                    q.done = true;
                }
            } else {
                q.progress = 0.0;
            }
        }
    }
}

/// Zbierz ukończone: zwraca (opis, nagroda), usuwa je i dobiera nowe.
pub fn collect_done(s: &mut Save) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.quests.len() {
        if s.quests[i].done {
            let q = s.quests.remove(i);
            if let Some(d) = quest_def(&q.id) {
                out.push((d.desc.to_string(), d.reward));
            }
        } else {
            i += 1;
        }
    }
    if !out.is_empty() {
        deal(s);
    }
    out
}
