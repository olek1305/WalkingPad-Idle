//! Tekst: font z ogonkami, `dtxt()`, liczby po polsku, format postępu questu.

use macroquad::prelude::*;

use crate::{quests, save};

// Fonty z polskimi ogonkami (pierwszy istniejący wygrywa).
pub const FONT_PATHS: [&str; 3] = [
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSans-Medium.ttf",
    "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf",
];

/// Liczby z kropką dziesiętną (UI po angielsku).
pub fn pl(n: f64, d: usize) -> String {
    format!("{:.*}", d, n)
}

pub fn dtxt(font: &Option<Font>, s: &str, x: f32, y: f32, size: u16, col: Color) {
    draw_text_ex(
        s,
        x,
        y,
        TextParams {
            font_size: size,
            color: col,
            font: font.as_ref(),
            ..Default::default()
        },
    );
}

/// Postęp questu: km, metry/sekundy/sztuki.
pub fn format_quest_progress(q: &save::QuestState) -> String {
    match quests::quest_def(&q.id).map(|d| d.unit) {
        Some("km") => format!("{}/{} km", pl(q.progress, 2), pl(q.goal, 1)),
        Some("m") => format!("{}/{} m", pl(q.progress, 0), pl(q.goal, 0)),
        Some("s") => format!(
            "{}:{:02}/{}:{:02}",
            (q.progress as u32) / 60,
            (q.progress as u32) % 60,
            (q.goal as u32) / 60,
            (q.goal as u32) % 60
        ),
        _ => format!("{}/{}", pl(q.progress, 0), pl(q.goal, 0)),
    }
}
