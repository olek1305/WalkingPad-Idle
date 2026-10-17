//! Geometry shadow maps for the 3D scene.
//!
//! The maps deliberately store depth in the color attachment. macroquad
//! 0.4.16 creates a depth attachment for `render_target_ex`, but its public
//! `RenderPass.depth_texture` is `None`; sampling that field would therefore
//! either be impossible or silently bind the wrong resource. Each map below
//! uses the same opaque `Mesh` triangles as the color pass, a depth-testing
//! render target, and a high-precision RGBA depth encoder.

use macroquad::material::{Material, gl_use_default_material, gl_use_material};
use macroquad::prelude::*;

use crate::world::LAMP_OUTER_ANGLE_RAD;

use super::Quality;
use super::lighting::{GpuLight, ShadowLight, ShadowSet};
use super::material::{self, MaterialFlags};
use super::world::WorldCache;

pub(crate) const DIRECTIONAL_MAP_SIZE: u32 = 4096;
pub(crate) const LOCAL_MAP_SIZE: u32 = 1024;

const DIRECTIONAL_HALF_EXTENT_MIN: f32 = 64.0;
// 112: nocne elewacje (10-28°) rzucają cienie drogowych słupów na 35-60 m.
// Box 48-64 obcinał je na wyrazistej prostej (brzeg mapy), większy box +
// rozpuszczanie w shaderze (SUN_BORDER_MELT_UV) usuwa ostry styk.
const DIRECTIONAL_HALF_EXTENT_MAX: f32 = 112.0;
/// Fokus przy biegaczu zamiast 30 m przed nim: ciaśniejsza projekcja = więcej
/// texeli na metr = ostrzejsze cienie przy tym samym rozmiarze mapy.
const DIRECTIONAL_FOCUS_LOCAL_X: f32 = 16.0;
const DIRECTIONAL_CAMERA_DISTANCE: f32 = 260.0;
pub(crate) const DIRECTIONAL_NEAR: f32 = 1.0;
pub(crate) const DIRECTIONAL_FAR: f32 = 560.0;
// The local camera starts just beyond the 0.20 m emitter sphere. Otherwise
// the lamp's own bulb would occupy every texel in its cone and self-shadow
// the entire road.
pub(crate) const LOCAL_NEAR: f32 = 0.28;
pub(crate) const LOCAL_FAR: f32 = 12.75;

// Bias is expressed in the map's normalized depth range. These deliberately
// stay small: with the directional far plane below, a large bias would lift
// foot/trunk shadows visibly away from their grounded contact.
pub(crate) const SUN_BASE_BIAS: f32 = 0.00004;
// Podniesione z 1e-4: przy texelu ~2-4 cm i niskim księżycu stare bias-y
// zostawiały równoległe pasy acne na gruncie/ścianach; normal-offset w PCF
// + wyższy slope-bias razem wycinają kreski bez widocznego odpływania.
pub(crate) const SUN_SLOPE_BIAS: f32 = 0.00018;
pub(crate) const LAMP_BASE_BIAS: f32 = 0.00012;
pub(crate) const LAMP_SLOPE_BIAS: f32 = 0.00030;

/// The far value is encoded as all-one RGBA. `clear_background` also clears
/// the internally attached depth surface to 1.0 through miniquad's
/// `PassAction::clear_color`.
pub(crate) const FAR_DEPTH_COLOR: Color = Color::new(1.0, 1.0, 1.0, 1.0);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ShadowBindings {
    pub directional_matrix: Mat4,
    pub lamp_matrices: [Mat4; 2],
    pub directional_inverse_transpose: Mat4,
    pub lamp_inverse_transpose: [Mat4; 2],
    /// Indices into the eight light uniforms. Negative means that the map is
    /// not associated with any active source in this frame.
    pub lamp_slots: [f32; 2],
    /// Fade weights for the sources owning the two local shadow maps.
    pub lamp_weights: [f32; 2],
}

impl Default for ShadowBindings {
    fn default() -> Self {
        Self {
            directional_matrix: Mat4::IDENTITY,
            lamp_matrices: [Mat4::IDENTITY; 2],
            directional_inverse_transpose: Mat4::IDENTITY,
            lamp_inverse_transpose: [Mat4::IDENTITY; 2],
            lamp_slots: [-1.0; 2],
            lamp_weights: [0.0; 2],
        }
    }
}

pub(crate) struct ShadowRenderer {
    directional: RenderTarget,
    lamps: [RenderTarget; 2],
    material: Material,
    quality: Quality,
    directional_map_size: u32,
    local_map_size: u32,
    last_draws: usize,
}

struct ShadowPass<'a> {
    cache: &'a WorldCache,
    dynamic_meshes: &'a [Mesh],
    cam_m: f64,
    x_min: f32,
    x_max: f32,
    quality: Quality,
}

impl ShadowRenderer {
    pub(crate) fn new(quality: Quality) -> Self {
        let (directional_map_size, local_map_size) = match quality {
            Quality::High => (DIRECTIONAL_MAP_SIZE, LOCAL_MAP_SIZE),
            other => other.shadow_sizes(),
        };
        let directional = depth_render_target(directional_map_size);
        let lamps = [
            depth_render_target(local_map_size),
            depth_render_target(local_map_size),
        ];
        Self {
            directional,
            lamps,
            material: material::shadow_material(),
            quality,
            directional_map_size,
            local_map_size,
            last_draws: 0,
        }
    }

    /// Renders the directional map and the two stable local-source maps. The
    /// input meshes are opaque scene meshes, including the runner; translucent
    /// weather/beam effects are held in a separate vector and never submitted.
    pub(crate) fn render(
        &mut self,
        cache: &WorldCache,
        dynamic_meshes: &[Mesh],
        sun_dir: Vec3,
        cam_m: f64,
        shadow_set: &ShadowSet,
    ) -> ShadowBindings {
        self.last_draws = 0;
        let half_extent = directional_half_extent(sun_dir);
        let directional_camera = directional_camera(
            self.directional.clone(),
            cam_m,
            sun_dir,
            half_extent,
            self.directional_map_size,
        );
        let directional_matrix = directional_camera.matrix();
        let directional_pass = ShadowPass {
            cache,
            dynamic_meshes,
            cam_m,
            x_min: -140.0,
            x_max: 430.0,
            quality: self.quality,
        };
        draw_shadow_map(
            &self.material,
            &directional_camera,
            &directional_pass,
            &mut self.last_draws,
        );

        let mut bindings = ShadowBindings {
            directional_matrix,
            directional_inverse_transpose: directional_matrix.inverse().transpose(),
            ..ShadowBindings::default()
        };
        for (map_index, target) in self.lamps.iter().enumerate() {
            let selected = shadow_set.lights[map_index].as_ref();
            let camera = lamp_camera(target.clone(), selected.map(|shadow| &shadow.light));
            let matrix = camera.matrix();
            set_lamp_binding(&mut bindings, map_index, matrix, selected);
            bindings.lamp_inverse_transpose[map_index] = matrix.inverse().transpose();
            let (center_x, extent) = selected
                .map(|shadow| (shadow.light.position.x, shadow.light.range_m.max(0.0) + 3.0))
                .unwrap_or((0.0, 16.0));
            let lamp_pass = ShadowPass {
                cache,
                dynamic_meshes,
                cam_m,
                x_min: center_x - extent,
                x_max: center_x + extent,
                quality: self.quality,
            };
            draw_shadow_map(&self.material, &camera, &lamp_pass, &mut self.last_draws);
        }
        bindings
    }

    pub(crate) fn bind_material(&self, target: &Material, bindings: &ShadowBindings) {
        material::bind_shadow_maps(
            target,
            bindings,
            &self.directional.texture,
            &self.lamps[0].texture,
            &self.lamps[1].texture,
            self.directional_map_size,
            self.local_map_size,
        );
    }

    pub(crate) fn last_draws(&self) -> usize {
        self.last_draws
    }
}

fn set_lamp_binding(
    bindings: &mut ShadowBindings,
    map_index: usize,
    matrix: Mat4,
    shadow: Option<&ShadowLight>,
) {
    bindings.lamp_matrices[map_index] = matrix;
    if let Some(shadow) = shadow {
        bindings.lamp_slots[map_index] = shadow.lamp_slot as f32;
        bindings.lamp_weights[map_index] = shadow.weight.clamp(0.0, 1.0);
    }
}

fn depth_render_target(size: u32) -> RenderTarget {
    let target = render_target_ex(
        size,
        size,
        RenderTargetParams {
            depth: true,
            sample_count: 1,
        },
    );
    // PCF is explicit in the fragment shader. Nearest filtering keeps each
    // tap an actual texel instead of introducing an untracked linear blend.
    target.texture.set_filter(FilterMode::Nearest);
    target
}

fn draw_shadow_map(
    material: &Material,
    camera: &Camera3D,
    pass: &ShadowPass<'_>,
    draw_count: &mut usize,
) {
    gl_use_default_material();
    set_camera(camera);
    clear_background(FAR_DEPTH_COLOR);
    gl_use_material(material);
    pass.cache.draw_shadow(
        pass.cam_m,
        pass.x_min,
        pass.x_max,
        pass.quality,
        |mesh, offset| {
            if !mesh_has_shadow_caster(mesh) {
                return;
            }
            material::bind_world_offset(material, offset);
            draw_mesh(mesh);
            *draw_count += 1;
        },
    );
    for mesh in pass.dynamic_meshes {
        if !mesh_has_shadow_caster(mesh) {
            continue;
        }
        material::bind_world_offset(material, Vec3::ZERO);
        draw_mesh(mesh);
        *draw_count += 1;
    }
    gl_use_default_material();
}

fn mesh_has_shadow_caster(mesh: &Mesh) -> bool {
    mesh.vertices
        .iter()
        .any(|vertex| (vertex.normal.w as u8) & MaterialFlags::NO_SHADOW.bits() == 0)
}

/// Smoothly widens the directional coverage when the light source rides low,
/// but keeps a hard upper bound. Without this clamp a shallow sun would make
/// the selected map's texels too coarse near the runner.
///
/// The result is quantized to 4 m steps: `doba` advances every frame, so a
/// continuous half-extent would rescale the anchor snapping grid constantly
/// and the shadow edges would swim. Steps keep the grid stable between rare
/// transitions.
pub(crate) fn directional_half_extent(sun_dir: Vec3) -> f32 {
    let elevation = sun_dir.y.abs().clamp(0.0, 1.0);
    let shallow = 1.0 - smoothstep(0.08, 0.35, elevation);
    let raw = DIRECTIONAL_HALF_EXTENT_MIN
        + (DIRECTIONAL_HALF_EXTENT_MAX - DIRECTIONAL_HALF_EXTENT_MIN) * shallow;
    const STEP: f32 = 4.0;
    (raw / STEP).round() * STEP
}

fn directional_camera(
    render_target: RenderTarget,
    cam_m: f64,
    sun_dir: Vec3,
    half_extent: f32,
    map_size: u32,
) -> Camera3D {
    let direction = safe_direction(sun_dir);
    let target = stabilized_directional_target(cam_m, direction, half_extent, map_size);
    let position = directional_camera_position(target, direction);
    let (_, up) = directional_basis(direction);
    Camera3D {
        position,
        target,
        up,
        fovy: half_extent * 2.0,
        aspect: Some(1.0),
        projection: Projection::Orthographics,
        render_target: Some(render_target),
        viewport: None,
        z_near: DIRECTIONAL_NEAR,
        z_far: DIRECTIONAL_FAR,
    }
}

fn directional_camera_position(target: Vec3, direction: Vec3) -> Vec3 {
    target + safe_direction(direction) * DIRECTIONAL_CAMERA_DISTANCE
}

fn lamp_camera(render_target: RenderTarget, light: Option<&GpuLight>) -> Camera3D {
    let (position, direction, far) = light
        .map(|light| {
            (
                light.position,
                safe_direction(light.direction),
                light.range_m.clamp(LOCAL_FAR, 24.0),
            )
        })
        .unwrap_or((
            Vec3::new(0.0, 5.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            LOCAL_FAR,
        ));
    Camera3D {
        position,
        target: position + direction * (far * 0.56),
        up: camera_up(direction),
        fovy: (2.0 * LAMP_OUTER_ANGLE_RAD).clamp(0.60, 2.10),
        aspect: Some(1.0),
        projection: Projection::Perspective,
        render_target: Some(render_target),
        viewport: None,
        z_near: LOCAL_NEAR,
        z_far: far,
    }
}

fn safe_direction(direction: Vec3) -> Vec3 {
    let direction = direction.normalize_or_zero();
    if direction.length_squared() > 1e-8 {
        direction
    } else {
        Vec3::new(0.0, -1.0, 0.0)
    }
}

fn camera_up(direction: Vec3) -> Vec3 {
    if direction.y.abs() > 0.92 {
        Vec3::Z
    } else {
        Vec3::Y
    }
}

fn directional_basis(direction: Vec3) -> (Vec3, Vec3) {
    let direction = safe_direction(direction);
    let forward = -direction;
    let reference_up = camera_up(direction);
    let right = forward.cross(reference_up).normalize_or_zero();
    let up = right.cross(forward).normalize_or_zero();
    (right, up)
}

/// Snap the orthographic center in absolute light-camera texel coordinates.
/// `cam_m` is subtracted only after snapping, so f64 world positions remain
/// stable while the local scene origin follows the runner.
pub(crate) fn stabilized_directional_target(
    cam_m: f64,
    direction: Vec3,
    half_extent: f32,
    map_size: u32,
) -> Vec3 {
    let direction = safe_direction(direction);
    let (right, camera_up) = directional_basis(direction);
    let texel = projection_texel_size(half_extent, map_size);
    let focus = [cam_m + DIRECTIONAL_FOCUS_LOCAL_X as f64, 0.0_f64, 0.0_f64];
    let dot = |basis: Vec3| -> f64 {
        basis.x as f64 * focus[0] + basis.y as f64 * focus[1] + basis.z as f64 * focus[2]
    };
    let u = dot(right);
    let v = dot(camera_up);
    let snapped_u = (u / texel).round() * texel;
    let snapped_v = (v / texel).round() * texel;
    let du = snapped_u - u;
    let dv = snapped_v - v;
    let world_center = [
        focus[0] + right.x as f64 * du + camera_up.x as f64 * dv,
        focus[1] + right.y as f64 * du + camera_up.y as f64 * dv,
        focus[2] + right.z as f64 * du + camera_up.z as f64 * dv,
    ];
    Vec3::new(
        (world_center[0] - cam_m) as f32,
        world_center[1] as f32,
        world_center[2] as f32,
    )
}

fn projection_texel_size(half_extent: f32, map_size: u32) -> f64 {
    (half_extent as f64 * 2.0) / map_size.max(1) as f64
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ShadowProjection {
    pub uv: Vec2,
    pub depth: f32,
}

/// Converts GL NDC `[-1, 1]` to texture UV/depth `[0, 1]`. The color target
/// is sampled without a vertical flip: miniquad's render-pass texture and the
/// GLSL `gl_FragCoord` convention use the same bottom-left attachment space.
/// A final backend capture must still verify this convention on the active
/// driver before art tuning.
#[cfg(test)]
pub(crate) fn project_shadow(matrix: Mat4, point: Vec3) -> Option<ShadowProjection> {
    let clip = matrix * point.extend(1.0);
    if !clip.x.is_finite()
        || !clip.y.is_finite()
        || !clip.z.is_finite()
        || !clip.w.is_finite()
        || clip.w <= 0.0
    {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    let uv = ndc.truncate() * 0.5 + Vec2::splat(0.5);
    let depth = ndc.z * 0.5 + 0.5;
    if !uv.x.is_finite()
        || !uv.y.is_finite()
        || !depth.is_finite()
        || uv.x < 0.0
        || uv.x > 1.0
        || uv.y < 0.0
        || uv.y > 1.0
        || depth <= 0.0
        || depth >= 1.0
    {
        return None;
    }
    Some(ShadowProjection { uv, depth })
}

#[cfg(test)]
pub(crate) fn receiver_plane_gradient(matrix: Mat4, normal: Vec3, point: Vec3) -> Option<Vec2> {
    let normal = normal.normalize_or_zero();
    if normal.length_squared() <= 1.0e-8 {
        return None;
    }
    let clip_plane = matrix.inverse().transpose() * normal.extend(-normal.dot(point));
    if !clip_plane.z.is_finite() || clip_plane.z.abs() < 1.0e-6 {
        return None;
    }
    Some(-clip_plane.truncate().truncate() / clip_plane.z)
}

#[cfg(test)]
pub(crate) fn nearest_texel_center(uv: Vec2, texel: Vec2) -> Vec2 {
    ((uv / texel).floor() + Vec2::splat(0.5)) * texel
}

/// CPU mirror of the GLSL `SUN_BORDER_MELT_UV`: 1.0 keeps the shadow fully
/// opaque, 0.0 hands the sample back as fully lit — the shader multiplies
/// `mix(1.0, vis, melt)`, so long night shadows dissolve instead of ending
/// as a razor line on the directional map's coverage border.
#[cfg(test)]
pub(crate) const SUN_BORDER_MELT_UV: f32 = 0.18;

#[cfg(test)]
pub(crate) fn shadow_border_melt(uv: Vec2) -> f32 {
    let border_gap = uv.min(Vec2::splat(1.0) - uv);
    let t = (border_gap.min_element() / SUN_BORDER_MELT_UV).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
pub(crate) fn receiver_depth_at_texel(
    matrix: Mat4,
    normal: Vec3,
    point: Vec3,
    sample_uv: Vec2,
    texel: Vec2,
) -> Option<f32> {
    let projected = project_shadow(matrix, point)?;
    let gradient = receiver_plane_gradient(matrix, normal, point)?;
    let snapped = nearest_texel_center(sample_uv, texel);
    Some(projected.depth + gradient.dot(snapped - projected.uv))
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_border_melt_mirrors_the_fragment_shader_constant_and_curve() {
        let needle = format!("SUN_BORDER_MELT_UV = {:.2};", SUN_BORDER_MELT_UV);
        assert!(
            material::MATERIAL_FRAGMENT_SHADER.contains(&needle),
            "fragment shader must carry the same melt band: {needle}"
        );
        assert!(
            material::MATERIAL_FRAGMENT_SHADER.contains("mix(1.0, visible / 9.0, melt)"),
            "fragment shader must blend the sun shadow toward light at the map border"
        );

        // The center of the map keeps the shadow at full depth.
        assert!(
            (shadow_border_melt(Vec2::splat(0.5)) - 1.0).abs() < 1.0e-6,
            "map center must keep shadows fully opaque"
        );
        // The melt band occupies only the outermost 18%: sampling just inside
        // the band's inner edge is still fully opaque.
        assert!(
            (shadow_border_melt(Vec2::new(SUN_BORDER_MELT_UV, 0.5)) - 1.0).abs() < 1.0e-3,
            "samples at the melt band inner edge must stay opaque"
        );
        // Exactly on the border the shadow yields to full light — the same
        // value the out-of-map path returns, so no razor seam can remain.
        assert!(
            shadow_border_melt(Vec2::splat(0.0)).abs() < 1.0e-6,
            "map border must return fully lit"
        );
        for uv in [
            Vec2::new(0.5, 0.0),
            Vec2::new(1.0, 0.5),
            Vec2::new(0.05, 0.05),
            Vec2::new(0.091, 0.091),
        ] {
            let melt = shadow_border_melt(uv);
            assert!(
                (0.0..=1.0).contains(&melt),
                "melt {melt} out of range at {uv:?}"
            );
        }
    }

    #[test]
    fn directional_center_snaps_to_absolute_texels() {
        let direction = Vec3::new(0.4, 0.5, 0.2).normalize();
        let a = stabilized_directional_target(10_000.0, direction, 180.0, 2048);
        let b = stabilized_directional_target(10_000.0 + 0.0001, direction, 180.0, 2048);
        // A sub-texel camera move changes the local origin but not the
        // snapped absolute light-space center.
        assert!((a.z - b.z).abs() < 1e-2, "{a:?} vs {b:?}");
        assert!((a.y - b.y).abs() < 1e-2, "{a:?} vs {b:?}");
    }

    #[test]
    fn quality_map_sizes_change_projection_stabilization_texels() {
        let half_extent = 56.0;
        let low = projection_texel_size(half_extent, Quality::Low.shadow_sizes().0);
        let medium = projection_texel_size(half_extent, Quality::Medium.shadow_sizes().0);
        let high = projection_texel_size(half_extent, Quality::High.shadow_sizes().0);
        assert!(low > medium && medium > high);
    }

    #[test]
    fn directional_camera_looks_from_source_side_and_orders_caster_before_receiver() {
        let target = Vec3::ZERO;
        let direction = Vec3::new(0.35, 0.75, -0.15).normalize();
        let position = directional_camera_position(target, direction);
        assert!(position.y > target.y);

        let matrix = Mat4::orthographic_rh_gl(
            -DIRECTIONAL_HALF_EXTENT_MAX,
            DIRECTIONAL_HALF_EXTENT_MAX,
            -DIRECTIONAL_HALF_EXTENT_MAX,
            DIRECTIONAL_HALF_EXTENT_MAX,
            DIRECTIONAL_NEAR,
            DIRECTIONAL_FAR,
        ) * Mat4::look_at_rh(position, target, directional_basis(direction).1);
        let receiver = target;
        let caster = receiver + direction * 2.0;
        let receiver_depth = project_shadow(matrix, receiver).unwrap().depth;
        let caster_depth = project_shadow(matrix, caster).unwrap().depth;
        assert!(
            caster_depth < receiver_depth,
            "source-side caster must be nearer than its ground receiver: {caster_depth} >= {receiver_depth}"
        );
    }

    #[test]
    fn projected_ndc_maps_to_uv_and_depth_and_rejects_far_points() {
        let matrix = Mat4::perspective_rh_gl(90.0_f32.to_radians(), 1.0, 0.1, 10.0)
            * Mat4::look_at_rh(Vec3::ZERO, -Vec3::Z, Vec3::Y);
        let center = project_shadow(matrix, Vec3::new(0.0, 0.0, -1.0)).unwrap();
        assert!((center.uv.x - 0.5).abs() < 1e-6);
        assert!((center.uv.y - 0.5).abs() < 1e-6);
        assert!(center.depth > 0.0 && center.depth < 1.0);
        assert!(project_shadow(matrix, Vec3::new(0.0, 0.0, -12.0)).is_none());
    }

    #[test]
    fn projected_point_outside_lamp_cone_is_outside_its_map_frustum() {
        let matrix = Mat4::perspective_rh_gl(2.0 * LAMP_OUTER_ANGLE_RAD, 1.0, 0.1, 12.0)
            * Mat4::look_at_rh(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), Vec3::Y);
        // This point is well outside the lamp's vertical cone and therefore
        // must not turn an out-of-frustum sample into a dark shadow.
        assert!(project_shadow(matrix, Vec3::new(8.0, 0.0, -2.0)).is_none());
    }

    #[test]
    fn shadow_bindings_preserve_sampler_slots_and_visibility_weights() {
        let shadows = ShadowSet {
            lights: [
                Some(ShadowLight {
                    source_id: 41,
                    light: GpuLight::default(),
                    lamp_slot: 6,
                    weight: 0.25,
                }),
                Some(ShadowLight {
                    source_id: 73,
                    light: GpuLight::default(),
                    lamp_slot: 2,
                    weight: 0.75,
                }),
            ],
        };
        let mut bindings = ShadowBindings::default();
        set_lamp_binding(
            &mut bindings,
            0,
            Mat4::from_translation(Vec3::X),
            shadows.lights[0].as_ref(),
        );
        set_lamp_binding(
            &mut bindings,
            1,
            Mat4::from_translation(Vec3::Y),
            shadows.lights[1].as_ref(),
        );

        assert_eq!(bindings.lamp_slots, [6.0, 2.0]);
        assert_eq!(bindings.lamp_weights, [0.25, 0.75]);
        assert_eq!(bindings.lamp_matrices[0], Mat4::from_translation(Vec3::X));
        assert_eq!(bindings.lamp_matrices[1], Mat4::from_translation(Vec3::Y));
    }

    #[test]
    fn sloped_directional_receiver_matches_actual_nearest_texel_centers() {
        let direction = Vec3::new(0.42, 0.48, -0.22).normalize();
        let target = Vec3::ZERO;
        let position = directional_camera_position(target, direction);
        let half_extent = DIRECTIONAL_HALF_EXTENT_MAX;
        let matrix = Mat4::orthographic_rh_gl(
            -half_extent,
            half_extent,
            -half_extent,
            half_extent,
            DIRECTIONAL_NEAR,
            DIRECTIONAL_FAR,
        ) * Mat4::look_at_rh(position, target, directional_basis(direction).1);
        let normal = Vec3::new(0.31, 0.93, -0.16).normalize();
        let receiver = Vec3::new(0.0, 0.0, 0.0);
        let projected = project_shadow(matrix, receiver).unwrap();
        let texel = Vec2::splat(1.0 / DIRECTIONAL_MAP_SIZE as f32);
        for offset in [Vec2::ZERO, Vec2::new(texel.x, 0.0), Vec2::new(0.0, texel.y)] {
            let sample = nearest_texel_center(projected.uv + offset, texel);
            let reference =
                receiver_depth_at_texel(matrix, normal, receiver, projected.uv + offset, texel)
                    .unwrap();
            let own_map_depth = projected.depth
                + receiver_plane_gradient(matrix, normal, receiver)
                    .unwrap()
                    .dot(sample - projected.uv);
            assert!((reference - own_map_depth).abs() < 1.0e-6);
        }

        let raised_caster = receiver + direction * 2.0;
        let caster_depth = project_shadow(matrix, raised_caster).unwrap().depth;
        let receiver_depth =
            receiver_depth_at_texel(matrix, normal, receiver, projected.uv, texel).unwrap();
        assert!(receiver_depth - SUN_BASE_BIAS > caster_depth);
    }

    #[test]
    fn perspective_lamp_receiver_plane_and_raised_caster_are_separate() {
        let source = Vec3::new(0.0, 5.2, 0.0);
        let receiver = Vec3::new(0.7, 0.0, -4.0);
        let direction = (receiver - source).normalize();
        let matrix =
            Mat4::perspective_rh_gl(2.0 * LAMP_OUTER_ANGLE_RAD, 1.0, LOCAL_NEAR, LOCAL_FAR)
                * Mat4::look_at_rh(source, source + direction * 8.0, Vec3::Y);
        let normal = Vec3::Y;
        let projected = project_shadow(matrix, receiver).unwrap();
        let texel = Vec2::splat(1.0 / LOCAL_MAP_SIZE as f32);
        let sample = nearest_texel_center(projected.uv + Vec2::new(texel.x, 0.0), texel);
        let gradient = receiver_plane_gradient(matrix, normal, receiver).unwrap();
        let reference = receiver_depth_at_texel(
            matrix,
            normal,
            receiver,
            projected.uv + Vec2::new(texel.x, 0.0),
            texel,
        )
        .unwrap();
        let own_map_depth = projected.depth + gradient.dot(sample - projected.uv);
        assert!((reference - own_map_depth).abs() < 1.0e-6);

        let caster = receiver - direction * 1.0;
        let caster_depth = project_shadow(matrix, caster).unwrap().depth;
        assert!(reference - LAMP_BASE_BIAS > caster_depth);
    }
}
