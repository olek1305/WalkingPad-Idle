//! Panel drzewka ulepszeń: 4 gałęzie × 3 poziomy, kupno po kliknięciu.

use macroquad::prelude::*;

use crate::save::Save;
use crate::text::{dtxt, pl};
use crate::theme::{BTN, BTN_HOVER, BTN_OFF, DIM, GOLD_C, PANEL, ROAD_LINE};

use super::fx::Popup;

/// Rysuje panel i rozlicza kliknięcia. Zwraca true, gdy coś kupiono
/// (pętla gry mruga licznikiem złota).
pub fn draw_tree(
    font: &Option<Font>,
    s: &mut Save,
    world_w: f32,
    panel_w: f32,
    h: f32,
    popups: &mut Vec<Popup>,
) -> bool {
    let mut bought = false;
    draw_rectangle(world_w, 0.0, panel_w, h, PANEL);
    draw_line(
        world_w,
        0.0,
        world_w,
        h,
        2.0,
        Color::new(0.3, 0.5, 0.7, 1.0),
    );
    dtxt(
        font,
        "UPGRADES (click)",
        world_w + 24.0,
        40.0,
        26,
        ROAD_LINE,
    );
    let (mx, my) = mouse_position();
    let clicked = is_mouse_button_pressed(MouseButton::Left);
    // gałąź: tytuł + 3 poziomy [opis, koszt]; poziom = ile kupionych (max III)
    let tree: [(&str, [(&str, f64); 3]); 4] = [
        (
            "SHOES",
            [
                ("×1.25 gold", 50.0),
                ("×1.50 gold", 150.0),
                ("×2.00 gold", 400.0),
            ],
        ),
        (
            "PIGGY BANK",
            [
                ("+0.5/min walking", 30.0),
                ("+1.5/min walking", 100.0),
                ("+4.0/min walking", 350.0),
            ],
        ),
        (
            "PULSE METER",
            [
                ("10% CRIT ×5", 80.0),
                ("20% CRIT ×5", 250.0),
                ("30% CRIT ×5", 500.0),
            ],
        ),
        (
            "ROUTE",
            [
                ("+40 gold/km", 40.0),
                ("+80 gold/km", 150.0),
                ("+175 gold/km", 450.0),
            ],
        ),
    ];
    let bx = world_w + 20.0;
    let bw = panel_w - 40.0;
    let top = 58.0;
    let bottom = h - 34.0;
    let per_branch = (bottom - top) / 4.0;
    let rh = ((per_branch - 30.0 - 2.0 * 6.0) / 3.0).clamp(28.0, 44.0);
    let numerals = ["I", "II", "III"];
    for (bi, (title, tiers)) in tree.iter().enumerate() {
        let lvl = match bi {
            0 => s.shoes,
            1 => s.bank,
            2 => s.pulse,
            _ => s.route,
        }
        .min(3);
        let by = top + bi as f32 * per_branch;
        dtxt(font, title, bx + 4.0, by + 18.0, 20, ROAD_LINE);
        // kręgosłup gałęzi + kolanka do poziomów
        let sx = bx + 10.0;
        let rows_top = by + 26.0;
        let rows_end = rows_top + 3.0 * rh + 2.0 * 6.0 - 8.0;
        draw_line(sx, rows_top + 6.0, sx, rows_end, 2.0, DIM);
        for (ti, (desc, cost)) in tiers.iter().enumerate() {
            let ry = rows_top + ti as f32 * (rh + 6.0);
            let indent = 22.0 + ti as f32 * 12.0;
            draw_line(sx, ry + rh / 2.0, bx + indent, ry + rh / 2.0, 2.0, DIM);
            let owned = (ti as u32) < lvl;
            let buyable = ti as u32 == lvl && lvl < 3;
            let afford = s.gold >= *cost;
            let (col, txt) = if owned {
                (Color::new(0.10, 0.35, 0.22, 1.0), WHITE)
            } else if buyable && afford {
                (
                    if mx >= bx + indent && mx <= bx + bw && my >= ry && my <= ry + rh {
                        BTN_HOVER
                    } else {
                        BTN
                    },
                    WHITE,
                )
            } else {
                (BTN_OFF, DIM)
            };
            draw_rectangle(bx + indent, ry, bw - indent, rh, col);
            draw_rectangle_lines(
                bx + indent,
                ry,
                bw - indent,
                rh,
                if owned { 2.0 } else { 1.0 },
                if owned { GOLD_C } else { WHITE },
            );
            let state = if owned {
                "owned".to_string()
            } else if !buyable {
                format!("needs {} first", numerals[lvl as usize])
            } else {
                format!("{} gold", pl(*cost, 0))
            };
            dtxt(
                font,
                &format!("{} · {} · {}", numerals[ti], desc, state),
                bx + indent + 8.0,
                ry + rh / 2.0 + 6.0,
                16,
                txt,
            );
            if buyable
                && afford
                && mx >= bx + indent
                && mx <= bx + bw
                && my >= ry
                && my <= ry + rh
                && clicked
            {
                s.gold -= *cost;
                match bi {
                    0 => s.shoes += 1,
                    1 => s.bank += 1,
                    2 => s.pulse += 1,
                    _ => s.route += 1,
                }
                bought = true;
                popups.push(Popup {
                    x: bx + 20.0,
                    y: ry,
                    t: 0.0,
                    text: "BOUGHT!".into(),
                    col: GOLD_C,
                });
            }
        }
    }
    dtxt(
        font,
        "F = fullscreen · T = album · M = music · Esc = exit",
        world_w + 24.0,
        h - 20.0,
        17,
        DIM,
    );
    bought
}
