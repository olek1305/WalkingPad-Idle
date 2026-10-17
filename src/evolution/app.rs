//! Pętla gry pierwszego wycinka ewolucji.

use macroquad::prelude::*;
use std::time::Instant;

use super::campaign;
use super::catalog;
use super::persistence::Persistence;
use super::ui::{self, Action, Tab, View};
use crate::render3d::{OceanRenderer, OceanScene};

const MAX_SIMULATION_DT: f64 = 0.05;
const VISUAL_METRES_PER_TREADMILL_KM: f64 = 24.0;
const NUMERIC_PROGRESS_ERROR: &str = "Błąd liczbowy postępu: suma przekracza zakres f64; naliczanie zatrzymano, aby nie utracić rzeczywistego dystansu.";

pub(crate) async fn run(font: &Option<Font>, cli: &crate::CliArgs) {
    prevent_quit();

    let opened = Persistence::open();
    let mut state = opened.state;
    let mut persistence = opened.persistence;
    let mut dirty = false;

    if campaign::sync_discoveries(&mut state) {
        dirty = true;
    }
    let mut selected_id = state
        .unlocked_discovery_ids
        .last()
        .cloned()
        .unwrap_or_else(|| catalog::DISCOVERIES[0].id.to_string());
    if campaign::mark_read(&mut state, &selected_id) {
        dirty = true;
    }
    if persistence.is_new_profile() || dirty {
        if persistence.save(&state).is_ok() {
            dirty = false;
        }
    }

    let mut renderer = if cli.quality == crate::render3d::Quality::default() {
        OceanRenderer::new()
    } else {
        OceanRenderer::with_quality(cli.quality)
    };
    let mut tab = Tab::Discoveries;
    let mut speed = cli.sim.unwrap_or(0.0).max(0.0);
    let mut live = cli.sim.is_some();
    let simulation = cli.sim.is_some();
    let mut poll_elapsed = 0.5_f64;
    let mut save_elapsed = 0.0_f64;
    let mut scene_time = 0.0_f64;
    let mut last_wall = Instant::now();
    let mut fullscreen = false;
    let mut music = crate::music::Music::start().await;
    let mut exit_prompt = false;
    let mut roadmap_page = 0_usize;
    let mut gameplay_error: Option<&'static str> = None;

    'game: loop {
        let now = Instant::now();
        let wall_dt = now.duration_since(last_wall).as_secs_f64();
        last_wall = now;
        let dt = wall_dt.clamp(0.0, MAX_SIMULATION_DT);
        scene_time += dt;
        poll_elapsed += wall_dt.min(1.0);
        save_elapsed += wall_dt;

        if !simulation && poll_elapsed >= 0.5 {
            poll_elapsed = 0.0;
            (speed, live) = crate::read_speed(&cli.speed_file);
        } else if let Some(simulated_speed) = cli.sim {
            speed = simulated_speed.max(0.0);
            live = true;
        }
        let observed_speed = if speed.is_finite() {
            speed.max(0.0)
        } else {
            0.0
        };
        let moving = observed_speed > 0.05;
        if moving && gameplay_error.is_none() {
            // Keep the small simulation step for progress, but do the
            // autosave clock in wall seconds. The multiplication order avoids
            // an avoidable intermediate overflow without imposing a BLE cap.
            let distance_delta = observed_speed * (dt / 3600.0);
            let gain_per_min =
                campaign::knowledge_per_minute(observed_speed, state.observation_accuracy_level);
            let gain = gain_per_min * (dt / 60.0);
            let next_distance = state.treadmill_distance_km + distance_delta;
            let next_active = state.active_seconds + dt;
            let next_knowledge = state.knowledge + gain;
            let next_earned = state.earned_knowledge + gain;
            if distance_delta.is_finite()
                && distance_delta >= 0.0
                && gain.is_finite()
                && gain >= 0.0
                && next_distance.is_finite()
                && next_active.is_finite()
                && next_knowledge.is_finite()
                && next_earned.is_finite()
                && (distance_delta == 0.0 || next_distance > state.treadmill_distance_km)
                && (dt == 0.0 || next_active > state.active_seconds)
                && (gain == 0.0 || next_knowledge > state.knowledge)
                && (gain == 0.0 || next_earned > state.earned_knowledge)
            {
                state.treadmill_distance_km = next_distance;
                state.active_seconds = next_active;
                state.knowledge = next_knowledge;
                state.earned_knowledge = next_earned;
                if distance_delta > 0.0 {
                    dirty = true;
                }
            } else {
                gameplay_error = Some(NUMERIC_PROGRESS_ERROR);
            }
        }

        if campaign::sync_discoveries(&mut state) {
            dirty = true;
            // Odkrycie jest zapisywane od razu, zanim użytkownik przejdzie do
            // następnej klatki lub kupi ulepszenie.
            if persistence.save(&state).is_ok() {
                dirty = false;
                exit_prompt = false;
            }
        }

        if is_key_pressed(KeyCode::F) {
            fullscreen = !fullscreen;
            set_fullscreen(fullscreen);
        }
        if is_key_pressed(KeyCode::M) {
            music.toggle();
        }
        if is_key_pressed(KeyCode::F5) && !persistence.is_read_only() {
            if persistence.save(&state).is_ok() {
                dirty = false;
                exit_prompt = false;
            }
            save_elapsed = 0.0;
        }

        if save_elapsed >= 10.0 {
            save_elapsed = 0.0;
            if dirty && persistence.save(&state).is_ok() {
                dirty = false;
                exit_prompt = false;
            }
        }

        if is_key_pressed(KeyCode::Escape) || (!exit_prompt && is_quit_requested()) {
            if persistence.save(&state).is_ok() {
                break;
            }
            exit_prompt = true;
        }
        if exit_prompt && is_key_pressed(KeyCode::Q) {
            break;
        }

        let world_area = ui::world_area();
        renderer.draw(&OceanScene {
            forward_m: visual_forward_m(state.treadmill_distance_km),
            seed: state.seed,
            time: scene_time,
            observation_range_level: state.observation_range_level,
            world_area,
        });

        let view = View {
            font,
            state: &state,
            selected_id: &selected_id,
            tab,
            speed: observed_speed,
            live,
            simulation,
            knowledge_per_minute: campaign::knowledge_per_minute(
                observed_speed,
                state.observation_accuracy_level,
            ),
            save_status: persistence.status(),
            save_read_only: persistence.is_read_only(),
            dirty,
            exit_prompt,
            roadmap_page,
            gameplay_error,
        };
        for action in ui::draw(&view) {
            match action {
                Action::Save => {
                    if persistence.save(&state).is_ok() {
                        dirty = false;
                        exit_prompt = false;
                    }
                    save_elapsed = 0.0;
                }
                Action::SelectDiscovery(id) => {
                    selected_id = id.to_string();
                    if campaign::mark_read(&mut state, id) {
                        dirty = true;
                        if persistence.save(&state).is_ok() {
                            dirty = false;
                            exit_prompt = false;
                        }
                    }
                }
                Action::BuyUpgrade(kind) => {
                    if !persistence.is_read_only()
                        && matches!(
                            campaign::purchase_upgrade(&mut state, kind),
                            campaign::PurchaseResult::Purchased { .. }
                        )
                    {
                        dirty = true;
                        if persistence.save(&state).is_ok() {
                            dirty = false;
                            exit_prompt = false;
                        }
                    }
                }
                Action::SelectTab(next_tab) => tab = next_tab,
                Action::RoadmapPage(delta) => {
                    let page_count = ui::roadmap_page_count();
                    roadmap_page = if delta.is_negative() {
                        roadmap_page.saturating_sub(delta.unsigned_abs() as usize)
                    } else {
                        roadmap_page
                            .saturating_add(delta as usize)
                            .min(page_count.saturating_sub(1))
                    };
                }
                Action::QuitWithoutSave => break 'game,
            }
        }

        if exit_prompt && is_key_pressed(KeyCode::Q) {
            break;
        }
        next_frame().await;
    }
}

fn visual_forward_m(distance_km: f64) -> f64 {
    if !distance_km.is_finite() || distance_km <= 0.0 {
        return 0.0;
    }
    // A finite save can exceed the useful range of a multiplied visual
    // coordinate. Clamp only the renderer input; the persisted distance stays
    // the actual f64 value.
    distance_km.min(f64::MAX / VISUAL_METRES_PER_TREADMILL_KM) * VISUAL_METRES_PER_TREADMILL_KM
}
