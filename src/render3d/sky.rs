//! Niebo, słońce/księżyc, chmury i dalekie wzgórza.
//!
//! Obiekty nieba są jawnie `UNLIT | NO_SHADOW`; nie mogą przyciemnić się od
//! własnej normalnej ani wejść do przyszłej mapy cieni. Kolory palety są nadal
//! bazowym sRGB, a emisja jest parametrem materiału.

use macroquad::prelude::*;

use crate::rng::hash1;

use super::geometry::Bld;
use super::material::{MaterialFlags, MaterialSpec};
use super::{EYE, Scene, TAU, TGT};

fn mix_color(a: Color, b: Color, t: f32) -> Color {
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

fn scale_color(color: Color, amount: f32) -> Color {
    Color::new(
        color.r * amount,
        color.g * amount,
        color.b * amount,
        color.a,
    )
}

pub(crate) fn sun_angles(day_phase: f32) -> (f32, f32) {
    let forward_az = (TGT[2] - EYE[2]).atan2(TGT[0] - EYE[0]);
    // One smooth periodic artistic trajectory serves both the sky source and
    // the directional shadow map. The old half-day rem_euclid reset made the
    // azimuth jump at phase 0.5 and 1.0, visibly flipping shadows.
    let cycle = day_phase.rem_euclid(1.0) * TAU;
    let elevation = (19.0 + 9.0 * cycle.sin()).to_radians();
    let azimuth = forward_az - 0.175 + 0.9 * cycle.cos();
    (azimuth, elevation)
}

pub(crate) fn directional_light(day_phase: f32) -> Vec3 {
    let (azimuth, elevation) = sun_angles(day_phase);
    Vec3::new(
        elevation.cos() * azimuth.cos(),
        elevation.sin(),
        elevation.cos() * azimuth.sin(),
    )
}

pub(crate) fn draw_sky(builder: &mut Bld, scene: &Scene, _fog_color: Color) {
    let unlit = MaterialSpec::unlit();
    let emissive = MaterialSpec::emissive(0.18, 1.5);
    let mountain = MaterialSpec::new(
        0.95,
        0.0,
        MaterialFlags::UNLIT.with(MaterialFlags::NO_SHADOW),
    );

    builder.with_material(unlit, |builder| {
        let direction = directional_light(scene.doba as f32);
        let sun_distance = 1800.0;
        let sun_center = Vec3::from_array(EYE) + direction * sun_distance;
        // A compact procedural sun/moon remains readable without becoming a
        // low-poly foreground blob (the previous 49.5 m radius was excessive).
        let sun_radius = sun_distance * 0.0105;

        builder.with_material(emissive, |builder| {
            builder.sphere(sun_center, sun_radius, 6, 12, scene.pal.sun);
        });

        if scene.night > 0.12 {
            let star_color = mix_color(
                scene.pal.sky_top,
                Color::new(1.0, 1.0, 0.95, 1.0),
                scene.night,
            );
            for k in 0..70u64 {
                let hash = hash1(k ^ 0x5717A9);
                let azimuth = -0.368 + ((hash % 1000) as f32 / 1000.0 - 0.5) * 1.9;
                let elevation = 0.006 + ((hash >> 10) % 1000) as f32 / 1000.0 * 0.30;
                let distance = 2600.0;
                let position = Vec3::new(
                    azimuth.cos() * elevation.cos(),
                    elevation.sin(),
                    azimuth.sin() * elevation.cos(),
                ) * distance;
                let size = (0.55 + ((hash >> 20) % 3) as f32 * 0.18) * 2.0;
                builder.cube(position, Vec3::splat(size), star_color);
            }
        }

        let cloud_tint = 0.12 + 0.68 * scene.night.clamp(0.0, 1.0);
        let cloud = scale_color(
            mix_color(
                Color::new(0.92, 0.95, 1.0, 1.0),
                scene.pal.sky_top,
                cloud_tint,
            ),
            0.94 - 0.24 * scene.night.clamp(0.0, 1.0),
        );
        for c in 0..5 {
            let drift = (scene.time * 4.0 + c as f32 * 180.0).rem_euclid(900.0) - 200.0;
            let base = Vec3::new(
                drift,
                86.0 + (c % 3) as f32 * 13.0,
                -500.0 + c as f32 * 140.0,
            );
            for j in 0..4 {
                let hash = hash1((c as u64 * 17 + j as u64) ^ 0xC10D);
                let offset = Vec3::new(
                    j as f32 * 15.0 - 23.0 + ((hash % 7) as f32 - 3.0),
                    (j % 2) as f32 * 5.0,
                    (j % 3) as f32 * 9.0 - 8.0,
                );
                let radius = 11.0 + ((hash >> 8) % 7) as f32 * 1.6;
                builder.ellipsoid(
                    base + offset,
                    Vec3::new(radius * 1.45, radius * 0.52, radius * 0.82),
                    4,
                    8,
                    cloud,
                );
            }
        }
    });

    // Wzgórza są nieoświetlane jako tło, ale zostawiamy im zwykłą mgłę GPU.
    builder.with_material(mountain, |builder| {
        for k in 0..8 {
            let hash = hash1(k as u64 ^ 0x8811C);
            let scale = 2.6;
            let x = (300.0 + (hash % 90) as f32) * scale;
            let z = (-420.0 + k as f32 * 130.0 + (hash >> 8) as f32 % 60.0) * scale;
            let radius = (130.0 + (hash >> 16) as f32 % 60.0) * scale;
            let height = (30.0 + (hash >> 24) as f32 % 30.0) * scale;
            builder.cone(Vec3::new(x, 0.0, z), radius, height, 7, scene.pal.hill);
        }
        for k in 0..6 {
            let hash = hash1(k as u64 ^ 0x1234AB);
            let scale = 4.0;
            let x = (150.0 + (hash % 50) as f32) * scale;
            let z = (-360.0 + k as f32 * 130.0) * scale;
            let radius = (70.0 + (hash >> 8) as f32 % 40.0) * scale;
            let height = (14.0 + (hash >> 16) as f32 % 16.0) * scale;
            builder.cone(
                Vec3::new(x, 0.0, z),
                radius,
                height,
                6,
                scale_color(scene.pal.hill, 1.1),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directional_source_is_continuous_at_both_phase_joins() {
        for join in [0.0_f32, 0.5, 1.0] {
            let before = directional_light(join - 1.0e-4);
            let after = directional_light(join + 1.0e-4);
            assert!(
                before.dot(after) > 0.999_999,
                "join {join}: {before:?} {after:?}"
            );
        }
        assert!(directional_light(0.0).dot(directional_light(1.0)) > 0.999_999);
    }

    #[test]
    fn sky_source_and_shadow_source_use_the_same_direction() {
        for phase in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let (azimuth, elevation) = sun_angles(phase);
            let expected = Vec3::new(
                elevation.cos() * azimuth.cos(),
                elevation.sin(),
                elevation.cos() * azimuth.sin(),
            );
            assert!(directional_light(phase).dot(expected) > 0.999_999);
        }
    }
}
