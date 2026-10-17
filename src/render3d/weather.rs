//! Efekty pogody i lokalne efekty atmosferyczne latarni.
//!
//! Opaque surfaces receive lamp energy in the fragment shader. The only lamp
//! geometry in this module is a very low-alpha beam for rain/fog; clear air
//! never gets a solid yellow pool or cone painted over the road.

use macroquad::prelude::*;

use crate::daynight;
use crate::physics::GROUND_Y;
use crate::rng::hash1;
use crate::world::LightSource;

use super::geometry::{Bld, GlowQuad};
use super::material::MaterialSpec;
use super::{Scene, TAU};

fn mix_color(a: Color, b: Color, t: f32) -> Color {
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

/// Kreski deszczu mają przestrzenną pozycję i materiał z emisją, ale nie
/// zmieniają ekonomii ani pozycji świata.
pub(crate) fn draw_rain(builder: &mut Bld, scene: &Scene, fog_color: Color) {
    if scene.rain <= 0.03 {
        return;
    }
    let material = MaterialSpec::emissive(0.72, 0.18);
    builder.with_material(material, |builder| {
        let count = (40.0 + 300.0 * scene.rain) as u32;
        let velocity_x = -(scene.spd / 3.6);
        let velocity_y = -11.0;
        let velocity_length = (velocity_x * velocity_x + velocity_y * velocity_y).sqrt();
        let length = 0.22;
        let (dx, dy) = (
            velocity_x / velocity_length * length,
            velocity_y / velocity_length * length,
        );
        let width = 0.018;
        let color = mix_color(fog_color, Color::new(1.0, 1.0, 1.0, 1.0), 0.55).with_alpha(0.55);
        const SPAN: f32 = 90.0;
        for i in 0..count {
            let hash = hash1(i as u64 ^ 0x2C9A7);
            let x0 = (hash % 1000) as f32 / 1000.0 * SPAN;
            let z = -42.0 + ((hash >> 10) % 1000) as f32 / 1000.0 * 84.0;
            let y0 = ((hash >> 20) % 1000) as f32 / 1000.0 * 16.0;
            let x = 2.5 + (x0 + velocity_x * scene.time).rem_euclid(SPAN);
            let fall_span = 16.0 - GROUND_Y;
            let y = GROUND_Y + (y0 - GROUND_Y + velocity_y * scene.time).rem_euclid(fall_span);
            if y <= GROUND_Y + 0.02 {
                continue;
            }
            let start = Vec3::new(x, y, z);
            let end = Vec3::new(start.x + dx, (start.y + dy).max(GROUND_Y + 0.025), start.z);
            builder.glow_quad(GlowQuad {
                points: [
                    start + Vec3::new(0.0, 0.0, -width),
                    start + Vec3::new(0.0, 0.0, width),
                    end + Vec3::new(0.0, 0.0, width),
                    end + Vec3::new(0.0, 0.0, -width),
                ],
                colors: [color; 4],
            });
        }

        // A small deterministic splash ring is emitted at the same floor
        // height used by Rapier/rendered asphalt. Drops therefore disappear
        // into the actual ground response instead of stopping as floating
        // streaks.
        let splash_count = (8.0 + 28.0 * scene.rain) as u32;
        for i in 0..splash_count {
            let hash = hash1(i as u64 ^ 0x005A_115A);
            let phase = (scene.time * (1.4 + scene.rain * 0.8) + (hash % 1000) as f32 / 1000.0)
                .rem_euclid(1.0);
            if phase > 0.62 {
                continue;
            }
            let x = 2.5 + ((hash >> 14) % 1000) as f32 / 1000.0 * SPAN;
            let z = -42.0 + ((hash >> 24) % 1000) as f32 / 1000.0 * 84.0;
            let radius = 0.08 + phase * 0.32;
            let alpha = (0.11 * (1.0 - phase / 0.62) * scene.rain).clamp(0.0, 0.12);
            let center = Vec3::new(x, GROUND_Y + 0.026, z);
            let axis = Vec3::new(radius, 0.0, radius * 0.38);
            builder.glow_quad(GlowQuad {
                points: [
                    center - axis,
                    center + Vec3::new(axis.x, 0.0, -axis.z),
                    center + axis,
                    center + Vec3::new(-axis.x, 0.0, axis.z),
                ],
                colors: [color.with_alpha(alpha); 4],
            });
        }
    });
}

/// Atmosfera lampy: mała poświata wokół klosza + krótka mgiełka tuż pod nim,
/// plus miękka plama na gruncie (`draw_lamp_pool`). Świadomie BEZ wielkiego
/// stożka do drogi: przy tym kącie kamery siluet stożka zawsze daje ostre
/// krawędzie i wygląda jak narysowana figura, nie jak światło.
pub(crate) fn draw_lamp_atmosphere(builder: &mut Bld, scene: &Scene, source: LightSource) {
    if scene.rain <= 0.03 && scene.fog <= 0.03 {
        return;
    }
    // Ten sam „przełącznik" co źródła światła w shaderze: ON nocą, OFF od rana.
    let lamp_output = daynight::lamp_schedule(scene.doba);
    if lamp_output <= 0.0 {
        return;
    }
    let aim = match lamp_aim(scene, &source, 0.04) {
        Some(aim) => aim,
        None => return,
    };
    if aim.band <= 0.01 {
        return;
    }
    let material = MaterialSpec::emissive(0.72, 0.0);
    let strength = (scene.rain * 0.30 + scene.fog * 0.60).clamp(0.0, 0.9) * aim.band;
    builder.with_material(material, |builder| {
        head_halo(builder, &aim, lamp_output, strength);
        mist_cone(builder, &aim, lamp_output, strength);
    });
}

struct LampAim {
    head: Vec3,
    end: Vec3,
    radius: f32,
    band: f32,
    direction: Vec3,
    side: Vec3,
    up: Vec3,
}

/// Wspólna geometria atmosfery lampy: oś od głowy do podłoża + baza prostopadła
/// do osi dla pierścieni poświaty/mgiełki.
fn lamp_aim(scene: &Scene, source: &LightSource, ground_lift: f32) -> Option<LampAim> {
    let head = Vec3::new(
        (source.position.along_m - scene.cam_m) as f32,
        source.position.height_m,
        source.position.lateral_m,
    );
    if head.x.abs() > 220.0 {
        return None;
    }
    let direction = Vec3::from_array(source.direction).normalize_or_zero();
    if direction.length_squared() < 1e-8 || -direction.y <= 0.05 {
        return None;
    }
    let ground_y = GROUND_Y + ground_lift;
    let length = ((head.y - ground_y) / -direction.y).clamp(0.6, 8.5);
    let end = head + direction * length;
    let radius = (length * source.cone_outer_angle_rad.tan()).clamp(0.5, 2.9);
    // 110 m pełny efekt, zanik do zera przy ~180 m.
    let band = (1.0 - ((head.x.abs() - 110.0) / 70.0)).clamp(0.0, 1.0);
    let reference = if direction.y.abs() > 0.9 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let side = direction.cross(reference).normalize_or_zero();
    let up = direction.cross(side).normalize_or_zero();
    Some(LampAim {
        head,
        end,
        radius,
        band,
        direction,
        side,
        up,
    })
}

fn head_halo(builder: &mut Bld, aim: &LampAim, lamp_output: f32, strength: f32) {
    let warm = Color::new(1.0, 0.92, 0.72, 1.0);
    let core = warm.with_alpha(0.14 * lamp_output * strength);
    let rim = warm.with_alpha(0.0);
    let radius = 0.5;
    const SEGMENTS: usize = 8;
    for i in 0..SEGMENTS {
        let a0 = TAU * i as f32 / SEGMENTS as f32;
        let a1 = TAU * (i + 1) as f32 / SEGMENTS as f32;
        let p0 = aim.head + (aim.side * a0.cos() + aim.up * a0.sin()) * radius;
        let p1 = aim.head + (aim.side * a1.cos() + aim.up * a1.sin()) * radius;
        builder.beam_quad(GlowQuad {
            points: [aim.head, aim.head, p1, p0],
            colors: [core, core, rim, rim],
        });
    }
}

/// Krótka mgiełka wzdłuż osi tuż pod kloszem: promień 0.22 → 0.75 na ~1.9 m,
/// alfa zanika do zera na końcu — mikroskopijny siluet, bez „płacht".
fn mist_cone(builder: &mut Bld, aim: &LampAim, lamp_output: f32, strength: f32) {
    let warm = Color::new(1.0, 0.93, 0.76, 1.0);
    let near_alpha = 0.070 * lamp_output * strength;
    let far_alpha = 0.0;
    let r0 = 0.22;
    let r1 = 0.75;
    let end = aim.head + aim.direction * 1.9;
    const SEGMENTS: usize = 12;
    for i in 0..SEGMENTS {
        let a0 = TAU * i as f32 / SEGMENTS as f32;
        let a1 = TAU * (i + 1) as f32 / SEGMENTS as f32;
        let h0 = aim.head + (aim.side * a0.cos() + aim.up * a0.sin()) * r0;
        let h1 = aim.head + (aim.side * a1.cos() + aim.up * a1.sin()) * r0;
        let p0 = end + (aim.side * a0.cos() + aim.up * a0.sin()) * r1;
        let p1 = end + (aim.side * a1.cos() + aim.up * a1.sin()) * r1;
        builder.beam_quad(GlowQuad {
            points: [h0, h1, p1, p0],
            colors: [
                warm.with_alpha(near_alpha),
                warm.with_alpha(near_alpha),
                warm.with_alpha(far_alpha),
                warm.with_alpha(far_alpha),
            ],
        });
    }
}

/// Delikatna addytywna plama na gruncie pod KAŻDĄ lampą w zasięgu widoku.
/// Prawdziwych slotów światła jest tylko 8 i obejmują najbliższe latarnie,
/// więc dalsza część drogi wyglądała na zgaszoną. Plama jest subtelna (skala
/// harmonogramu latarni), a pełne światło shadera i tak dominuje przy
/// biegaczu, gdzie sloty działają.
pub(crate) fn draw_lamp_pool(builder: &mut Bld, scene: &Scene, source: LightSource) {
    let lamp_output = daynight::lamp_schedule(scene.doba);
    if lamp_output <= 0.03 {
        return;
    }
    let aim = match lamp_aim(scene, &source, 0.04) {
        Some(aim) => aim,
        None => return,
    };
    if aim.band <= 0.02 {
        return;
    }
    let material = MaterialSpec::emissive(0.72, 0.0);
    builder.with_material(material, |builder| {
        let warm = Color::new(1.0, 0.92, 0.72, 1.0);
        let core = warm.with_alpha(0.020 * lamp_output * aim.band);
        let rim = warm.with_alpha(0.0);
        // Plama mniejsza od dolnego kręgu stożka — czyta się jak najjaśniejsze
        // dno snopa światła, nie jak oddzielna płachta.
        let radius = (aim.radius * 0.55).clamp(0.5, 1.45);
        // Wachlarz 8 segmentów z alfa → 0 na obrzeżu: plama nie ma prostej
        // krawędzi „płyty", tylko miękki gradient jak podświetlenie asfaltu.
        const SEGMENTS: usize = 8;
        for i in 0..SEGMENTS {
            let a0 = TAU * i as f32 / SEGMENTS as f32;
            let a1 = TAU * (i + 1) as f32 / SEGMENTS as f32;
            let p0 = aim.end + Vec3::new(a0.cos() * radius, 0.0, a0.sin() * radius * 0.85);
            let p1 = aim.end + Vec3::new(a1.cos() * radius, 0.0, a1.sin() * radius * 0.85);
            builder.beam_quad(GlowQuad {
                points: [aim.end, aim.end, p1, p0],
                colors: [core, core, rim, rim],
            });
        }
    });
}
