//! Testy: ekonomia (święta baza 1:1), Save/serde (stare zapisy), questy, RNG/biomy,
//! doba, pogoda, formatowanie liczb (teksty gracza: EN). Zero okna, czysta logika.

use macroquad::prelude::*;

use crate::daynight;
use crate::quests;
use crate::render3d::material::{
    MaterialFlags, MaterialSpec, SurfaceKind, hsl_to_srgb, linear_to_srgb, scale_linear_srgb,
    srgb_to_hsl, srgb_to_linear, surface_preset, vertex_data,
};
use crate::rng::{
    Biom, REGION_M, Rng, animal_idx, biom, biom_at, hash1, region_of, settle_m, shroom_types,
};
use crate::save::{
    BANK_RATE, BUTY_MULT, CRIT_CH, QuestState, ROUTE_BON, Save, level_for_km, level_title,
    take_milestone, tick_gold,
};
use crate::text::{format_quest_progress, pl};
use crate::weather::{self, Pogoda};
use crate::world::{self, Geometry};

const EPS: f64 = 1e-9;

fn base() -> Save {
    Save::default()
}

// ---------------------------------------------------------------- ekonomia

#[test]
fn baza_1to1_cztery_km_h_cztery_golda_na_minute() {
    for (spd, per_min) in [(0.0, 0.0), (1.0, 1.0), (4.0, 4.0), (5.0, 5.0), (20.0, 20.0)] {
        let got = crate::save::income_per_min(&base(), spd);
        assert!(
            (got - per_min).abs() < EPS,
            "speed {spd}: {got} != {per_min}"
        );
    }
    // 4 km/h w ticku 1 s = 4/60 golda
    let (g, crit) = tick_gold(&base(), 4.0, 1.0);
    assert!(!crit);
    assert!((g - 4.0 / 60.0).abs() < EPS, "{g}");
    // 60 takich ticków = dokładnie 4 golda
    let mut s = base();
    for _ in 0..60 {
        let (g, _) = tick_gold(&s, 4.0, 1.0);
        s.gold += g;
    }
    assert!((s.gold - 4.0).abs() < 1e-9, "{}", s.gold);
}

#[test]
fn tabele_wartosci_zamrozone() {
    assert_eq!(BUTY_MULT, [1.0, 1.25, 1.5, 2.0]);
    assert_eq!(BANK_RATE, [0.0, 0.5, 1.5, 4.0]);
    assert_eq!(CRIT_CH, [0.0, 0.10, 0.20, 0.30]);
    assert_eq!(ROUTE_BON, [25.0, 40.0, 80.0, 175.0]);
}

#[test]
fn skarbonka_na_postoju_daje_zero() {
    let mut s = base();
    s.bank = 3; // poziom III = +4,0 gold/min (NIE kwota!)
    // skarbonka nie działa poniżej 0,1 km/h — ale baza (prędkość × buty) zostaje
    assert_eq!(crate::save::income_per_min(&s, 0.0), 0.0, "na postoju zero");
    assert!(
        (crate::save::income_per_min(&s, 0.09) - 0.09).abs() < EPS,
        "tylko baza"
    );
    assert!(
        (crate::save::income_per_min(&s, 0.1) - 4.1).abs() < EPS,
        "0,1 × 1,0 + 4,0 skarbonki"
    );
    let (g, _) = tick_gold(&s, 0.0, 1.0);
    assert_eq!(g, 0.0, "na postoju złota nie ma");
}

#[test]
fn poziomy_przyciete_do_trzeciego() {
    let mut s = base();
    s.shoes = 99;
    s.bank = 99;
    s.pulse = 99;
    s.route = 99;
    assert_eq!(crate::save::shoes_mult(&s), 2.0);
    assert_eq!(crate::save::bank_rate(&s), 4.0);
    assert_eq!(crate::save::crit_chance(&s), 0.30);
    assert_eq!(crate::save::route_bonus(&s), 175.0);
    s.shoes = 1;
    assert_eq!(crate::save::shoes_mult(&s), 1.25);
}

#[test]
fn kryty_tylko_gdy_roll_mniejszy_od_szansy() {
    let mut s = base();
    s.pulse = 1; // 10%
    let (g, crit) = tick_gold(&s, 4.0, 0.05);
    assert!(crit);
    assert!((g - 4.0 / 60.0 * 5.0).abs() < EPS);
    let (_, crit2) = tick_gold(&s, 4.0, 0.10);
    assert!(!crit2);
    // bez Pulsometru kryt nigdy
    let (_, crit3) = tick_gold(&base(), 4.0, 0.0);
    assert!(!crit3);
    // kryt na postoju nie tworzy złota ani popupu
    let (g4, crit4) = tick_gold(&s, 0.0, 0.0);
    assert_eq!(g4, 0.0);
    assert!(!crit4);
}

#[test]
fn kamien_milowy_raz_na_kilometr() {
    let mut s = base();
    s.total_km = 0.99;
    assert_eq!(take_milestone(&mut s), None);
    s.total_km = 1.0;
    assert_eq!(take_milestone(&mut s), Some(25.0));
    assert_eq!(s.next_km, 2.0);
    assert_eq!(take_milestone(&mut s), None);
    s.route = 2; // +80/km
    s.total_km = 2.0;
    assert_eq!(take_milestone(&mut s), Some(80.0));
    assert_eq!(s.next_km, 3.0);
}

#[test]
fn poziom_wedrowca_progi_i_tytuly() {
    for (km, lvl) in [
        (0.0, 0),
        (0.999, 0),
        (1.0, 1),
        (2.999, 1),
        (3.0, 2),
        (5.999, 2),
        (6.0, 3),
        (10.0, 4),
        (15.0, 5),
        (21.0, 6),
        (30.0, 7),
        (1e6, 7),
    ] {
        assert_eq!(level_for_km(km), lvl, "km {km}");
    }
    assert_eq!(level_title(0), "Rambler");
    assert_eq!(level_title(6), "Trail Master");
    assert_eq!(level_title(7), "Treadmill Legend");
    assert_eq!(level_title(99), "Treadmill Legend");
}

// ------------------------------------------------------------------- serde

#[test]
fn stary_save_v1_laduje_sie_z_wartoscia_domyslna_nowych_pol() {
    let old = r#"{
        "gold": 18.25, "total_gold": 18.25, "total_km": 0.3055786,
        "shoes": 1, "bank": 1, "pulse": 1, "route": 1, "next_km": 1.0
    }"#;
    let s: Save = serde_json::from_str(old).expect("stary save musi się wczytać");
    assert_eq!(s.gold, 18.25);
    assert_eq!(s.shoes, 1);
    assert_eq!(s.doba_off, 0.0);
    assert!(!s.doba_init);
    assert_eq!(s.level, 0);
    assert_eq!(s.album_shrooms, [false; 20]);
    assert_eq!(s.album_animals, [0; 5]);
    assert_eq!(s.osady, 0);
    assert_eq!(s.night_km, 0.0);
    assert_eq!(s.rain_km, 0.0);
    assert!(s.quests.is_empty());
    assert_eq!(s.quest_cursor, 0);
}

#[test]
fn save_v2_przez_pelen_obieg() {
    let mut s = base();
    s.gold = 123.5;
    s.total_km = 42.0;
    s.doba_off = 1194.42;
    s.doba_init = true;
    s.level = 3;
    s.album_shrooms[7] = true;
    s.album_animals[2] = 4;
    s.osady = 3;
    s.quests.push(QuestState {
        id: "walk05".into(),
        progress: 0.25,
        goal: 0.5,
        done: false,
    });
    s.quest_cursor = 5;
    let t = serde_json::to_string(&s).unwrap();
    let back: Save = serde_json::from_str(&t).unwrap();
    assert_eq!(back.gold, 123.5);
    assert_eq!(back.total_km, 42.0);
    assert_eq!(back.doba_off, 1194.42);
    assert!(back.doba_init);
    assert_eq!(back.level, 3);
    assert!(back.album_shrooms[7]);
    assert_eq!(back.album_animals[2], 4);
    assert_eq!(back.osady, 3);
    assert_eq!(back.quests.len(), 1);
    assert_eq!(back.quests[0].id, "walk05");
    assert_eq!(back.quest_cursor, 5);
}

#[test]
fn quest_state_bez_id_tez_sie_laduje() {
    let q: QuestState = serde_json::from_str(r#"{"progress": 1.0}"#).unwrap();
    assert_eq!(q.id, "");
    assert_eq!(q.progress, 1.0);
    assert_eq!(
        q.goal, 1.0,
        "brak goal = default, nie 0 (quest skończony od razu)"
    );
    assert!(!q.done);
}

// ------------------------------------------------------------------ questy

#[test]
fn rozdanie_daje_trzy_rozne_questy_w_kolejnosci_szablonow() {
    let mut s = base();
    quests::deal(&mut s);
    assert_eq!(s.quests.len(), 3);
    let ids: Vec<&str> = s.quests.iter().map(|q| q.id.as_str()).collect();
    assert_eq!(ids, ["walk05", "tempo5", "walk2"]);
    assert_eq!(s.quest_cursor, 3);
    // ponowne rozdanie nie dubluje
    quests::deal(&mut s);
    assert_eq!(s.quests.len(), 3);
    assert_ne!(s.quests[0].id, s.quests[1].id);
}

#[test]
fn postep_dotyczy_tylko_swojego_questu_i_jest_przyciety() {
    let mut s = base();
    quests::deal(&mut s);
    quests::add_walk_km(&mut s, 0.6); // walk05 (goal 0.5) kończy, walk2 rośnie
    assert!(s.quests.iter().find(|q| q.id == "walk05").unwrap().done);
    assert!(s.quests.iter().find(|q| q.id == "walk05").unwrap().progress <= 0.5);
    let walk2 = s.quests.iter().find(|q| q.id == "walk2").unwrap();
    assert!(!walk2.done);
    assert!((walk2.progress - 0.6).abs() < EPS);
    // tempo5 nietknięty
    let tempo = s.quests.iter().find(|q| q.id == "tempo5").unwrap();
    assert_eq!(tempo.progress, 0.0);
}

#[test]
fn tempo_rowne_nabija_poza_zakresem_zeruje() {
    let mut s = base();
    s.quests.push(QuestState {
        id: "tempo5".into(),
        progress: 0.0,
        goal: 300.0,
        done: false,
    });
    quests::tempo_tick(&mut s, 60.0, true);
    assert_eq!(s.quests[0].progress, 60.0);
    quests::tempo_tick(&mut s, 60.0, false);
    assert_eq!(s.quests[0].progress, 0.0, "poza zakresem 3–5 km/h = reset");
    quests::tempo_tick(&mut s, 1e9, true);
    assert_eq!(s.quests[0].progress, 300.0);
    assert!(s.quests[0].done);
    let (gained, rewards) = {
        let got = quests::collect_done(&mut s);
        (got.len(), got.iter().map(|r| r.1).sum::<f64>())
    };
    assert_eq!(gained, 1);
    assert_eq!(rewards, 40.0);
    assert_eq!(s.quests.len(), 3, "po odbiorze dobierane są nowe");
}

#[test]
fn odbior_nagrod_i_dobranie_do_trzech() {
    let mut s = base();
    quests::deal(&mut s);
    quests::add_village(&mut s);
    let empty = quests::collect_done(&mut s);
    assert!(empty.is_empty(), "nic nie gotowe");
    quests::add_walk_km(&mut s, 0.5);
    let done = quests::collect_done(&mut s);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].1, 15.0);
    assert!(done[0].0.contains("Warm-up"));
    assert_eq!(s.quests.len(), 3);
}

#[test]
fn pula_questow_spójna() {
    assert_eq!(quests::QUESTS.len(), 8);
    let mut ids: Vec<&str> = quests::QUESTS.iter().map(|q| q.id).collect();
    ids.dedup();
    assert_eq!(ids.len(), 8, "id muszą być unikalne");
    for q in quests::QUESTS.iter() {
        assert!(q.goal > 0.0, "{}", q.id);
        assert!(q.reward > 0.0, "{}", q.id);
        assert!(quests::quest_def(q.id).is_some());
    }
    assert!(quests::quest_def("nie_ma").is_none());
}

// -------------------------------------------------------------------- rng

#[test]
fn regiony_granice_i_determinizm() {
    assert_eq!(region_of(0.0), 0);
    assert_eq!(region_of(REGION_M - 0.001), 0);
    assert_eq!(region_of(REGION_M), 1);
    assert_eq!(region_of(12345.6), 41);
    assert_eq!(region_of(-1.0), -1);
    assert_eq!(hash1(0), hash1(0));
    assert_ne!(hash1(0), hash1(1));
    let mut a = Rng::new(7);
    let mut b = Rng::new(7);
    assert_eq!(a.next(), b.next());
    for _ in 0..100 {
        assert!(Rng::new(1).below(20) < 20);
    }
}

#[test]
fn rozklad_biomow_blisko_35_25_20_20() {
    let mut c = [0usize; 4];
    for r in 0..4000 {
        match biom(r) {
            Biom::Las => c[0] += 1,
            Biom::Laka => c[1] += 1,
            Biom::Wies => c[2] += 1,
            Biom::Miasto => c[3] += 1,
        }
    }
    let want = [1400.0, 1000.0, 800.0, 800.0];
    for (i, w) in want.iter().enumerate() {
        let dev = (c[i] as f64 - w).abs() / w;
        assert!(dev < 0.25, "biom {i}: {} vs {}", c[i], w);
    }
    assert_eq!(biom_at(0.0), biom(0));
    assert_eq!(biom_at(REGION_M * 3.0), biom(3));
}

#[test]
fn osada_lezy_w_swoim_regionie() {
    for r in [0i64, 1, 7, 1000, -3] {
        let m = settle_m(r);
        let lo = r as f64 * REGION_M;
        assert!(m >= lo + 50.0 && m <= lo + 150.0, "region {r}: {m}");
        assert_eq!(region_of(m), r);
    }
}

#[test]
fn wspolny_generator_trzyma_wszystkie_spawny_w_regionie() {
    for region in [-100i64, -3, 0, 1, 42, 999] {
        let desc = world::region_description(region);
        assert_eq!(desc.region, region);
        assert_eq!(desc.start_m, region as f64 * REGION_M);
        assert_eq!(desc.end_m, (region as f64 + 1.0) * REGION_M);
        assert!(!desc.objects.is_empty());
        for object in desc.objects {
            assert_eq!(object.region, region);
            assert!(
                (desc.start_m..desc.end_m).contains(&object.position.along_m),
                "obiekt {:?} poza regionem {}: {}",
                object.geometry,
                region,
                object.position.along_m
            );
        }
    }
}

#[test]
fn wspolny_generator_zachowuje_tozsamosci_albumu() {
    let mut forests = 0;
    for region in 0..200i64 {
        let desc = world::region_description(region);
        if desc.biom != Biom::Las {
            continue;
        }
        forests += 1;
        let discovery = desc.discovery.expect("las musi mieć odkrycia");
        assert_eq!(discovery.mushrooms, shroom_types(region));
        assert_eq!(discovery.animal.index(), animal_idx(region));

        let mushrooms: Vec<u32> = desc
            .objects
            .iter()
            .filter_map(|object| match object.geometry {
                Geometry::Mushroom { species } => Some(species),
                _ => None,
            })
            .collect();
        assert_eq!(mushrooms, discovery.mushrooms);
        let animal = desc
            .objects
            .iter()
            .find_map(|object| match object.geometry {
                Geometry::Animal { species, .. } => Some(species),
                _ => None,
            });
        assert_eq!(animal, Some(discovery.animal));
    }
    assert!(forests >= 3, "test powinien objąć kilka lasów");
}

#[test]
fn szybka_diagnostyka_przechodzi_przez_regiony_bez_przepełnienia() {
    let simulated_m = 20_000.0_f64 / 3.6 * 60.0;
    let crossed = world::crossed_regions(0.0, simulated_m);
    assert!(crossed.len() > 1_000);
    assert_eq!(crossed.first(), Some(&0));
    assert_eq!(crossed.last(), Some(&region_of(simulated_m)));
    assert!(crossed.windows(2).all(|pair| pair[1] == pair[0] + 1));
}

#[test]
fn latarnie_maja_stabilne_zrodla_i_klosz_zgodny_z_geometria() {
    let region = (0..200i64)
        .find(|&r| {
            world::region_description(r)
                .objects
                .iter()
                .any(|object| matches!(object.geometry, Geometry::Lamp { .. }))
        })
        .expect("w zakresie testowym musi być miasto z latarnią");
    let description = world::region_description(region);
    for object in description
        .objects
        .iter()
        .filter(|object| matches!(object.geometry, Geometry::Lamp { .. }))
    {
        let Geometry::Lamp { side } = object.geometry else {
            unreachable!();
        };
        let source = object.light.expect("latarnia musi mieć źródło");
        let head = world::lamp_head_position(object.position, side);
        assert_eq!(source.id, object.id);
        assert_eq!(source.position, head);
        assert!(source.cone_inner_angle_rad < source.cone_outer_angle_rad);
        let toward_road = [0.0, world::LAMP_TARGET_Y_M - head.height_m, -head.lateral_m];
        let length = toward_road
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        let dot = source
            .direction
            .iter()
            .zip(toward_road)
            .map(|(a, b)| a * b)
            .sum::<f32>();
        assert!((dot / length - 1.0).abs() < 1e-5);
    }
}

#[test]
fn scena_dwoch_lamp_wybiera_wspolna_pozycje_i_sciane_z_generatora() {
    let anchor = crate::first_city_lamp_anchor().expect("diagnostic needs a city lamp pair");
    let mut found = Vec::new();
    let mut has_wall = false;
    for region in 0..512_i64 {
        let description = world::region_description(region);
        for object in &description.objects {
            if !matches!(object.geometry, Geometry::Lamp { .. })
                || object.position.along_m.round() as i64 != anchor.along_m
            {
                continue;
            }
            found.push(object.id);
            has_wall |= description.objects.iter().any(|other| {
                matches!(other.geometry, Geometry::House { .. })
                    && (other.position.along_m - object.position.along_m).abs() <= 28.0
            });
        }
    }
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|id| anchor.ids.contains(id)));
    assert!(
        has_wall,
        "lamp fixture should have a nearby wall for occlusion"
    );
}

#[test]
fn osade_w_srodku_regionu_mija_sie_raz() {
    let region = (0..100i64)
        .find(|&r| biom(r) == Biom::Wies)
        .expect("w zakresie testowym musi być wieś");
    let position = settle_m(region);
    let before = position - 1.0;
    let after = position + 1.0;
    assert_eq!(region_of(before), region_of(after));
    assert!(world::settlements_between(position - 10.0, before).is_empty());
    let crossed = world::settlements_between(before, after);
    assert_eq!(crossed.len(), 1);
    assert_eq!(crossed[0].region, region);
    assert_eq!(crossed[0].position_m, position);
    assert!(world::settlements_between(after, position + 10.0).is_empty());
}

#[test]
fn grzyby_i_zwierzyna_stabilne_w_zakresie() {
    for r in [0i64, 1, 42, 999] {
        let t = shroom_types(r);
        assert_eq!(t, shroom_types(r), "musi być deterministyczne");
        for &x in &t {
            assert!(x < 20, "typ grzyba {x} poza albumem 0..19");
        }
        let a = animal_idx(r);
        assert_eq!(a, animal_idx(r));
        assert!(a < 5, "indeks zwierzyny {a}");
    }
}

// ------------------------------------------------------------------ doba

#[test]
fn doba_phase_w_zakresie_i_zawija_sie_po_dobie() {
    for cam in [0.0, 1.0, 999.9, 2000.0, -50.0, 1e7] {
        for off in [0.0, 1194.42, -12345.0] {
            let p = daynight::doba_phase(cam, off);
            assert!((0.0..1.0).contains(&p), "phase {p} przy cam {cam}");
        }
    }
    assert!((daynight::doba_phase(0.0, 0.0)).abs() < EPS);
    assert!((daynight::doba_phase(daynight::DAY_M, 0.0)).abs() < EPS);
    // pełna doba dystansu = pełny obrót fazy
    let a = daynight::doba_phase(123.0, 456.0);
    let b = daynight::doba_phase(123.0 + daynight::DAY_M, 456.0);
    assert!((a - b).abs() < EPS);
}

#[test]
fn noc_i_dzien_na_swoich_fazach() {
    let night = daynight::nightness(0.75);
    let day = daynight::nightness(0.25);
    assert!(night > 0.999, "{night}");
    assert!(day < 0.001, "{day}");
    assert!((daynight::brightness(0.75) - 0.35).abs() < 1e-3);
    assert!((daynight::brightness(0.25) - 1.0).abs() < 1e-3);
    assert_eq!(daynight::phase_name(0.0), "dawn");
    assert_eq!(daynight::phase_name(0.25), "day");
    assert_eq!(daynight::phase_name(0.5), "dusk");
    assert_eq!(daynight::phase_name(0.75), "night");
    assert_eq!(daynight::phase_name(-0.3), daynight::phase_name(0.7));
}

#[test]
fn paleta_zapetla_sie_po_calym_obiegu() {
    let a = daynight::palette_at(0.0);
    let b = daynight::palette_at(1.0);
    let c = daynight::palette_at(2.5);
    assert_eq!(a.sky_top.r, b.sky_top.r);
    assert_eq!(a.sky_bot.g, b.sky_bot.g);
    let d = daynight::palette_at(0.5); // zmierzch (2,5 doby = 0,5 fazy)
    assert_eq!(c.sun.b, d.sun.b);
    assert_ne!(a.sun.b, d.sun.b); // świt != zmierzch
    for p in [0.0, 0.25, 0.5, 0.75, 0.999, -0.2] {
        let pal = daynight::palette_at(p);
        assert_eq!(pal.sky_top.a, 1.0);
        assert!(pal.sky_top.r >= 0.0 && pal.sky_top.r <= 1.0);
    }
    // init_offset: faza po starcie = godzina systemowa (6h→świt, 12h→dzień, 0h→noc)
    let hour = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_secs() % 86400) as f64 / 3600.0)
        .unwrap_or(12.0);
    let want = (hour - 6.0).rem_euclid(24.0) / 24.0;
    let p = daynight::doba_phase(500.0, daynight::init_offset(500.0));
    let dist = (p - want).abs().min(1.0 - (p - want).abs());
    assert!(
        dist < 1e-6,
        "faza {p} != oczekiwana {want} (godzina {hour})"
    );
}

// ----------------------------------------------------------------- pogoda

#[test]
fn intensywnosci_w_zakresie() {
    for i in 0..750i64 {
        let m = -500.0 + i as f64 * 7.0;
        let (rain, fog) = weather::intensity_at(m);
        assert!((0.0..=1.0).contains(&rain), "rain {rain} @ {m}");
        assert!((0.0..=1.0).contains(&fog), "fog {fog} @ {m}");
        assert!(
            rain == 0.0 || fog == 0.0,
            "nigdy jednocześnie: {rain}/{fog}"
        );
    }
}

#[test]
fn w_glebi_regionu_pelnia_a_na_granicy_sasiedni_region() {
    let r = 5i64;
    let edge = r as f64 * REGION_M;
    // w głębi (>= FADE_M od obu granic) = pełnia swojego regionu
    let mid = edge + weather::FADE_M + 1.0;
    let want = match weather::pogoda_regionu(r) {
        Pogoda::Deszcz => (1.0, 0.0),
        Pogoda::Mgla => (0.0, 1.0),
        Pogoda::Pogodnie => (0.0, 0.0),
    };
    let got = weather::intensity_at(mid);
    assert_eq!(got, want, "pełnia regionu {r}");
    // na samej granicy liczy się sąsiad z lewej (ciągłość)
    let left = match weather::pogoda_regionu(r - 1) {
        Pogoda::Deszcz => (1.0, 0.0),
        Pogoda::Mgla => (0.0, 1.0),
        Pogoda::Pogodnie => (0.0, 0.0),
    };
    assert_eq!(weather::intensity_at(edge), left);
}

#[test]
fn pogoda_70_20_10_i_nazwy() {
    let mut c = [0usize; 3];
    for r in 0..3000 {
        match weather::pogoda_regionu(r) {
            Pogoda::Pogodnie => c[0] += 1,
            Pogoda::Deszcz => c[1] += 1,
            Pogoda::Mgla => c[2] += 1,
        }
    }
    assert!((c[0] as f64 / 3000.0 - 0.70).abs() < 0.05, "{}", c[0]);
    assert!((c[1] as f64 / 3000.0 - 0.20).abs() < 0.05, "{}", c[1]);
    assert!((c[2] as f64 / 3000.0 - 0.10).abs() < 0.05, "{}", c[2]);
    assert_eq!(weather::pogoda_name(0.0, 0.0), "clear");
    assert_eq!(weather::pogoda_name(0.2, 0.0), "drizzle");
    assert_eq!(weather::pogoda_name(1.0, 0.0), "rain");
    assert_eq!(weather::pogoda_name(0.0, 0.2), "mist");
    assert_eq!(weather::pogoda_name(0.0, 1.0), "fog");
}

// ------------------------------------------------------------- kolor / shader

fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).abs();
    d.min(1.0 - d)
}

#[test]
fn hsl_i_srgb_maja_stabilny_obieg() {
    for hsl in [
        crate::render3d::material::Hsl {
            h: 0.02,
            s: 0.82,
            l: 0.44,
        },
        crate::render3d::material::Hsl {
            h: 0.31,
            s: 0.54,
            l: 0.27,
        },
        crate::render3d::material::Hsl {
            h: 0.67,
            s: 0.18,
            l: 0.71,
        },
    ] {
        let color = hsl_to_srgb(hsl);
        let back = srgb_to_hsl(color);
        assert!(
            hue_distance(back.h, hsl.h) < 1e-5,
            "hue {:?} -> {:?}",
            hsl,
            back
        );
        assert!((back.s - hsl.s).abs() < 1e-5, "sat {:?} -> {:?}", hsl, back);
        assert!(
            (back.l - hsl.l).abs() < 1e-5,
            "light {:?} -> {:?}",
            hsl,
            back
        );
    }

    let color = Color::new(0.73, 0.28, 0.11, 0.8);
    let linear = srgb_to_linear(color);
    let round_trip = linear_to_srgb(linear, color.a);
    assert!((round_trip.r - color.r).abs() < 1e-6);
    assert!((round_trip.g - color.g).abs() < 1e-6);
    assert!((round_trip.b - color.b).abs() < 1e-6);
    assert_eq!(round_trip.a, color.a);
}

#[test]
fn mokrosc_zachowuje_odcien_i_zmienia_chropowatosc() {
    let dry = surface_preset(SurfaceKind::Asphalt, 0.0);
    let wet = surface_preset(SurfaceKind::Asphalt, 1.0);
    let dry_hsl = srgb_to_hsl(dry.albedo);
    let wet_hsl = srgb_to_hsl(wet.albedo);
    assert!(hue_distance(dry_hsl.h, wet_hsl.h) < 1e-5);
    assert!(wet.roughness < dry.roughness);
    assert!(wet.albedo.r < dry.albedo.r);
    assert!(wet.albedo.g < dry.albedo.g);
    assert!(wet.albedo.b < dry.albedo.b);

    let scaled = scale_linear_srgb(Color::new(0.8, 0.35, 0.12, 1.0), 0.6);
    let a = srgb_to_linear(Color::new(0.8, 0.35, 0.12, 1.0));
    let b = srgb_to_linear(scaled);
    assert!((b.x / a.x - 0.6).abs() < 1e-5);
    assert!((b.y / a.y - 0.6).abs() < 1e-5);
    assert!((b.z / a.z - 0.6).abs() < 1e-5);
}

#[test]
fn wierzcholek_przechowuje_normalne_i_flage_emisji() {
    let material = MaterialSpec::emissive(0.23, 1.4);
    let vertex = vertex_data(
        Vec3::new(1.0, 2.0, 3.0),
        Vec3::new(0.0, 2.0, 0.0),
        Color::new(0.8, 0.4, 0.2, 0.75),
        material,
    );
    assert_eq!(vertex.color, [204, 102, 51, 191]);
    assert!((vertex.normal.x).abs() < 1e-6);
    assert!((vertex.normal.y - 1.0).abs() < 1e-6);
    assert!((vertex.normal.z).abs() < 1e-6);
    assert_eq!(
        vertex.normal.w as u8,
        MaterialFlags::UNLIT
            .with(MaterialFlags::EMISSIVE)
            .with(MaterialFlags::NO_SHADOW)
            .bits()
    );
    assert!((vertex.uv.x - 0.23).abs() < 1e-6);
    assert!((vertex.uv.y - 1.4).abs() < 1e-6);
    assert!(MaterialSpec::unlit().flags.contains(MaterialFlags::NO_FOG));
    assert!(
        MaterialSpec::unlit()
            .flags
            .contains(MaterialFlags::NO_SHADOW)
    );
}

#[test]
fn diagnostyka_przyjmuje_tylko_skonczona_liczbe_i_znane_scene() {
    assert_eq!(crate::finite_speed("4.0"), Some(4.0));
    assert_eq!(crate::finite_speed("-2.0"), None);
    assert_eq!(crate::finite_speed("NaN"), None);
    assert_eq!(crate::finite_speed("inf"), None);
    assert_eq!(crate::finite_speed("1e100"), None);
    assert_eq!(crate::finite_speed("4,0"), None);
    assert_eq!(
        crate::DiagnosticScene::parse("night"),
        Some(crate::DiagnosticScene::Night)
    );
    assert_eq!(
        crate::DiagnosticScene::parse("rain"),
        Some(crate::DiagnosticScene::Rain)
    );
    assert_eq!(
        crate::DiagnosticScene::parse("physics"),
        Some(crate::DiagnosticScene::Physics)
    );
    assert_eq!(
        crate::DiagnosticScene::parse("two-lights"),
        Some(crate::DiagnosticScene::TwoLights)
    );
    assert_eq!(
        crate::DiagnosticScene::parse("boundary"),
        Some(crate::DiagnosticScene::Boundary)
    );
    assert_eq!(crate::DiagnosticScene::parse("nope"), None);
    assert_eq!(
        crate::render3d::Quality::parse("low"),
        Some(crate::render3d::Quality::Low)
    );
    assert_eq!(
        crate::render3d::Quality::parse("high"),
        Some(crate::render3d::Quality::High)
    );
    assert_eq!(crate::render3d::Quality::parse("ultra"), None);
    assert_eq!(crate::bounded_capture_frames("0"), None);
    assert_eq!(crate::bounded_capture_frames("60"), Some(60));
    assert_eq!(crate::bounded_duration("NaN"), None);
    assert_eq!(crate::bounded_duration("5"), Some(5.0));
}

#[test]
fn parser_cli_rejects_invalid_controls_before_diagnostic_state_exists() {
    let valid = crate::parse_cli_args([
        "--2d",
        "--scene",
        "boundary",
        "--quality",
        "high",
        "--capture",
        "/tmp/scene.png",
        "--capture-frames",
        "12",
    ])
    .expect("valid diagnostic options");
    assert_eq!(valid.scene, Some(crate::DiagnosticScene::Boundary));
    assert_eq!(valid.quality, crate::render3d::Quality::High);
    assert_eq!(valid.capture_frames, Some(12));

    for arguments in [
        vec!["--scene", "nope"],
        vec!["--quality", "ultra"],
        vec!["--capture", ""],
        vec!["--capture-frames", "0"],
        vec!["--capture-frames", "3"],
        vec!["--benchmark", "NaN"],
        vec!["--sim", "-1"],
        vec!["--sim", "1e100"],
        vec!["--unknown"],
    ] {
        assert!(
            crate::parse_cli_args(arguments).is_err(),
            "accepted invalid arguments"
        );
    }
    assert!(crate::parse_cli_args(["--sim", "20"]).is_ok());
    assert!(crate::parse_cli_args(["--2d", "--3d"]).is_ok());
}

#[test]
fn diagnostyczny_zegar_posuwa_sie_przy_zatrzymanej_tasmie_i_jest_powtarzalny() {
    let capture_clock = || {
        (0..90).fold(0.0_f32, |time, _| {
            crate::advance_diagnostic_time(time, crate::CAPTURE_FRAME_DT)
        })
    };
    let first = capture_clock();
    let second = capture_clock();
    assert_eq!(first, second, "stały krok capture musi być powtarzalny");
    assert!((first - 1.5).abs() < 1e-5, "czas symulacji: {first}");

    let treadmill_speed = 0.0_f32;
    let stopped_clock = (0..3).fold(0.0_f32, |time, _| {
        crate::advance_diagnostic_time(time, crate::CAPTURE_FRAME_DT)
    });
    assert_eq!(treadmill_speed, 0.0);
    assert!(stopped_clock > 0.0, "efekty nie mogą zamarzać na postoju");
}

#[test]
fn benchmark_nie_przycina_dlugiej_klatki_w_rozliczeniu() {
    let start = std::time::Instant::now();
    let end = start + std::time::Duration::from_millis(250);
    assert!((crate::monotonic_elapsed_s(start, end) - 0.25).abs() < 1e-12);
    assert_eq!(crate::render3d::raw_frame_ms(0.25), 250.0);
}

// ------------------------------------------------------- tekst po polsku

#[test]
fn liczby_z_kropka() {
    assert_eq!(pl(10.2, 1), "10.2");
    assert_eq!(pl(1000.5, 2), "1000.50");
    assert_eq!(pl(0.0, 0), "0");
    assert_eq!(pl(-1.25, 2), "-1.25");
    assert_eq!(pl(4.0, 2), "4.00");
    assert!(
        !pl(3.17, 2).contains(','),
        "nigdy przecinek: {}",
        pl(3.17, 2)
    );
}

#[test]
fn format_postepu_questu() {
    let q = QuestState {
        id: "walk05".into(),
        progress: 0.0,
        goal: 0.5,
        done: false,
    };
    assert_eq!(format_quest_progress(&q), "0.00/0.5 km");
    let q = QuestState {
        id: "forest300".into(),
        progress: 150.0,
        goal: 300.0,
        done: false,
    };
    assert_eq!(format_quest_progress(&q), "150/300 m");
    let q = QuestState {
        id: "tempo5".into(),
        progress: 65.0,
        goal: 300.0,
        done: false,
    };
    assert_eq!(format_quest_progress(&q), "1:05/5:00");
    let q = QuestState {
        id: "village2".into(),
        progress: 1.0,
        goal: 2.0,
        done: false,
    };
    assert_eq!(format_quest_progress(&q), "1/2");
}

// ----------------------------------------------------------------- kurier

#[test]
fn kurier_odbior_limit_i_dostawa_fresh() {
    let mut s = base();
    let start = world::SettlementDesc {
        region: 3,
        kind: Biom::Wies,
        position_m: 950.0,
    };
    crossing(&mut s, &start, start.position_m);
    assert_eq!(s.parcels.len(), 1, "paczka po pierwszej osadzie");
    let t1 = s.parcels[0].target_region;
    // druga osada (dalsza niż cel) → druga paczka do limitu
    let (r2, m2, k2) = crate::courier::next_settlement_after(t1).expect("osada istnieje");
    assert_ne!(r2, t1);
    crossing(
        &mut s,
        &world::SettlementDesc {
            region: r2,
            kind: k2,
            position_m: m2,
        },
        m2,
    );
    assert_eq!(s.parcels.len(), 2, "druga paczka do limitu");
    // trzecia osada: jej celem jest paczka z kroku drugiego → dostawa + łańcuch
    let (r3, m3, k3) = crate::courier::next_settlement_after(r2).unwrap();
    crossing(
        &mut s,
        &world::SettlementDesc {
            region: r3,
            kind: k3,
            position_m: m3,
        },
        m3,
    );
    assert_eq!(s.deliveries, 1, "paczka #2 doręczona dokładnie raz");
    assert_eq!(s.parcels.len(), 2, "po łańcuchu znowu 2");
    // dostawa do t1 na czas → fresh = bazowa nagroda × 1.4
    let pay_expected = s
        .parcels
        .iter()
        .find(|p| p.target_region == t1)
        .unwrap()
        .reward
        * 1.4;
    let gold_before = s.gold;
    crossing(
        &mut s,
        &world::SettlementDesc {
            region: t1,
            kind: Biom::Wies,
            position_m: crate::rng::settle_m(t1),
        },
        crate::rng::settle_m(t1),
    );
    assert_eq!(s.deliveries, 2, "paczka #1 doręczona");
    assert!(
        (s.gold - gold_before - pay_expected).abs() < EPS,
        "fresh bonus = reward × 1.4"
    );
}

fn crossing(s: &mut Save, sett: &world::SettlementDesc, runner_m: f64) {
    crate::courier::handle_crossings(s, std::slice::from_ref(sett), runner_m);
}

#[test]
fn nazwy_miejscowosci_deterministyczne() {
    for r in [3i64, 42, -7, 99999] {
        let a = crate::rng::place_name(r);
        let b = crate::rng::place_name(r);
        assert_eq!(a, b, "region {r}: determinizm");
        assert!(a.chars().count() >= 5, "nazwa {a} ma sens");
    }
}

// -------------------------------------------------- latarnie / cienie / świat

#[test]
fn harmonogram_latarni_przelacznik_okno() {
    // pełny dzień (rano → zmierzch): zero
    for phase in [0.10_f64, 0.25, 0.30, 0.45, 0.50] {
        assert_eq!(daynight::lamp_schedule(phase), 0.0, "dzień {phase}");
    }
    // pełna noc (wieczór → rano): pełny zapłon
    for phase in [0.61_f64, 0.75, 0.90, 0.05, 0.0] {
        assert!(
            daynight::lamp_schedule(phase) > 0.95,
            "noc {phase} ma świecić"
        );
    }
    // zápłon i zgaszenie to krótkie przejścia, nie rozmycie na pół nocy
    let mid = daynight::lamp_schedule(0.533);
    assert!((0.05..0.95).contains(&mid), "fade przejściowy, on={mid:.3}");
}

#[test]
fn frustum_cieni_kwantomizowany_do_schodkow4m() {
    for dir in [
        Vec3::new(0.3, 0.22, 0.4).normalize(),
        Vec3::new(0.42, 0.31, -0.2).normalize(),
        Vec3::new(0.1, 0.28, 0.5).normalize(),
    ] {
        let e = crate::render3d::shadow::directional_half_extent(dir);
        assert!(
            e >= 48.0 - 1e-3 && e <= 64.0 + 1e-3,
            "połówka poza limitem: {e}"
        );
        let snapped = (e / 4.0).round() * 4.0;
        assert!(
            (e - snapped).abs() < 1e-4,
            "texel-grid nie może reskalować co klatkę (pływające cienie): {e}"
        );
    }
}

#[test]
fn swiat_urozmaicen_kamienie_kwiaty_parkany() {
    let mut laka = None;
    for region in 0..64i64 {
        if biom(region) == Biom::Laka {
            laka = Some(region);
            break;
        }
    }
    let laka = laka.expect("łąka w pierwszych 64 regionach");
    let desc = world::region_description(laka);
    assert!(
        desc.objects
            .iter()
            .any(|o| matches!(o.geometry, Geometry::Rock { .. })),
        "łąka ma kamień"
    );
    assert!(
        desc.objects
            .iter()
            .any(|o| matches!(o.geometry, Geometry::FlowerPatch { .. })),
        "łąka ma kwiaty"
    );

    let mut wies = None;
    for region in 0..64i64 {
        if biom(region) == Biom::Wies {
            wies = Some(region);
            break;
        }
    }
    let wies = wies.expect("wieś w pierwszych 64 regionach");
    let desc = world::region_description(wies);
    assert!(
        desc.objects
            .iter()
            .any(|o| matches!(o.geometry, Geometry::FenceSegment { .. })),
        "wieś ma parkan"
    );

    let las_rock_ok = world::region_description(mushroomless_las())
        .objects
        .iter()
        .any(|o| matches!(o.geometry, Geometry::Rock { .. }));
    assert!(las_rock_ok, "las ma kamień");
}

fn mushroomless_las() -> i64 {
    for region in 0..64i64 {
        if biom(region) == Biom::Las {
            return region;
        }
    }
    panic!("las w pierwszych 64 regionach");
}
