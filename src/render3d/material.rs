//! Kolory powierzchni i wspólny materiał GPU dla świata 3D.
//!
//! `Vertex.color` jest bazowym kolorem sRGB zapisanym jako `Byte4`; nie wolno
//! wpisywać tu jasności doby, normalnego oświetlenia ani mgły. `texcoord.x`
//! niesie chropowatość, `texcoord.y` emisję, a `normal.w` bity
//! [`MaterialFlags`]. Normalne powierzchni są w `normal.xyz`. Dzięki temu ten
//! sam opis wierzchołka może później zasilać pass oświetlenia i mapę głębi.

use std::sync::OnceLock;

use macroquad::material::{Material, MaterialParams, load_material};
use macroquad::prelude::*;
use macroquad::window::miniquad::{
    BlendFactor, BlendState, BlendValue, Comparison, Equation, PipelineParams, ShaderSource,
    UniformDesc, UniformType,
};

use super::lighting::{LightSet, MAX_LOCAL_LIGHTS};
use super::shadow::ShadowBindings;

/// Odcień, nasycenie i jasność w zakresach `h = 0..1`, `s/l = 0..1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsl {
    pub h: f32,
    pub s: f32,
    pub l: f32,
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    t = t.rem_euclid(1.0);
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

/// Zamienia HSL na kolor sRGB (wartości kanałów pozostają w sRGB do czasu
/// wejścia do shadera).
pub fn hsl_to_srgb(hsl: Hsl) -> Color {
    let h = hsl.h.rem_euclid(1.0);
    let s = hsl.s.clamp(0.0, 1.0);
    let l = hsl.l.clamp(0.0, 1.0);
    if s <= f32::EPSILON {
        return Color::new(l, l, l, 1.0);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    Color::new(
        hue_to_rgb(p, q, h + 1.0 / 3.0),
        hue_to_rgb(p, q, h),
        hue_to_rgb(p, q, h - 1.0 / 3.0),
        1.0,
    )
}

/// Zamienia kolor sRGB na HSL. Alfa jest celowo pomijana, bo nie jest częścią
/// palety powierzchni.
pub fn srgb_to_hsl(color: Color) -> Hsl {
    let r = color.r.clamp(0.0, 1.0);
    let g = color.g.clamp(0.0, 1.0);
    let b = color.b.clamp(0.0, 1.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) * 0.5;
    let delta = max - min;
    if delta <= f32::EPSILON {
        return Hsl { h: 0.0, s: 0.0, l };
    }
    let s = delta / (1.0 - (2.0 * l - 1.0).abs());
    let mut h = if (max - r).abs() <= f32::EPSILON {
        ((g - b) / delta).rem_euclid(6.0)
    } else if (max - g).abs() <= f32::EPSILON {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    h = (h / 6.0).rem_euclid(1.0);
    Hsl { h, s, l }
}

/// Dokładna konwersja pojedynczego kanału sRGB do liniowego RGB.
pub fn srgb_to_linear_channel(channel: f32) -> f32 {
    let c = channel.clamp(0.0, 1.0);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Dokładna konwersja pojedynczego kanału liniowego RGB do sRGB.
pub fn linear_to_srgb_channel(channel: f32) -> f32 {
    let c = channel.max(0.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

pub fn srgb_to_linear(color: Color) -> Vec3 {
    Vec3::new(
        srgb_to_linear_channel(color.r),
        srgb_to_linear_channel(color.g),
        srgb_to_linear_channel(color.b),
    )
}

pub fn linear_to_srgb(linear: Vec3, alpha: f32) -> Color {
    Color::new(
        linear_to_srgb_channel(linear.x),
        linear_to_srgb_channel(linear.y),
        linear_to_srgb_channel(linear.z),
        alpha,
    )
}

/// Skaluje kolor w liniowym RGB jednym współczynnikiem. Stosunek kanałów, a
/// więc także odcień, zostaje zachowany dokładniej niż przy skalowaniu sRGB.
pub fn scale_linear_srgb(color: Color, factor: f32) -> Color {
    linear_to_srgb(srgb_to_linear(color) * factor.max(0.0), color.a)
}

/// Wygodny test/łącznik dla korekt artystycznych opartych o HSL.
pub fn scale_hsl_lightness(color: Color, factor: f32) -> Color {
    let mut hsl = srgb_to_hsl(color);
    hsl.l = (hsl.l * factor.max(0.0)).clamp(0.0, 1.0);
    let mut out = hsl_to_srgb(hsl);
    out.a = color.a;
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceKind {
    Asphalt,
    Soil,
    Foliage,
    Wood,
    Metal,
    Skin,
    Cloth,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfacePreset {
    pub albedo: Color,
    pub roughness: f32,
    pub emission: f32,
    /// Współczynnik subtelnego przyciemnienia bazowych kolorów obiektu.
    pub albedo_factor: f32,
}

fn base_surface(kind: SurfaceKind) -> (Hsl, f32, f32) {
    match kind {
        SurfaceKind::Asphalt => (
            Hsl {
                h: 0.64,
                s: 0.20,
                l: 0.19,
            },
            0.88,
            0.0,
        ),
        SurfaceKind::Soil => (
            Hsl {
                h: 0.09,
                s: 0.42,
                l: 0.31,
            },
            0.96,
            0.0,
        ),
        SurfaceKind::Foliage => (
            Hsl {
                h: 0.34,
                s: 0.52,
                l: 0.27,
            },
            0.82,
            0.0,
        ),
        SurfaceKind::Wood => (
            Hsl {
                h: 0.07,
                s: 0.48,
                l: 0.34,
            },
            0.72,
            0.0,
        ),
        SurfaceKind::Metal => (
            Hsl {
                h: 0.60,
                s: 0.16,
                l: 0.43,
            },
            0.27,
            0.0,
        ),
        SurfaceKind::Skin => (
            Hsl {
                h: 0.07,
                s: 0.43,
                l: 0.68,
            },
            0.58,
            0.0,
        ),
        SurfaceKind::Cloth => (
            Hsl {
                h: 0.78,
                s: 0.48,
                l: 0.47,
            },
            0.91,
            0.0,
        ),
    }
}

/// Powierzchnia bazowa z płynną korektą mokrości. Mokrość lekko przyciemnia
/// albedo z pomocą odcienio-zachowawczej korekty HSL; główną zmianą jest
/// ciągłe obniżenie chropowatości.
pub fn surface_preset(kind: SurfaceKind, wetness: f32) -> SurfacePreset {
    let (hsl, roughness, emission) = base_surface(kind);
    let wet = wetness.clamp(0.0, 1.0);
    let mut albedo = hsl_to_srgb(hsl);
    albedo = scale_hsl_lightness(albedo, 1.0 - 0.08 * wet);
    SurfacePreset {
        albedo,
        roughness: (roughness * (1.0 - 0.62 * wet)).max(0.04),
        emission,
        albedo_factor: 1.0 - 0.08 * wet,
    }
}

/// Bity w `Vertex.normal.w`. Wartość jest stała dla całego prymitywu, więc
/// może być interpolowana przez GLSL bez artefaktów.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialFlags(u8);

impl MaterialFlags {
    pub const NONE: Self = Self(0);
    pub const UNLIT: Self = Self(1 << 0);
    pub const EMISSIVE: Self = Self(1 << 1);
    pub const NO_FOG: Self = Self(1 << 2);
    pub const NO_SHADOW: Self = Self(1 << 3);
    /// The fragment shader lowers roughness only while the shared wetness
    /// uniform is active.  The mesh therefore remains reusable across weather
    /// transitions.
    pub const WET_PATCH: Self = Self(1 << 4);
    /// Windows and lamp heads keep their stable geometry while their night
    /// colour/emission follows the GPU night uniform.
    pub const NIGHT_GLOW: Self = Self(1 << 5);
    /// LED panels keep their neutral albedo and add only night-time emission.
    pub const NIGHT_LED: Self = Self(1 << 6);

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialSpec {
    pub roughness: f32,
    pub emission: f32,
    pub flags: MaterialFlags,
    pub albedo_factor: f32,
}

impl MaterialSpec {
    pub const fn new(roughness: f32, emission: f32, flags: MaterialFlags) -> Self {
        Self {
            roughness,
            emission,
            flags,
            albedo_factor: 1.0,
        }
    }

    pub fn from_surface(surface: SurfacePreset) -> Self {
        let mut spec = Self::new(surface.roughness, surface.emission, MaterialFlags::NONE);
        spec.albedo_factor = surface.albedo_factor;
        spec
    }

    pub const fn unlit() -> Self {
        Self::new(
            1.0,
            0.0,
            MaterialFlags::UNLIT
                .with(MaterialFlags::NO_FOG)
                .with(MaterialFlags::NO_SHADOW),
        )
    }

    pub const fn emissive(roughness: f32, emission: f32) -> Self {
        Self::new(
            roughness,
            emission,
            MaterialFlags::UNLIT
                .with(MaterialFlags::EMISSIVE)
                .with(MaterialFlags::NO_SHADOW),
        )
    }
}

/// Tworzy wierzchołek z konwencją używaną przez oba materiały świata.
pub fn vertex_data(position: Vec3, normal: Vec3, color: Color, material: MaterialSpec) -> Vertex {
    let normal = if normal.length_squared() > 1e-8 {
        normal.normalize()
    } else {
        Vec3::Y
    };
    let color = if material.albedo_factor < 0.9999 {
        scale_hsl_lightness(color, material.albedo_factor)
    } else {
        color
    };
    Vertex {
        position,
        uv: Vec2::new(
            material.roughness.clamp(0.0, 1.0),
            material.emission.max(0.0),
        ),
        color: color.into(),
        normal: Vec4::new(normal.x, normal.y, normal.z, material.flags.bits() as f32),
    }
}

fn shader_uniforms() -> Vec<UniformDesc> {
    let mut uniforms = vec![
        UniformDesc::new("u_sun", UniformType::Float4),
        UniformDesc::new("u_ambient", UniformType::Float4),
        UniformDesc::new("u_fog_color", UniformType::Float4),
        UniformDesc::new("u_fog_params", UniformType::Float4),
        UniformDesc::new("u_camera", UniformType::Float4),
        UniformDesc::new("u_lamp_output", UniformType::Float1),
        UniformDesc::new("u_night", UniformType::Float1),
        UniformDesc::new("u_wetness", UniformType::Float1),
        UniformDesc::new("u_world_offset", UniformType::Float4),
        UniformDesc::new("u_shadow_sun_matrix", UniformType::Mat4),
        UniformDesc::new("u_shadow_lamp0_matrix", UniformType::Mat4),
        UniformDesc::new("u_shadow_lamp1_matrix", UniformType::Mat4),
        UniformDesc::new("u_shadow_sun_inverse_transpose", UniformType::Mat4),
        UniformDesc::new("u_shadow_lamp0_inverse_transpose", UniformType::Mat4),
        UniformDesc::new("u_shadow_lamp1_inverse_transpose", UniformType::Mat4),
        UniformDesc::new("u_shadow_lamp_slot", UniformType::Float4),
        UniformDesc::new("u_shadow_lamp_weight", UniformType::Float4),
        UniformDesc::new("u_shadow_texel", UniformType::Float4),
        UniformDesc::new("u_shadow_bias", UniformType::Float4),
    ];
    for name in [
        "u_lamp_position",
        "u_lamp_direction",
        "u_lamp_color",
        "u_lamp_params",
    ] {
        uniforms.push(UniformDesc::array(
            UniformDesc::new(name, UniformType::Float4),
            MAX_LOCAL_LIGHTS,
        ));
    }
    uniforms
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PipelineConvention {
    pub depth_test: Comparison,
    pub depth_write: bool,
    pub alpha_blend: bool,
    /// Adresowanie addytywne: wkład światła DODAJE się do tła (`dst * One`),
    /// więc nachodzące na siebie stożki lamp dają jaśniejszy przeciętny obszar
    /// (fizyczne sumowanie rozproszenia we mgle), a nie miks alpha.
    pub additive: bool,
}

pub(crate) const fn opaque_pipeline_convention() -> PipelineConvention {
    PipelineConvention {
        depth_test: Comparison::LessOrEqual,
        depth_write: true,
        alpha_blend: false,
        additive: false,
    }
}

pub(crate) const fn effect_pipeline_convention() -> PipelineConvention {
    PipelineConvention {
        depth_test: Comparison::LessOrEqual,
        depth_write: false,
        alpha_blend: true,
        additive: false,
    }
}

/// Kreski lamp: addytywne jak efekty, ale wkład się sumuje z tłem.
pub(crate) const fn beam_pipeline_convention() -> PipelineConvention {
    PipelineConvention {
        depth_test: Comparison::LessOrEqual,
        depth_write: false,
        alpha_blend: true,
        additive: true,
    }
}

fn make_material(convention: PipelineConvention) -> Material {
    let color_blend = if convention.additive {
        // `src * srcAlpha + dst * One`: nakładanie źródeł rozświetla, a
        // kolejność nakładania nie ma znaczenia (suma jest przemienna).
        Some(BlendState::new(
            Equation::Add,
            BlendFactor::Value(BlendValue::SourceAlpha),
            BlendFactor::One,
        ))
    } else if convention.alpha_blend {
        Some(BlendState::new(
            Equation::Add,
            BlendFactor::Value(BlendValue::SourceAlpha),
            BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
        ))
    } else {
        None
    };
    let pipeline_params = PipelineParams {
        color_blend,
        depth_test: convention.depth_test,
        depth_write: convention.depth_write,
        ..Default::default()
    };
    load_material(
        ShaderSource::Glsl {
            vertex: MATERIAL_VERTEX_SHADER,
            fragment: MATERIAL_FRAGMENT_SHADER,
        },
        MaterialParams {
            pipeline_params,
            uniforms: shader_uniforms(),
            textures: vec![
                "u_shadow_sun".to_string(),
                "u_shadow_lamp0".to_string(),
                "u_shadow_lamp1".to_string(),
            ],
        },
    )
    .expect("materiał świata 3D")
}

pub(crate) fn shadow_material() -> Material {
    load_material(
        ShaderSource::Glsl {
            vertex: SHADOW_VERTEX_SHADER,
            fragment: SHADOW_FRAGMENT_SHADER,
        },
        MaterialParams {
            pipeline_params: PipelineParams {
                depth_test: Comparison::LessOrEqual,
                depth_write: true,
                ..Default::default()
            },
            uniforms: vec![UniformDesc::new("u_world_offset", UniformType::Float4)],
            ..Default::default()
        },
    )
    .expect("materiał mapy głębi świata 3D")
}

pub fn surface_material() -> &'static Material {
    static MATERIAL: OnceLock<Material> = OnceLock::new();
    MATERIAL.get_or_init(|| make_material(opaque_pipeline_convention()))
}

pub fn effect_material() -> &'static Material {
    static MATERIAL: OnceLock<Material> = OnceLock::new();
    MATERIAL.get_or_init(|| make_material(effect_pipeline_convention()))
}

/// Materiał kres lamp (imbus/fog beam): addytywny blending.
pub fn beam_material() -> &'static Material {
    static MATERIAL: OnceLock<Material> = OnceLock::new();
    MATERIAL.get_or_init(|| make_material(beam_pipeline_convention()))
}

/// Ustawia wszystkie uniformy sceny przed rysowaniem meshów. Kolory uniformów
/// są liniowe, bo shader wykonuje sRGB decode tylko dla `Vertex.color`.
pub struct LightingFrame<'a> {
    pub camera: Vec3,
    pub sun_dir: Vec3,
    pub sun_strength: f32,
    pub ambient_srgb: Color,
    pub ambient_strength: f32,
    pub fog_srgb: Color,
    pub fog_start: f32,
    pub fog_end: f32,
    pub night: f32,
    pub wetness: f32,
    /// Harmonogram latarni (0..1): „przełącznik" ON nocą, OFF od rana.
    /// Ma zastąpić płynny smoothstep z `night` w `u_lamp_output` oraz mgle
    /// okien/lamp-LED w shaderze.
    pub lamp: f32,
    pub lights: &'a LightSet,
}

pub fn bind_lighting(material: &Material, frame: &LightingFrame<'_>) {
    let sun = frame.sun_dir.normalize_or_zero();
    let ambient = srgb_to_linear(frame.ambient_srgb);
    let fog = srgb_to_linear(frame.fog_srgb);
    material.set_uniform(
        "u_sun",
        vec4(sun.x, sun.y, sun.z, frame.sun_strength.max(0.0)),
    );
    material.set_uniform(
        "u_ambient",
        vec4(ambient.x, ambient.y, ambient.z, frame.ambient_strength),
    );
    material.set_uniform("u_fog_color", vec4(fog.x, fog.y, fog.z, 1.0));
    material.set_uniform(
        "u_fog_params",
        vec4(frame.fog_start, frame.fog_end, 1.4, 0.0),
    );
    material.set_uniform(
        "u_camera",
        vec4(frame.camera.x, frame.camera.y, frame.camera.z, 1.0),
    );
    material.set_uniform("u_lamp_output", frame.lamp.clamp(0.0, 1.0));
    material.set_uniform("u_night", frame.night.clamp(0.0, 1.0));
    material.set_uniform("u_wetness", frame.wetness.clamp(0.0, 1.0));

    let mut positions = [Vec4::ZERO; MAX_LOCAL_LIGHTS];
    let mut directions = [Vec4::ZERO; MAX_LOCAL_LIGHTS];
    let mut colors = [Vec4::ZERO; MAX_LOCAL_LIGHTS];
    let mut params = [Vec4::ZERO; MAX_LOCAL_LIGHTS];
    for (index, light) in frame.lights.lights.iter().enumerate() {
        let color = srgb_to_linear(Color::new(
            light.color[0],
            light.color[1],
            light.color[2],
            1.0,
        ));
        positions[index] = vec4(
            light.position.x,
            light.position.y,
            light.position.z,
            light.weight.clamp(0.0, 1.0),
        );
        directions[index] = vec4(light.direction.x, light.direction.y, light.direction.z, 0.0);
        colors[index] = vec4(color.x, color.y, color.z, 0.0);
        params[index] = vec4(
            light.range_m.max(0.0),
            light.intensity.max(0.0),
            light.cone_inner_cos,
            light.cone_outer_cos,
        );
    }
    material.set_uniform_array("u_lamp_position", &positions);
    material.set_uniform_array("u_lamp_direction", &directions);
    material.set_uniform_array("u_lamp_color", &colors);
    material.set_uniform_array("u_lamp_params", &params);
}

/// Sets the camera-relative translation for one queued mesh. macroquad copies
/// uniforms into the draw call when geometry is submitted and marks the batch
/// boundary on every `set_uniform`, so an offset cannot leak into a following
/// cached region or shadow pass.
pub(crate) fn bind_world_offset(material: &Material, offset: Vec3) {
    material.set_uniform("u_world_offset", vec4(offset.x, offset.y, offset.z, 0.0));
}

pub(crate) fn bind_shadow_maps(
    material: &Material,
    bindings: &ShadowBindings,
    directional: &Texture2D,
    lamp0: &Texture2D,
    lamp1: &Texture2D,
    directional_map_size: u32,
    local_map_size: u32,
) {
    material.set_uniform("u_shadow_sun_matrix", bindings.directional_matrix);
    material.set_uniform("u_shadow_lamp0_matrix", bindings.lamp_matrices[0]);
    material.set_uniform("u_shadow_lamp1_matrix", bindings.lamp_matrices[1]);
    material.set_uniform(
        "u_shadow_sun_inverse_transpose",
        bindings.directional_inverse_transpose,
    );
    material.set_uniform(
        "u_shadow_lamp0_inverse_transpose",
        bindings.lamp_inverse_transpose[0],
    );
    material.set_uniform(
        "u_shadow_lamp1_inverse_transpose",
        bindings.lamp_inverse_transpose[1],
    );
    material.set_uniform(
        "u_shadow_lamp_slot",
        vec4(bindings.lamp_slots[0], bindings.lamp_slots[1], -1.0, -1.0),
    );
    material.set_uniform(
        "u_shadow_lamp_weight",
        vec4(bindings.lamp_weights[0], bindings.lamp_weights[1], 0.0, 0.0),
    );
    material.set_uniform(
        "u_shadow_texel",
        shadow_texel_sizes(directional_map_size, local_map_size),
    );
    material.set_uniform(
        "u_shadow_bias",
        vec4(
            super::shadow::SUN_BASE_BIAS,
            super::shadow::SUN_SLOPE_BIAS,
            super::shadow::LAMP_BASE_BIAS,
            super::shadow::LAMP_SLOPE_BIAS,
        ),
    );
    material.set_texture("u_shadow_sun", directional.clone());
    material.set_texture("u_shadow_lamp0", lamp0.clone());
    material.set_texture("u_shadow_lamp1", lamp1.clone());
}

pub(crate) fn shadow_texel_sizes(directional_map_size: u32, local_map_size: u32) -> Vec4 {
    vec4(
        1.0 / directional_map_size.max(1) as f32,
        1.0 / directional_map_size.max(1) as f32,
        1.0 / local_map_size.max(1) as f32,
        1.0 / local_map_size.max(1) as f32,
    )
}

/// CPU reference for the highp RGBA8 packing used by the depth fragment
/// shader. RGB carries an exact 24-bit payload and alpha is a far-depth
/// sentinel. It is intentionally kept testable without a GL context.
#[cfg(test)]
pub(crate) fn pack_depth_rgba(depth: f32) -> [u8; 4] {
    let depth = depth.clamp(0.0, 1.0) as f64;
    if depth >= 1.0 {
        return [255; 4];
    }
    let value = (depth * 16_777_215.0).floor() as u32;
    [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]
}

#[cfg(test)]
pub(crate) fn unpack_depth_rgba(bytes: [u8; 4]) -> f32 {
    let value = ((bytes[0] as u32) << 16) | ((bytes[1] as u32) << 8) | bytes[2] as u32;
    value as f32 / 16_777_215.0
}

#[cfg(test)]
fn compress_linear_peak(linear: Vec3) -> Vec3 {
    let peak = linear.x.max(linear.y).max(linear.z);
    if peak > 1.0 {
        linear * (1.0 / peak)
    } else {
        linear
    }
}

pub const MATERIAL_VERTEX_SHADER: &str = r#"#version 100
precision highp float;
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;

varying vec4 v_color;
varying vec2 v_material;
varying vec3 v_normal;
varying vec3 v_world_pos;
varying float v_flags;

uniform mat4 Model;
uniform mat4 Projection;
uniform vec4 u_world_offset;

void main() {
    vec3 world_pos = position + u_world_offset.xyz;
    gl_Position = Projection * Model * vec4(world_pos, 1.0);
    v_color = color0 / 255.0;
    // texcoord.x = roughness, texcoord.y = emission.
    v_material = texcoord;
    v_normal = normal.xyz;
    v_flags = normal.w;
    v_world_pos = world_pos;
}"#;

pub const MATERIAL_FRAGMENT_SHADER: &str = r#"#version 100
precision highp float;

varying vec4 v_color;
varying vec2 v_material;
varying vec3 v_normal;
varying vec3 v_world_pos;
varying float v_flags;

uniform vec4 u_sun;        // xyz = direction TO light, w = strength
uniform vec4 u_ambient;    // rgb = linear hemispheric color, a = strength
uniform vec4 u_fog_color;  // rgb = linear fog color
uniform vec4 u_fog_params; // x = start, y = end, z = power
uniform vec4 u_camera;     // xyz = camera in local world coordinates
uniform float u_lamp_output;
uniform float u_night;
uniform float u_wetness;
uniform vec4 u_world_offset;
uniform vec4 u_lamp_position[8];  // xyz = local head position, w = slot fade
uniform vec4 u_lamp_direction[8]; // xyz = direction from head toward road
uniform vec4 u_lamp_color[8];     // rgb = linear source color
uniform vec4 u_lamp_params[8];    // x = range, y = intensity, z/w = cone cosines
uniform sampler2D u_shadow_sun;
uniform sampler2D u_shadow_lamp0;
uniform sampler2D u_shadow_lamp1;
uniform mat4 u_shadow_sun_matrix;
uniform mat4 u_shadow_lamp0_matrix;
uniform mat4 u_shadow_lamp1_matrix;
uniform mat4 u_shadow_sun_inverse_transpose;
uniform mat4 u_shadow_lamp0_inverse_transpose;
uniform mat4 u_shadow_lamp1_inverse_transpose;
uniform vec4 u_shadow_lamp_slot; // x/y = local-light slots owning lamp maps
uniform vec4 u_shadow_lamp_weight; // x/y = visibility fade for those maps
uniform vec4 u_shadow_texel;     // xy = sun, zw = local-map texel sizes
uniform vec4 u_shadow_bias;      // base/slope bias for sun and lamps

float flag_bit(float flags, float bit) {
    return step(0.5, mod(floor(flags / bit), 2.0));
}

vec3 srgb_to_linear(vec3 c) {
    vec3 low = c / 12.92;
    vec3 high = pow((c + 0.055) / 1.055, vec3(2.4));
    return mix(low, high, step(vec3(0.04045), c));
}

vec3 linear_to_srgb(vec3 c) {
    c = max(c, vec3(0.0));
    vec3 low = c * 12.92;
    vec3 high = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return mix(low, high, step(vec3(0.0031308), c));
}

// Rozpuszczanie cienia przy granicy mapy kierunkowej (ułamek wymiaru UV).
// 0.18 boxa 128-224 m = 23-40 m strefy przejścia: ogon nocnego cienia znika
// płynnie nad trawą zamiast kąsać brzytwą na granicy pokrycia.
const float SUN_BORDER_MELT_UV = 0.18;

float range_attenuation(float distance_to_light, float range) {
    if (range <= 0.0 || distance_to_light >= range) {
        return 0.0;
    }
    float normalized = distance_to_light / range;
    float inverse_square = 1.0 / (1.0 + 0.12 * distance_to_light * distance_to_light);
    float range_fade = 1.0 - smoothstep(0.72, 1.0, normalized);
    return inverse_square * range_fade;
}

float cone_attenuation(float direction_cosine, float inner_cos, float outer_cos) {
    if (inner_cos <= outer_cos) {
        return step(inner_cos, direction_cosine);
    }
    return smoothstep(outer_cos, inner_cos, direction_cosine);
}

float direct_specular(
    vec3 n,
    vec3 light_dir,
    vec3 view_dir,
    float ndl,
    float roughness
) {
    // A direct specular lobe is never allowed on a backside or from a
    // disabled source. `ndl` and the caller's source strength gate it.
    if (ndl <= 0.0) {
        return 0.0;
    }
    vec3 half_dir = normalize(light_dir + view_dir + vec3(0.0001));
    float shininess = mix(128.0, 8.0, roughness * roughness);
    float specular_strength = (1.0 - roughness) * 0.26;
    return pow(max(dot(n, half_dir), 0.0), shininess) * specular_strength * ndl;
}

highp float unpack_shadow_depth(vec4 encoded_depth) {
    highp vec3 bytes = floor(clamp(encoded_depth.rgb, 0.0, 1.0) * 255.0 + 0.5);
    highp float value = bytes.x * 65536.0 +
                        bytes.y * 256.0 +
                        bytes.z;
    return clamp(value / 16777215.0, 0.0, 1.0);
}

highp vec4 project_shadow(mat4 matrix, vec3 world_pos) {
    highp vec4 clip = matrix * vec4(world_pos, 1.0);
    if (clip.w <= 0.0) {
        return vec4(-1.0);
    }
    highp vec3 ndc = clip.xyz / clip.w;
    return vec4(ndc.xy * 0.5 + vec2(0.5), ndc.z * 0.5 + 0.5, 1.0);
}

highp vec2 receiver_plane_gradient(
    mat4 inverse_transpose_matrix,
    vec3 world_pos,
    vec3 n
) {
    // The uploaded matrix is transpose(inverse(world_to_shadow_clip)).  In
    // clip space the tangent plane is a*x+b*y+c*z+d*w=0; after the shared
    // NDC -> UV/depth scale, dz/du = -a/c and dz/dv = -b/c, including for a
    // perspective lamp projection.
    highp vec4 plane = inverse_transpose_matrix *
                       vec4(n, -dot(n, world_pos));
    if (abs(plane.z) < 0.00001) {
        return vec2(0.0);
    }
    return clamp(-plane.xy / plane.z, vec2(-8.0), vec2(8.0));
}

vec2 nearest_texel_center(vec2 requested_uv, vec2 texel) {
    return (floor(requested_uv / texel) + vec2(0.5)) * texel;
}

float pcf_sun_shadow(vec3 world_pos, vec3 n, vec3 light_dir) {
    vec3 n_unit = normalize(n);
    // Normal-offset: odbiorca przesunięty ~1 texel świata wzdłuż normalnej —
    // wycina acne stripes (równoległe kreski) na gruncie i ścianach przy
    // niskim księżycu, bez peterskiego odpływania cieni przy obiektach.
    vec3 offset_pos = world_pos + n_unit * 0.05;
    vec4 projected = project_shadow(u_shadow_sun_matrix, offset_pos);
    if (projected.w < 0.0 || projected.x < 0.0 || projected.x > 1.0 ||
        projected.y < 0.0 || projected.y > 1.0 || projected.z <= 0.0 ||
        projected.z >= 1.0) {
        return 1.0;
    }
    // Border melt: mapa kierunkowa pokrywa skończony box wokół biegacza.
    // Nocne cienie (elewacja 10-28°) biegną 30-60 m i dochodziły do granicy
    // boxa jako twarda brzytwa: ciemność kończyła się w połowie drogi, a
    // obok (poza boxem) ta sama powierzchnia była już rozświetlona. Im
    // bliższy granicy, tym mocniej podnosimy widoczność ku 1.0 —
    // dokładnie tej wartości, którą zwraca obszar poza mapą — więc próg
    // znika całkowicie, a ogon cienia rozpuszcza się nad trawą zamiast się
    // odcinać.
    vec2 border_gap = min(projected.xy, vec2(1.0) - projected.xy);
    float border_dist = min(border_gap.x, border_gap.y);
    float melt = clamp(border_dist / SUN_BORDER_MELT_UV, 0.0, 1.0);
    melt = melt * melt * (3.0 - 2.0 * melt);
    float ndl = max(dot(normalize(n), normalize(light_dir)), 0.0);
    highp float bias = u_shadow_bias.x + u_shadow_bias.y * (1.0 - ndl);
    highp vec2 gradient = receiver_plane_gradient(
        u_shadow_sun_inverse_transpose,
        world_pos,
        normalize(n)
    );
    float visible = 0.0;
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            vec2 requested_uv = projected.xy +
                vec2(float(x), float(y)) * u_shadow_texel.xy;
            vec2 uv = nearest_texel_center(requested_uv, u_shadow_texel.xy);
            if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
                visible += 1.0;
            } else {
                float stored = unpack_shadow_depth(texture2D(u_shadow_sun, uv));
                highp float receiver_depth = projected.z +
                    dot(gradient, uv - projected.xy);
                visible += step(receiver_depth - bias, stored);
            }
        }
    }
    return mix(1.0, visible / 9.0, melt);
}

float pcf_lamp0_shadow(vec3 world_pos, vec3 n, vec3 light_dir) {
    vec4 projected = project_shadow(u_shadow_lamp0_matrix, world_pos);
    if (projected.w < 0.0 || projected.x < 0.0 || projected.x > 1.0 ||
        projected.y < 0.0 || projected.y > 1.0 || projected.z <= 0.0 ||
        projected.z >= 1.0) {
        return 1.0;
    }
    float ndl = max(dot(normalize(n), normalize(light_dir)), 0.0);
    highp float bias = u_shadow_bias.z + u_shadow_bias.w * (1.0 - ndl);
    highp vec2 gradient = receiver_plane_gradient(
        u_shadow_lamp0_inverse_transpose,
        world_pos,
        normalize(n)
    );
    float visible = 0.0;
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            vec2 requested_uv = projected.xy +
                vec2(float(x), float(y)) * u_shadow_texel.zw;
            vec2 uv = nearest_texel_center(requested_uv, u_shadow_texel.zw);
            if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
                visible += 1.0;
            } else {
                float stored = unpack_shadow_depth(texture2D(u_shadow_lamp0, uv));
                highp float receiver_depth = projected.z +
                    dot(gradient, uv - projected.xy);
                visible += step(receiver_depth - bias, stored);
            }
        }
    }
    return visible / 9.0;
}

float pcf_lamp1_shadow(vec3 world_pos, vec3 n, vec3 light_dir) {
    vec4 projected = project_shadow(u_shadow_lamp1_matrix, world_pos);
    if (projected.w < 0.0 || projected.x < 0.0 || projected.x > 1.0 ||
        projected.y < 0.0 || projected.y > 1.0 || projected.z <= 0.0 ||
        projected.z >= 1.0) {
        return 1.0;
    }
    float ndl = max(dot(normalize(n), normalize(light_dir)), 0.0);
    highp float bias = u_shadow_bias.z + u_shadow_bias.w * (1.0 - ndl);
    highp vec2 gradient = receiver_plane_gradient(
        u_shadow_lamp1_inverse_transpose,
        world_pos,
        normalize(n)
    );
    float visible = 0.0;
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            vec2 requested_uv = projected.xy +
                vec2(float(x), float(y)) * u_shadow_texel.zw;
            vec2 uv = nearest_texel_center(requested_uv, u_shadow_texel.zw);
            if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
                visible += 1.0;
            } else {
                float stored = unpack_shadow_depth(texture2D(u_shadow_lamp1, uv));
                highp float receiver_depth = projected.z +
                    dot(gradient, uv - projected.xy);
                visible += step(receiver_depth - bias, stored);
            }
        }
    }
    return visible / 9.0;
}

void main() {
    vec3 albedo = srgb_to_linear(clamp(v_color.rgb, 0.0, 1.0));
    float emission = max(v_material.y, 0.0);
    float wetness = clamp(u_wetness, 0.0, 1.0);
    float wet_patch = flag_bit(v_flags, 16.0);
    float night_glow = flag_bit(v_flags, 32.0);
    // window glow & LED heads follow the SAME lamp schedule as the light
    // sources: crisp "switched on at night, off from morning to noon".
    float glow = u_lamp_output * night_glow;
    float night_led = flag_bit(v_flags, 64.0);
    float led_glow = u_lamp_output * night_led;
    albedo *= 1.0 - 0.025 * wetness;
    if (wet_patch > 0.5) {
        // A stable puddle mesh becomes a reflective patch only in wet weather;
        // its geometry and base colour remain reusable through day/night and
        // region-cache eviction.
        float variation = 0.985 + 0.015 * sin(v_world_pos.x * 2.7 +
                                                v_world_pos.z * 5.1);
        albedo *= mix(1.0, variation, wetness);
    }
    if (night_glow > 0.5) {
        vec3 warm_window = srgb_to_linear(vec3(1.0, 0.72, 0.25));
        albedo = mix(albedo, warm_window, glow * 0.78);
        emission += glow * 0.82;
    }
    if (night_led > 0.5) {
        emission += led_glow * 1.05;
    }
    float unlit = flag_bit(v_flags, 1.0);
    vec3 lit;
    if (unlit > 0.5) {
        // Keep the unlit path before all PCF, cone, attenuation and specular
        // work. Fog, tone mapping and output encoding remain shared below.
        lit = albedo + albedo * emission;
    } else {
        vec3 n = normalize(v_normal);
        vec3 l = normalize(u_sun.xyz);
        vec3 view_dir = normalize(u_camera.xyz - v_world_pos);
        float roughness = clamp(v_material.x, 0.04, 1.0);
        roughness = mix(roughness, max(0.04, roughness * 0.38), wetness);
        if (wet_patch > 0.5) {
            roughness = mix(roughness, 0.035, wetness);
        }
        float ndl = max(dot(n, l), 0.0);
        float hemi = mix(0.58, 1.06, clamp(n.y * 0.5 + 0.5, 0.0, 1.0));
        vec3 ambient = u_ambient.rgb * u_ambient.a * hemi;
        float sun_strength = max(u_sun.a, 0.0);
        float sun_visibility = pcf_sun_shadow(v_world_pos, n, l);
        vec3 direct = albedo * (sun_strength * ndl * sun_visibility);
        direct += vec3(direct_specular(n, l, view_dir, ndl, roughness) * sun_strength * sun_visibility);

        for (int i = 0; i < 8; i++) {
            float range = u_lamp_params[i].x;
            float slot_weight = max(u_lamp_position[i].w, 0.0);
            vec3 source_to_fragment = v_world_pos - u_lamp_position[i].xyz;
            float distance_to_light = length(source_to_fragment);
            if (range <= 0.0 || distance_to_light <= 0.0001 || slot_weight <= 0.0) {
                continue;
            }
            vec3 from_source = source_to_fragment / distance_to_light;
            float cone = cone_attenuation(
                dot(normalize(u_lamp_direction[i].xyz), from_source),
                u_lamp_params[i].z,
                u_lamp_params[i].w
            );
            float attenuation = range_attenuation(distance_to_light, range) * cone;
            float source_strength = u_lamp_params[i].y * u_lamp_output * slot_weight * attenuation;
            if (source_strength <= 0.0) {
                continue;
            }
            vec3 light_dir = -from_source;
            float lamp_ndl = max(dot(n, light_dir), 0.0);
            float lamp_visibility = 1.0;
            if (float(i) == u_shadow_lamp_slot.x) {
                lamp_visibility = mix(
                    1.0,
                    pcf_lamp0_shadow(v_world_pos, n, light_dir),
                    clamp(u_shadow_lamp_weight.x, 0.0, 1.0)
                );
            } else if (float(i) == u_shadow_lamp_slot.y) {
                lamp_visibility = mix(
                    1.0,
                    pcf_lamp1_shadow(v_world_pos, n, light_dir),
                    clamp(u_shadow_lamp_weight.y, 0.0, 1.0)
                );
            }
            vec3 lamp_color = u_lamp_color[i].rgb;
            direct += lamp_color * albedo * (source_strength * lamp_ndl * lamp_visibility);
            direct += lamp_color * direct_specular(
                n,
                light_dir,
                view_dir,
                lamp_ndl,
                roughness
            ) * source_strength * lamp_visibility;
        }

        lit = albedo * ambient + direct + albedo * emission;
    }

    // Fog is applied only after lighting and emission. NO_FOG is bit 4.
    float no_fog = flag_bit(v_flags, 4.0);
    float distance_to_camera = distance(v_world_pos, u_camera.xyz);
    float fog_t = clamp((distance_to_camera - u_fog_params.x) /
                        max(u_fog_params.y - u_fog_params.x, 0.001), 0.0, 1.0);
    fog_t = pow(fog_t, u_fog_params.z) * (1.0 - no_fog);
    lit = mix(lit, u_fog_color.rgb, fog_t);

    // Compress every overbright color into displayable linear RGB by one
    // common factor. This keeps channel ratios intact; the final clamp only
    // guards numerical error after the shared-ratio map.
    float peak = max(max(lit.r, lit.g), lit.b);
    if (peak > 1.0) {
        lit *= 1.0 / peak;
    }
    gl_FragColor = vec4(clamp(linear_to_srgb(lit), 0.0, 1.0), v_color.a);
}"#;

pub const SHADOW_VERTEX_SHADER: &str = r#"#version 100
precision highp float;
attribute vec3 position;
attribute vec4 normal;

varying float v_shadow_flags;

uniform mat4 Model;
uniform mat4 Projection;
uniform vec4 u_world_offset;

void main() {
    vec3 world_pos = position + u_world_offset.xyz;
    gl_Position = Projection * Model * vec4(world_pos, 1.0);
    v_shadow_flags = normal.w;
}"#;

pub const SHADOW_FRAGMENT_SHADER: &str = r#"#version 100
precision highp float;

varying float v_shadow_flags;

float flag_bit(float flags, float bit) {
    return step(0.5, mod(floor(flags / bit), 2.0));
}

highp vec4 pack_shadow_depth(highp float depth) {
    // RGB carries an exact 24-bit payload in highp; alpha is a far-depth
    // sentinel so the RGBA8 attachment does not depend on 32-bit mantissa.
    depth = clamp(depth, 0.0, 1.0);
    if (depth >= 1.0) {
        return vec4(1.0);
    }
    highp float value = floor(depth * 16777215.0);
    highp float r = floor(value / 65536.0);
    value -= r * 65536.0;
    highp float g = floor(value / 256.0);
    highp float b = value - g * 256.0;
    return vec4(r, g, b, 255.0) / 255.0;
}

void main() {
    // NO_SHADOW is bit 8. Sky, sun, clouds, stars, mountains and atmosphere
    // are still drawn by the color pass but cannot become giant casters.
    if (flag_bit(v_shadow_flags, 8.0) > 0.5) {
        discard;
    }
    gl_FragColor = pack_shadow_depth(gl_FragCoord.z);
}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_depth_conventions_are_stable_without_gl_context() {
        let opaque = opaque_pipeline_convention();
        assert_eq!(opaque.depth_test, Comparison::LessOrEqual);
        assert!(opaque.depth_write);
        assert!(!opaque.alpha_blend);

        let effects = effect_pipeline_convention();
        assert_eq!(effects.depth_test, Comparison::LessOrEqual);
        assert!(!effects.depth_write);
        assert!(effects.alpha_blend);
        assert!(!effects.additive);

        let beams = beam_pipeline_convention();
        assert_eq!(beams.depth_test, Comparison::LessOrEqual);
        assert!(!beams.depth_write);
        assert!(beams.alpha_blend);
        assert!(beams.additive, "kreski lamp muszą sumować wkład światła");
    }

    #[test]
    fn packed_depth_round_trips_across_the_rgba8_attachment() {
        for depth in [0.0, 0.000001, 0.1, 0.5, 0.999999, 1.0] {
            let bytes = pack_depth_rgba(depth);
            let unpacked = unpack_depth_rgba(bytes);
            assert!(
                (unpacked - depth).abs() < 2e-7,
                "depth {depth} -> {bytes:?} -> {unpacked}"
            );
            assert_eq!(bytes[3], 255, "alpha remains the far-depth sentinel");
        }
        assert_eq!(pack_depth_rgba(1.0), [255, 255, 255, 255]);
        assert_eq!(unpack_depth_rgba([255, 255, 255, 255]), 1.0);
    }

    #[test]
    fn overbright_linear_compression_preserves_ratios_and_stays_in_gamut() {
        let input = Vec3::new(2.4, 0.8, 1.2);
        let compressed = compress_linear_peak(input);
        let scale = compressed.x / input.x;

        for (actual, original) in [
            (compressed.x, input.x),
            (compressed.y, input.y),
            (compressed.z, input.z),
        ] {
            assert!((actual - original * scale).abs() < 1e-6);
            assert!((0.0..=1.0).contains(&actual));
        }

        let display = linear_to_srgb(compressed, 1.0);
        assert!((0.0..=1.0).contains(&display.r));
        assert!((0.0..=1.0).contains(&display.g));
        assert!((0.0..=1.0).contains(&display.b));
    }

    #[test]
    fn shadow_sampler_texels_follow_the_selected_quality_maps() {
        let low = shadow_texel_sizes(1024, 256);
        let medium = shadow_texel_sizes(2048, 512);
        let high = shadow_texel_sizes(4096, 1024);
        assert!(low.x > medium.x && medium.x > high.x);
        assert!(low.z > medium.z && medium.z > high.z);
    }
}
