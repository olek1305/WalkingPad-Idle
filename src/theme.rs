//! Stałe kolory UI i świata + paleta grzybów (CAPS).

use macroquad::prelude::Color;

// Paleta bazowa (zmierzch) — noc/dzień wyliczane w daynight::palette_at.
// Zachowane dla elementów stałych (droga, UI); niebo/wzgórza idą z palety doby.
pub const ROAD: Color = Color::new(0.10, 0.12, 0.24, 1.0);
pub const ROAD_LINE: Color = Color::new(1.0, 0.85, 0.25, 1.0);
pub const GOLD_C: Color = Color::new(1.0, 0.80, 0.15, 1.0);
pub const PANEL: Color = Color::new(0.05, 0.07, 0.16, 1.0);
pub const BTN: Color = Color::new(0.12, 0.45, 0.55, 1.0);
pub const BTN_HOVER: Color = Color::new(0.15, 0.60, 0.70, 1.0);
pub const BTN_OFF: Color = Color::new(0.20, 0.22, 0.32, 1.0);
pub const INK: Color = Color::new(0.85, 0.90, 1.0, 1.0);
pub const DIM: Color = Color::new(0.60, 0.65, 0.75, 1.0);

pub const CAPS: [Color; 5] = [
    Color::new(0.80, 0.16, 0.14, 1.0), // muchomor
    Color::new(0.85, 0.45, 0.12, 1.0), // pomarańcz
    Color::new(0.48, 0.30, 0.16, 1.0), // brąz
    Color::new(0.85, 0.75, 0.30, 1.0), // żółć
    Color::new(0.88, 0.84, 0.74, 1.0), // krem
];
