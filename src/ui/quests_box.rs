//! Box questów pod wykresem: 3 aktywne + pasek postępu.

use macroquad::prelude::*;

use crate::quests;
use crate::save::Save;
use crate::text::{dtxt, format_quest_progress, pl};
use crate::theme::{DIM, GOLD_C, ROAD_LINE};

pub fn draw_quests(font: &Option<Font>, s: &Save) {
    let qy = 282.0;
    let found = s.album_shrooms.iter().filter(|&&b| b).count();
    draw_rectangle(20.0, qy, 300.0, 22.0, Color::new(0.03, 0.05, 0.12, 0.78));
    dtxt(
        font,
        &format!("QUESTS   •   album {}/20  (T)", found),
        28.0,
        qy + 16.0,
        15,
        ROAD_LINE,
    );
    for (i, q) in s.quests.iter().enumerate() {
        let ry = qy + 26.0 + i as f32 * 40.0;
        let prog = format_quest_progress(q);
        draw_rectangle(20.0, ry, 300.0, 36.0, Color::new(0.03, 0.05, 0.12, 0.78));
        let frac = if q.goal > 0.0 {
            (q.progress / q.goal).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        draw_rectangle(20.0, ry + 28.0, 300.0 * frac, 5.0, GOLD_C);
        if let Some(d) = quests::quest_def(&q.id) {
            dtxt(font, d.desc, 28.0, ry + 15.0, 14, WHITE);
            dtxt(
                font,
                &format!("{}  •  +{} gold", prog, pl(d.reward, 0)),
                28.0,
                ry + 29.0,
                13,
                DIM,
            );
        }
    }
}
