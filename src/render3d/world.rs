//! Środowisko i obiekty wspólnego generatora świata.
//!
//! Pozycje są absolutnymi metrami z `crate::world`; tutaj tylko zamieniamy je
//! na lokalny układ względem kamery. Wszystkie bryły korzystają z buildera
//! geometrii i z bazowych kolorów materiałów, bez CPU-owego światła lub mgły.

use std::collections::{HashMap, hash_map::Entry};

use macroquad::prelude::*;

use crate::rng::{Biom, biom, hash1, region_of};
use crate::theme::{CAPS, ROAD, ROAD_LINE};
use crate::world::{
    AnimalSpecies, Geometry, LAMP_LED_COLOR_SRGB, LAMP_POLE_HEIGHT_M, MaterialKind, ObjectDesc,
    lamp_direction, lamp_head_offset,
};

use super::geometry::Bld;
use super::material::{
    MaterialFlags, MaterialSpec, SurfaceKind, scale_linear_srgb, surface_preset,
};
use super::weather::{draw_lamp_atmosphere, draw_lamp_pool};
use super::{FAR, GROUND_FAR, HW, Quality, Scene, TAU};

pub(crate) const GROUND_TILE_M: f64 = 30.0;
pub(crate) const REGION_CACHE_LIMIT: usize = 8;
pub(crate) const GROUND_TILE_CACHE_LIMIT: usize = 32;

const COLOR_X_MIN: f64 = -70.0;
const COLOR_X_MAX: f64 = GROUND_FAR as f64;
const SHADOW_X_MIN: f64 = -140.0;
const SHADOW_X_MAX: f64 = 430.0;

#[derive(Clone, Copy)]
enum ObjectLod {
    Near,
    Far,
}

struct HouseShape {
    relative: f32,
    lateral: f32,
    height: f32,
    half_x: f32,
    half_z: f32,
    city: bool,
    lod: ObjectLod,
}

const WINDOW_HALF_HEIGHT: f32 = 0.5;
const WINDOW_LOWER_ROW_Y: f32 = 1.1;
const WINDOW_ROW_STEP_DIVISOR: f32 = 2.4;
const WINDOW_MIN_ROW_STEP: f32 = 1.6;
const WINDOW_PROJECTION: f32 = 0.05;
const CITY_BOARD_HEIGHT: f32 = 0.08;
const CITY_BOARD_WALL_OFFSET: f32 = 0.24;

#[derive(Clone, Copy, Debug, PartialEq)]
struct WindowRow {
    center_y: f32,
    half_height: f32,
}

fn window_row(height: f32, row: usize) -> Option<WindowRow> {
    let center_y = WINDOW_LOWER_ROW_Y
        + row as f32 * (height / WINDOW_ROW_STEP_DIVISOR).max(WINDOW_MIN_ROW_STEP);
    (center_y + WINDOW_HALF_HEIGHT <= height).then_some(WindowRow {
        center_y,
        half_height: WINDOW_HALF_HEIGHT,
    })
}

fn city_board_center_y(height: f32) -> Option<f32> {
    window_row(height, 0)
        .map(|window| window.center_y - window.half_height - CITY_BOARD_HEIGHT * 0.5)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RoadFacade {
    wall: Vec3,
    normal: Vec3,
}

fn road_facade(base: Vec3, half_z: f32, lateral: f32) -> RoadFacade {
    let side = lateral.signum();
    let normal = Vec3::new(0.0, 0.0, -side);
    RoadFacade {
        wall: base + Vec3::new(0.0, 0.0, -half_z * side),
        normal,
    }
}

struct CachedRegion {
    start_m: f64,
    end_m: f64,
    near_meshes: Vec<Mesh>,
    far_meshes: Vec<Mesh>,
    dynamic_objects: Vec<ObjectDesc>,
    lights: Vec<crate::world::LightSource>,
}

struct CachedGroundTile {
    start_m: f64,
    end_m: f64,
    meshes: Vec<Mesh>,
}

/// Stable CPU geometry cache. Region descriptions and mesh buffers are keyed
/// by deterministic world coordinates, while retention is limited to the
/// directional shadow neighborhood. Lighting, rain and night state never
/// participate in a cache key.
pub(crate) struct WorldCache {
    quality: Quality,
    regions: HashMap<i64, CachedRegion>,
    ground_tiles: HashMap<i64, CachedGroundTile>,
    build_count: u64,
}

impl WorldCache {
    pub(crate) fn new(quality: Quality) -> Self {
        Self {
            quality,
            regions: HashMap::with_capacity(REGION_CACHE_LIMIT),
            ground_tiles: HashMap::with_capacity(GROUND_TILE_CACHE_LIMIT),
            build_count: 0,
        }
    }

    pub(crate) fn update(&mut self, cam_m: f64) {
        let first_region = region_of(cam_m + SHADOW_X_MIN) - 1;
        let last_region = region_of(cam_m + SHADOW_X_MAX) + 1;
        for region in first_region..=last_region {
            if let Entry::Vacant(entry) = self.regions.entry(region) {
                entry.insert(build_region(region, self.quality));
                self.build_count = self.build_count.saturating_add(1);
            }
        }
        self.regions
            .retain(|region, _| (first_region..=last_region).contains(region));

        let first_tile = ((cam_m + SHADOW_X_MIN) / GROUND_TILE_M).floor() as i64 - 1;
        let last_tile = ((cam_m + COLOR_X_MAX) / GROUND_TILE_M).floor() as i64 + 1;
        for tile in first_tile..=last_tile {
            if let Entry::Vacant(entry) = self.ground_tiles.entry(tile) {
                entry.insert(build_ground_tile(tile));
                self.build_count = self.build_count.saturating_add(1);
            }
        }
        self.ground_tiles
            .retain(|tile, _| (first_tile..=last_tile).contains(tile));
    }

    pub(crate) fn draw_color<F>(&self, cam_m: f64, quality: Quality, mut draw: F)
    where
        F: FnMut(&Mesh, Vec3),
    {
        let first_tile = ((cam_m + COLOR_X_MIN) / GROUND_TILE_M).floor() as i64 - 1;
        let last_tile = ((cam_m + COLOR_X_MAX) / GROUND_TILE_M).floor() as i64 + 1;
        for tile_index in first_tile..=last_tile {
            let Some(tile) = self.ground_tiles.get(&tile_index) else {
                continue;
            };
            let offset = Vec3::new((tile.start_m - cam_m) as f32, 0.0, 0.0);
            for mesh in &tile.meshes {
                draw(mesh, offset);
            }
        }

        let first_region = region_of(cam_m + COLOR_X_MIN) - 1;
        let last_region = region_of(cam_m + FAR as f64) + 1;
        for region_index in first_region..=last_region {
            let Some(region) = self.regions.get(&region_index) else {
                continue;
            };
            let offset = Vec3::new((region.start_m - cam_m) as f32, 0.0, 0.0);
            let distance = if cam_m < region.start_m {
                region.start_m - cam_m
            } else if cam_m > region.end_m {
                cam_m - region.end_m
            } else {
                0.0
            };
            let meshes = region_meshes(region, distance, quality);
            for mesh in meshes {
                draw(mesh, offset);
            }
        }
    }

    /// Select only static meshes whose route span can affect one shadow map.
    /// The lamp pass uses a narrow local range instead of submitting the full
    /// 600 m ground strip to a selected local shadow-map target.
    pub(crate) fn draw_shadow<F>(
        &self,
        cam_m: f64,
        x_min: f32,
        x_max: f32,
        quality: Quality,
        mut draw: F,
    ) where
        F: FnMut(&Mesh, Vec3),
    {
        let x_min = x_min as f64;
        let x_max = x_max as f64;
        let first_tile = ((cam_m + x_min) / GROUND_TILE_M).floor() as i64 - 1;
        let last_tile = ((cam_m + x_max) / GROUND_TILE_M).floor() as i64 + 1;
        for tile_index in first_tile..=last_tile {
            let Some(tile) = self.ground_tiles.get(&tile_index) else {
                continue;
            };
            if tile.end_m < cam_m + x_min || tile.start_m > cam_m + x_max {
                continue;
            }
            let offset = Vec3::new((tile.start_m - cam_m) as f32, 0.0, 0.0);
            for mesh in &tile.meshes {
                draw(mesh, offset);
            }
        }

        let first_region = region_of(cam_m + x_min) - 1;
        let last_region = region_of(cam_m + x_max) + 1;
        for region_index in first_region..=last_region {
            let Some(region) = self.regions.get(&region_index) else {
                continue;
            };
            if region.end_m < cam_m + x_min || region.start_m > cam_m + x_max {
                continue;
            }
            let offset = Vec3::new((region.start_m - cam_m) as f32, 0.0, 0.0);
            let distance = if cam_m < region.start_m {
                region.start_m - cam_m
            } else if cam_m > region.end_m {
                cam_m - region.end_m
            } else {
                0.0
            };
            for mesh in region_meshes(region, distance, quality) {
                draw(mesh, offset);
            }
        }
    }

    pub(crate) fn dynamic_objects(&self, cam_m: f64) -> impl Iterator<Item = &ObjectDesc> {
        let min_m = cam_m + COLOR_X_MIN;
        let max_m = cam_m + FAR as f64 + 20.0;
        self.regions.values().flat_map(move |region| {
            region.dynamic_objects.iter().filter(move |object| {
                object.position.along_m >= min_m && object.position.along_m <= max_m
            })
        })
    }

    pub(crate) fn lamps(&self, cam_m: f64) -> impl Iterator<Item = crate::world::LightSource> + '_ {
        let min_m = cam_m + COLOR_X_MIN;
        let max_m = cam_m + FAR as f64 + 20.0;
        self.regions.values().flat_map(move |region| {
            region.lights.iter().copied().filter(move |source| {
                source.position.along_m >= min_m && source.position.along_m <= max_m
            })
        })
    }

    pub(crate) fn region_count(&self) -> usize {
        self.regions.len()
    }

    pub(crate) fn ground_tile_count(&self) -> usize {
        self.ground_tiles.len()
    }

    pub(crate) fn build_count(&self) -> u64 {
        self.build_count
    }
}

fn region_meshes(region: &CachedRegion, distance: f64, quality: Quality) -> &[Mesh] {
    if distance > quality.far_lod_start() as f64 {
        &region.far_meshes
    } else {
        &region.near_meshes
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

fn smoothstep(edge0: f32, edge1: f32, value: f64) -> f32 {
    let t = ((value as f32 - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn surface(kind: SurfaceKind) -> MaterialSpec {
    MaterialSpec::from_surface(surface_preset(kind, 0.0))
}

fn object_surface(kind: MaterialKind) -> MaterialSpec {
    match kind {
        MaterialKind::Foliage | MaterialKind::Mushroom => surface(SurfaceKind::Foliage),
        MaterialKind::Animal => surface(SurfaceKind::Wood),
        MaterialKind::House | MaterialKind::Sign | MaterialKind::Fence => {
            surface(SurfaceKind::Wood)
        }
        MaterialKind::Metal | MaterialKind::Signal | MaterialKind::Rock => {
            surface(SurfaceKind::Metal)
        }
    }
}

fn biome_ground(biom: Biom) -> (Color, MaterialSpec) {
    match biom {
        Biom::Las => {
            let preset = surface_preset(SurfaceKind::Foliage, 0.0);
            (
                Color::new(0.11, 0.24, 0.14, 1.0),
                MaterialSpec::from_surface(preset),
            )
        }
        Biom::Laka => {
            let preset = surface_preset(SurfaceKind::Foliage, 0.0);
            (
                Color::new(0.28, 0.44, 0.22, 1.0),
                MaterialSpec::from_surface(preset),
            )
        }
        Biom::Wies => {
            let preset = surface_preset(SurfaceKind::Soil, 0.0);
            (
                Color::new(0.42, 0.37, 0.21, 1.0),
                MaterialSpec::from_surface(preset),
            )
        }
        Biom::Miasto => {
            let preset = surface_preset(SurfaceKind::Asphalt, 0.0);
            (
                Color::new(0.24, 0.27, 0.32, 1.0),
                MaterialSpec::from_surface(preset),
            )
        }
    }
}

fn biome_ground_at(along_m: f64) -> (Color, MaterialSpec) {
    let region = region_of(along_m);
    let (base_color, material) = biome_ground(biom(region));
    let region_start = region as f64 * crate::rng::REGION_M;
    let region_end = region_start + crate::rng::REGION_M;
    const TRANSITION_M: f64 = 26.0;
    let (neighbor, distance_to_edge) = if along_m - region_start < TRANSITION_M {
        (region - 1, along_m - region_start)
    } else if region_end - along_m < TRANSITION_M {
        (region + 1, region_end - along_m)
    } else {
        return (base_color, material);
    };
    let (neighbor_color, neighbor_material) = biome_ground(biom(neighbor));
    // Each side approaches the same midpoint at the common edge. This avoids
    // the old reversed blend, where the colour jumped from A to B exactly at
    // the boundary and the material changed independently.
    let toward_edge = 1.0 - smoothstep(0.0, TRANSITION_M as f32, distance_to_edge);
    let midpoint = mix_color(base_color, neighbor_color, 0.5);
    let color = mix_color(base_color, midpoint, toward_edge);
    let material = mix_material(material, neighbor_material, toward_edge * 0.5);
    (color, material)
}

fn mix_material(a: MaterialSpec, b: MaterialSpec, t: f32) -> MaterialSpec {
    MaterialSpec {
        roughness: a.roughness + (b.roughness - a.roughness) * t,
        emission: a.emission + (b.emission - a.emission) * t,
        flags: if a.flags == b.flags {
            a.flags
        } else {
            MaterialFlags::NONE
        },
        albedo_factor: a.albedo_factor + (b.albedo_factor - a.albedo_factor) * t,
    }
}

fn road_dash_offset(along_m: f64) -> f32 {
    (-along_m).rem_euclid(4.5) as f32
}

fn build_ground_tile(tile_index: i64) -> CachedGroundTile {
    let tile_start = tile_index as f64 * GROUND_TILE_M;
    let tile_end = tile_start + GROUND_TILE_M;
    let mut builder = Bld::new(Vec3::ZERO);
    const STEP: f32 = 10.0;
    const ZMIN: f32 = -560.0;
    const ZMAX: f32 = 150.0;
    const ZSTEP: f32 = 20.0;
    let verge = Color::new(0.34, 0.52, 0.26, 1.0);
    let asphalt = surface(SurfaceKind::Asphalt);
    let asphalt_color = ROAD;
    let line_material = surface(SurfaceKind::Metal);

    let mut x = 0.0;
    while x < (tile_end - tile_start) as f32 {
        let x0 = x;
        let x1 = (x + STEP).min((tile_end - tile_start) as f32);
        let (left_color, left_material) = biome_ground_at(tile_start + x0 as f64);
        let (right_color, right_material) = biome_ground_at(tile_start + x1 as f64);
        let mut z = ZMIN;
        builder.with_material(left_material, |builder| {
            while z < ZMAX {
                let z0 = z;
                let z1 = (z + ZSTEP).min(ZMAX);
                builder.quad_with_vertex_materials(
                    [
                        Vec3::new(x0, 0.0, z0),
                        Vec3::new(x0, 0.0, z1),
                        Vec3::new(x1, 0.0, z1),
                        Vec3::new(x1, 0.0, z0),
                    ],
                    [left_color, left_color, right_color, right_color],
                    [left_material, left_material, right_material, right_material],
                );
                z += ZSTEP;
            }
        });

        builder.with_material(asphalt, |builder| {
            builder.quad(
                Vec3::new(x0, 0.02, -HW),
                Vec3::new(x0, 0.02, HW),
                Vec3::new(x1, 0.02, HW),
                Vec3::new(x1, 0.02, -HW),
                asphalt_color,
            );
            puddle(builder, tile_start + x0 as f64, x0, x1, asphalt_color);
        });

        builder.with_material(line_material, |builder| {
            for side in [-1.0, 1.0] {
                let (z0, z1) = (side * HW, side * (HW + 0.18));
                builder.quad(
                    Vec3::new(x0, 0.04, z0),
                    Vec3::new(x0, 0.04, z1),
                    Vec3::new(x1, 0.04, z1),
                    Vec3::new(x1, 0.04, z0),
                    ROAD_LINE,
                );
                let (z2, z3) = (side * (HW + 0.18), side * (HW + 2.4));
                builder.quad(
                    Vec3::new(x0, 0.01, z2),
                    Vec3::new(x0, 0.01, z3),
                    Vec3::new(x1, 0.01, z3),
                    Vec3::new(x1, 0.01, z2),
                    verge,
                );
            }
        });
        x += STEP;
    }

    // Środkowe przerywane pasy pozostają deterministyczne względem absolutnej
    // trasy, but only the small periodic remainder is converted to f32. This
    // remains precise after a billion-metre route.
    let mut dash = road_dash_offset(tile_start);
    builder.with_material(line_material, |builder| {
        while dash < (tile_end - tile_start) as f32 {
            builder.cube(
                Vec3::new(dash, 0.05, 0.0),
                Vec3::new(2.0, 0.04, 0.13),
                ROAD_LINE,
            );
            dash += 4.5;
        }
    });

    let (meshes, _) = builder.finish();
    CachedGroundTile {
        start_m: tile_start,
        end_m: tile_end,
        meshes,
    }
}

fn build_region(region_index: i64, quality: Quality) -> CachedRegion {
    let region = crate::world::region_description(region_index);
    // Keep the cached description coupled to the same stable region metadata
    // used by album discovery, even though rendering only needs its spans and
    // object ids.
    let _ = (region.region, region.discovery);
    let mut near_builder = Bld::new(Vec3::ZERO);
    let mut far_builder = Bld::new(Vec3::ZERO);
    let mut dynamic_objects = Vec::new();
    let mut lights = Vec::new();
    for object in region.objects {
        if let Some(source) = object.light {
            lights.push(source);
        }
        if is_dynamic_object(object.geometry) {
            dynamic_objects.push(object);
            continue;
        }
        let relative = (object.position.along_m - region.start_m) as f32;
        let mut material = object_surface(object.material);
        if !object.casts_shadow {
            material.flags = material.flags.with(MaterialFlags::NO_SHADOW);
        }
        near_builder.with_material(material, |builder| {
            draw_static_geometry(
                builder,
                object.geometry,
                relative,
                object.position.lateral_m,
                ObjectLod::Near,
            );
        });
        far_builder.with_material(material, |builder| {
            draw_static_geometry(
                builder,
                object.geometry,
                relative,
                object.position.lateral_m,
                ObjectLod::Far,
            );
        });
    }
    let (near_meshes, _) = near_builder.finish();
    let (far_meshes, _) = far_builder.finish();
    let _ = quality;
    CachedRegion {
        start_m: region.start_m,
        end_m: region.end_m,
        near_meshes,
        far_meshes,
        dynamic_objects,
        lights,
    }
}

fn is_dynamic_object(geometry: Geometry) -> bool {
    matches!(geometry, Geometry::Animal { .. } | Geometry::Signal { .. })
}

pub(crate) fn draw_dynamic_objects(builder: &mut Bld, scene: &Scene, cache: &WorldCache) {
    for object in cache.dynamic_objects(scene.cam_m) {
        let relative = (object.position.along_m - scene.cam_m) as f32;
        let mut object_material = object_surface(object.material);
        if !object.casts_shadow {
            object_material.flags = object_material.flags.with(MaterialFlags::NO_SHADOW);
        }
        builder.with_material(object_material, |builder| {
            draw_geometry(
                builder,
                scene,
                object.geometry,
                relative,
                object.position.lateral_m,
            );
        });
    }
    for source in cache.lamps(scene.cam_m) {
        draw_lamp_atmosphere(builder, scene, source);
        draw_lamp_pool(builder, scene, source);
    }

    let material = surface(SurfaceKind::Wood);
    for object in &scene.physics.dynamics {
        if !(-70.0..=FAR + 20.0).contains(&object.position[0]) {
            continue;
        }
        let center = Vec3::from_array(object.position);
        let rotation = Quat::from_xyzw(
            object.rotation[0],
            object.rotation[1],
            object.rotation[2],
            object.rotation[3],
        );
        let color = match object.kind {
            0 => Color::new(0.66, 0.31, 0.13, 1.0),
            1 => Color::new(0.20, 0.46, 0.25, 1.0),
            _ => Color::new(0.72, 0.57, 0.20, 1.0),
        };
        builder.with_material(material, |builder| match object.kind {
            1 => builder.sphere(center, object.size[0] * 0.5, 7, 10, color),
            0 => builder.cube_rotated(center, Vec3::from_array(object.size), rotation, color),
            _ => {
                let half_segment = (object.size[1] - object.size[0]).max(0.04) * 0.5;
                let axis = rotation * Vec3::Y;
                builder.capsule(
                    center - axis * half_segment,
                    center + axis * half_segment,
                    object.size[0] * 0.5,
                    color,
                );
            }
        });
    }
}

fn puddle(builder: &mut Bld, absolute_x0: f64, x0: f32, x1: f32, asphalt: Color) {
    let segment = (absolute_x0 / 10.0).floor() as i64;
    let hash = hash1(segment as u64 ^ 0x90DD1E);
    if hash % 100 >= 50 {
        return;
    }
    let center_x = x0 + ((hash >> 8) % 1000) as f32 / 1000.0 * (x1 - x0);
    let center_z = ((hash >> 18) % 1000) as f32 / 1000.0 * (HW * 1.5) - HW * 0.75;
    let radius_x = 0.9 + ((hash >> 28) % 100) as f32 / 100.0 * 1.7;
    let radius_z = 0.28 + ((hash >> 38) % 100) as f32 / 100.0 * 0.45;
    let center = Vec3::new(center_x, 0.03, center_z);
    let material = MaterialSpec::new(0.88, 0.0, MaterialFlags::WET_PATCH);
    builder.with_material(material, |builder| {
        for (scale, tint) in [(1.0f32, 1.0f32), (0.45, 0.72)] {
            let color = scale_linear_srgb(asphalt, 0.98 + tint * 0.02);
            const SEGMENTS: usize = 10;
            for i in 0..SEGMENTS {
                let a0 = TAU * i as f32 / SEGMENTS as f32;
                let a1 = TAU * (i + 1) as f32 / SEGMENTS as f32;
                builder.tri_with_normal(
                    center,
                    center
                        + Vec3::new(
                            a0.cos() * radius_x * scale,
                            0.0,
                            a0.sin() * radius_z * scale,
                        ),
                    center
                        + Vec3::new(
                            a1.cos() * radius_x * scale,
                            0.0,
                            a1.sin() * radius_z * scale,
                        ),
                    Vec3::Y,
                    color,
                );
            }
        }
    });
}

fn draw_static_geometry(
    builder: &mut Bld,
    geometry: Geometry,
    relative: f32,
    lateral: f32,
    lod: ObjectLod,
) {
    match geometry {
        Geometry::Fir { scale } => fir(builder, relative, lateral, scale, lod),
        Geometry::Bush { radius } => bush(builder, relative, lateral, radius, lod),
        Geometry::Mushroom { species } => mushroom(builder, relative, lateral, species, lod),
        Geometry::House {
            city,
            height,
            half_x,
            half_z,
        } => house(
            builder,
            HouseShape {
                relative,
                lateral,
                height,
                half_x,
                half_z,
                city,
                lod,
            },
        ),
        Geometry::SettlementSign { city } => settlement_sign(builder, relative, lateral, city),
        Geometry::Lamp { side } => lamp_body(builder, relative, lateral, side, lod),
        Geometry::Rock { scale } => rock(builder, relative, lateral, scale, lod),
        Geometry::FlowerPatch { tint } => flower_patch(builder, relative, lateral, tint, lod),
        Geometry::FenceSegment { posts } => fence_segment(builder, relative, lateral, posts, lod),
        Geometry::Animal { .. } | Geometry::Signal { .. } => {}
    }
}

fn draw_geometry(
    builder: &mut Bld,
    scene: &Scene,
    geometry: Geometry,
    relative: f32,
    lateral: f32,
) {
    match geometry {
        Geometry::Fir { scale } => fir(builder, relative, lateral, scale, ObjectLod::Near),
        Geometry::Bush { radius } => bush(builder, relative, lateral, radius, ObjectLod::Near),
        Geometry::Mushroom { species } => {
            mushroom(builder, relative, lateral, species, ObjectLod::Near)
        }
        Geometry::Animal { species, seed } => {
            let visible = visible_animal(species, scene.night);
            let sway = (scene.time * (0.8 + seed as f32 * 0.07)).sin() * 0.045;
            let base = Vec3::new(relative + sway, 0.0, lateral);
            if relative.abs() > 120.0 {
                animal_lod(builder, visible, base);
            } else {
                animal_body(builder, visible, seed, base, -lateral.signum(), scene.night);
            }
        }
        Geometry::House {
            city,
            height,
            half_x,
            half_z,
        } => house(
            builder,
            HouseShape {
                relative,
                lateral,
                height,
                half_x,
                half_z,
                city,
                lod: ObjectLod::Near,
            },
        ),
        Geometry::SettlementSign { city } => settlement_sign(builder, relative, lateral, city),
        Geometry::Lamp { side } => lamp_body(builder, relative, lateral, side, ObjectLod::Near),
        Geometry::Rock { scale } => rock(builder, relative, lateral, scale, ObjectLod::Near),
        Geometry::FlowerPatch { tint } => {
            flower_patch(builder, relative, lateral, tint, ObjectLod::Near)
        }
        Geometry::FenceSegment { posts } => {
            fence_segment(builder, relative, lateral, posts, ObjectLod::Near)
        }
        Geometry::Signal { side } => signal_body(builder, scene, relative, lateral, side),
    }
}

fn fir(builder: &mut Bld, relative: f32, lateral: f32, scale: f32, lod: ObjectLod) {
    let trunk_height = 1.6 * scale;
    let trunk_radius = 0.18 * scale;
    let base = Vec3::new(relative, 0.0, lateral);
    builder.with_material(surface(SurfaceKind::Wood), |builder| {
        builder.capsule(
            base + Vec3::Y * trunk_radius,
            base + Vec3::new(0.0, trunk_height, 0.0),
            trunk_radius,
            Color::new(0.34, 0.24, 0.15, 1.0),
        );
    });
    let foliage = surface(SurfaceKind::Foliage);
    builder.with_material(foliage, |builder| {
        let green = Color::new(0.10, 0.34, 0.18, 1.0);
        let tiers = match lod {
            ObjectLod::Near => 4,
            ObjectLod::Far => 2,
        };
        let segments = match lod {
            ObjectLod::Near => 10,
            ObjectLod::Far => 6,
        };
        for tier in 0..tiers {
            let tier = tier as f32;
            builder.cone(
                base + Vec3::new(0.0, trunk_height + tier * 1.25 * scale, 0.0),
                (2.45 - tier * 0.52) * scale,
                2.25 * scale,
                segments,
                scale_linear_srgb(green, 1.0 - tier * 0.06),
            );
        }
    });
}

fn bush(builder: &mut Bld, relative: f32, lateral: f32, radius: f32, lod: ObjectLod) {
    let (rings, slices) = match lod {
        ObjectLod::Near => (6, 10),
        ObjectLod::Far => (4, 6),
    };
    builder.sphere(
        Vec3::new(relative, radius * 0.6, lateral),
        radius,
        rings,
        slices,
        Color::new(0.22, 0.42, 0.20, 1.0),
    );
}

/// Bryła skalna: spłaszczona, szara sfera — czytelna z daleka, tania w
/// wierzchołkach i rzucająca cień jak drzewo; poniżej ciemna plama gleby.
fn rock(builder: &mut Bld, relative: f32, lateral: f32, scale: f32, lod: ObjectLod) {
    let (rings, slices) = match lod {
        ObjectLod::Near => (5, 8),
        ObjectLod::Far => (3, 5),
    };
    let radius = 0.45 * scale.max(0.35);
    let concrete = Color::new(0.52, 0.53, 0.54, 1.0);
    builder.sphere(
        Vec3::new(relative, radius * 0.62, lateral),
        radius,
        rings,
        slices,
        concrete,
    );
    builder.with_material(surface(SurfaceKind::Soil), |builder| {
        builder.ellipsoid(
            Vec3::new(relative, 0.012, lateral),
            Vec3::new(radius * 1.5, 0.02, radius * 1.5),
            1,
            6,
            Color::new(0.24, 0.21, 0.18, 1.0),
        );
    });
}

/// Kwiatowa łata: 9−14 mikro-kostek w barwach wiosennych rozrzuconych wokół
/// punktu (tint wybiera paletę); wyraziste kolory są czytelne w stylizacji.
fn flower_patch(builder: &mut Bld, relative: f32, lateral: f32, tint: u32, lod: ObjectLod) {
    if matches!(lod, ObjectLod::Far) {
        return;
    }
    const BLOOMS: [Color; 5] = [
        Color::new(0.95, 0.35, 0.55, 1.0), // róż
        Color::new(0.98, 0.78, 0.20, 1.0), // żółć
        Color::new(0.95, 0.95, 0.92, 1.0), // biały
        Color::new(0.60, 0.35, 0.85, 1.0), // fiolet
        Color::new(0.90, 0.45, 0.25, 1.0), // pomarańcz
    ];
    let bloom = BLOOMS[(tint % 5) as usize];
    let leaves = 9 + (tint % 6) as usize;
    for k in 0..leaves {
        let h = hash1(tint as u64 ^ 0xB105_55 ^ (k as u64).wrapping_mul(0x2545_F491));
        let dx = ((h % 1000) as f32 / 1000.0 - 0.5) * 1.7;
        let dz = (((h >> 12) % 1000) as f32 / 1000.0 - 0.5) * 1.7;
        let size = 0.055 + ((h >> 24) % 5) as f32 * 0.014;
        builder.cube(
            Vec3::new(relative + dx, 0.05 + size * 0.5, lateral + dz),
            Vec3::new(size * 0.85, size, size * 0.85),
            if k % 4 == 3 {
                Color::new(0.16, 0.45, 0.22, 1.0)
            } else {
                bloom
            },
        );
    }
}

/// Parkan: N słupków (0.85 m) + dwie poziome szyny między pierwszym a ostatnim.
fn fence_segment(builder: &mut Bld, relative: f32, lateral: f32, posts: u32, lod: ObjectLod) {
    if matches!(lod, ObjectLod::Far) {
        // Daleki LOD parkanu: jedna belka — grubszy przekrój, żeby z odległości
        // nie mrugała jako 1-pikselowa linia.
        builder.with_material(surface(SurfaceKind::Wood), |builder| {
            builder.ellipsoid(
                Vec3::new(relative, 0.42, lateral),
                Vec3::new(1.4, 0.05, 0.055),
                1,
                4,
                Color::new(0.42, 0.28, 0.16, 1.0),
            );
        });
        return;
    }
    let posts = posts.max(2).clamp(2, 14);
    let spacing = 1.55;
    let half_len = (posts as f32 - 1.0) * spacing * 0.5;
    let wood = Color::new(0.45, 0.30, 0.17, 1.0);
    let wood_light = Color::new(0.55, 0.38, 0.22, 1.0);
    builder.with_material(surface(SurfaceKind::Wood), |builder| {
        for i in 0..posts {
            let x = relative - half_len + i as f32 * spacing;
            let jitter = ((hash1(0x7FAD ^ (i as u64)) % 100) as f32 / 100.0 - 0.5) * 0.05;
            // Słupki i szyny grubość ~2x większa względem pierwszej wersji:
            // cieniutkie kreski migoczą z odległości (aliasing), grubszy
            // przekrój czyta się jako parkan, nie wibrująca linia.
            builder.cube(
                Vec3::new(x + jitter, 0.425, lateral),
                Vec3::new(0.13, 0.85, 0.10),
                wood,
            );
        }
        // dwie poziome szyny na 0.30 i 0.60 wysokości
        for rail_y in [0.30_f32, 0.60_f32] {
            builder.cube(
                Vec3::new(relative, rail_y, lateral),
                Vec3::new(half_len * 2.0, 0.085, 0.055),
                wood_light,
            );
        }
    });
}

fn mushroom(builder: &mut Bld, relative: f32, lateral: f32, species: u32, lod: ObjectLod) {
    let cap = CAPS[(species % 5) as usize];
    let scale = 0.8 + ((species * 13) % 5) as f32 * 0.15;
    let stem_height = 0.42 * scale;
    let base = Vec3::new(relative, 0.0, lateral);
    builder.with_material(surface(SurfaceKind::Skin), |builder| {
        builder.capsule(
            base + Vec3::Y * (stem_height * 0.5),
            base + Vec3::Y * stem_height,
            0.09 * scale,
            Color::new(0.90, 0.86, 0.76, 1.0),
        );
    });
    builder.with_material(surface(SurfaceKind::Foliage), |builder| {
        let segments = match lod {
            ObjectLod::Near => 9,
            ObjectLod::Far => 6,
        };
        match species / 5 {
            0 => builder.sphere(base + Vec3::Y * stem_height, 0.22 * scale, 4, segments, cap),
            1 => builder.cone(
                base + Vec3::Y * stem_height,
                0.22 * scale,
                0.30 * scale,
                segments,
                cap,
            ),
            2 => builder.cone(
                base + Vec3::Y * stem_height,
                0.16 * scale,
                0.45 * scale,
                segments,
                cap,
            ),
            _ => builder.sphere(
                base + Vec3::Y * (stem_height + 0.05),
                0.24 * scale,
                4,
                segments,
                cap,
            ),
        }
    });
}

fn visible_animal(species: AnimalSpecies, night: f32) -> AnimalSpecies {
    if species == AnimalSpecies::Owl && night <= 0.3 {
        AnimalSpecies::Deer
    } else {
        species
    }
}

fn animal_lod(builder: &mut Bld, kind: AnimalSpecies, base: Vec3) {
    if kind == AnimalSpecies::Deer {
        // Jeleń w dalekim LOD: korpus + jasna głowa na szyi, a nie
        // pojedyncza kula (blob czytelny jako zwierzę z odległości).
        let coat = Color::new(0.55, 0.40, 0.26, 1.0);
        let head_color = Color::new(0.62, 0.47, 0.31, 1.0);
        builder.with_material(surface(SurfaceKind::Wood), |builder| {
            builder.sphere(base + Vec3::new(0.0, 1.02, 0.0), 0.30, 5, 7, coat);
            builder.sphere(base + Vec3::new(0.0, 1.68, -0.95), 0.155, 5, 6, head_color);
        });
        return;
    }
    let (center_y, radius, color) = match kind {
        AnimalSpecies::Owl => (1.05, 0.22, Color::new(0.36, 0.25, 0.16, 1.0)),
        AnimalSpecies::Deer => (0.72, 0.30, Color::new(0.55, 0.40, 0.26, 1.0)),
        AnimalSpecies::Hedgehog => (0.24, 0.30, Color::new(0.26, 0.23, 0.18, 1.0)),
        AnimalSpecies::Bear => (0.72, 0.58, Color::new(0.30, 0.21, 0.14, 1.0)),
        AnimalSpecies::Wolf => (0.62, 0.26, Color::new(0.44, 0.45, 0.47, 1.0)),
    };
    builder.with_material(surface(SurfaceKind::Wood), |builder| {
        builder.sphere(base + Vec3::new(0.0, center_y, 0.0), radius, 4, 6, color);
    });
}

fn animal_body(
    builder: &mut Bld,
    kind: AnimalSpecies,
    seed: u32,
    base: Vec3,
    facing: f32,
    night: f32,
) {
    let point = |x: f32, y: f32, z: f32| base + Vec3::new(x, y, z * facing);
    let fur = surface(SurfaceKind::Wood);
    let skin = surface(SurfaceKind::Skin);
    match kind {
        AnimalSpecies::Owl => {
            builder.with_material(fur, |builder| {
                builder.capsule(
                    point(0.0, 0.4, 0.0),
                    point(0.0, 1.65, 0.0),
                    0.10,
                    Color::new(0.30, 0.20, 0.12, 1.0),
                );
                builder.limb(
                    point(0.0, 1.55, 0.0),
                    point(0.55, 1.85, 0.0),
                    0.09,
                    Color::new(0.30, 0.20, 0.12, 1.0),
                );
                builder.sphere(
                    point(0.62, 2.0, 0.0),
                    0.22,
                    5,
                    8,
                    Color::new(0.42, 0.28, 0.16, 1.0),
                );
                builder.sphere(
                    point(0.62, 2.26, 0.0),
                    0.17,
                    5,
                    8,
                    Color::new(0.42, 0.28, 0.16, 1.0),
                );
            });
            let eye = Color::new(1.0, 0.85, 0.35, 1.0);
            builder.with_material(MaterialSpec::emissive(0.2, 0.9 * night), |builder| {
                for z in [-0.07f32, 0.07] {
                    builder.sphere(point(0.74, 2.28, z), 0.05, 4, 6, eye);
                }
            });
        }
        AnimalSpecies::Deer => {
            let coat = Color::new(0.55, 0.40, 0.26, 1.0);
            let coat_dark = Color::new(0.47, 0.33, 0.21, 1.0);
            let antler = Color::new(0.66, 0.58, 0.40, 1.0);
            builder.with_material(fur, |builder| {
                // Korpus poziomy: tułów + zad + pierś (jak u wilka/lisa).
                builder.capsule(point(0.0, 1.02, 0.55), point(0.0, 1.02, -0.52), 0.30, coat);
                builder.sphere(point(0.0, 1.02, 0.66), 0.26, 6, 9, coat);
                builder.sphere(point(0.0, 1.06, -0.60), 0.28, 6, 9, coat);
                // Szyja od łopatki w górę, głowa wysoko jak u jelenia.
                builder.capsule(point(0.0, 1.08, -0.58), point(0.0, 1.62, -0.98), 0.13, coat);
                builder.sphere(point(0.0, 1.70, -1.02), 0.16, 6, 10, coat);
                for x in [-1.0f32, 1.0] {
                    // uszy odchylone na boki
                    builder.capsule(
                        point(x * 0.12, 1.84, -0.98),
                        point(x * 0.30, 2.06, -0.92),
                        0.05,
                        coat_dark,
                    );
                    // poroże zawsze widoczne: belka główna + widełki przednie
                    builder.limb(
                        point(x * 0.14, 1.84, -0.96),
                        point(x * 0.21, 2.28, -0.74),
                        0.048,
                        antler,
                    );
                    builder.limb(
                        point(x * 0.16, 2.06, -0.84),
                        point(x * 0.19, 2.36, -1.04),
                        0.036,
                        antler,
                    );
                    if seed.is_multiple_of(2) {
                        builder.limb(
                            point(x * 0.17, 2.18, -0.80),
                            point(x * 0.22, 2.50, -0.62),
                            0.030,
                            antler,
                        );
                    }
                }
                // Nogi dwusegmentowe: udo + goleń + ciemne kopytko.
                for x in [-0.18f32, 0.18] {
                    for z in [-0.44f32, 0.46] {
                        builder.capsule(point(x, 0.95, z), point(x, 0.55, z), 0.068, coat);
                        builder.capsule(point(x, 0.55, z), point(x, 0.03, z), 0.052, coat);
                        builder.with_material(skin, |builder| {
                            builder.cube(
                                point(x, 0.05, z),
                                Vec3::new(0.085, 0.075, 0.075),
                                Color::new(0.22, 0.16, 0.12, 1.0),
                            );
                        });
                    }
                }
                // ogon z jaśniejszym podbarwem
                builder.sphere(
                    point(0.0, 1.16, 0.94),
                    0.07,
                    4,
                    6,
                    Color::new(0.85, 0.78, 0.66, 1.0),
                );
            });
            builder.with_material(skin, |builder| {
                // pysk + nos zamykają głowę
                builder.capsule(
                    point(0.0, 1.68, -1.06),
                    point(0.0, 1.50, -1.34),
                    0.075,
                    Color::new(0.35, 0.26, 0.18, 1.0),
                );
                builder.sphere(
                    point(0.0, 1.47, -1.38),
                    0.05,
                    4,
                    6,
                    Color::new(0.22, 0.16, 0.12, 1.0),
                );
            });
        }
        AnimalSpecies::Hedgehog => {
            builder.with_material(fur, |builder| {
                builder.sphere(
                    point(0.0, 0.24, 0.0),
                    0.30,
                    6,
                    10,
                    Color::new(0.26, 0.23, 0.18, 1.0),
                );
            });
            builder.with_material(skin, |builder| {
                builder.sphere(
                    point(0.0, 0.20, -0.30),
                    0.11,
                    5,
                    8,
                    Color::new(0.55, 0.45, 0.35, 1.0),
                );
            });
        }
        AnimalSpecies::Bear => {
            builder.with_material(fur, |builder| {
                builder.sphere(
                    point(0.0, 0.72, 0.0),
                    0.58,
                    7,
                    10,
                    Color::new(0.30, 0.21, 0.14, 1.0),
                );
                builder.sphere(
                    point(0.0, 1.05, -0.52),
                    0.30,
                    6,
                    9,
                    Color::new(0.30, 0.21, 0.14, 1.0),
                );
                for x in [-0.28f32, 0.28] {
                    for z in [-0.32f32, 0.32] {
                        builder.capsule(
                            point(x, 0.50, z),
                            point(x, 0.05, z),
                            0.17,
                            Color::new(0.30, 0.21, 0.14, 1.0),
                        );
                    }
                }
            });
        }
        AnimalSpecies::Wolf => {
            builder.with_material(fur, |builder| {
                for (offset_x, offset_z) in [(0.0f32, 0.0f32), (0.9, 0.7)] {
                    let point = |x: f32, y: f32, z: f32| point(x + offset_x, y, z + offset_z);
                    let color = Color::new(0.44, 0.45, 0.47, 1.0);
                    builder.capsule(point(0.0, 0.55, 0.0), point(0.0, 0.82, 0.0), 0.19, color);
                    builder.sphere(point(0.0, 0.82, -0.58), 0.14, 5, 8, color);
                    builder.limb(point(0.0, 0.70, 0.45), point(0.0, 0.92, 0.85), 0.07, color);
                    for x in [-0.14f32, 0.14] {
                        for z in [-0.35f32, 0.35] {
                            builder.capsule(point(x, 0.48, z), point(x, 0.05, z), 0.075, color);
                        }
                    }
                }
            });
        }
    }
}

fn settlement_sign(builder: &mut Bld, relative: f32, lateral: f32, city: bool) {
    let pole = Color::new(0.34, 0.24, 0.15, 1.0);
    let board = if city {
        Color::new(0.20, 0.35, 0.65, 1.0)
    } else {
        Color::new(0.22, 0.48, 0.26, 1.0)
    };
    builder.with_material(surface(SurfaceKind::Wood), |builder| {
        builder.capsule(
            Vec3::new(relative, 0.0, lateral),
            Vec3::new(relative, 2.2, lateral),
            0.06,
            pole,
        );
        builder.cube(
            Vec3::new(relative, 2.05, lateral),
            Vec3::new(0.10, 0.55, 1.3),
            board,
        );
    });
}

fn house(builder: &mut Bld, shape: HouseShape) {
    let base = Vec3::new(shape.relative, 0.0, shape.lateral);
    let road = road_facade(base, shape.half_z, shape.lateral);
    let wall = if shape.city {
        Color::new(0.44, 0.44, 0.50, 1.0)
    } else {
        Color::new(0.78, 0.70, 0.52, 1.0)
    };
    builder.with_material(surface(SurfaceKind::Wood), |builder| {
        builder.cube(
            base + Vec3::new(0.0, shape.height * 0.5, 0.0),
            Vec3::new(shape.half_x * 2.0, shape.height, shape.half_z * 2.0),
            wall,
        );
        if !shape.city {
            builder.roof(
                base + Vec3::new(0.0, shape.height, 0.0),
                shape.half_x + 0.5,
                shape.half_z + 0.5,
                2.2,
                Color::new(0.55, 0.24, 0.18, 1.0),
            );
        } else if matches!(shape.lod, ObjectLod::Near) {
            // Płaski gzyms i dolny parapet dają bryle miasta więcej detalu bez
            // usuwania wspólnego klastra domów.
            builder.cube(
                base + Vec3::new(0.0, shape.height + 0.08, 0.0),
                Vec3::new(shape.half_x * 2.16, 0.16, shape.half_z * 2.16),
                Color::new(0.27, 0.28, 0.34, 1.0),
            );
            if let Some(board_center_y) = city_board_center_y(shape.height) {
                let board_center = road.wall
                    + road.normal * CITY_BOARD_WALL_OFFSET
                    + Vec3::new(0.0, board_center_y, 0.0);
                builder.cube(
                    board_center,
                    Vec3::new(shape.half_x * 1.35, CITY_BOARD_HEIGHT, 0.42),
                    Color::new(0.18, 0.20, 0.25, 1.0),
                );
            }
        }
    });
    // Okna są w KAŻDYM LOD-zie: z daleka blok to szary korpus, więc bez
    // nocnych okien traciłby żółty blask dopiero po dojechaniu (konflikt
    // szare/żółte przy pop-inie). Quady okien są tanie, a shader te same
    // flagi NIGHT_GLOW obsługuje identycznie bliżej i dalej.
    // UNLIT | NO_SHADOW: szyby nie receivują mapy cieni (przy płaskim kącie
    // księżyca crawlują po nich pasy — „coś się rusza w oknie") i same
    // niczego nie cieniują. Szyba i tak jest ledwie widoczna poza emisją.
    let window_material = MaterialSpec::new(
        0.20,
        0.0,
        MaterialFlags::NIGHT_GLOW
            .with(MaterialFlags::UNLIT)
            .with(MaterialFlags::NO_SHADOW),
    );
    let window_color = Color::new(0.08, 0.11, 0.16, 1.0);
    builder.with_material(window_material, |builder| {
        windows_on(
            builder,
            base + Vec3::new(shape.half_x, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::X,
            shape.height,
            window_color,
        );
        windows_on(
            builder,
            base + Vec3::new(-shape.half_x, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            -Vec3::X,
            shape.height,
            window_color,
        );
        windows_on(
            builder,
            road.wall,
            Vec3::X,
            road.normal,
            shape.height,
            window_color,
        );
    });
}

fn windows_on(
    builder: &mut Bld,
    bottom: Vec3,
    axis: Vec3,
    normal: Vec3,
    height: f32,
    color: Color,
) {
    for row in 0..2 {
        let Some(window) = window_row(height, row) else {
            continue;
        };
        for offset in [-0.9f32, 0.9] {
            let center = Vec3::new(
                (bottom + axis * offset + normal * WINDOW_PROJECTION).x,
                window.center_y,
                (bottom + axis * offset + normal * WINDOW_PROJECTION).z,
            );
            let half_axis = axis * 0.55;
            let vertical = Vec3::new(0.0, window.half_height, 0.0);
            builder.quad(
                center - half_axis - vertical,
                center - half_axis + vertical,
                center + half_axis + vertical,
                center + half_axis - vertical,
                color,
            );
        }
    }
}

const LAMP_HOUSING_WIDTH_M: f32 = 0.28;
const LAMP_HOUSING_LENGTH_M: f32 = 0.64;
const LAMP_HOUSING_THICKNESS_M: f32 = 0.09;
const LAMP_PANEL_INSET_M: f32 = 0.012;
const LAMP_HOUSING_BEVEL_M: f32 = 0.024;
const LAMP_PANEL_WIDTH_M: f32 = 0.21;
const LAMP_PANEL_LENGTH_M: f32 = 0.50;
const LAMP_PANEL_BEVEL_M: f32 = 0.014;

fn lamp_footprint(half_width: f32, half_length: f32, bevel: f32) -> [Vec2; 8] {
    [
        Vec2::new(-half_width + bevel, -half_length),
        Vec2::new(half_width - bevel, -half_length),
        Vec2::new(half_width, -half_length + bevel),
        Vec2::new(half_width, half_length - bevel),
        Vec2::new(half_width - bevel, half_length),
        Vec2::new(-half_width + bevel, half_length),
        Vec2::new(-half_width, half_length - bevel),
        Vec2::new(-half_width, -half_length + bevel),
    ]
}

fn lamp_body(builder: &mut Bld, relative: f32, lateral: f32, side: f32, lod: ObjectLod) {
    let height = LAMP_POLE_HEIGHT_M;
    let base = Vec3::new(relative, 0.0, lateral);
    let pole = Color::new(0.13, 0.15, 0.17, 1.0);
    let head = base + Vec3::from_array(lamp_head_offset(side));
    let light_direction = Vec3::from_array(lamp_direction(head.z)).normalize();
    // The local underside is -Y, so the fixture and its panel share the exact
    // direction used by the world light source. The arm lands on the top/rear
    // of the shell instead of crossing the emitting plane at `head`.
    let housing_rotation = Quat::from_rotation_arc(-Vec3::Y, light_direction);
    let arm_anchor = head
        + housing_rotation
            * Vec3::new(
                0.0,
                LAMP_HOUSING_THICKNESS_M - LAMP_PANEL_INSET_M,
                side * 0.20,
            );
    builder.with_material(surface(SurfaceKind::Metal), |builder| {
        // Słup 8.5 cm promienia (zamiast 7): za cienki migał 1-2 pikselami
        // z odległości — user widział „wibrujące kreski".
        builder.capsule(base, base + Vec3::Y * height, 0.085, pole);
        builder.capsule(base + Vec3::Y * (height - 0.06), arm_anchor, 0.075, pole);
    });
    lamp_housing(builder, head, housing_rotation, lod);
}

fn lamp_housing(builder: &mut Bld, head: Vec3, rotation: Quat, _lod: ObjectLod) {
    let outer = lamp_footprint(
        LAMP_HOUSING_WIDTH_M * 0.5,
        LAMP_HOUSING_LENGTH_M * 0.5,
        LAMP_HOUSING_BEVEL_M,
    );
    let panel = lamp_footprint(
        LAMP_PANEL_WIDTH_M * 0.5,
        LAMP_PANEL_LENGTH_M * 0.5,
        LAMP_PANEL_BEVEL_M,
    );
    let bottom_y = -LAMP_PANEL_INSET_M;
    let top_y = bottom_y + LAMP_HOUSING_THICKNESS_M;
    let to_world = |point: Vec3| head + rotation * point;
    let normal = |normal: Vec3| rotation * normal;
    let outer_bottom: [Vec3; 8] =
        std::array::from_fn(|index| to_world(Vec3::new(outer[index].x, bottom_y, outer[index].y)));
    let outer_top: [Vec3; 8] =
        std::array::from_fn(|index| to_world(Vec3::new(outer[index].x, top_y, outer[index].y)));
    let panel_inset: [Vec3; 8] =
        std::array::from_fn(|index| to_world(Vec3::new(panel[index].x, 0.0, panel[index].y)));
    let housing_color = Color::new(0.09, 0.11, 0.13, 1.0);
    let housing_material = surface(SurfaceKind::Metal);

    // The underside is an open dark rim. This leaves the smaller diffuser
    // visibly recessed instead of putting a glowing slab over the whole head.
    builder.with_material(housing_material, |builder| {
        let top_center = to_world(Vec3::new(0.0, top_y, 0.0));
        for index in 0..8 {
            let next = (index + 1) % 8;
            builder.tri_with_normal(
                top_center,
                outer_top[next],
                outer_top[index],
                normal(Vec3::Y),
                housing_color,
            );
        }
        for index in 0..8 {
            let next = (index + 1) % 8;
            let edge = outer[next] - outer[index];
            let side_normal = Vec3::new(edge.y, 0.0, -edge.x).normalize();
            builder.quad_with_normals(
                outer_bottom[index],
                outer_top[index],
                outer_top[next],
                outer_bottom[next],
                [normal(side_normal); 4],
                housing_color,
            );
            builder.quad_with_normals(
                outer_bottom[index],
                outer_bottom[next],
                panel_inset[next],
                panel_inset[index],
                [normal(-Vec3::Y); 4],
                housing_color,
            );
        }
    });

    let led_color = Color::new(
        LAMP_LED_COLOR_SRGB[0],
        LAMP_LED_COLOR_SRGB[1],
        LAMP_LED_COLOR_SRGB[2],
        1.0,
    );
    let panel_normal = normal(-Vec3::Y);
    let led_material = MaterialSpec::new(
        0.32,
        0.0,
        MaterialFlags::NIGHT_LED.with(MaterialFlags::NO_SHADOW),
    );
    builder.with_material(led_material, |builder| {
        let panel_center = to_world(Vec3::ZERO);
        for index in 0..8 {
            let next = (index + 1) % 8;
            builder.tri_with_normal(
                panel_center,
                panel_inset[index],
                panel_inset[next],
                panel_normal,
                led_color,
            );
        }
        // Pasy LED również w dalekim LOD-zie: bez nich głowy z odległości
        // 100+ m wyglądały na zgaszone (dwa quady — koszt pomijalny).
        for center_x in [-0.052f32, 0.052] {
            let half_width = 0.008;
            let half_length = 0.19;
            let points = [
                to_world(Vec3::new(center_x - half_width, -0.001, -half_length)),
                to_world(Vec3::new(center_x + half_width, -0.001, -half_length)),
                to_world(Vec3::new(center_x + half_width, -0.001, half_length)),
                to_world(Vec3::new(center_x - half_width, -0.001, half_length)),
            ];
            builder.quad_with_normals(
                points[0],
                points[1],
                points[2],
                points[3],
                [panel_normal; 4],
                led_color,
            );
        }
    });
}

fn signal_body(builder: &mut Bld, scene: &Scene, relative: f32, lateral: f32, side: f32) {
    let height = 3.5;
    let base = Vec3::new(relative, 0.0, lateral);
    let metal = surface(SurfaceKind::Metal);
    builder.with_material(metal, |builder| {
        builder.capsule(
            base,
            base + Vec3::Y * height,
            0.095,
            Color::new(0.20, 0.22, 0.24, 1.0),
        );
        builder.cube(
            base + Vec3::new(0.0, height + 0.55, -side * 0.32),
            Vec3::new(0.38, 1.10, 0.32),
            Color::new(0.12, 0.13, 0.14, 1.0),
        );
    });
    let cycle = scene.time.rem_euclid(9.0) / 9.0;
    let states = [
        (Color::new(1.0, 0.15, 0.12, 1.0), cycle < 0.55),
        (
            Color::new(1.0, 0.75, 0.12, 1.0),
            (0.55..0.68).contains(&cycle),
        ),
        (Color::new(0.20, 1.0, 0.40, 1.0), cycle >= 0.68),
    ];
    for (index, (color, on)) in states.into_iter().enumerate() {
        let center = base + Vec3::new(0.0, height + 0.98 - index as f32 * 0.36, -side * 0.50);
        let material = if on {
            MaterialSpec::new(0.18, 1.2 * (0.45 + 0.55 * scene.night), MaterialFlags::NONE)
        } else {
            metal
        };
        builder.with_material(material, |builder| {
            builder.sphere(
                center,
                0.11,
                5,
                8,
                if on {
                    color
                } else {
                    scale_linear_srgb(color, 0.20)
                },
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_cache_reuses_regions_and_stays_bounded() {
        let mut cache = WorldCache::new(Quality::Low);
        cache.update(0.0);
        let first_builds = cache.build_count();
        let first_counts = (cache.regions.len(), cache.ground_tiles.len());
        cache.update(0.0);
        assert_eq!(cache.build_count(), first_builds);
        assert_eq!(
            (cache.regions.len(), cache.ground_tiles.len()),
            first_counts
        );

        for step in 1..=24 {
            cache.update(step as f64 * 300.0);
        }
        let (regions, ground_tiles) = (cache.regions.len(), cache.ground_tiles.len());
        assert!(regions <= REGION_CACHE_LIMIT);
        assert!(ground_tiles <= GROUND_TILE_CACHE_LIMIT);
        assert!(cache.build_count() > first_builds);
    }

    #[test]
    fn road_dash_offset_uses_small_periodic_f64_remainder() {
        let at_large_origin = road_dash_offset(1_000_000_000.0);
        let one_period_later = road_dash_offset(1_000_000_004.5);
        assert!((0.0..4.5).contains(&at_large_origin));
        assert!((at_large_origin - one_period_later).abs() < 1.0e-4);
    }

    #[test]
    fn biome_transition_has_one_midpoint_from_both_sides() {
        let region = (0..1000_i64)
            .find(|&region| biom(region) != biom(region + 1))
            .expect("test needs two different neighboring biomes");
        let edge = (region + 1) as f64 * crate::rng::REGION_M;
        let (left_base, left_material) = biome_ground(biom(region));
        let (right_base, right_material) = biome_ground(biom(region + 1));
        let midpoint = mix_color(left_base, right_base, 0.5);
        let (left, left_transition) = biome_ground_at(edge - 1.0e-4);
        let (at_edge, edge_material) = biome_ground_at(edge);
        let (right, right_transition) = biome_ground_at(edge + 1.0e-4);

        for color in [left, at_edge, right] {
            assert!((color.r - midpoint.r).abs() < 1.0e-4, "{color:?}");
            assert!((color.g - midpoint.g).abs() < 1.0e-4, "{color:?}");
            assert!((color.b - midpoint.b).abs() < 1.0e-4, "{color:?}");
        }
        assert!((left_transition.roughness - edge_material.roughness).abs() < 1.0e-4);
        assert!((right_transition.roughness - edge_material.roughness).abs() < 1.0e-4);
        if (left_material.roughness - right_material.roughness).abs() > 1.0e-5 {
            assert!(
                left_transition.roughness > left_material.roughness.min(right_material.roughness)
            );
            assert!(
                left_transition.roughness < left_material.roughness.max(right_material.roughness)
            );
        }
    }

    #[test]
    fn biome_transition_stays_continuous_at_a_billion_meter_origin() {
        let region = (3_333_333_i64..3_334_333_i64)
            .find(|&region| biom(region) != biom(region + 1))
            .expect("large-origin test needs two different neighboring biomes");
        let edge = (region + 1) as f64 * crate::rng::REGION_M;
        let left = biome_ground_at(edge - 1.0e-4).0;
        let right = biome_ground_at(edge + 1.0e-4).0;
        assert!((left.r - right.r).abs() < 1.0e-4);
        assert!((left.g - right.g).abs() < 1.0e-4);
        assert!((left.b - right.b).abs() < 1.0e-4);
    }

    fn ground_vertex(tile: &CachedGroundTile, x: f32, z: f32) -> Vertex {
        tile.meshes
            .iter()
            .flat_map(|mesh| mesh.vertices.iter())
            .find(|vertex| {
                vertex.position.y == 0.0
                    && (vertex.position.x - x).abs() < 1.0e-5
                    && (vertex.position.z - z).abs() < 1.0e-5
            })
            .copied()
            .unwrap_or_else(|| panic!("missing ground vertex at ({x}, {z})"))
    }

    #[test]
    fn cached_ground_mesh_interpolates_absolute_biome_endpoints() {
        let region = (0..1000_i64)
            .find(|&region| biom(region) != biom(region + 1))
            .expect("test needs a biome transition");
        let edge = (region + 1) as f64 * crate::rng::REGION_M;
        let tile_index = (edge / GROUND_TILE_M) as i64 - 1;
        let tile = build_ground_tile(tile_index);
        let z = -560.0;

        for (x, absolute) in [(0.0, edge - 30.0), (10.0, edge - 20.0)] {
            let vertex = ground_vertex(&tile, x, z);
            let (expected_color, expected_material) = biome_ground_at(absolute);
            let expected_bytes: [u8; 4] = expected_color.into();
            assert_eq!(vertex.color, expected_bytes, "x={x}");
            assert!((vertex.uv.x - expected_material.roughness).abs() < 1.0e-6);
        }

        let left = ground_vertex(&tile, 0.0, z);
        let right = ground_vertex(&tile, 10.0, z);
        assert_ne!(
            left.color, right.color,
            "transition must produce a gradient"
        );
    }

    fn assert_ground_tile_seam(tile_index: i64) {
        let left = build_ground_tile(tile_index);
        let right = build_ground_tile(tile_index + 1);
        for z in [-560.0, -540.0, 0.0, 140.0, 150.0] {
            let left_vertex = ground_vertex(&left, 30.0, z);
            let right_vertex = ground_vertex(&right, 0.0, z);
            assert_eq!(left_vertex.color, right_vertex.color, "z={z}");
            assert_eq!(left_vertex.uv, right_vertex.uv, "z={z}");
            assert_eq!(left_vertex.normal, right_vertex.normal, "z={z}");
        }
    }

    #[test]
    fn cached_ground_mesh_neighbor_seams_match_at_normal_and_large_origins() {
        assert_ground_tile_seam(0);
        assert_ground_tile_seam((1_000_000_000.0 / GROUND_TILE_M).floor() as i64);
    }

    #[test]
    fn city_board_is_a_sill_below_road_windows_on_both_facades() {
        let height = 6.0;
        let lower_window = window_row(height, 0).expect("city house has a lower row");
        let lower_edge = lower_window.center_y - lower_window.half_height;
        let board_center_y = city_board_center_y(height).expect("city house has a sill");
        let board_top = board_center_y + CITY_BOARD_HEIGHT * 0.5;
        assert!(board_top <= lower_edge);
        assert!((board_top - lower_edge).abs() < 1.0e-6);

        for lateral in [-11.0, 11.0] {
            let base = Vec3::new(4.0, 0.0, lateral);
            let road = road_facade(base, 3.0, lateral);
            let board_center = road.wall
                + road.normal * CITY_BOARD_WALL_OFFSET
                + Vec3::new(0.0, board_center_y, 0.0);
            let window_center = road.wall
                + road.normal * WINDOW_PROJECTION
                + Vec3::new(0.0, lower_window.center_y, 0.0);
            let board_to_wall = board_center - road.wall;
            let window_to_wall = window_center - road.wall;
            assert!((board_to_wall.dot(road.normal) - CITY_BOARD_WALL_OFFSET).abs() < 1.0e-6);
            assert!((window_to_wall.dot(road.normal) - WINDOW_PROJECTION).abs() < 1.0e-6);
            assert_eq!(road.normal, Vec3::new(0.0, 0.0, -lateral.signum()));
            assert_eq!(board_to_wall.z.signum(), road.normal.z.signum());
        }
    }
}
