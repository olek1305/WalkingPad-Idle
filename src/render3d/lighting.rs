//! CPU-side local-light descriptions and stable selection.
//!
//! The selector is deliberately independent from the GPU material. It keeps
//! the stable source id beside every selected light so stage 4 can bind the
//! same slots to shadow maps without inventing a second light order.

use macroquad::prelude::*;

use crate::rng::region_of;
use crate::world::{self, LightPosition, LightSource};

use super::{FAR, LANE};

pub(crate) const MAX_LOCAL_LIGHTS: usize = 8;
pub(crate) const RANGE_FADE_START: f32 = 0.72;
pub(crate) const INVERSE_SQUARE_SCALE: f32 = 0.12;

const SELECTION_HYSTERESIS: f32 = 0.18;
pub(crate) const FADE_DURATION_SECONDS: f32 = 0.20;
const SHADOW_SELECTION_HYSTERESIS: f32 = 0.24;
const SHADOW_FADE_DURATION_SECONDS: f32 = 0.20;
const VISIBLE_X_MIN: f32 = -60.0;
const VISIBLE_X_MAX: f32 = FAR;
const VISIBLE_Z_MIN: f32 = -48.0;
const VISIBLE_Z_MAX: f32 = 48.0;
const VISIBLE_Y_MIN: f32 = 0.0;
const VISIBLE_Y_MAX: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LightView {
    pub x_min: f32,
    pub x_max: f32,
    /// The high-attention part of the visible world. This affects only which
    /// sources fit in eight GPU slots; it never attenuates a selected source.
    pub focus_x: f32,
    pub z_min: f32,
    pub z_max: f32,
    pub y_min: f32,
    pub y_max: f32,
}

impl Default for LightView {
    fn default() -> Self {
        Self {
            x_min: VISIBLE_X_MIN,
            x_max: VISIBLE_X_MAX,
            focus_x: 0.0,
            z_min: VISIBLE_Z_MIN,
            z_max: VISIBLE_Z_MAX,
            y_min: VISIBLE_Y_MIN,
            y_max: VISIBLE_Y_MAX,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GpuLight {
    /// Stable source identity. This is intentionally kept outside the packed
    /// GLSL values; stage 4 uses it to associate this slot with a shadow map.
    pub source_id: u64,
    /// Absolute position is retained while a slot fades out. The shader only
    /// receives the camera-relative `position`; keeping this source-of-truth
    /// prevents a retired lamp from freezing in local space as the camera
    /// advances.
    pub absolute_position: LightPosition,
    pub position: Vec3,
    pub direction: Vec3,
    /// Base sRGB source color. `material::bind_lighting` uploads linear RGB.
    pub color: [f32; 3],
    pub range_m: f32,
    pub intensity: f32,
    pub cone_inner_cos: f32,
    pub cone_outer_cos: f32,
    /// Cross-fade weight used when a slot is introduced or retired.
    pub weight: f32,
}

impl Default for GpuLight {
    fn default() -> Self {
        Self {
            source_id: 0,
            absolute_position: LightPosition {
                along_m: 0.0,
                lateral_m: 0.0,
                height_m: 0.0,
            },
            position: Vec3::ZERO,
            direction: Vec3::Y,
            color: [0.0; 3],
            range_m: 0.0,
            intensity: 0.0,
            cone_inner_cos: 1.0,
            cone_outer_cos: 1.0,
            weight: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LightSet {
    pub lights: [GpuLight; MAX_LOCAL_LIGHTS],
}

impl Default for LightSet {
    fn default() -> Self {
        Self {
            lights: [GpuLight::default(); MAX_LOCAL_LIGHTS],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LightCandidate {
    pub light: GpuLight,
    pub priority: f32,
}

/// Smoothly bounded range attenuation shared by CPU prioritisation and the
/// fragment shader. It has an inverse-square-style core but never diverges at
/// the source and reaches exactly zero at the configured range.
pub(crate) fn range_attenuation(distance_m: f32, range_m: f32) -> f32 {
    if !distance_m.is_finite() || !range_m.is_finite() || range_m <= 0.0 {
        return 0.0;
    }
    let distance = distance_m.max(0.0);
    let normalized = distance / range_m;
    if normalized >= 1.0 {
        return 0.0;
    }
    let inverse_square = 1.0 / (1.0 + INVERSE_SQUARE_SCALE * distance * distance);
    inverse_square * (1.0 - smoothstep(RANGE_FADE_START, 1.0, normalized))
}

/// Smooth cone penumbra. The arguments use the same angle convention as the
/// world descriptor: full power at `inner_angle_rad`, zero outside
/// `outer_angle_rad`.
pub(crate) fn cone_attenuation(
    direction_cosine: f32,
    inner_angle_rad: f32,
    outer_angle_rad: f32,
) -> f32 {
    let inner = inner_angle_rad.clamp(0.0, std::f32::consts::PI).cos();
    let outer = outer_angle_rad.clamp(0.0, std::f32::consts::PI).cos();
    if inner <= outer {
        return if direction_cosine >= inner { 1.0 } else { 0.0 };
    }
    smoothstep(outer, inner, direction_cosine.clamp(-1.0, 1.0))
}

pub(crate) fn spot_attenuation(
    distance_m: f32,
    direction_cosine: f32,
    range_m: f32,
    inner_angle_rad: f32,
    outer_angle_rad: f32,
) -> f32 {
    range_attenuation(distance_m, range_m)
        * cone_attenuation(direction_cosine, inner_angle_rad, outer_angle_rad)
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    if (edge1 - edge0).abs() <= f32::EPSILON {
        return if value >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn source_as_gpu(source: LightSource, cam_m: f64) -> GpuLight {
    let direction = Vec3::from_array(source.direction).normalize_or_zero();
    GpuLight {
        source_id: source.id,
        absolute_position: source.position,
        position: Vec3::new(
            (source.position.along_m - cam_m) as f32,
            source.position.height_m,
            source.position.lateral_m,
        ),
        direction: if direction.length_squared() > 1e-8 {
            direction
        } else {
            Vec3::new(0.0, -1.0, 0.0)
        },
        color: source.color,
        range_m: source.range_m.max(0.0),
        intensity: source.intensity.max(0.0),
        cone_inner_cos: source
            .cone_inner_angle_rad
            .clamp(0.0, std::f32::consts::PI)
            .cos(),
        cone_outer_cos: source
            .cone_outer_angle_rad
            .clamp(0.0, std::f32::consts::PI)
            .cos(),
        weight: 1.0,
    }
}

fn localize(light: &mut GpuLight, cam_m: f64) {
    light.position = Vec3::new(
        (light.absolute_position.along_m - cam_m) as f32,
        light.absolute_position.height_m,
        light.absolute_position.lateral_m,
    );
}

fn fade_fraction(dt: f32, duration: f32) -> f32 {
    if !dt.is_finite() || dt <= 0.0 || duration <= 0.0 {
        0.0
    } else {
        (dt / duration).clamp(0.0, 1.0)
    }
}

fn influence_at(light: &GpuLight, point: Vec3) -> f32 {
    let offset = point - light.position;
    let distance = offset.length();
    let to_point = if distance > 1e-5 {
        offset / distance
    } else {
        light.direction
    };
    spot_attenuation(
        distance,
        light.direction.dot(to_point),
        light.range_m,
        light.cone_inner_cos.acos(),
        light.cone_outer_cos.acos(),
    )
}

fn distance_to_view(light: &GpuLight, view: LightView) -> f32 {
    let dx = if light.position.x < view.x_min {
        view.x_min - light.position.x
    } else if light.position.x > view.x_max {
        light.position.x - view.x_max
    } else {
        0.0
    };
    let dy = if light.position.y < view.y_min {
        view.y_min - light.position.y
    } else if light.position.y > view.y_max {
        light.position.y - view.y_max
    } else {
        0.0
    };
    let dz = if light.position.z < view.z_min {
        view.z_min - light.position.z
    } else if light.position.z > view.z_max {
        light.position.z - view.z_max
    } else {
        0.0
    };
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn view_point(view: LightView, light: Vec3, road_center: bool) -> Vec3 {
    Vec3::new(
        light.x.clamp(view.x_min, view.x_max),
        0.0_f32.clamp(view.y_min, view.y_max),
        if road_center {
            0.0_f32.clamp(view.z_min, view.z_max)
        } else {
            light.z.clamp(view.z_min, view.z_max)
        },
    )
}

fn candidate_for(source: LightSource, cam_m: f64, view: LightView) -> Option<LightCandidate> {
    let light = source_as_gpu(source, cam_m);
    if distance_to_view(&light, view) > light.range_m {
        return None;
    }

    // The road centre and the closest point in the visible world make the
    // priority represent potential visible influence, rather than raw camera
    // distance. The broad box includes nearby walls and trees as well as road.
    let road_influence = influence_at(&light, view_point(view, light.position, true));
    let world_influence = influence_at(&light, view_point(view, light.position, false));
    let elevated = Vec3::new(
        light.position.x.clamp(view.x_min, view.x_max),
        light.position.y.clamp(view.y_min, view.y_max),
        light.position.z.clamp(view.z_min, view.z_max),
    );
    let elevated_influence = influence_at(&light, elevated);
    let broad_influence = road_influence.max(world_influence).max(elevated_influence);
    let focus = influence_at(
        &light,
        Vec3::new(view.focus_x.clamp(view.x_min, view.x_max), 0.0, 0.0),
    );
    // Prioritise the visible foreground while retaining a smaller contribution
    // for lamps that influence the far end of the view. This is selection
    // priority, not a camera-distance fade in the physical lighting equation.
    let influence = (focus * 1.7 + broad_influence * 0.35).max(broad_influence * 0.35);
    let priority = light.intensity * influence;
    if priority <= 1e-5 {
        return None;
    }
    Some(LightCandidate { light, priority })
}

pub(crate) fn visible_candidates(cam_m: f64, view: LightView) -> Vec<LightCandidate> {
    let margin = 20.0_f64;
    let first_region = region_of(cam_m + view.x_min as f64 - margin) - 1;
    let last_region = region_of(cam_m + view.x_max as f64 + margin) + 1;
    let mut candidates = Vec::new();
    for region_index in first_region..=last_region {
        for object in world::region_description(region_index).objects {
            if let Some(source) = object.light
                && let Some(candidate) = candidate_for(source, cam_m, view)
            {
                candidates.push(candidate);
            }
        }
    }
    candidates.sort_by(|a, b| {
        b.priority
            .total_cmp(&a.priority)
            .then_with(|| a.light.source_id.cmp(&b.light.source_id))
    });
    candidates
}

fn stable_selection_ids(previous: &[u64], candidates: &[LightCandidate]) -> Vec<u64> {
    let mut ranked = candidates.to_vec();
    ranked.sort_by(|a, b| {
        b.priority
            .total_cmp(&a.priority)
            .then_with(|| a.light.source_id.cmp(&b.light.source_id))
    });
    let cutoff = ranked
        .get(MAX_LOCAL_LIGHTS.saturating_sub(1))
        .map(|candidate| candidate.priority)
        .unwrap_or(0.0);
    let keep_threshold = cutoff * (1.0 - SELECTION_HYSTERESIS);
    let mut ids = Vec::with_capacity(MAX_LOCAL_LIGHTS);

    for id in previous {
        let Some(candidate) = ranked
            .iter()
            .find(|candidate| candidate.light.source_id == *id)
        else {
            continue;
        };
        let rank = ranked
            .iter()
            .position(|ranked| ranked.light.source_id == *id)
            .unwrap_or(MAX_LOCAL_LIGHTS);
        if rank < MAX_LOCAL_LIGHTS || candidate.priority + 1e-5 >= keep_threshold {
            ids.push(*id);
        }
        if ids.len() == MAX_LOCAL_LIGHTS {
            break;
        }
    }
    for candidate in ranked {
        if ids.len() == MAX_LOCAL_LIGHTS {
            break;
        }
        if !ids.contains(&candidate.light.source_id) {
            ids.push(candidate.light.source_id);
        }
    }
    ids
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LightSlot {
    light: GpuLight,
    target: bool,
    weight: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct LightSelector {
    slots: [Option<LightSlot>; MAX_LOCAL_LIGHTS],
}

impl Default for LightSelector {
    fn default() -> Self {
        Self {
            slots: [None; MAX_LOCAL_LIGHTS],
        }
    }
}

impl LightSelector {
    pub(crate) fn update(
        &mut self,
        candidates: &[LightCandidate],
        cam_m: f64,
        dt: f32,
    ) -> LightSet {
        let fade = fade_fraction(dt, FADE_DURATION_SECONDS);
        let previous: Vec<u64> = self
            .slots
            .iter()
            .filter_map(|slot| {
                slot.filter(|slot| slot.target)
                    .map(|slot| slot.light.source_id)
            })
            .collect();
        let desired = stable_selection_ids(&previous, candidates);

        for slot in &mut self.slots {
            let Some(current) = slot.as_mut() else {
                continue;
            };
            if let Some(candidate) = candidates
                .iter()
                .find(|candidate| candidate.light.source_id == current.light.source_id)
            {
                current.light = candidate.light;
            } else {
                // The source is no longer in the visible candidate window,
                // but its direct contribution is still fading out. Rebuild
                // the local x from its absolute descriptor every update.
                localize(&mut current.light, cam_m);
            }
            current.target = desired.contains(&current.light.source_id);
            if current.target {
                current.weight = (current.weight + fade).min(1.0);
            } else {
                current.weight = (current.weight - fade).max(0.0);
            }
            if !current.target && current.weight <= f32::EPSILON {
                *slot = None;
            }
        }

        for id in desired {
            if self
                .slots
                .iter()
                .any(|slot| slot.is_some_and(|slot| slot.light.source_id == id))
            {
                continue;
            }
            let Some(candidate) = candidates
                .iter()
                .find(|candidate| candidate.light.source_id == id)
            else {
                continue;
            };
            let Some(slot) = self.slots.iter_mut().find(|slot| slot.is_none()) else {
                break;
            };
            *slot = Some(LightSlot {
                light: candidate.light,
                target: true,
                weight: fade,
            });
        }

        let mut result = LightSet::default();
        for (index, slot) in self.slots.iter().flatten().enumerate() {
            if index == MAX_LOCAL_LIGHTS {
                break;
            }
            let mut light = slot.light;
            light.weight = slot.weight;
            result.lights[index] = light;
        }
        result
    }

    #[cfg(test)]
    fn target_ids(&self) -> Vec<u64> {
        self.slots
            .iter()
            .filter_map(|slot| {
                slot.filter(|slot| slot.target)
                    .map(|slot| slot.light.source_id)
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ShadowLight {
    pub source_id: u64,
    pub light: GpuLight,
    pub lamp_slot: usize,
    pub weight: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ShadowSet {
    pub lights: [Option<ShadowLight>; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ShadowSlot {
    light: GpuLight,
    target: bool,
    weight: f32,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ShadowSelector {
    slots: [Option<ShadowSlot>; 2],
}

fn shadow_focus_priority(light: &GpuLight) -> f32 {
    // Shadow maps are a scarce resource. Rank by actual illumination at the
    // runner and road, rather than by hash/id or camera distance. The latter
    // is only a selection heuristic; the fragment shader keeps physical
    // distance attenuation in world-local coordinates.
    let runner = Vec3::new(0.0, 1.12, LANE);
    let road = Vec3::new(0.0, 0.08, 0.0);
    let influence = influence_at(light, runner).max(influence_at(light, road));
    light.intensity * influence
}

fn desired_shadow_ids(current: &LightSet, previous: &[u64]) -> Vec<u64> {
    let mut ranked: Vec<(usize, u64, f32)> = current
        .lights
        .iter()
        .enumerate()
        .filter(|(_, light)| light.source_id != 0 && light.weight > 0.0)
        .map(|(slot, light)| (slot, light.source_id, shadow_focus_priority(light)))
        .filter(|(_, _, priority)| *priority > 1e-5)
        .collect();
    ranked.sort_by(|a, b| b.2.total_cmp(&a.2).then_with(|| a.1.cmp(&b.1)));
    let cutoff = ranked.get(1).map(|entry| entry.2).unwrap_or(0.0);
    let keep_threshold = cutoff * (1.0 - SHADOW_SELECTION_HYSTERESIS);
    let mut ids = Vec::with_capacity(2);
    for id in previous {
        let Some((rank, entry)) = ranked.iter().enumerate().find(|(_, entry)| entry.1 == *id)
        else {
            continue;
        };
        if rank < 2 || entry.2 + 1e-5 >= keep_threshold {
            ids.push(*id);
        }
        if ids.len() == 2 {
            break;
        }
    }
    for (_, id, _) in ranked {
        if ids.len() == 2 {
            break;
        }
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

impl ShadowSelector {
    pub(crate) fn update(&mut self, current: &LightSet, cam_m: f64, dt: f32) -> ShadowSet {
        let fade = fade_fraction(dt, SHADOW_FADE_DURATION_SECONDS);
        let previous: Vec<u64> = self
            .slots
            .iter()
            .filter_map(|slot| {
                slot.filter(|slot| slot.target)
                    .map(|slot| slot.light.source_id)
            })
            .collect();
        let desired = desired_shadow_ids(current, &previous);

        for slot in &mut self.slots {
            let Some(shadow) = slot.as_mut() else {
                continue;
            };
            if let Some((_, light)) = current
                .lights
                .iter()
                .enumerate()
                .find(|(_, light)| light.source_id == shadow.light.source_id)
            {
                shadow.light = *light;
            } else {
                localize(&mut shadow.light, cam_m);
            }
            shadow.target = desired.contains(&shadow.light.source_id);
            if shadow.target {
                shadow.weight = (shadow.weight + fade).min(1.0);
            } else {
                shadow.weight = (shadow.weight - fade).max(0.0);
            }
            if !shadow.target && shadow.weight <= f32::EPSILON {
                *slot = None;
            }
        }

        for source_id in desired {
            if self
                .slots
                .iter()
                .any(|slot| slot.is_some_and(|slot| slot.light.source_id == source_id))
            {
                continue;
            }
            let Some((_, light)) = current
                .lights
                .iter()
                .enumerate()
                .find(|(_, light)| light.source_id == source_id)
            else {
                continue;
            };
            let Some(slot) = self.slots.iter_mut().find(|slot| slot.is_none()) else {
                break;
            };
            *slot = Some(ShadowSlot {
                light: *light,
                target: true,
                weight: fade,
            });
        }

        let mut result = ShadowSet::default();
        for (map_index, slot) in self.slots.iter().flatten().enumerate() {
            if map_index == 2 {
                break;
            }
            let Some((lamp_slot, light)) = current
                .lights
                .iter()
                .enumerate()
                .find(|(_, light)| light.source_id == slot.light.source_id)
            else {
                continue;
            };
            result.lights[map_index] = Some(ShadowLight {
                source_id: light.source_id,
                light: *light,
                lamp_slot,
                weight: slot.weight,
            });
        }
        result
    }
}

pub(crate) fn select_visible(
    cam_m: f64,
    dt: f32,
    selector: &mut LightSelector,
    shadow_selector: &mut ShadowSelector,
) -> (LightSet, ShadowSet) {
    let candidates = visible_candidates(cam_m, LightView::default());
    let lights = selector.update(&candidates, cam_m, dt);
    let shadows = shadow_selector.update(&lights, cam_m, dt);
    (lights, shadows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: u64, priority: f32) -> LightCandidate {
        LightCandidate {
            light: GpuLight {
                source_id: id,
                ..GpuLight::default()
            },
            priority,
        }
    }

    #[test]
    fn attenuation_has_bounded_range_and_smooth_penumbra() {
        let near = range_attenuation(0.0, 12.0);
        let mid = range_attenuation(5.0, 12.0);
        let edge = range_attenuation(12.0, 12.0);
        assert!((near - 1.0).abs() < 1e-6);
        assert!(mid > 0.0 && mid < near);
        assert_eq!(edge, 0.0);
        assert_eq!(cone_attenuation(1.0, 0.38, 0.86), 1.0);
        assert_eq!(cone_attenuation(-1.0, 0.38, 0.86), 0.0);
        let inner = 0.38_f32.cos();
        let outer = 0.86_f32.cos();
        let penumbra = cone_attenuation((inner + outer) * 0.5, 0.38, 0.86);
        assert!(penumbra > 0.0 && penumbra < 1.0);
        assert!(spot_attenuation(3.0, inner, 12.0, 0.38, 0.86) > 0.0);
    }

    #[test]
    fn selector_keeps_near_cutoff_source_until_a_clear_winner_exists() {
        let mut selector = LightSelector::default();
        let dt = 1.0 / 60.0;
        let first_candidates = [
            candidate(11, 10.0),
            candidate(22, 9.0),
            candidate(33, 8.0),
            candidate(44, 7.0),
            candidate(55, 6.0),
            candidate(66, 5.0),
            candidate(77, 4.0),
            candidate(88, 3.0),
        ];
        let first = selector.update(&first_candidates, 0.0, dt);
        assert_eq!(selector.target_ids(), vec![11, 22, 33, 44, 55, 66, 77, 88]);
        assert!(first.lights.iter().all(|light| light.weight <= 1.0));

        selector.update(
            &[
                candidate(11, 10.0),
                candidate(22, 9.0),
                candidate(33, 8.0),
                candidate(44, 7.0),
                candidate(55, 6.0),
                candidate(66, 5.0),
                candidate(77, 4.0),
                candidate(88, 3.0),
                candidate(99, 3.1),
            ],
            0.0,
            dt,
        );
        assert_eq!(selector.target_ids(), vec![11, 22, 33, 44, 55, 66, 77, 88]);

        selector.update(
            &[
                candidate(11, 10.0),
                candidate(22, 9.0),
                candidate(33, 8.0),
                candidate(44, 7.0),
                candidate(55, 6.0),
                candidate(66, 5.0),
                candidate(77, 4.0),
                candidate(88, 3.0),
                candidate(99, 12.0),
            ],
            0.0,
            dt,
        );
        // The outgoing slot is allowed to fade for one update instead of
        // popping out and making room for the challenger immediately.
        assert_eq!(selector.target_ids(), vec![11, 22, 33, 44, 55, 66, 77]);
        let next = selector.update(
            &[
                candidate(11, 10.0),
                candidate(22, 9.0),
                candidate(33, 8.0),
                candidate(44, 7.0),
                candidate(55, 6.0),
                candidate(66, 5.0),
                candidate(77, 4.0),
                candidate(99, 12.0),
            ],
            0.0,
            dt,
        );
        assert_eq!(selector.target_ids(), vec![11, 22, 33, 44, 55, 66, 77, 99]);
        assert!(next.lights.iter().any(|light| light.source_id == 99));
    }

    fn weight_after(fps: f32, total_seconds: f32, present: bool) -> f32 {
        let mut selector = LightSelector::default();
        let source = [candidate(7, 10.0)];
        if !present {
            selector.update(&source, 0.0, FADE_DURATION_SECONDS);
        }
        let mut remaining = total_seconds;
        while remaining > 0.0 {
            let dt = remaining.min(1.0 / fps);
            let lights = if present {
                selector.update(&source, 0.0, dt)
            } else {
                selector.update(&[], 0.0, dt)
            };
            remaining -= dt;
            if remaining <= 0.0 {
                return lights
                    .lights
                    .iter()
                    .find(|light| light.source_id == 7)
                    .map(|light| light.weight)
                    .unwrap_or(0.0);
            }
        }
        0.0
    }

    #[test]
    fn fade_duration_is_frame_rate_independent_at_30_60_and_144_fps() {
        let enter = [
            weight_after(30.0, 0.11, true),
            weight_after(60.0, 0.11, true),
            weight_after(144.0, 0.11, true),
        ];
        let leave = [
            weight_after(30.0, 0.11, false),
            weight_after(60.0, 0.11, false),
            weight_after(144.0, 0.11, false),
        ];
        for weights in [enter, leave] {
            assert!((weights[0] - weights[1]).abs() < 1e-5, "{weights:?}");
            assert!((weights[1] - weights[2]).abs() < 1e-5, "{weights:?}");
        }
    }

    #[test]
    fn retired_source_keeps_absolute_position_while_camera_moves() {
        let source = LightCandidate {
            light: GpuLight {
                source_id: 7,
                absolute_position: LightPosition {
                    along_m: 100.0,
                    lateral_m: -3.0,
                    height_m: 5.0,
                },
                ..GpuLight::default()
            },
            priority: 10.0,
        };
        let mut selector = LightSelector::default();
        selector.update(&[source], 80.0, FADE_DURATION_SECONDS);
        let retired = selector.update(&[], 90.0, 1.0 / 60.0);
        let lamp = retired
            .lights
            .iter()
            .find(|light| light.source_id == 7)
            .expect("retired source still fades");
        assert!((lamp.position.x - 10.0).abs() < 1e-5);
        assert_eq!(lamp.position.z, -3.0);
        assert_eq!(lamp.position.y, 5.0);
    }

    #[test]
    fn shadow_selection_follows_runner_influence_and_keeps_light_slots_aligned() {
        let runner = Vec3::new(0.0, 1.12, LANE);
        let far = GpuLight {
            source_id: 11,
            position: Vec3::new(40.0, 5.0, -2.5),
            direction: (runner - Vec3::new(40.0, 5.0, -2.5)).normalize(),
            range_m: 12.0,
            intensity: 50.0,
            cone_inner_cos: 0.95,
            cone_outer_cos: 0.65,
            weight: 1.0,
            ..GpuLight::default()
        };
        let near = GpuLight {
            source_id: 99,
            position: Vec3::new(0.0, 5.0, -2.5),
            direction: (runner - Vec3::new(0.0, 5.0, -2.5)).normalize(),
            range_m: 12.0,
            intensity: 5.5,
            cone_inner_cos: 0.95,
            cone_outer_cos: 0.65,
            weight: 1.0,
            ..GpuLight::default()
        };
        let mut lights = LightSet::default();
        lights.lights[3] = far;
        lights.lights[5] = near;
        let mut selector = ShadowSelector::default();
        let shadows = selector.update(&lights, 0.0, SHADOW_FADE_DURATION_SECONDS);
        let selected = shadows.lights[0].expect("runner light receives the first map");
        assert_eq!(selected.source_id, 99);
        assert_eq!(selected.lamp_slot, 5);
        assert!(shadows.lights[1].is_none());
    }
}
