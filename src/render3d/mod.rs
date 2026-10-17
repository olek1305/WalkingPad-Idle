//! Orkiestracja jedynej sceny świata 3D.
//!
//! Szczegóły są rozdzielone na builder geometrii, biegacza, środowisko,
//! niebo, pogodę i materiał GPU. Ta warstwa zna tylko kolejność passów,
//! kamerę oraz rozmiar obszaru świata obok panelu UI.

pub(crate) mod geometry;
pub(crate) mod lighting;
pub(crate) mod material;
pub(crate) mod ocean;
mod runner;
pub(crate) mod shadow;
mod sky;
mod weather;
mod world;

use macroquad::material::{Material, gl_use_default_material, gl_use_material};
use macroquad::prelude::*;
use std::time::Instant;

use crate::daynight::{Palette, lamp_schedule};

use geometry::Bld;

pub(crate) use ocean::{OceanRenderer, OceanScene};

pub(crate) const FAR: f32 = 280.0;
pub(crate) const GROUND_FAR: f32 = 600.0;
pub(crate) const FOG_M: f32 = 520.0;
pub(crate) const HW: f32 = 2.6;
pub(crate) const LANE: f32 = -1.15;
pub(crate) const TAU: f32 = std::f32::consts::TAU;
pub(crate) const PI: f32 = std::f32::consts::PI;
pub(crate) const EYE: [f32; 3] = [-7.6, 3.9, 4.6];
pub(crate) const TGT: [f32; 3] = [6.5, 1.35, LANE + 0.3];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quality {
    Low,
    /// Default: leaves headroom for the 2048x1152 Arc B580 desktop window.
    #[default]
    Medium,
    High,
}

impl Quality {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    pub(crate) const fn shadow_sizes(self) -> (u32, u32) {
        match self {
            Self::Low => (1024, 256),
            // Medium: wyższa mapa kierunkowa (3072) i lamp (768) — texel
            // ~2.3 cm w lugar dotychczasowych ~3.5 cm: ostrzejsze cienie.
            Self::Medium => (3072, 768),
            Self::High => (4096, 1024),
        }
    }

    pub(crate) const fn far_lod_start(self) -> f32 {
        match self {
            Self::Low => 95.0,
            Self::Medium => 135.0,
            Self::High => 180.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldArea {
    pub width: f32,
    pub height: f32,
}

impl WorldArea {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width: width.max(1.0),
            height: height.max(1.0),
        }
    }

    fn viewport(self) -> (i32, i32, i32, i32) {
        (0, 0, self.width.round() as i32, self.height.round() as i32)
    }

    fn aspect(self) -> f32 {
        self.width / self.height
    }
}

/// Dane jednej klatki. `cam_m` jest absolutną pozycją w metrach; model
/// biegacza zawsze pozostaje w lokalnym `x = 0`.
pub struct Scene<'a> {
    pub cam_m: f64,
    pub spd: f32,
    pub physics: &'a crate::physics::PhysicsSnapshot,
    pub pal: &'a Palette,
    pub night: f32,
    pub bright: f32,
    pub rain: f32,
    pub fog: f32,
    pub time: f32,
    pub dt: f32,
    /// Actual frame interval for pacing statistics; `dt` is the bounded
    /// simulation step used by physics and gameplay.
    pub frame_dt: f32,
    pub doba: f64,
    pub world_area: WorldArea,
}

/// Stan renderera tworzony raz w `main`. Material/target ownership może zostać
/// tu rozszerzona w etapach 3/4 bez przenoszenia stanu do pętli klatek.
pub struct Renderer {
    pub world_area: WorldArea,
    frame: u64,
    quality: Quality,
    surface_material: Material,
    effect_material: Material,
    beam_material: Material,
    light_selector: lighting::LightSelector,
    shadow_selector: lighting::ShadowSelector,
    shadow_renderer: shadow::ShadowRenderer,
    world_cache: world::WorldCache,
    dynamic_builder: Bld,
    last_stats: RenderStats,
    total_cpu_ms: f64,
    benchmark_start_frame: u64,
    benchmark_active: bool,
    benchmark_frame_ms: Vec<f64>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub cpu_ms: f64,
    pub opaque_meshes: usize,
    pub opaque_vertices: usize,
    pub effect_meshes: usize,
    pub effect_vertices: usize,
    pub shadow_draws: usize,
    pub cache_regions: usize,
    pub cache_ground_tiles: usize,
    pub cache_builds: u64,
    pub dynamic_bodies: usize,
}

impl Renderer {
    pub fn new() -> Self {
        Self::with_quality(Quality::default())
    }

    pub fn with_quality(quality: Quality) -> Self {
        Self {
            world_area: WorldArea::new(1.0, 1.0),
            frame: 0,
            quality,
            surface_material: material::surface_material().clone(),
            effect_material: material::effect_material().clone(),
            beam_material: material::beam_material().clone(),
            light_selector: lighting::LightSelector::default(),
            shadow_selector: lighting::ShadowSelector::default(),
            shadow_renderer: shadow::ShadowRenderer::new(quality),
            world_cache: world::WorldCache::new(quality),
            dynamic_builder: Bld::new(Vec3::from_array(EYE)),
            last_stats: RenderStats::default(),
            total_cpu_ms: 0.0,
            benchmark_start_frame: 0,
            benchmark_active: false,
            benchmark_frame_ms: Vec::new(),
        }
    }

    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn stats(&self) -> RenderStats {
        self.last_stats
    }

    pub fn begin_benchmark(&mut self) {
        self.benchmark_start_frame = self.frame;
        self.total_cpu_ms = 0.0;
        self.benchmark_active = true;
        self.benchmark_frame_ms.clear();
    }

    pub fn benchmark_summary(
        &self,
        duration_s: f64,
        bodies: usize,
        viewport_width: u32,
        viewport_height: u32,
    ) -> String {
        let measured_frames = self.frame.saturating_sub(self.benchmark_start_frame).max(1);
        let frames = measured_frames as f64;
        let seconds = duration_s.max(0.001);
        let stats = self.stats();
        let frame_ms_avg = average(&self.benchmark_frame_ms);
        format!(
            "benchmark: quality={:?} viewport={}x{} frames={} elapsed_s={:.3} avg_fps={:.1} frame_ms_avg={:.2} frame_ms_p50={:.2} frame_ms_p95={:.2} renderer_cpu_avg_ms={:.2} renderer_cpu_last_ms={:.2} last_meshes={} last_vertices={} shadow_draws={} cache_regions={} cache_ground={} cache_builds={} bodies={}",
            self.quality,
            viewport_width,
            viewport_height,
            measured_frames,
            seconds,
            frames / seconds,
            frame_ms_avg,
            percentile(&self.benchmark_frame_ms, 0.50),
            percentile(&self.benchmark_frame_ms, 0.95),
            self.total_cpu_ms / frames,
            stats.cpu_ms,
            stats.opaque_meshes + stats.effect_meshes,
            stats.opaque_vertices + stats.effect_vertices,
            stats.shadow_draws,
            stats.cache_regions,
            stats.cache_ground_tiles,
            stats.cache_builds,
            stats.dynamic_bodies.max(bodies),
        )
    }

    pub fn draw(&mut self, scene: &Scene) {
        let cpu_start = Instant::now();
        self.frame = self.frame.wrapping_add(1);
        self.world_area = scene.world_area;
        self.world_cache.update(scene.cam_m);

        let sky_color = mix_color(scene.pal.sky_top, scene.pal.sky_bot, 0.55);
        clear_background(sky_color);

        let eye = Vec3::from_array(EYE);
        let target = Vec3::from_array(TGT);
        let color_camera = Camera3D {
            position: eye,
            target,
            up: Vec3::Y,
            fovy: 55f32.to_radians(),
            aspect: Some(scene.world_area.aspect()),
            viewport: Some(scene.world_area.viewport()),
            z_far: 3000.0,
            ..Default::default()
        };

        let fog_end = FOG_M * (1.0 - 0.72 * scene.fog) * (1.0 - 0.25 * scene.rain);
        let fog_start = (fog_end * 0.16).max(8.0);
        let sun = sky::directional_light(scene.doba as f32);
        let ambient = ambient_color(scene.pal, scene.night);
        let sun_strength = (0.92 * scene.bright + 0.08).clamp(0.16, 1.0);
        // Mgła rozprasza światło kierunkowe: plama od słońca/księżyca nie
        // ma świecić przez gęstą mgłę. Mgła na obrazie świata pracuje dalej
        // jako zwykły miksu wg odległości od kamery (shader, po oświetleniu).
        let sun_strength = sun_strength * (1.0 - 0.85 * scene.fog);
        let ambient_strength = (0.90 + 0.10 * (1.0 - scene.night)).clamp(0.0, 1.0);
        let (local_lights, shadow_set) = lighting::select_visible(
            scene.cam_m,
            scene.dt,
            &mut self.light_selector,
            &mut self.shadow_selector,
        );

        self.dynamic_builder.reset_reusable(eye);
        sky::draw_sky(&mut self.dynamic_builder, scene, sky_color);
        world::draw_dynamic_objects(&mut self.dynamic_builder, scene, &self.world_cache);
        weather::draw_rain(&mut self.dynamic_builder, scene, sky_color);
        runner::draw_runner(&mut self.dynamic_builder, scene.physics.runner);
        self.dynamic_builder.finish_reusable();
        let meshes = self.dynamic_builder.opaque_meshes();

        // Shadow maps are rendered before the color pass from the same
        // opaque meshes. The map pass changes camera/viewport/material state;
        // restore the color camera explicitly before any visible draw call.
        let shadow_bindings =
            self.shadow_renderer
                .render(&self.world_cache, meshes, sun, scene.cam_m, &shadow_set);
        set_camera(&color_camera);

        let lighting = material::LightingFrame {
            camera: eye,
            sun_dir: sun,
            sun_strength,
            ambient_srgb: ambient,
            ambient_strength,
            fog_srgb: sky_color,
            fog_start,
            fog_end,
            night: scene.night,
            wetness: scene.rain,
            lamp: lamp_schedule(scene.doba),
            lights: &local_lights,
        };
        material::bind_lighting(&self.surface_material, &lighting);
        self.shadow_renderer
            .bind_material(&self.surface_material, &shadow_bindings);
        gl_use_material(&self.surface_material);
        let mut opaque_meshes = 0;
        let mut opaque_vertices = 0;
        self.world_cache
            .draw_color(scene.cam_m, self.quality, |mesh, offset| {
                material::bind_world_offset(&self.surface_material, offset);
                opaque_meshes += 1;
                opaque_vertices += mesh.vertices.len();
                draw_mesh(mesh);
            });
        for mesh in meshes {
            material::bind_world_offset(&self.surface_material, Vec3::ZERO);
            opaque_meshes += 1;
            opaque_vertices += mesh.vertices.len();
            draw_mesh(mesh);
        }

        material::bind_lighting(&self.effect_material, &lighting);
        self.shadow_renderer
            .bind_material(&self.effect_material, &shadow_bindings);
        gl_use_material(&self.effect_material);
        let mut effect_meshes = 0;
        let mut effect_vertices = 0;
        for mesh in self.dynamic_builder.effect_meshes() {
            material::bind_world_offset(&self.effect_material, Vec3::ZERO);
            effect_meshes += 1;
            effect_vertices += mesh.vertices.len();
            draw_mesh(mesh);
        }
        // Stożki lamp w osobnym passie z addytywnym blendingiem: przecięcia
        // stożków rozjaśniają się (sumowanie rozproszenia we mgle), deszcz
        // zostaje na alpha-blend, żeby dzienny step kres nie wybielił nieba.
        material::bind_lighting(&self.beam_material, &lighting);
        self.shadow_renderer
            .bind_material(&self.beam_material, &shadow_bindings);
        gl_use_material(&self.beam_material);
        let mut beam_mesh_count = 0;
        let mut beam_vertex_count = 0;
        for mesh in self.dynamic_builder.beam_meshes() {
            material::bind_world_offset(&self.beam_material, Vec3::ZERO);
            beam_mesh_count += 1;
            beam_vertex_count += mesh.vertices.len();
            draw_mesh(mesh);
        }
        gl_use_default_material();
        set_default_camera();

        let cpu_ms = cpu_start.elapsed().as_secs_f64() * 1000.0;
        self.total_cpu_ms += cpu_ms;
        if self.benchmark_active {
            self.benchmark_frame_ms.push(raw_frame_ms(scene.frame_dt));
        }
        self.last_stats = RenderStats {
            cpu_ms,
            opaque_meshes,
            opaque_vertices,
            effect_meshes: effect_meshes + beam_mesh_count,
            effect_vertices: effect_vertices + beam_vertex_count,
            shadow_draws: self.shadow_renderer.last_draws(),
            cache_regions: self.world_cache.region_count(),
            cache_ground_tiles: self.world_cache.ground_tile_count(),
            cache_builds: self.world_cache.build_count(),
            dynamic_bodies: scene.physics.dynamic_bodies,
        };
    }
}

pub(crate) fn raw_frame_ms(frame_dt: f32) -> f64 {
    frame_dt.max(0.0) as f64 * 1000.0
}

fn percentile(values: &[f64], quantile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() - 1) as f64 * quantile.clamp(0.0, 1.0)).round() as usize;
    sorted[index]
}

fn average(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn mix_color(a: Color, b: Color, t: f32) -> Color {
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

fn ambient_color(palette: &Palette, night: f32) -> Color {
    let sky = mix_color(palette.sky_top, palette.sky_bot, 0.55);
    // A readable blue hemispheric fill keeps shadowed skin and cloth visible;
    // direct lamps and the directional source still provide the night
    // contrast. Values are sRGB and converted once in `bind_lighting`.
    let night_fill = Color::new(0.30, 0.33, 0.42, 1.0);
    mix_color(sky, night_fill, night.clamp(0.0, 1.0))
}
