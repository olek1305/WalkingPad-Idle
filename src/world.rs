//! Wspólny, deterministyczny opis świata w metrach.
//!
//! Ten moduł jest źródłem prawdy dla widocznych obiektów regionu, odkryć
//! albumu oraz przyszłych colliderów i świateł. Renderowanie nie losuje
//! własnych pozycji ani tożsamości obiektów.

use crate::rng::{Biom, REGION_M, Rng, animal_idx, biom, hash1, region_of, settle_m, shroom_types};

/// Rodzaj geometrii rozpoznawany przez renderer. Parametry są opisem spawnu,
/// a nie stanem renderera, więc później mogą zostać użyte przez fizykę.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Geometry {
    Fir {
        scale: f32,
    },
    Bush {
        radius: f32,
    },
    Mushroom {
        species: u32,
    },
    Animal {
        species: AnimalSpecies,
        seed: u32,
    },
    House {
        city: bool,
        height: f32,
        half_x: f32,
        half_z: f32,
    },
    SettlementSign {
        city: bool,
    },
    Lamp {
        side: f32,
    },
    Signal {
        side: f32,
    },
    /// Bryła skalna w łące/lesie — urozmaica puste trawniki.
    Rock {
        scale: f32,
    },
    /// Kwiatowa łata: kolorowy zbitek mikro-kostek w łące.
    FlowerPatch {
        tint: u32,
    },
    /// Parkan: N słupków + dwie szyny (ciąg ~12 m) w wsi.
    FenceSegment {
        posts: u32,
    },
}

/// Prosty materiał bazowy obiektu. Etap 1 nie zmienia obecnego cieniowania,
/// ale opis pozostaje wspólny dla renderera, colliderów i oświetlenia.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialKind {
    Foliage,
    Mushroom,
    Animal,
    House,
    Sign,
    Metal,
    Signal,
    Rock,
    Fence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColliderKind {
    None,
    Static,
}

/// Pozycja klosza latarni w absolutnym układzie świata.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightPosition {
    pub along_m: f64,
    pub lateral_m: f32,
    pub height_m: f32,
}

/// Geometria wspólna dla modelu latarni i jej źródła światła. Klosz jest
/// umieszczony na końcu ramienia skierowanego do osi drogi, a kierunek źródła
/// celuje w punkt drogi. Renderer, selektor lamp i przyszłe mapy cieni
/// korzystają z tych samych wartości.
pub const LAMP_POLE_HEIGHT_M: f32 = 5.2;
pub const LAMP_HEAD_Y_M: f32 = LAMP_POLE_HEIGHT_M - 0.22;
pub const LAMP_POLE_SETBACK_M: f32 = 0.5;
pub const LAMP_ARM_REACH_M: f32 = 0.9;
pub const LAMP_TARGET_Y_M: f32 = 0.04;
pub const LAMP_INNER_ANGLE_RAD: f32 = 0.38;
pub const LAMP_OUTER_ANGLE_RAD: f32 = 0.86;
pub const LAMP_LED_COLOR_SRGB: [f32; 3] = [1.0, 0.94, 0.86];

pub fn lamp_head_offset(side: f32) -> [f32; 3] {
    [0.0, LAMP_HEAD_Y_M, -side * LAMP_ARM_REACH_M]
}

pub fn lamp_head_position(base: WorldPosition, side: f32) -> LightPosition {
    let offset = lamp_head_offset(side);
    LightPosition {
        along_m: base.along_m,
        lateral_m: base.lateral_m + offset[2],
        height_m: offset[1],
    }
}

pub fn lamp_direction(head_lateral_m: f32) -> [f32; 3] {
    let x = 0.0;
    let y = LAMP_TARGET_Y_M - LAMP_HEAD_Y_M;
    let z = -head_lateral_m;
    let length = (x * x + y * y + z * z).sqrt().max(f32::EPSILON);
    [x / length, y / length, z / length]
}

/// Pełny opis lokalnego źródła. Kolor jest bazowym sRGB, a konwersja do
/// liniowego RGB odbywa się dopiero przy bindowaniu uniformów GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightSource {
    pub id: u64,
    pub position: LightPosition,
    pub direction: [f32; 3],
    pub cone_inner_angle_rad: f32,
    pub cone_outer_angle_rad: f32,
    pub color: [f32; 3],
    pub range_m: f32,
    pub intensity: f32,
}

/// Pozycja wzdłuż trasy jest absolutna i ma typ f64. Lateral pozostaje w
/// metrach względem osi drogi.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldPosition {
    pub along_m: f64,
    pub lateral_m: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObjectDesc {
    pub id: u64,
    pub region: i64,
    pub position: WorldPosition,
    pub geometry: Geometry,
    pub material: MaterialKind,
    pub collider: ColliderKind,
    pub light: Option<LightSource>,
    pub casts_shadow: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimalSpecies {
    Owl = 0,
    Deer = 1,
    Hedgehog = 2,
    Bear = 3,
    Wolf = 4,
}

impl AnimalSpecies {
    pub fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Owl,
            1 => Self::Deer,
            2 => Self::Hedgehog,
            3 => Self::Bear,
            _ => Self::Wolf,
        }
    }

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Tożsamości zapisywane w albumie. Dzienny jeleń dla sowy jest tylko
/// wyglądem zastępczym; odkrycie nadal ma tożsamość `Owl`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionDiscovery {
    pub mushrooms: [u32; 4],
    pub animal: AnimalSpecies,
}

#[derive(Clone, Debug)]
pub struct RegionDesc {
    pub region: i64,
    pub biom: Biom,
    pub start_m: f64,
    pub end_m: f64,
    pub objects: Vec<ObjectDesc>,
    pub discovery: Option<RegionDiscovery>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettlementDesc {
    pub region: i64,
    pub kind: Biom,
    pub position_m: f64,
}

const ROAD_HALF_M: f32 = 2.6;

fn object_id(region: i64, salt: u64, index: u64) -> u64 {
    hash1((region as u64) ^ salt ^ index.wrapping_mul(0x9E37_79B9))
}

struct ObjectSpec {
    geometry: Geometry,
    along_m: f64,
    lateral_m: f32,
    material: MaterialKind,
    collider: ColliderKind,
    light: Option<LightSource>,
    casts_shadow: bool,
}

fn object(region: i64, index: u64, spec: ObjectSpec) -> ObjectDesc {
    let id = object_id(region, 0xD35C_7101, index);
    ObjectDesc {
        id,
        region,
        position: WorldPosition {
            along_m: spec.along_m,
            lateral_m: spec.lateral_m,
        },
        geometry: spec.geometry,
        material: spec.material,
        collider: spec.collider,
        light: spec.light.map(|mut source| {
            source.id = id;
            source
        }),
        casts_shadow: spec.casts_shadow,
    }
}

fn tree_objects(region: i64, count: u64, out: &mut Vec<ObjectDesc>) {
    for j in 0..count {
        let h = hash1(region as u64 ^ 0x9A17 ^ j.wrapping_mul(0x85EB_CA6B));
        let along_m = region as f64 * REGION_M + 8.0 + (h % 284) as f64;
        let side = if (h >> 9) & 1 == 0 { -1.0 } else { 1.0 };
        let lateral_m = side * (5.5 + ((h >> 16) % 30) as f32);
        let scale = 0.8 + ((h >> 24) % 40) as f32 / 100.0;
        out.push(object(
            region,
            j,
            ObjectSpec {
                geometry: Geometry::Fir { scale },
                along_m,
                lateral_m,
                material: MaterialKind::Foliage,
                collider: ColliderKind::Static,
                light: None,
                casts_shadow: true,
            },
        ));
    }
}

fn bush_objects(region: i64, out: &mut Vec<ObjectDesc>) {
    for j in 0..4u64 {
        let h = hash1(region as u64 ^ 0xB05 ^ j.wrapping_mul(0x9E37));
        let along_m = region as f64 * REGION_M + 8.0 + (h % 284) as f64;
        let side = if (h >> 9) & 1 == 0 { -1.0 } else { 1.0 };
        let lateral_m = side * (6.0 + ((h >> 16) % 30) as f32);
        let radius = 0.7 + ((h >> 24) % 40) as f32 / 40.0;
        out.push(object(
            region,
            100 + j,
            ObjectSpec {
                geometry: Geometry::Bush { radius },
                along_m,
                lateral_m,
                material: MaterialKind::Foliage,
                collider: ColliderKind::Static,
                light: None,
                casts_shadow: true,
            },
        ));
    }
}

fn mushroom_spawns(region: i64) -> [(f64, u32); 4] {
    let mut out = [(0.0, 0); 4];
    for j in 0..4u64 {
        let mut rng = Rng::new(region as u64 ^ 0x9A17 ^ j.wrapping_mul(0x85EB_CA6B));
        // To samo range → below co shroom_types(); zmieniamy tylko pozycję na
        // metry, aby każdy grzyb pozostał w swoim regionie 300 m.
        let jitter = rng.range(0.0, 120.0);
        let species = rng.below(20) as u32;
        let local = 12.0 + j as f64 * 66.0 + jitter * 0.30;
        out[j as usize] = (region as f64 * REGION_M + local, species);
    }
    out
}

fn animal_spawn(region: i64) -> (f64, f32, AnimalSpecies, u32) {
    let mut rng = Rng::new(region as u64 ^ 0xA41E);
    let old_range = rng.range(300.0, 1200.0);
    let category = rng.below(100);
    let seed = rng.below(7) as u32;
    let local = 30.0 + ((old_range - 300.0) / 900.0) * 220.0;
    let hash = hash1(region as u64 ^ 0xA41F);
    let side = if (hash >> 9) & 1 == 0 { -1.0 } else { 1.0 };
    let lateral = side * (6.5 + ((hash >> 16) % 9) as f32);
    let species = match category {
        0..40 => AnimalSpecies::from_index(animal_idx(region)),
        40..55 => AnimalSpecies::Deer,
        55..80 => AnimalSpecies::Hedgehog,
        80..92 => AnimalSpecies::Bear,
        _ => AnimalSpecies::Wolf,
    };
    (region as f64 * REGION_M + local, lateral, species, seed)
}

fn settlement_objects(region: i64, kind: Biom, out: &mut Vec<ObjectDesc>) {
    let city = kind == Biom::Miasto;
    let settlement_m = settle_m(region);
    let hash = hash1(region as u64 ^ 0x70);
    let side = if (hash >> 9) & 1 == 0 { -1.0 } else { 1.0 };
    for j in 0..3u64 {
        let h = hash1(region as u64 ^ 0x517E ^ j.wrapping_mul(0x9E37));
        let spacing = if city { 11.0 } else { 9.0 };
        let along_m = settlement_m + (j as f64 - 1.0) * spacing;
        let lateral_m = side
            * if city {
                11.0 + (h % 5) as f32
            } else {
                9.5 + (h % 4) as f32
            };
        let height = if city {
            6.0 + (h % 6) as f32 * 2.5
        } else {
            3.4
        };
        let (half_x, half_z) = if city { (3.0, 3.0) } else { (4.2, 3.6) };
        out.push(object(
            region,
            200 + j,
            ObjectSpec {
                geometry: Geometry::House {
                    city,
                    height,
                    half_x,
                    half_z,
                },
                along_m,
                lateral_m,
                material: MaterialKind::House,
                collider: ColliderKind::Static,
                light: None,
                casts_shadow: true,
            },
        ));
    }
    out.push(object(
        region,
        210,
        ObjectSpec {
            geometry: Geometry::SettlementSign { city },
            along_m: settlement_m - 6.0,
            lateral_m: side * (ROAD_HALF_M + 1.7),
            material: MaterialKind::Sign,
            collider: ColliderKind::Static,
            light: None,
            casts_shadow: true,
        },
    ));
}

/// Kamienie w łące i lesie: przerywają pustą płaszczyznę, jedna bryła czuwa
/// z hasha regionu (deterministycznie, jak cała generacja).
fn rock_objects(region: i64, out: &mut Vec<ObjectDesc>) {
    for j in 0..3u64 {
        let h = hash1(region as u64 ^ 0x7_A0C1 ^ j.wrapping_mul(0x9E37_79B9));
        let along_m = region as f64 * REGION_M + 6.0 + (h % 288) as f64;
        let side = if (h >> 8) & 1 == 0 { -1.0 } else { 1.0 };
        let lateral_m = side * (4.6 + ((h >> 18) % 26) as f32);
        let scale = 0.5 + ((h >> 26) % 90) as f32 / 100.0;
        out.push(object(
            region,
            500 + j,
            ObjectSpec {
                geometry: Geometry::Rock { scale },
                along_m,
                lateral_m,
                material: MaterialKind::Rock,
                collider: ColliderKind::Static,
                light: None,
                casts_shadow: true,
            },
        ));
    }
}

/// Kwiatowe łaty w łące: kolorowy kleks blisko skarpy, bez kolizji.
fn flower_objects(region: i64, out: &mut Vec<ObjectDesc>) {
    for j in 0..2u64 {
        let h = hash1(region as u64 ^ 0xF105 ^ j.wrapping_mul(0x77F1));
        let along_m = region as f64 * REGION_M + 20.0 + (h % 260) as f64;
        let side = if (h >> 7) & 1 == 0 { -1.0 } else { 1.0 };
        let lateral_m = side * (5.0 + ((h >> 15) % 10) as f32);
        out.push(object(
            region,
            510 + j,
            ObjectSpec {
                geometry: Geometry::FlowerPatch {
                    tint: ((h >> 21) % 5) as u32,
                },
                along_m,
                lateral_m,
                material: MaterialKind::Foliage,
                collider: ColliderKind::None,
                light: None,
                casts_shadow: false,
            },
        ));
    }
}

/// Parkany w wsi — po stronie przeciwnej do domów (naturalna asymetria).
fn fence_objects(region: i64, out: &mut Vec<ObjectDesc>) {
    let hash = hash1(region as u64 ^ 0x70);
    let side = if (hash >> 9) & 1 == 0 { -1.0 } else { 1.0 };
    let fence_side = -side;
    let settle = settle_m(region);
    for j in 0..2u64 {
        out.push(object(
            region,
            520 + j,
            ObjectSpec {
                geometry: Geometry::FenceSegment { posts: 8 },
                along_m: settle + if j == 0 { -34.0 } else { 34.0 },
                lateral_m: fence_side * (ROAD_HALF_M + 1.5),
                material: MaterialKind::Fence,
                collider: ColliderKind::None,
                light: None,
                casts_shadow: true,
            },
        ));
    }
}

fn light_objects(region: i64, kind: Biom, out: &mut Vec<ObjectDesc>) {
    let (spacing, both) = match kind {
        Biom::Miasto => (26.0, true),
        Biom::Wies => (58.0, false),
        _ => return,
    };
    let count = (REGION_M as f32 / spacing).floor() as i32;
    for i in 0..count {
        let h = hash1(region as u64 ^ 0x1A7E5 ^ (i as u64).wrapping_mul(0x09E3_79B9));
        let along_m = region as f64 * REGION_M + 10.0 + i as f64 * spacing as f64;
        let side0 = if (h >> 9) & 1 == 0 { -1.0 } else { 1.0 };
        let sides = [side0, if both { -side0 } else { side0 }];
        for (copy, side) in sides.into_iter().enumerate().take(if both { 2 } else { 1 }) {
            let lateral_m = side * (ROAD_HALF_M + LAMP_POLE_SETBACK_M);
            let base_position = WorldPosition { along_m, lateral_m };
            let head = lamp_head_position(base_position, side);
            out.push(object(
                region,
                300 + i as u64 * 2 + copy as u64,
                ObjectSpec {
                    geometry: Geometry::Lamp { side },
                    along_m,
                    lateral_m,
                    material: MaterialKind::Metal,
                    collider: ColliderKind::Static,
                    light: Some(LightSource {
                        id: 0,
                        position: head,
                        direction: lamp_direction(head.lateral_m),
                        cone_inner_angle_rad: LAMP_INNER_ANGLE_RAD,
                        cone_outer_angle_rad: LAMP_OUTER_ANGLE_RAD,
                        color: LAMP_LED_COLOR_SRGB,
                        range_m: 12.0,
                        intensity: 5.5,
                    }),
                    casts_shadow: true,
                },
            ));
        }
    }
    if kind == Biom::Miasto {
        let along_m = region as f64 * REGION_M + 70.0;
        for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
            out.push(object(
                region,
                350 + i as u64,
                ObjectSpec {
                    geometry: Geometry::Signal { side },
                    along_m,
                    lateral_m: side * (ROAD_HALF_M + 1.5),
                    material: MaterialKind::Signal,
                    collider: ColliderKind::Static,
                    light: None,
                    casts_shadow: true,
                },
            ));
        }
    }
}

/// Opis wszystkich obiektów regionu. Każda pozycja wzdłuż trasy należy do
/// `[region * REGION_M, (region + 1) * REGION_M)`.
pub fn region_description(region: i64) -> RegionDesc {
    let kind = biom(region);
    let mut objects = Vec::new();
    tree_objects(region, if kind == Biom::Las { 12 } else { 8 }, &mut objects);
    match kind {
        Biom::Laka => {
            bush_objects(region, &mut objects);
            rock_objects(region, &mut objects);
            flower_objects(region, &mut objects);
        }
        Biom::Las => {
            for (j, (along_m, species)) in mushroom_spawns(region).into_iter().enumerate() {
                objects.push(object(
                    region,
                    400 + j as u64,
                    ObjectSpec {
                        geometry: Geometry::Mushroom { species },
                        along_m,
                        lateral_m: if j % 2 == 0 { -4.5 } else { 4.5 },
                        material: MaterialKind::Mushroom,
                        collider: ColliderKind::None,
                        light: None,
                        casts_shadow: true,
                    },
                ));
            }
            let (along_m, lateral_m, species, seed) = animal_spawn(region);
            objects.push(object(
                region,
                410,
                ObjectSpec {
                    geometry: Geometry::Animal { species, seed },
                    along_m,
                    lateral_m,
                    material: MaterialKind::Animal,
                    collider: ColliderKind::None,
                    light: None,
                    casts_shadow: true,
                },
            ));
            rock_objects(region, &mut objects);
        }
        Biom::Wies | Biom::Miasto => settlement_objects(region, kind, &mut objects),
    }
    if kind == Biom::Wies {
        fence_objects(region, &mut objects);
    }
    light_objects(region, kind, &mut objects);
    RegionDesc {
        region,
        biom: kind,
        start_m: region as f64 * REGION_M,
        end_m: (region as f64 + 1.0) * REGION_M,
        objects,
        discovery: discovery_for_region(region),
    }
}

/// Tożsamości odkrywane przy wejściu do lasu. Zawsze te same co istniejące
/// funkcje albumu w `rng`, a wygląd dzienny nie zmienia identyfikatora.
pub fn discovery_for_region(region: i64) -> Option<RegionDiscovery> {
    if biom(region) != Biom::Las {
        return None;
    }
    Some(RegionDiscovery {
        mushrooms: shroom_types(region),
        animal: AnimalSpecies::from_index(animal_idx(region)),
    })
}

/// Regiony przecięte przez ruch do przodu. Przy przejściu granicy zwracamy
/// także region poprzedni, tak jak dotychczasowa logika albumu.
pub fn crossed_regions(from_m: f64, to_m: f64) -> Vec<i64> {
    if to_m <= from_m {
        return Vec::new();
    }
    (region_of(from_m)..=region_of(to_m)).collect()
}

/// Osady przecięte przez odcinek ruchu. Warunek półotwarty od strony startu
/// zapewnia jedno rozliczenie, gdy poprzednia klatka już minęła punkt osady.
pub fn settlements_between(from_m: f64, to_m: f64) -> Vec<SettlementDesc> {
    if to_m <= from_m {
        return Vec::new();
    }
    let mut out = Vec::new();
    for region in region_of(from_m)..=region_of(to_m) {
        let kind = biom(region);
        if !matches!(kind, Biom::Wies | Biom::Miasto) {
            continue;
        }
        let position_m = settle_m(region);
        if from_m < position_m && position_m <= to_m {
            out.push(SettlementDesc {
                region,
                kind,
                position_m,
            });
        }
    }
    out
}
