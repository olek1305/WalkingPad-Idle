//! Monety, popupy i wykres tempa.

use macroquad::prelude::*;
use std::collections::VecDeque;

use crate::text::{dtxt, pl};
use crate::theme::{DIM, GOLD_C};

pub struct Coin {
    pub t: f32,
    pub dur: f32,
    pub x0: f32,
    pub y0: f32,
}
pub struct Popup {
    pub x: f32,
    pub y: f32,
    pub t: f32,
    pub text: String,
    pub col: Color,
}

/// Monety lecą do licznika (krzywa Béziera). true = któraś doszła (puls licznika).
pub fn draw_coins(coins: &mut Vec<Coin>, dt: f32) -> bool {
    let target = Vec2::new(150.0, 44.0);
    let mut hit = false;
    coins.retain_mut(|c| {
        c.t += dt / c.dur;
        if c.t >= 1.0 {
            hit = true;
            return false;
        }
        let p0 = Vec2::new(c.x0, c.y0);
        let p1 = Vec2::new(c.x0 + 120.0, c.y0 - 160.0);
        let t = c.t;
        let a = p0.lerp(p1, t);
        let b = p1.lerp(target, t);
        let pos = a.lerp(b, t);
        draw_circle(pos.x, pos.y, 9.0, GOLD_C);
        draw_circle_lines(pos.x, pos.y, 9.0, 2.0, Color::new(0.6, 0.4, 0.0, 1.0));
        draw_line(
            pos.x - 3.6,
            pos.y - 3.6,
            pos.x - 0.9,
            pos.y - 0.9,
            2.0,
            WHITE,
        );
        true
    });
    hit
}

pub fn draw_popups(font: &Option<Font>, popups: &mut Vec<Popup>, dt: f32) {
    popups.retain_mut(|p| {
        p.t += dt;
        if p.t > 1.4 {
            return false;
        }
        dtxt(
            font,
            &p.text,
            p.x,
            p.y - p.t * 60.0,
            32,
            Color::new(p.col.r, p.col.g, p.col.b, 1.0 - p.t / 1.4),
        );
        true
    });
}

/// Wykres tempa: polilinia z historii + oś i siatka.
pub fn draw_graph(
    font: &Option<Font>,
    x: f32,
    y: f32,
    gw: f32,
    gh: f32,
    hist: &VecDeque<f32>,
    max_v: f32,
) {
    draw_rectangle(x, y, gw, gh, Color::new(0.03, 0.05, 0.12, 0.78));
    draw_rectangle_lines(x, y, gw, gh, 2.0, Color::new(0.3, 0.5, 0.7, 1.0));
    dtxt(font, "pace 48 s (km/h) 0–20", x + 8.0, y + 20.0, 16, DIM);
    // siatka co 5 km/h: 6 (spacer), 12 (bieg), 18 (max WalkingPad X218)
    let mut v = 5.0;
    while v < max_v {
        let gy = y + gh - 10.0 - (v / max_v) * (gh - 34.0);
        draw_line(
            x + 4.0,
            gy,
            x + gw - 4.0,
            gy,
            1.0,
            Color::new(0.3, 0.4, 0.6, 0.4),
        );
        dtxt(font, &pl(v as f64, 0), x + 6.0, gy - 4.0, 14, DIM);
        v += 5.0;
    }
    if hist.len() < 2 {
        return;
    }
    let n = hist.len();
    let mut prev: Option<Vec2> = None;
    for (i, &val) in hist.iter().enumerate() {
        let px = x + 4.0 + (i as f32 / (n - 1) as f32) * (gw - 8.0);
        let py = y + gh - 10.0 - (val.min(max_v) / max_v) * (gh - 34.0);
        if let Some(p) = prev {
            draw_line(p.x, p.y, px, py, 2.5, Color::new(0.3, 1.0, 0.7, 1.0));
        }
        prev = Some(Vec2::new(px, py));
    }
}
