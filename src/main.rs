//! Idle WalkingPad 3D — biegasz (taśma), gold kapie sam, klikasz tylko SKLEP.
//! Grafika proceduralna: bryły 3D i UI bez zewnętrznych assetów.
//! Ekonomia 1:1 — 4 km/h = 4 gold/min (x Buty, + Skarbonka, x5 kryty z Pulsometru,
//! + premia za kilometry z Trasy).
//!   Prędkość z pliku mostka (`--speed-file /tmp/walkingpad-speed`) albo `--sim <km/h>`.

use macroquad::prelude::*;
use std::collections::VecDeque;
use std::fs;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

mod courier;
mod daynight;
mod evolution;
mod music;
mod physics;
mod quests;
mod render3d;
mod rng;
mod save;
#[cfg(test)]
mod tests;
mod text;
mod theme;
mod ui;
mod weather;
mod world;

use courier::Event;
use rng::{Biom, biom_at, region_of};
use save::{
    Save, income_per_min, level_for_km, level_title, load_save, store_save, take_milestone,
    tick_gold,
};
use text::{FONT_PATHS, pl};
use theme::*;
use ui::fx::{Coin, Popup};
use ui::{album, fx, hud, quests_box, tree};

const DEFAULT_SPEED_FILE: &str = "/tmp/walkingpad-speed";
const STALE_SECS: u64 = 5;
/// `--sim` is a diagnostic input, not a hardware limit.  The bound keeps an
/// accidental finite value from overflowing local f32 state or region loops.
const MAX_SIM_SPEED_KMH: f64 = 20_000.0;
const CAPTURE_FRAME_DT: f32 = 1.0 / 60.0;
const BENCHMARK_WARMUP_S: f64 = 1.0;

static VALIDATED_CLI: OnceLock<Result<CliArgs, String>> = OnceLock::new();

fn validated_cli() -> Result<CliArgs, String> {
    VALIDATED_CLI
        .get_or_init(|| parse_cli_args(std::env::args().skip(1)))
        .clone()
}

fn window_conf() -> Conf {
    let cli = match validated_cli() {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("Argument error: {error}");
            std::process::exit(2);
        }
    };
    if cli.evolution {
        Conf {
            window_title: "WalkingPad — evolution 3D".to_string(),
            window_width: 1280,
            window_height: 720,
            ..Default::default()
        }
    } else {
        Conf {
            window_title: "Idle WalkingPad 3D".to_string(),
            window_width: 1280,
            window_height: 720,
            ..Default::default()
        }
    }
}

/// (speed, świeży). Stary/brak pliku = 0.
pub(crate) fn read_speed(path: &str) -> (f64, bool) {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return (0.0, false),
    };
    let speed = text
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|speed| speed.is_finite())
        .unwrap_or(0.0)
        .max(0.0);
    let fresh = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|mtime| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|now| now.as_secs().saturating_sub(mtime.as_secs()) <= STALE_SECS)
        })
        .unwrap_or(false);
    if fresh { (speed, true) } else { (0.0, false) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiagnosticScene {
    Day,
    Dawn,
    Night,
    Rain,
    Fog,
    Physics,
    TwoLights,
    Boundary,
}

impl DiagnosticScene {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "day" => Some(Self::Day),
            "dawn" => Some(Self::Dawn),
            "night" => Some(Self::Night),
            "rain" => Some(Self::Rain),
            "fog" => Some(Self::Fog),
            "physics" => Some(Self::Physics),
            "two-lights" => Some(Self::TwoLights),
            "boundary" => Some(Self::Boundary),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct CliArgs {
    evolution: bool,
    sim: Option<f64>,
    speed_file: String,
    scene: Option<DiagnosticScene>,
    capture: Option<String>,
    capture_frames: Option<u32>,
    benchmark: Option<f64>,
    quality: render3d::Quality,
}

fn finite_speed(value: &str) -> Option<f64> {
    value
        .parse::<f64>()
        .ok()
        .filter(|speed| speed.is_finite() && (0.0..=MAX_SIM_SPEED_KMH).contains(speed))
}

fn bounded_duration(value: &str) -> Option<f64> {
    value
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && (0.1..=3600.0).contains(value))
}

fn bounded_capture_frames(value: &str) -> Option<u32> {
    value
        .parse::<u32>()
        .ok()
        .filter(|frames| (1..=60_000).contains(frames))
}

fn advance_diagnostic_time(time: f32, simulation_dt: f32) -> f32 {
    time + simulation_dt.max(0.0)
}

fn monotonic_elapsed_s(start: Instant, now: Instant) -> f64 {
    now.duration_since(start).as_secs_f64()
}

fn parse_cli_args<I, S>(arguments: I) -> Result<CliArgs, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let a: Vec<String> = arguments.into_iter().map(Into::into).collect();
    let mut evolution = false;
    let mut sim = None;
    let mut file = DEFAULT_SPEED_FILE.to_string();
    let mut scene = None;
    let mut capture = None;
    let mut capture_frames = None;
    let mut benchmark = None;
    let mut quality = render3d::Quality::default();
    let mut i = 0;
    while i < a.len() {
        match a[i].as_str() {
            // Flagi zachowane jako zgodność ze starymi launcherami. Świat
            // zawsze rysuje renderer 3D.
            "--3d" | "--2d" => {}
            "--evolution" => {
                evolution = true;
            }
            "--sim" => {
                let value = a
                    .get(i + 1)
                    .ok_or_else(|| "--sim requires a speed in km/h".to_string())?;
                sim = Some(finite_speed(value).ok_or_else(|| {
                    format!(
                        "invalid --sim {:?}; expected a finite value 0..{} km/h",
                        value, MAX_SIM_SPEED_KMH
                    )
                })?);
                i += 1;
            }
            "--speed-file" => {
                let value = a
                    .get(i + 1)
                    .ok_or_else(|| "--speed-file requires a path".to_string())?;
                if value.is_empty() {
                    return Err("--speed-file must not be empty".to_string());
                }
                file = value.clone();
                i += 1;
            }
            "--scene" => {
                let value = a
                    .get(i + 1)
                    .ok_or_else(|| "--scene requires a scene name".to_string())?;
                scene = Some(DiagnosticScene::parse(value).ok_or_else(|| {
                    format!(
                        "unknown scene {:?}; use day, dawn, night, rain, fog, physics, two-lights or boundary",
                        value
                    )
                })?);
                i += 1;
            }
            "--capture" => {
                let value = a
                    .get(i + 1)
                    .ok_or_else(|| "--capture requires a PNG path".to_string())?;
                if value.is_empty() {
                    return Err("--capture must not have an empty path".to_string());
                }
                capture = Some(value.clone());
                i += 1;
            }
            "--capture-frames" => {
                let value = a
                    .get(i + 1)
                    .ok_or_else(|| "--capture-frames requires a frame count".to_string())?;
                capture_frames = Some(bounded_capture_frames(value).ok_or_else(|| {
                    format!("invalid --capture-frames {:?}; expected 1..60000", value)
                })?);
                i += 1;
            }
            "--benchmark" => {
                let value = a
                    .get(i + 1)
                    .ok_or_else(|| "--benchmark requires a duration in seconds".to_string())?;
                benchmark = Some(bounded_duration(value).ok_or_else(|| {
                    format!("invalid --benchmark {:?}; expected 0.1..3600 s", value)
                })?);
                i += 1;
            }
            "--quality" => {
                let value = a
                    .get(i + 1)
                    .ok_or_else(|| "--quality requires low, medium or high".to_string())?;
                quality = render3d::Quality::parse(value).ok_or_else(|| {
                    format!("unknown quality {:?}; use low, medium or high", value)
                })?;
                i += 1;
            }
            option if option.starts_with('-') => {
                return Err(format!("unknown option {option:?}"));
            }
            value => {
                return Err(format!("unexpected argument {value:?}"));
            }
        }
        i += 1;
    }
    if capture_frames.is_some() && capture.is_none() {
        return Err("--capture-frames requires --capture".to_string());
    }
    if capture.is_some() && benchmark.is_some() {
        return Err("--capture and --benchmark cannot be used together".to_string());
    }
    if evolution
        && (scene.is_some() || capture.is_some() || capture_frames.is_some() || benchmark.is_some())
    {
        return Err(
            "--evolution cannot be combined with --scene, --capture, --capture-frames or --benchmark"
                .to_string(),
        );
    }
    Ok(CliArgs {
        evolution,
        sim,
        speed_file: file,
        scene,
        capture,
        capture_frames,
        benchmark,
        quality,
    })
}

fn args() -> Result<CliArgs, String> {
    validated_cli()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LampDiagnosticAnchor {
    along_m: i64,
    ids: [u64; 2],
}

fn first_city_lamp_anchor() -> Option<LampDiagnosticAnchor> {
    // Stały, mały zakres wystarcza, bo generator obejmuje każdy region
    // deterministycznie. Pozycja pozostaje niezależna od zapisu użytkownika.
    let mut fallback = None;
    for region in 0..512_i64 {
        let description = world::region_description(region);
        if description.biom != Biom::Miasto {
            continue;
        }
        let lamps: Vec<_> = description
            .objects
            .iter()
            .filter(|object| matches!(object.geometry, world::Geometry::Lamp { .. }))
            .collect();
        for lamp in lamps.iter().copied() {
            let mut pair = lamps
                .iter()
                .copied()
                .filter(|other| other.position.along_m == lamp.position.along_m)
                .map(|other| other.id);
            let Some(first_id) = pair.next() else {
                continue;
            };
            let Some(second_id) = pair.next() else {
                continue;
            };
            let anchor = LampDiagnosticAnchor {
                along_m: lamp.position.along_m.round() as i64,
                ids: [first_id, second_id],
            };
            // Prefer a pair with an actual nearby wall so the normal shadow
            // pass can be inspected for both limb/object separation and wall
            // occlusion, while retaining a stable pair as fallback.
            let nearby_wall = description.objects.iter().any(|object| {
                matches!(object.geometry, world::Geometry::House { .. })
                    && (object.position.along_m - lamp.position.along_m).abs() <= 28.0
            });
            if nearby_wall {
                return Some(anchor);
            }
            fallback = Some(anchor);
        }
    }
    fallback
}

fn first_city_lamp_m() -> f64 {
    first_city_lamp_anchor()
        .map(|anchor| anchor.along_m as f64)
        .unwrap_or(0.0)
}

fn first_biome_boundary_m() -> f64 {
    (0..512_i64)
        .find(|&region| crate::rng::biom(region) != crate::rng::biom(region + 1))
        .map(|region| (region + 1) as f64 * crate::rng::REGION_M)
        .unwrap_or(0.0)
}

fn diagnostic_position(scene: DiagnosticScene) -> f64 {
    match scene {
        DiagnosticScene::Day => 150.0,
        DiagnosticScene::Dawn => 0.0,
        DiagnosticScene::Night | DiagnosticScene::Rain | DiagnosticScene::TwoLights => {
            first_city_lamp_m()
        }
        DiagnosticScene::Fog => 450.0,
        // The physics scene is a deterministic inspection palette, not a
        // second dawn capture; keep the drop fixture near route zero.
        DiagnosticScene::Physics => 0.0,
        DiagnosticScene::Boundary => first_biome_boundary_m(),
    }
}

fn diagnostic_phase(scene: DiagnosticScene) -> f64 {
    match scene {
        DiagnosticScene::Dawn => 0.0,
        DiagnosticScene::Day => 0.25,
        DiagnosticScene::Night | DiagnosticScene::Rain => 0.75,
        DiagnosticScene::Fog => 0.25,
        DiagnosticScene::Physics | DiagnosticScene::Boundary => 0.25,
        DiagnosticScene::TwoLights => 0.75,
    }
}

fn diagnostic_weather(scene: DiagnosticScene) -> (f32, f32) {
    match scene {
        DiagnosticScene::Rain => (1.0, 0.0),
        DiagnosticScene::Fog => (0.0, 1.0),
        DiagnosticScene::Day
        | DiagnosticScene::Dawn
        | DiagnosticScene::Night
        | DiagnosticScene::Physics
        | DiagnosticScene::TwoLights
        | DiagnosticScene::Boundary => (0.0, 0.0),
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let cli = match args() {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("Argument error: {error}");
            std::process::exit(2);
        }
    };

    // font z ogonkami albo wbudowany (wtedy bez ą/ł).
    let mut font: Option<Font> = None;
    for p in FONT_PATHS {
        if let Ok(f) = load_ttf_font(p).await {
            font = Some(f);
            break;
        }
    }

    if cli.evolution {
        evolution::run(&font, &cli).await;
        return;
    }

    // Spokojna muzyka tła (proceduralna pętla, M = wł./wył.).
    // Diagnostyka (sceny/capture/benchmark) gra po cichu.
    let mut music = if cli.scene.is_none() && cli.capture.is_none() && cli.benchmark.is_none() {
        music::Music::start().await
    } else {
        music::Music::off()
    };

    // Diagnostyka działa na świeżym stanie w pamięci. Nie może odczytać ani
    // zapisać normalnego postępu, nawet gdy użytkownik poda tylko --capture.
    let diagnostic_scene = cli.scene.or_else(|| {
        (cli.capture.is_some() || cli.benchmark.is_some()).then_some(DiagnosticScene::Day)
    });
    let diagnostic = diagnostic_scene.is_some();
    let mut s = if diagnostic {
        Save::default()
    } else {
        load_save()
    };
    if let Some(scene) = diagnostic_scene {
        let cam_m = diagnostic_position(scene);
        if matches!(
            scene,
            DiagnosticScene::Night | DiagnosticScene::Rain | DiagnosticScene::TwoLights
        ) && let Some(anchor) = first_city_lamp_anchor()
        {
            eprintln!(
                "diagnostic lamp anchor: along={}m source_ids={:?}",
                anchor.along_m, anchor.ids
            );
        }
        s.total_km = cam_m / 1000.0;
        s.doba_off = diagnostic_phase(scene) * daynight::DAY_M - cam_m;
        s.doba_init = true;
    } else if !s.doba_init {
        // doba: pierwszy start dostaje offset z godziny systemowej (stare zapisy też)
        s.doba_off = daynight::init_offset(s.total_km * 1000.0);
        s.doba_init = true;
        store_save(&s);
    }
    quests::deal(&mut s);
    let mut shown_level = level_for_km(s.total_km);
    // Ślad kamery do delty dystansu (questy) i wejść w regiony (album).
    let mut prev_cam_m = s.total_km * 1000.0;
    let mut prev_runner_m = prev_cam_m;
    let mut show_album = false;
    // Kamera absolutna w metrach (f64): cam_m = total_km × 1000. Bez wrapu.
    let mut coins: Vec<Coin> = Vec::new();
    let mut popups: Vec<Popup> = Vec::new();
    let mut coin_acc = 0.0f64;
    let mut poll_t = 99.0f32;
    let mut sample_t = 99.0f32;
    let mut speed = 0.0f64;
    let mut live = cli.sim.is_some() || diagnostic;
    let mut save_t = 0.0f32;
    let mut tick_acc = 0.0f32;
    let mut fps_t = 0.0f32;
    let mut counter_pulse = 0.0f32;
    let mut hist: VecDeque<f32> = VecDeque::with_capacity(96);
    let mut fullscreen = false;
    let mut rain_v = 0.0f32; // wygładzona pogoda (tylko obraz/HUD)
    let mut fog_v = 0.0f32;
    let mut renderer = if cli.quality == render3d::Quality::default() {
        render3d::Renderer::new()
    } else {
        render3d::Renderer::with_quality(cli.quality)
    };
    let mut physics = physics::Physics::new(
        prev_cam_m,
        diagnostic_scene == Some(DiagnosticScene::Physics),
    );
    let mut rng = 987654321u64;
    let benchmark_wall_start = Instant::now();
    let mut benchmark_measure_start = None;
    let mut diagnostic_time = 0.0_f32;

    // LCG zamiast zależności: kryty i tak muszą być losowe.
    let mut roll = move || {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((rng >> 33) as f64) / (u32::MAX as f64)
    };

    loop {
        let frame_dt = if cli.capture.is_some() {
            // Captures are a deterministic visual fixture: physics and seeded
            // effects receive the same step on every machine. Normal play and
            // benchmarks retain their real frame pacing separately below.
            CAPTURE_FRAME_DT
        } else {
            get_frame_time().max(0.0)
        };
        // Simulation and physics use a bounded step; benchmark accounting and
        // renderer frame percentiles retain the unbounded frame interval.
        let dt = frame_dt.min(0.05);
        if diagnostic {
            diagnostic_time = advance_diagnostic_time(diagnostic_time, dt);
        }
        let w = screen_width();
        let h = screen_height();
        let panel_w = 360.0_f32.min(w * 0.34);
        let world_w = w - panel_w;
        let ground = h * 0.78;

        if is_key_pressed(KeyCode::Escape) {
            if !diagnostic {
                store_save(&s);
            }
            break;
        }
        if is_key_pressed(KeyCode::F) {
            fullscreen = !fullscreen;
            set_fullscreen(fullscreen);
        }
        if is_key_pressed(KeyCode::M) {
            music.toggle();
        }
        if is_key_pressed(KeyCode::T) {
            show_album = !show_album;
        }

        // prędkość z mostka co 0,5 s + próbka do wykresu
        poll_t += dt;
        sample_t += dt;
        if !diagnostic && cli.sim.is_none() && poll_t >= 0.5 {
            poll_t = 0.0;
            (speed, live) = read_speed(&cli.speed_file);
        } else if let Some(v) = cli.sim {
            speed = v.max(0.0);
        } else if diagnostic {
            speed = 0.0;
            live = true;
        }
        if sample_t >= 0.5 {
            sample_t = 0.0;
            if hist.len() == 96 {
                hist.pop_front();
            }
            hist.push_back(speed as f32);
        }
        let spd = speed as f32;
        let moving = spd > 0.05;
        // DYSTANS OSOBNO OD TICKA: kamera musi płynąć co klatkę (60 Hz),
        // nie skakać raz na sekundę jak tick golda.
        s.total_km += speed * dt as f64 / 3600.0;

        // ekonomia: 4 km/h = 4 gold/min; tick co sekundę z możliwością kryta ×5.
        // NA POSTOJU ZERO: stoi w realu (0 km/h) = zero golda, nawet ze Skarbonką.
        tick_acc += dt;
        if tick_acc >= 1.0 {
            tick_acc = 0.0;
            let (g, crit) = tick_gold(&s, speed, roll());
            if crit {
                popups.push(Popup {
                    x: world_w * 0.3,
                    y: ground - 220.0,
                    t: 0.0,
                    text: "CRIT ×5!".into(),
                    col: Color::new(1.0, 0.3, 0.5, 1.0),
                });
            }
            s.gold += g;
            s.total_gold += g;
            // kamień milowy: pełny km = premia z Trasy
            if let Some(bonus) = take_milestone(&mut s) {
                s.gold += bonus;
                s.total_gold += bonus;
                popups.push(Popup {
                    x: world_w * 0.3,
                    y: ground - 260.0,
                    t: 0.0,
                    text: format!("{} km! +{} gold", pl(s.next_km - 1.0, 0), pl(bonus, 0)),
                    col: GOLD_C,
                });
            }
        }

        // monety-wizualizacja: im większy przychód, tym gęściej lecą
        let income_min = income_per_min(&s, speed);
        coin_acc += income_min / 60.0 * dt as f64 * 2.0;
        if coin_acc >= 1.0 && coins.len() < 40 {
            coin_acc = 0.0;
            coins.push(Coin {
                t: 0.0,
                dur: 0.9,
                x0: world_w * 0.3 + 20.0,
                y0: ground - 150.0,
            });
        }

        // kamera z dystansu (metry, f64) — świat stoi, aż sam wyjedzie za ekran
        let cam_m = s.total_km * 1000.0;
        // doba z dystansu: paleta + jasność + nocność do wszystkich warstw
        let natural_doba = daynight::doba_phase(cam_m, s.doba_off);
        let doba = diagnostic_scene
            .map(diagnostic_phase)
            .unwrap_or(natural_doba);
        let pal = daynight::palette_at(doba);
        let night = daynight::nightness(doba);
        let bright = daynight::brightness(doba);
        let is_night = night > 0.5;
        // statystyka do questów: km nocą (tylko w marszu)
        if moving && is_night {
            s.night_km += speed * dt as f64 / 3600.0;
        }
        // poziom Wędrowca (prestiż, zero ekonomii) + popup o awansie
        let lvl_now = level_for_km(s.total_km);
        if lvl_now != shown_level {
            shown_level = lvl_now;
            s.level = lvl_now;
            popups.push(Popup {
                x: world_w * 0.3,
                y: ground - 300.0,
                t: 0.0,
                text: format!("Level {}: {}", lvl_now, level_title(lvl_now)),
                col: GOLD_C,
            });
        }
        // --- sygnały questów i albumu (tylko realny ruch taśmy) ---
        let d_m = (cam_m - prev_cam_m).max(0.0);
        let d_km = d_m / 1000.0;
        prev_cam_m = cam_m;
        // Model 3D stoi przy lokalnym x = 0, więc cam_m jest jedyną
        // autorytatywną pozycją postaci dla pogody, questów i albumu.
        let runner_m = cam_m;
        if moving && d_m > 0.0 {
            quests::add_walk_km(&mut s, d_km);
            if biom_at(runner_m) == Biom::Las {
                quests::add_forest_m(&mut s, d_m);
            }
            if is_night {
                quests::add_night_km(&mut s, d_km);
            }
        }
        let (rain_i, fog_i) = diagnostic_scene
            .map(diagnostic_weather)
            .unwrap_or_else(|| weather::intensity_at(runner_m));
        // Wygładzenie obrazu pogody (~1,5 s stała): przy większych prędkościach
        // regiony 300 m schodzą co ~21 s, a pas przejścia FADE_M ma 30 m — bez
        // wygładzenia mgła/deszcz prószyno gaśnie ("miganie"). Logika gameplayu
        // (statystyki noc/deszcz, questy) dalej liczy SUROWE rain_i/fog_i.
        let fx_k = (dt / 1.5).clamp(0.0, 1.0);
        rain_v += (rain_i - rain_v) * fx_k;
        fog_v += (fog_i - fog_v) * fx_k;
        let (rain_fx, fog_fx) = if diagnostic {
            (rain_i, fog_i)
        } else {
            (rain_v, fog_v)
        };
        if moving && rain_i > 0.5 && d_m > 0.0 {
            s.rain_km += d_km;
            quests::add_rain_m(&mut s, d_m);
        }
        quests::tempo_tick(&mut s, dt, moving && (3.0..=5.0).contains(&spd));
        // Odkrycia używają tych samych opisów, które rysuje scena 3D.
        if region_of(runner_m) != region_of(prev_runner_m) {
            for region in world::crossed_regions(prev_runner_m, runner_m) {
                if let Some(discovery) = world::discovery_for_region(region) {
                    let mut fresh = 0.0;
                    for species in discovery.mushrooms {
                        if !s.album_shrooms[species as usize] {
                            s.album_shrooms[species as usize] = true;
                            fresh += 1.0;
                        }
                    }
                    if fresh > 0.0 {
                        quests::add_shrooms(&mut s, fresh);
                    }
                    s.album_animals[discovery.animal.index()] += 1;
                }
            }
        }
        // Osada może leżeć w środku regionu, więc jej przejście sprawdzamy
        // niezależnie od przejścia granicy biomu. Odcinki klatek nie dublują
        // punktu dzięki półotwartemu warunkowi w settlements_between().
        let settlements = world::settlements_between(prev_runner_m, runner_m);
        for settlement in &settlements {
            s.osady += 1;
            if settlement.kind == Biom::Wies {
                quests::add_village(&mut s);
            }
        }
        // kurier: odbiór/dostawa paczek na przejściu osady (stan w Save)
        for (ei, event) in courier::handle_crossings(&mut s, &settlements, runner_m)
            .into_iter()
            .enumerate()
        {
            let (text, col) = match event {
                Event::Pickup { text } => (text, ROAD_LINE),
                Event::Deliver { text } => (text, GOLD_C),
            };
            popups.push(Popup {
                x: world_w * 0.3,
                y: ground - 310.0 - ei as f32 * 26.0,
                t: 0.0,
                text,
                col,
            });
        }
        prev_runner_m = runner_m;
        // rozliczenie ukończonych questów: jednorazowy gold + popup + nowe rozdanie
        for (desc, reward) in quests::collect_done(&mut s) {
            s.gold += reward;
            s.total_gold += reward;
            popups.push(Popup {
                x: world_w * 0.3,
                y: ground - 280.0,
                t: 0.0,
                text: format!("Quest: {} +{}", desc, pl(reward, 0)),
                col: GOLD_C,
            });
        }
        counter_pulse = (counter_pulse - dt * 3.0).max(0.0);

        let _ = physics.advance(cam_m, spd, dt);
        let physics_snapshot = physics.snapshot(cam_m);

        // Jedyna scena świata; UI jest rysowane po powrocie z kamery 3D.
        renderer.draw(&render3d::Scene {
            cam_m,
            spd,
            physics: &physics_snapshot,
            pal: &pal,
            night,
            bright,
            rain: rain_fx,
            fog: fog_fx,
            time: if diagnostic {
                diagnostic_time
            } else {
                get_time() as f32
            },
            dt,
            frame_dt,
            doba,
            world_area: render3d::WorldArea::new(world_w, h),
        });

        // monety lecą do licznika (krzywa Béziera) + popupy
        if fx::draw_coins(&mut coins, dt) {
            counter_pulse = 1.0;
        }
        fx::draw_popups(&font, &mut popups, dt);

        // HUD
        hud::draw_hud(&hud::Ctx {
            font: &font,
            gold: s.gold,
            income_min,
            speed,
            live,
            total_km: s.total_km,
            phase: daynight::phase_name(doba),
            weather: weather::pogoda_name(rain_fx, fog_fx),
            level_name: level_title(shown_level),
            level_no: shown_level,
            hist: &hist,
            counter_pulse,
            world_w,
        });
        // ---------- questy Wędrówki: 3 aktywne pod wykresem ----------
        quests_box::draw_quests(&font, &s);

        // ---------- drzewko ulepszeń: 4 gałęzie × 3 poziomy, kupno po kolei ----------
        if tree::draw_tree(&font, &mut s, world_w, panel_w, h, &mut popups) {
            counter_pulse = 1.0;
        }

        // ---------- album kolekcji (overlay na T) ----------
        if show_album {
            album::draw_album(&font, &s, w, h);
        }

        save_t += dt;
        if !diagnostic && save_t >= 10.0 {
            save_t = 0.0;
            store_save(&s);
        }
        fps_t += dt;
        if fps_t >= 5.0 && cli.benchmark.is_none() {
            fps_t = 0.0;
            eprintln!("FPS: {}", get_fps());
        }

        if let Some(path) = cli.capture.as_deref() {
            let capture_frames = cli.capture_frames.unwrap_or(90) as u64;
            if renderer.frame() >= capture_frames {
                // `get_screen_data` flushes the active frame, so this includes
                // the complete UI already drawn after the 3D camera pass.
                get_screen_data().export_png(path);
                break;
            }
        }
        if let Some(duration) = cli.benchmark {
            let now = Instant::now();
            let warmup_elapsed = monotonic_elapsed_s(benchmark_wall_start, now);
            if benchmark_measure_start.is_none() && warmup_elapsed >= BENCHMARK_WARMUP_S {
                renderer.begin_benchmark();
                benchmark_measure_start = Some(now);
            }
            if let Some(measure_start) = benchmark_measure_start {
                let elapsed = monotonic_elapsed_s(measure_start, now);
                if elapsed >= duration {
                    eprintln!(
                        "{}",
                        renderer.benchmark_summary(
                            elapsed,
                            physics_snapshot.dynamic_bodies,
                            w.round() as u32,
                            h.round() as u32,
                        )
                    );
                    break;
                }
            }
        }
        next_frame().await;
    }
}
