//! Album kolekcji (overlay na T): 20 grzybów + 5 zwierząt + statystyki.

use macroquad::prelude::*;

use crate::save::Save;
use crate::text::{dtxt, pl};
use crate::theme::{CAPS, DIM, GOLD_C, ROAD_LINE};

const ANIMAL_NAMES: [&str; 5] = ["Owl", "Deer", "Hedgehog", "Bear", "Wolf"];

pub fn draw_album(font: &Option<Font>, s: &Save, w: f32, h: f32) {
    let aw = 460.0_f32.min(w - 40.0);
    let ah = 380.0_f32.min(h - 40.0);
    let ax = (w - aw) / 2.0;
    let ay = (h - ah) / 2.0;
    draw_rectangle(ax, ay, aw, ah, Color::new(0.04, 0.06, 0.14, 0.96));
    draw_rectangle_lines(ax, ay, aw, ah, 2.0, GOLD_C);
    dtxt(
        font,
        "ALBUM — mushrooms and wildlife (T to close)",
        ax + 20.0,
        ay + 32.0,
        20,
        ROAD_LINE,
    );
    // 20 grzybów: odkryte w kolorze kapelusza, reszta „?"
    for t in 0..20u32 {
        let cx = ax + 20.0 + (t % 5) as f32 * 86.0;
        let cy = ay + 56.0 + (t / 5) as f32 * 52.0;
        if s.album_shrooms[t as usize] {
            draw_rectangle(cx, cy, 72.0, 40.0, Color::new(0.10, 0.30, 0.22, 1.0));
            draw_circle(cx + 36.0, cy + 14.0, 10.0, CAPS[(t % 5) as usize]);
            dtxt(font, &format!("No. {}", t), cx + 8.0, cy + 36.0, 14, WHITE);
        } else {
            draw_rectangle(cx, cy, 72.0, 40.0, Color::new(0.12, 0.13, 0.20, 1.0));
            dtxt(font, "?", cx + 30.0, cy + 28.0, 22, DIM);
        }
    }
    let by = ay + 56.0 + 4.0 * 52.0 + 14.0;
    for (i, name) in ANIMAL_NAMES.iter().enumerate() {
        let c = s.album_animals[i];
        let txt = if c > 0 {
            format!("{} ×{}", name, c)
        } else {
            format!("{} — ?", name)
        };
        dtxt(
            font,
            &txt,
            ax + 20.0 + (i % 3) as f32 * 145.0,
            by + (i / 3) as f32 * 24.0,
            16,
            if c > 0 { WHITE } else { DIM },
        );
    }
    dtxt(
        font,
        &format!(
            "Deliveries: {}   Villages: {}   Night: {} km   Rain: {} km",
            s.deliveries,
            s.osady,
            pl(s.night_km, 2),
            pl(s.rain_km, 2)
        ),
        ax + 20.0,
        ay + ah - 16.0,
        15,
        DIM,
    );
}
