//! Renderer pierwszego, podwodnego mikroświata ewolucji.
//!
//! Geometria jest lokalna względem kamery, ale jej pozycje wynikają z
//! deterministycznych indeksów chunków i z absolutnego f64 trasy. Dzięki temu
//! elementy mijają organizm w kierunku -X bez narastania błędu f32. Renderer
//! korzysta z istniejącego materiału powierzchni, liniowego światła, HSL i
//! map cieni. Pusty `WorldCache` jest przekazywany wyłącznie dlatego, że
//! wspólny `ShadowRenderer` wymaga tego interfejsu; stary świat nie jest tu
//! aktualizowany ani rysowany.

use macroquad::material::{Material, gl_use_default_material, gl_use_material};
use macroquad::prelude::*;

use super::geometry::Bld;
use super::lighting::{LightSet, ShadowSet};
use super::material::{
    Hsl, LightingFrame, MaterialFlags, MaterialSpec, bind_lighting, bind_world_offset,
    effect_material, hsl_to_srgb, surface_material,
};
use super::shadow::ShadowRenderer;
use super::world::WorldCache;
use super::{Quality, WorldArea};

const CHUNK_M: f64 = 24.0;
const VISIBLE_X_MIN: f32 = -45.0;
const VISIBLE_X_MAX: f32 = 210.0;
const CAMERA: Vec3 = Vec3::new(-7.8, 3.6, 7.2);
const TARGET: Vec3 = Vec3::new(5.0, 1.35, 0.0);
const SUN_DIRECTION: Vec3 = Vec3::new(-0.28, 0.86, -0.38);
const FOG_COLOR: Color = Color::new(0.045, 0.16, 0.23, 1.0);

pub struct OceanScene {
    pub forward_m: f64,
    pub seed: u64,
    /// Monotonic visual time. f64 keeps phase precision over long sessions;
    /// it is never used as the subject's travel coordinate.
    pub time: f64,
    pub observation_range_level: u8,
    pub world_area: WorldArea,
}

pub struct OceanRenderer {
    surface_material: Material,
    effect_material: Material,
    shadow_renderer: ShadowRenderer,
    empty_world: WorldCache,
    builder: Bld,
}

impl OceanRenderer {
    pub fn new() -> Self {
        Self::with_quality(Quality::default())
    }

    pub fn with_quality(quality: Quality) -> Self {
        Self {
            surface_material: surface_material().clone(),
            effect_material: effect_material().clone(),
            shadow_renderer: ShadowRenderer::new(quality),
            empty_world: WorldCache::new(quality),
            builder: Bld::new(CAMERA),
        }
    }

    pub fn draw(&mut self, scene: &OceanScene) {
        let fog_end = 175.0;
        let fog_start = 7.0;
        let sun = SUN_DIRECTION.normalize();
        clear_background(FOG_COLOR);

        let camera = Camera3D {
            position: CAMERA,
            target: TARGET,
            up: Vec3::Y,
            fovy: 54.0_f32.to_radians(),
            aspect: Some(scene.world_area.width / scene.world_area.height),
            viewport: Some((
                0,
                (screen_height() - scene.world_area.height).max(0.0).round() as i32,
                scene.world_area.width.round() as i32,
                scene.world_area.height.round() as i32,
            )),
            z_near: 0.05,
            z_far: 260.0,
            ..Default::default()
        };

        self.builder.reset_reusable(CAMERA);
        self.build_environment(scene);
        self.build_subject(scene);
        self.builder.finish_reusable();
        let meshes = self.builder.opaque_meshes();
        let shadow_bindings = self.shadow_renderer.render(
            &self.empty_world,
            meshes,
            sun,
            scene.forward_m,
            &ShadowSet::default(),
        );
        set_camera(&camera);

        let lights = LightSet::default();
        let lighting = LightingFrame {
            camera: CAMERA,
            sun_dir: sun,
            sun_strength: 0.62,
            ambient_srgb: Color::new(0.10, 0.29, 0.39, 1.0),
            ambient_strength: 0.94,
            fog_srgb: FOG_COLOR,
            fog_start,
            fog_end,
            night: 0.0,
            lamp: 0.0,
            wetness: 0.18,
            lights: &lights,
        };
        bind_lighting(&self.surface_material, &lighting);
        self.shadow_renderer
            .bind_material(&self.surface_material, &shadow_bindings);
        gl_use_material(&self.surface_material);
        for mesh in meshes {
            bind_world_offset(&self.surface_material, Vec3::ZERO);
            draw_mesh(mesh);
        }

        bind_lighting(&self.effect_material, &lighting);
        self.shadow_renderer
            .bind_material(&self.effect_material, &shadow_bindings);
        gl_use_material(&self.effect_material);
        for mesh in self.builder.effect_meshes() {
            bind_world_offset(&self.effect_material, Vec3::ZERO);
            draw_mesh(mesh);
        }
        gl_use_default_material();
        set_default_camera();
    }

    fn build_subject(&mut self, scene: &OceanScene) {
        let material = MaterialSpec::new(0.42, 0.0, MaterialFlags::NONE);
        // The treadmill coordinate belongs to the environment. Keep the
        // reconstructed cell camera-relative, with only a bounded gentle
        // drift so it cannot leave the frame during a long session.
        let center = Vec3::new(
            (scene.time * 0.34).sin() as f32 * 0.10,
            1.32 + (scene.time * 0.52).sin() as f32 * 0.11,
            (scene.time * 0.29).cos() as f32 * 0.08,
        );
        let membrane = hsl_to_srgb(Hsl {
            h: 0.53,
            s: 0.52,
            l: 0.47,
        });
        self.builder.with_material(material, |builder| {
            // Jedna gładka bryła bez jądra, kończyn, oczu, ust i sztucznie
            // dopisanego aparatu ruchu. To ilustracja, nie pewny wygląd.
            builder.ellipsoid(center, Vec3::new(1.32, 0.58, 0.48), 10, 16, membrane);
        });
    }

    fn build_environment(&mut self, scene: &OceanScene) {
        let cell_material = MaterialSpec::new(0.66, 0.0, MaterialFlags::NONE);
        let particle_material = MaterialSpec::new(0.88, 0.0, MaterialFlags::NONE);
        let (first_chunk, last_chunk) = visible_chunk_range(scene.forward_m);
        let range_level = scene.observation_range_level.min(2) as usize;
        let cells_per_chunk = 3 + range_level * 2;
        let particles_per_chunk = 9 + range_level * 4;

        self.builder.with_material(cell_material, |builder| {
            for chunk in first_chunk..=last_chunk {
                let chunk_origin = chunk as f64 * CHUNK_M;
                for index in 0..cells_per_chunk {
                    let hash = scene_hash(scene.seed, chunk, index as u64, 0xC311);
                    let local_x = 2.0_f64 + unit_float(hash) as f64 * (CHUNK_M - 4.0);
                    let relative_x = (chunk_origin + local_x - scene.forward_m) as f32;
                    if !(VISIBLE_X_MIN..=VISIBLE_X_MAX).contains(&relative_x) {
                        continue;
                    }
                    let z = -7.2 + unit_float(hash.rotate_left(11)) * 14.4;
                    let y = -0.9 + unit_float(hash.rotate_left(23)) * 7.0;
                    if relative_x.abs() < 3.0 && z.abs() < 1.6 {
                        continue;
                    }
                    let drift_phase =
                        unit_float(hash.rotate_left(31)) as f64 * std::f64::consts::TAU;
                    let drift = (scene.time
                        * (0.18 + unit_float(hash.rotate_left(37)) as f64 * 0.18)
                        + drift_phase)
                        .sin() as f32;
                    let radii = Vec3::new(
                        0.24 + unit_float(hash.rotate_left(43)) * 0.42,
                        0.12 + unit_float(hash.rotate_left(47)) * 0.23,
                        0.14 + unit_float(hash.rotate_left(53)) * 0.26,
                    );
                    let color = hsl_to_srgb(Hsl {
                        h: 0.46 + unit_float(hash.rotate_left(59)) * 0.16,
                        s: 0.36 + unit_float(hash.rotate_left(61)) * 0.22,
                        l: 0.31 + unit_float(hash.rotate_left(67)) * 0.15,
                    });
                    builder.ellipsoid(
                        Vec3::new(relative_x, y + drift * 0.15, z + drift * 0.12),
                        radii,
                        5,
                        8,
                        color,
                    );
                }
            }
        });

        self.builder.with_material(particle_material, |builder| {
            for chunk in first_chunk..=last_chunk {
                let chunk_origin = chunk as f64 * CHUNK_M;
                for index in 0..particles_per_chunk {
                    let hash = scene_hash(scene.seed, chunk, index as u64, 0xB471);
                    let local_x = unit_float(hash) as f64 * CHUNK_M;
                    let relative_x = (chunk_origin + local_x - scene.forward_m) as f32;
                    if !(VISIBLE_X_MIN..=VISIBLE_X_MAX).contains(&relative_x) {
                        continue;
                    }
                    let z = -8.5 + unit_float(hash.rotate_left(13)) * 17.0;
                    let base_y = -1.1 + unit_float(hash.rotate_left(19)) * 8.2;
                    let phase = unit_float(hash.rotate_left(29)) as f64 * std::f64::consts::TAU;
                    let y = base_y
                        + (scene.time * (0.12 + unit_float(hash.rotate_left(41)) as f64 * 0.14)
                            + phase)
                            .sin() as f32
                            * 0.12;
                    let radius = 0.025 + unit_float(hash.rotate_left(47)) * 0.045;
                    let color = hsl_to_srgb(Hsl {
                        h: 0.08 + unit_float(hash.rotate_left(71)) * 0.16,
                        s: 0.32 + unit_float(hash.rotate_left(73)) * 0.22,
                        l: 0.40 + unit_float(hash.rotate_left(79)) * 0.16,
                    });
                    builder.sphere(Vec3::new(relative_x, y, z), radius, 3, 5, color);
                }
            }
        });
    }
}

fn visible_chunk_range(forward_m: f64) -> (i64, i64) {
    if !forward_m.is_finite() {
        return (-2, 10);
    }
    // Keep the additions below representable even if a valid finite save is
    // far beyond the useful precision of a local visual coordinate. The
    // saved f64 distance remains untouched; only procedural display is bounded.
    let chunk = (forward_m / CHUNK_M)
        .floor()
        .clamp((i64::MIN + 2) as f64, (i64::MAX - 10) as f64) as i64;
    (chunk.saturating_sub(2), chunk.saturating_add(10))
}

fn scene_hash(seed: u64, chunk: i64, index: u64, salt: u64) -> u64 {
    let mut value = seed
        ^ (chunk as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ index.wrapping_mul(0xD1B5_4A32_D192_ED03)
        ^ salt;
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn unit_float(value: u64) -> f32 {
    ((value >> 40) as f32) / ((1_u64 << 24) as f32)
}
