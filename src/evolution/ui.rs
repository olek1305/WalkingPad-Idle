//! Polski, adaptacyjny HUD kampanii ewolucji.

use macroquad::prelude::*;

use crate::render3d::WorldArea;
use crate::text::pl;

use super::campaign;
use super::catalog::{self, UpgradeKind};
use super::persistence::EvolutionSave;

const PANEL: Color = Color::new(0.025, 0.085, 0.13, 0.96);
const PANEL_ALT: Color = Color::new(0.045, 0.14, 0.19, 0.96);
const INK: Color = Color::new(0.86, 0.96, 0.97, 1.0);
const DIM: Color = Color::new(0.56, 0.76, 0.79, 1.0);
const ACCENT: Color = Color::new(0.32, 0.88, 0.77, 1.0);
const ACCENT_DARK: Color = Color::new(0.08, 0.32, 0.33, 1.0);
const WARNING: Color = Color::new(1.0, 0.64, 0.34, 1.0);
const ERROR: Color = Color::new(1.0, 0.40, 0.36, 1.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Discoveries,
    Observation,
    Roadmap,
    Tree,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Save,
    SelectDiscovery(&'static str),
    BuyUpgrade(UpgradeKind),
    SelectTab(Tab),
    RoadmapPage(i8),
    QuitWithoutSave,
}

pub struct View<'a> {
    pub font: &'a Option<Font>,
    pub state: &'a EvolutionSave,
    pub selected_id: &'a str,
    pub tab: Tab,
    pub speed: f64,
    pub live: bool,
    pub simulation: bool,
    pub knowledge_per_minute: f64,
    pub save_status: &'a str,
    pub save_read_only: bool,
    pub dirty: bool,
    pub exit_prompt: bool,
    pub roadmap_page: usize,
    pub gameplay_error: Option<&'a str>,
}

pub const ROADMAP_PAGE_SIZE: usize = 3;

pub fn roadmap_page_count() -> usize {
    catalog::ROADMAP.len().div_ceil(ROADMAP_PAGE_SIZE).max(1)
}

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub world_area: WorldArea,
    panel_x: f32,
    panel_y: f32,
    panel_w: f32,
    panel_h: f32,
}

pub fn layout() -> Layout {
    let width = screen_width().max(320.0);
    let height = screen_height().max(240.0);
    if width >= height || width >= 900.0 {
        let panel_w = (width * 0.36).clamp(390.0, 500.0).min(width - 280.0);
        let world_w = (width - panel_w).max(280.0);
        Layout {
            world_area: WorldArea::new(world_w, height),
            panel_x: world_w,
            panel_y: 0.0,
            panel_w: width - world_w,
            panel_h: height,
        }
    } else {
        let world_h = (height * 0.37).clamp(250.0, 430.0).min(height - 270.0);
        Layout {
            world_area: WorldArea::new(width, world_h),
            panel_x: 0.0,
            panel_y: world_h,
            panel_w: width,
            panel_h: height - world_h,
        }
    }
}

pub fn world_area() -> WorldArea {
    layout().world_area
}

pub fn draw(view: &View<'_>) -> Vec<Action> {
    let layout = layout();
    let mut actions = Vec::new();
    draw_rectangle(
        layout.panel_x,
        layout.panel_y,
        layout.panel_w,
        layout.panel_h,
        PANEL,
    );
    draw_line(
        layout.panel_x,
        layout.panel_y,
        layout.panel_x + layout.panel_w,
        layout.panel_y,
        2.0,
        ACCENT,
    );

    let x = layout.panel_x + 18.0;
    let right = layout.panel_x + layout.panel_w - 18.0;
    let content_width = (right - x).max(160.0);
    let mut y = layout.panel_y + 27.0;
    y = wrapped(
        view.font,
        "PIERWSZY WYCINEK • MIKROŚWIAT",
        x,
        y,
        content_width,
        20,
        ACCENT,
        2,
    );
    y += 4.0;
    y = wrapped(
        view.font,
        "Archaik • około 3,5 mld lat temu • habitat wodny",
        x,
        y,
        content_width,
        13,
        INK,
        2,
    );
    y += 4.0;
    y = wrapped(
        view.font,
        "ANONIMOWA REKONSTRUKCJA: powiększona ilustracja jednej wczesnej komórki. Nie znamy jej dokładnej barwy, kształtu ani ruchu. Wiedza jest abstrakcją gry, nie ATP ani energią biologiczną.",
        x,
        y,
        content_width,
        12,
        DIM,
        2,
    );
    y += 4.0;
    if let Some(error) = view.gameplay_error {
        y = wrapped(view.font, error, x, y, content_width, 10, ERROR, 2);
        y += 3.0;
    }

    let live_label = if view.simulation {
        "symulacja"
    } else if view.live {
        "na żywo"
    } else {
        "brak/stary plik — postój"
    };
    let speed_label = format!("{:.1}", view.speed).replace('.', ",");
    let gain_label = if view.knowledge_per_minute.is_finite() {
        format!("{}/min", pl(view.knowledge_per_minute, 2))
    } else {
        "błąd liczbowy".to_string()
    };
    text(
        view.font,
        &format!("Wiedza: {}", pl(view.state.knowledge, 1)),
        x,
        y,
        12,
        INK,
    );
    text(
        view.font,
        &format!("Zysk: {gain_label}"),
        x + content_width * 0.51,
        y,
        12,
        INK,
    );
    y += 18.0;
    y = wrapped(
        view.font,
        &format!("Bieżnia: {} km/h • {}", speed_label, live_label),
        x,
        y,
        content_width,
        12,
        INK,
        2,
    );
    text(
        view.font,
        &format!("Dystans: {} km", pl(view.state.treadmill_distance_km, 2)),
        x,
        y,
        12,
        INK,
    );
    text(
        view.font,
        &format!(
            "Aktywność: {} min",
            pl(view.state.active_seconds.max(0.0) / 60.0, 1)
        ),
        x + content_width * 0.51,
        y,
        12,
        INK,
    );
    y += 18.0;
    y = wrapped(
        view.font,
        &format!(
            "Odkrycia: {}/4 • cel {} km",
            campaign::unlocked_count(view.state),
            pl(campaign::END_OF_SLICE_KM, 2)
        ),
        x,
        y,
        content_width,
        12,
        INK,
        1,
    );
    y += 8.0;

    let tab_gap = 5.0;
    let tab_width = (content_width - tab_gap * 3.0) / 4.0;
    for (index, (tab, label)) in [
        (Tab::Discoveries, "Odkrycia"),
        (Tab::Observation, "Obserwacja"),
        (Tab::Roadmap, "Plan"),
        (Tab::Tree, "Drzewo"),
    ]
    .into_iter()
    .enumerate()
    {
        let rect = Rect::new(x + index as f32 * (tab_width + tab_gap), y, tab_width, 27.0);
        button(
            view.font,
            rect,
            label,
            view.tab == tab,
            Some(Action::SelectTab(tab)),
            &mut actions,
        );
    }
    y += 39.0;

    let bottom = layout.panel_y + layout.panel_h - if view.exit_prompt { 93.0 } else { 54.0 };
    match view.tab {
        Tab::Discoveries => draw_discoveries(view, x, y, content_width, bottom, &mut actions),
        Tab::Observation => draw_observation(view, x, y, content_width, bottom, &mut actions),
        Tab::Roadmap => draw_roadmap(view, x, y, content_width, bottom, &mut actions),
        Tab::Tree => draw_tree(view, x, y, content_width, bottom),
    }

    let footer_y = layout.panel_y + layout.panel_h - 37.0;
    let save_rect = Rect::new(x, footer_y - 25.0, (content_width * 0.40).min(150.0), 27.0);
    button(
        view.font,
        save_rect,
        if view.save_read_only {
            "Zapis zablokowany"
        } else {
            "Zapisz (F5)"
        },
        false,
        (!view.save_read_only).then_some(Action::Save),
        &mut actions,
    );
    let save_color = if view.save_read_only || view.save_status.contains("Nie") {
        ERROR
    } else if view.dirty {
        WARNING
    } else {
        DIM
    };
    let status_y = footer_y + 9.0;
    wrapped(
        view.font,
        view.save_status,
        x + (content_width * 0.43).min(162.0),
        footer_y - 10.0,
        (content_width * 0.57).max(120.0),
        11,
        save_color,
        2,
    );
    text(
        view.font,
        "F — pełny ekran • M — muzyka • Escape — zapisz i wyjdź",
        x,
        status_y,
        10,
        DIM,
    );

    if view.exit_prompt {
        let prompt_y = layout.panel_y + layout.panel_h - 83.0;
        draw_rectangle(
            x - 6.0,
            prompt_y - 18.0,
            content_width + 12.0,
            48.0,
            Color::new(0.28, 0.08, 0.07, 0.98),
        );
        text(
            view.font,
            "Zapis nieudany — gra pozostaje otwarta.",
            x,
            prompt_y,
            12,
            ERROR,
        );
        text(
            view.font,
            "Q lub przycisk: wyjdź bez zapisu.",
            x,
            prompt_y + 18.0,
            11,
            INK,
        );
        let quit_rect = Rect::new(right - 166.0, prompt_y + 29.0, 166.0, 27.0);
        button(
            view.font,
            quit_rect,
            "Wyjdź bez zapisu",
            false,
            Some(Action::QuitWithoutSave),
            &mut actions,
        );
    }
    actions
}

fn draw_discoveries(
    view: &View<'_>,
    x: f32,
    mut y: f32,
    width: f32,
    bottom: f32,
    actions: &mut Vec<Action>,
) {
    y = wrapped(
        view.font,
        "KARTY OBSERWACJI • jedna wybrana scena",
        x,
        y,
        width,
        13,
        ACCENT,
        2,
    );
    y += 5.0;
    if view.state.treadmill_distance_km >= campaign::END_OF_SLICE_KM {
        y = wrapped(
            view.font,
            "Koniec dostępnego etapu: następne epoki są planowane. Możesz nadal podróżować i kupować ulepszenia.",
            x,
            y,
            width,
            11,
            WARNING,
            2,
        );
        y += 4.0;
    }
    for discovery in catalog::DISCOVERIES {
        let unlocked = view
            .state
            .unlocked_discovery_ids
            .iter()
            .any(|id| id == discovery.id);
        let label = if unlocked {
            format!(
                "✓ {} · {} km",
                discovery.title,
                pl(discovery.distance_km, 2)
            )
        } else {
            format!(
                "🔒 {} · od {} km",
                discovery.title,
                pl(discovery.distance_km, 2)
            )
        };
        let rect = Rect::new(x, y, width, 23.0);
        button(
            view.font,
            rect,
            &label,
            view.selected_id == discovery.id,
            unlocked.then_some(Action::SelectDiscovery(discovery.id)),
            actions,
        );
        y += 26.0;
    }
    y += 3.0;
    let discovery = campaign::selected_discovery(view.state, view.selected_id);
    y = wrapped(view.font, discovery.title, x, y, width, 17, INK, 2);
    y += 3.0;
    y = wrapped(view.font, discovery.summary, x, y, width, 12, INK, 4);
    y += 2.0;
    y = wrapped(
        view.font,
        discovery.uncertainty,
        x,
        y,
        width,
        11,
        WARNING,
        2,
    );
    y += 5.0;
    y = wrapped(
        view.font,
        "Źródła (tytuły/DOI lub identyfikator):",
        x,
        y,
        width,
        11,
        ACCENT,
        2,
    );
    y += 4.0;
    for source in discovery.sources {
        if y > bottom - 88.0 {
            break;
        }
        y = wrapped(view.font, source.title, x, y, width, 9, INK, 2);
        y = wrapped(view.font, source.compact, x, y, width, 10, INK, 1);
        y = wrapped(view.font, source.url, x, y + 1.0, width, 9, DIM, 2);
        y += 2.0;
    }
}

fn draw_observation(
    view: &View<'_>,
    x: f32,
    mut y: f32,
    width: f32,
    bottom: f32,
    actions: &mut Vec<Action>,
) {
    y = wrapped(
        view.font,
        "OBSERWACJA • ULEPSZENIA JAKO MECHANIKI GRY",
        x,
        y,
        width,
        13,
        ACCENT,
        2,
    );
    y += 5.0;
    y = wrapped(
        view.font,
        "Wiedza i ulepszenia są abstrakcją gry, nie ATP, energią biologiczną ani zakupioną ewolucją. Nie zmieniają jednego mikroorganizmu w człowieka.",
        x,
        y,
        width,
        11,
        DIM,
        4,
    );
    y += 10.0;
    if view.save_read_only {
        y = wrapped(
            view.font,
            "Profil tylko do odczytu: zakupy są wyłączone.",
            x,
            y,
            width,
            11,
            ERROR,
            2,
        );
        y += 7.0;
    }
    if y < bottom {
        y = draw_upgrade(view, x, y, width, UpgradeKind::Accuracy, actions);
        y += 12.0;
    }
    if y < bottom {
        draw_upgrade(view, x, y, width, UpgradeKind::Range, actions);
    }
}

fn draw_upgrade(
    view: &View<'_>,
    x: f32,
    y: f32,
    width: f32,
    kind: UpgradeKind,
    actions: &mut Vec<Action>,
) -> f32 {
    let spec = catalog::upgrade(kind);
    let level = campaign::upgrade_level(view.state, kind);
    let level_text = format!("{} · poziom {}/{}", spec.title, level, spec.max_level);
    text(view.font, &level_text, x, y, 11, INK);
    let next_y = wrapped(view.font, spec.description, x, y + 12.0, width, 10, DIM, 2);
    let (label, enabled) = if view.save_read_only {
        ("Tylko odczyt".to_string(), false)
    } else if level >= spec.max_level {
        ("Maksymalny poziom".to_string(), false)
    } else {
        let cost = spec.costs[level as usize];
        (
            format!("Kup za {} wiedzy", pl(cost, 0)),
            view.state.knowledge + f64::EPSILON >= cost,
        )
    };
    button(
        view.font,
        Rect::new(x, next_y + 3.0, width, 27.0),
        &label,
        false,
        enabled.then_some(Action::BuyUpgrade(kind)),
        actions,
    );
    next_y + 34.0
}

fn draw_roadmap(
    view: &View<'_>,
    x: f32,
    mut y: f32,
    width: f32,
    bottom: f32,
    actions: &mut Vec<Action>,
) {
    y = wrapped(
        view.font,
        "OŚ KAMPANII • PLANOWANE / NIEGRYWALNE",
        x,
        y,
        width,
        13,
        ACCENT,
        2,
    );
    let page_count = roadmap_page_count();
    let page = view.roadmap_page.min(page_count.saturating_sub(1));
    let page_label = format!("Strona {}/{}", page + 1, page_count);
    text(view.font, &page_label, x, y + 16.0, 10, DIM);
    let nav_y = y - 2.0;
    let next_x = x + width - 29.0;
    button(
        view.font,
        Rect::new(next_x - 33.0, nav_y, 28.0, 24.0),
        "‹",
        false,
        (page > 0).then_some(Action::RoadmapPage(-1)),
        actions,
    );
    button(
        view.font,
        Rect::new(next_x, nav_y, 28.0, 24.0),
        "›",
        false,
        (page + 1 < page_count).then_some(Action::RoadmapPage(1)),
        actions,
    );
    y += 32.0;
    y = wrapped(
        view.font,
        "Kolejne epoki nie odblokują się przez zmianę chapter ID w pliku. Każda przyszła karta wymaga osobnego opracowania i źródeł.",
        x,
        y,
        width,
        11,
        WARNING,
        3,
    );
    y += 6.0;
    for card in catalog::ROADMAP
        .iter()
        .skip(page * ROADMAP_PAGE_SIZE)
        .take(ROADMAP_PAGE_SIZE)
    {
        if y >= bottom - 18.0 {
            break;
        }
        let title = format!("• {} — PLANOWANE", card.title);
        y = wrapped(view.font, &title, x, y, width, 11, INK, 2);
        y = wrapped(
            view.font,
            &format!("{} · {}", card.interval, card.note),
            x + 9.0,
            y,
            width - 9.0,
            9,
            DIM,
            3,
        );
        y += 7.0;
    }
}

fn draw_tree(view: &View<'_>, x: f32, mut y: f32, width: f32, bottom: f32) {
    y = wrapped(
        view.font,
        "DRZEWO ŻYCIA • wyjaśnienie, nie drabina",
        x,
        y,
        width,
        13,
        ACCENT,
        2,
    );
    y += 5.0;
    y = wrapped(
        view.font,
        "Gałęzie żyją równocześnie, rozchodzą się i czasem wymierają. Odblokowanie karty opisuje postęp obserwatora, nie relację rodzic → dziecko.",
        x,
        y,
        width,
        12,
        INK,
        4,
    );
    y += 9.0;
    for line in [
        "wiele wczesnych linii komórkowych",
        "├─ bakterie        ├─ archeony",
        "└─ eukarionty → wiele niezależnych gałęzi",
        "   └─ zwierzęta → strunowce → czworonogi",
        "      └─ synapsydy → ssaki → naczelne",
        "         └─ wiele linii homininów → Homo sapiens",
    ] {
        if y > bottom - 24.0 {
            break;
        }
        y = wrapped(view.font, line, x, y, width, 11, DIM, 2);
        y += 4.0;
    }
    y += 5.0;
    let endpoint = format!(
        "Koniec dostępnego etapu: po {} km nadal możesz podróżować i kupować ulepszenia. Następne epoki są planowane; ten mikroorganizm nie zmienia się w człowieka.",
        pl(campaign::END_OF_SLICE_KM, 2)
    );
    wrapped(view.font, &endpoint, x, y, width, 12, WARNING, 4);
}

fn button(
    font: &Option<Font>,
    rect: Rect,
    label: &str,
    selected: bool,
    action: Option<Action>,
    actions: &mut Vec<Action>,
) {
    let mouse = Vec2::from(mouse_position());
    let hovered = rect.contains(mouse);
    let fill = if selected {
        ACCENT_DARK
    } else if hovered && action.is_some() {
        Color::new(0.10, 0.27, 0.29, 0.98)
    } else if action.is_some() {
        PANEL_ALT
    } else {
        Color::new(0.09, 0.14, 0.16, 0.98)
    };
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, fill);
    draw_rectangle_lines(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        1.0,
        if selected { ACCENT } else { DIM },
    );
    let size = if rect.w < 150.0 { 10 } else { 11 };
    let dimensions = measure_text(label, font.as_ref(), size, 1.0);
    let scale = (rect.w - 10.0) / dimensions.width.max(1.0);
    let font_scale = scale.clamp(0.62, 1.0);
    draw_text_ex(
        label,
        rect.x + 5.0,
        rect.y + rect.h * 0.69,
        TextParams {
            font: font.as_ref(),
            font_size: size,
            font_scale,
            color: if action.is_some() { INK } else { DIM },
            ..Default::default()
        },
    );
    if hovered && is_mouse_button_pressed(MouseButton::Left) {
        if let Some(action) = action {
            actions.push(action);
        }
    }
}

fn text(font: &Option<Font>, value: &str, x: f32, y: f32, size: u16, color: Color) {
    draw_text_ex(
        value,
        x,
        y,
        TextParams {
            font: font.as_ref(),
            font_size: size,
            color,
            ..Default::default()
        },
    );
}

fn wrapped(
    font: &Option<Font>,
    value: &str,
    x: f32,
    y: f32,
    width: f32,
    size: u16,
    color: Color,
    max_lines: usize,
) -> f32 {
    let lines = wrap_lines(font, value, size, width.max(20.0));
    let step = size as f32 * 1.22;
    let mut drawn = 0;
    for (index, line) in lines.iter().enumerate() {
        if index >= max_lines {
            break;
        }
        text(font, line, x, y + index as f32 * step, size, color);
        drawn += 1;
    }
    y + drawn as f32 * step
}

fn wrap_lines(font: &Option<Font>, value: &str, size: u16, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in value.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if measure_text(&candidate, font.as_ref(), size, 1.0).width <= width {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        let mut segment = String::new();
        for character in word.chars() {
            let candidate = format!("{segment}{character}");
            if !segment.is_empty()
                && measure_text(&candidate, font.as_ref(), size, 1.0).width > width
            {
                lines.push(std::mem::take(&mut segment));
            }
            segment.push(character);
        }
        current = segment;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
