//! HUD: licznik złota, wiersz statusu, wykres tempa.

use macroquad::prelude::*;
use std::collections::VecDeque;

use crate::text::{dtxt, pl};
use crate::theme::{DIM, GOLD_C, INK, ROAD_LINE};

use super::fx::draw_graph;

/// Dane z pętli gry potrzebne do HUD-u (żeby nie ciągać całego `Save`).
pub struct Ctx<'a> {
    pub font: &'a Option<Font>,
    pub gold: f64,
    pub income_min: f64,
    pub speed: f64,
    pub live: bool,
    pub total_km: f64,
    pub phase: &'static str,
    pub weather: &'static str,
    pub level_name: &'static str,
    pub level_no: u32,
    pub hist: &'a VecDeque<f32>,
    pub counter_pulse: f32,
    pub world_w: f32,
}

pub fn draw_hud(st: &Ctx) {
    let pulse = 1.0 + st.counter_pulse * 0.25;
    draw_circle(28.0, 44.0, 16.0 * pulse, GOLD_C);
    draw_circle_lines(
        28.0,
        44.0,
        16.0 * pulse,
        2.0,
        Color::new(0.6, 0.4, 0.0, 1.0),
    );
    dtxt(
        st.font,
        &format!("{} FPS", get_fps()),
        st.world_w - 90.0,
        36.0,
        18,
        DIM,
    );
    dtxt(st.font, &pl(st.gold, 1), 54.0, 56.0, 40, WHITE);
    let hud2 = format!(
        "+{}/min   {} km/h{}   {} km",
        pl(st.income_min, 2),
        pl(st.speed, 1),
        if st.live { "" } else { " (no bridge)" },
        pl(st.total_km, 2),
    );
    dtxt(st.font, &hud2, 54.0, 84.0, 20, INK);
    // doba + pogoda + poziom Wędrowca (prestiż): jeden wiersz, zero ekonomii
    let hud3 = format!(
        "{} • {}   •   {} (Lv. {})",
        st.phase, st.weather, st.level_name, st.level_no
    );
    dtxt(st.font, &hud3, 54.0, 108.0, 20, ROAD_LINE);
    if st.speed < 0.05 {
        dtxt(
            st.font,
            "Step on the belt and walk — or test with: --sim 4.0",
            54.0,
            132.0,
            20,
            ROAD_LINE,
        );
    }
    draw_graph(st.font, 20.0, 150.0, 300.0, 120.0, st.hist, 20.0);
}
