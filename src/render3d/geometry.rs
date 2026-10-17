//! Proceduralna geometria świata 3D.
//!
//! Builder nie wykonuje oświetlenia. Zapisuje bazowy sRGB, normalną oraz
//! parametry materiału w każdym wierzchołku. Opaque meshes are also submitted
//! to the stage-4 shadow passes, so the shadow map sees precisely the same
//! triangles and runner pose as the visible color pass.

use macroquad::prelude::*;

use super::material::{MaterialFlags, MaterialSpec, vertex_data};

const CHUNK_V: usize = 3000;
const CHUNK_I: usize = 4900;
const TAU: f32 = std::f32::consts::TAU;
const PI: f32 = std::f32::consts::PI;

pub(crate) struct GlowQuad {
    pub points: [Vec3; 4],
    pub colors: [Color; 4],
}

pub(crate) struct Bld {
    vertices: Vec<Vertex>,
    indices: Vec<u16>,
    chunks: Vec<Mesh>,
    reusable_chunks: Vec<Mesh>,
    effect_vertices: Vec<Vertex>,
    effect_indices: Vec<u16>,
    effect_chunks: Vec<Mesh>,
    reusable_effect_chunks: Vec<Mesh>,
    beam_vertices: Vec<Vertex>,
    beam_indices: Vec<u16>,
    beam_chunks: Vec<Mesh>,
    reusable_beam_chunks: Vec<Mesh>,
    pub(crate) eye: Vec3,
    material: MaterialSpec,
    shadow_class: Option<bool>,
}

impl Bld {
    pub(crate) fn new(eye: Vec3) -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            chunks: Vec::new(),
            reusable_chunks: Vec::new(),
            effect_vertices: Vec::new(),
            effect_indices: Vec::new(),
            effect_chunks: Vec::new(),
            reusable_effect_chunks: Vec::new(),
            beam_vertices: Vec::new(),
            beam_indices: Vec::new(),
            beam_chunks: Vec::new(),
            reusable_beam_chunks: Vec::new(),
            eye,
            material: MaterialSpec::new(0.8, 0.0, MaterialFlags::NONE),
            shadow_class: None,
        }
    }

    pub(crate) fn with_material<F, R>(&mut self, material: MaterialSpec, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let old = self.material;
        let no_shadow = material.flags.contains(MaterialFlags::NO_SHADOW);
        if !self.vertices.is_empty() && self.shadow_class != Some(no_shadow) {
            self.flush();
        }
        self.material = material;
        self.shadow_class = Some(no_shadow);
        let result = f(self);
        self.material = old;
        result
    }

    fn room(&mut self, vertices: usize, indices: usize) {
        if self.vertices.len() + vertices > CHUNK_V || self.indices.len() + indices > CHUNK_I {
            self.flush();
        }
    }

    fn effect_room(&mut self, vertices: usize, indices: usize) {
        if self.effect_vertices.len() + vertices > CHUNK_V
            || self.effect_indices.len() + indices > CHUNK_I
        {
            self.flush_effects();
        }
    }

    fn beam_room(&mut self, vertices: usize, indices: usize) {
        if self.beam_vertices.len() + vertices > CHUNK_V
            || self.beam_indices.len() + indices > CHUNK_I
        {
            self.flush_beams();
        }
    }

    fn flush(&mut self) {
        if self.vertices.is_empty() {
            return;
        }
        let mut mesh = self.reusable_chunks.pop().unwrap_or(Mesh {
            vertices: Vec::new(),
            indices: Vec::new(),
            texture: None,
        });
        std::mem::swap(&mut self.vertices, &mut mesh.vertices);
        std::mem::swap(&mut self.indices, &mut mesh.indices);
        self.chunks.push(mesh);
        self.shadow_class = None;
    }

    fn flush_effects(&mut self) {
        if self.effect_vertices.is_empty() {
            return;
        }
        let mut mesh = self.reusable_effect_chunks.pop().unwrap_or(Mesh {
            vertices: Vec::new(),
            indices: Vec::new(),
            texture: None,
        });
        std::mem::swap(&mut self.effect_vertices, &mut mesh.vertices);
        std::mem::swap(&mut self.effect_indices, &mut mesh.indices);
        self.effect_chunks.push(mesh);
    }

    fn flush_beams(&mut self) {
        if self.beam_vertices.is_empty() {
            return;
        }
        let mut mesh = self.reusable_beam_chunks.pop().unwrap_or(Mesh {
            vertices: Vec::new(),
            indices: Vec::new(),
            texture: None,
        });
        std::mem::swap(&mut self.beam_vertices, &mut mesh.vertices);
        std::mem::swap(&mut self.beam_indices, &mut mesh.indices);
        self.beam_chunks.push(mesh);
    }

    fn add_vertex(&mut self, position: Vec3, normal: Vec3, color: Color) {
        self.vertices
            .push(vertex_data(position, normal, color, self.material));
    }

    fn add_effect_vertex(&mut self, position: Vec3, normal: Vec3, color: Color) {
        self.effect_vertices
            .push(vertex_data(position, normal, color, self.material));
    }

    fn add_beam_vertex(&mut self, position: Vec3, normal: Vec3, color: Color) {
        self.beam_vertices
            .push(vertex_data(position, normal, color, self.material));
    }

    pub(crate) fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Color) {
        let normal = unit((b - a).cross(c - a));
        self.quad_with_normals(a, b, c, d, [normal; 4], color);
    }

    pub(crate) fn quad_with_normals(
        &mut self,
        a: Vec3,
        b: Vec3,
        c: Vec3,
        d: Vec3,
        normals: [Vec3; 4],
        color: Color,
    ) {
        self.room(4, 6);
        let base = self.vertices.len() as u16;
        for (position, normal) in [
            (a, normals[0]),
            (b, normals[1]),
            (c, normals[2]),
            (d, normals[3]),
        ] {
            self.add_vertex(position, normal, color);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Adds an opaque quad whose base color and material parameters may vary
    /// per vertex. Material flags remain constant across the primitive so the
    /// fragment shader never interpolates a feature bit.
    pub(crate) fn quad_with_vertex_materials(
        &mut self,
        points: [Vec3; 4],
        colors: [Color; 4],
        materials: [MaterialSpec; 4],
    ) {
        let flags = materials[0].flags;
        assert!(
            materials.iter().all(|material| material.flags == flags),
            "material flags must be constant across a quad"
        );
        self.room(4, 6);
        let base = self.vertices.len() as u16;
        let normal = unit((points[1] - points[0]).cross(points[2] - points[0]));
        for ((point, color), material) in points.into_iter().zip(colors).zip(materials) {
            self.vertices
                .push(vertex_data(point, normal, color, material));
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    pub(crate) fn tri_with_normal(
        &mut self,
        a: Vec3,
        b: Vec3,
        c: Vec3,
        normal: Vec3,
        color: Color,
    ) {
        self.room(3, 3);
        let base = self.vertices.len() as u16;
        self.add_vertex(a, normal, color);
        self.add_vertex(b, normal, color);
        self.add_vertex(c, normal, color);
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    pub(crate) fn cube(&mut self, center: Vec3, size: Vec3, color: Color) {
        let half = size * 0.5;
        let (x0, x1) = (center.x - half.x, center.x + half.x);
        let (y0, y1) = (center.y - half.y, center.y + half.y);
        let (z0, z1) = (center.z - half.z, center.z + half.z);
        self.quad(
            Vec3::new(x0, y1, z0),
            Vec3::new(x0, y1, z1),
            Vec3::new(x1, y1, z1),
            Vec3::new(x1, y1, z0),
            color,
        );
        self.quad(
            Vec3::new(x0, y0, z0),
            Vec3::new(x1, y0, z0),
            Vec3::new(x1, y0, z1),
            Vec3::new(x0, y0, z1),
            color,
        );
        self.quad(
            Vec3::new(x1, y0, z0),
            Vec3::new(x1, y1, z0),
            Vec3::new(x1, y1, z1),
            Vec3::new(x1, y0, z1),
            color,
        );
        self.quad(
            Vec3::new(x0, y0, z0),
            Vec3::new(x0, y0, z1),
            Vec3::new(x0, y1, z1),
            Vec3::new(x0, y1, z0),
            color,
        );
        self.quad(
            Vec3::new(x0, y0, z1),
            Vec3::new(x1, y0, z1),
            Vec3::new(x1, y1, z1),
            Vec3::new(x0, y1, z1),
            color,
        );
        self.quad(
            Vec3::new(x0, y0, z0),
            Vec3::new(x0, y1, z0),
            Vec3::new(x1, y1, z0),
            Vec3::new(x1, y0, z0),
            color,
        );
    }

    pub(crate) fn cube_rotated(&mut self, center: Vec3, size: Vec3, rotation: Quat, color: Color) {
        let half = size * 0.5;
        let corner = |x: f32, y: f32, z: f32| {
            center + rotation * Vec3::new(x * half.x, y * half.y, z * half.z)
        };
        let (x0, x1) = (-1.0, 1.0);
        let (y0, y1) = (-1.0, 1.0);
        let (z0, z1) = (-1.0, 1.0);
        self.quad(
            corner(x0, y1, z0),
            corner(x0, y1, z1),
            corner(x1, y1, z1),
            corner(x1, y1, z0),
            color,
        );
        self.quad(
            corner(x0, y0, z0),
            corner(x1, y0, z0),
            corner(x1, y0, z1),
            corner(x0, y0, z1),
            color,
        );
        self.quad(
            corner(x1, y0, z0),
            corner(x1, y1, z0),
            corner(x1, y1, z1),
            corner(x1, y0, z1),
            color,
        );
        self.quad(
            corner(x0, y0, z0),
            corner(x0, y0, z1),
            corner(x0, y1, z1),
            corner(x0, y1, z0),
            color,
        );
        self.quad(
            corner(x0, y0, z1),
            corner(x1, y0, z1),
            corner(x1, y1, z1),
            corner(x0, y1, z1),
            color,
        );
        self.quad(
            corner(x0, y0, z0),
            corner(x0, y1, z0),
            corner(x1, y1, z0),
            corner(x1, y0, z0),
            color,
        );
    }

    /// Kapsuła jest cylindrem z półkulistymi końcami. Półkule są osobnymi
    /// siatkami, ale mają wspólny materiał i gładkie normalne radialne.
    pub(crate) fn capsule(&mut self, a: Vec3, b: Vec3, radius: f32, color: Color) {
        let axis = b - a;
        let length = axis.length();
        if length < 1e-4 || radius <= 0.0 {
            return;
        }
        let n = axis / length;
        let reference = if n.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
        let u = unit(n.cross(reference));
        let v = unit(n.cross(u));
        const SEGMENTS: usize = 8;
        for k in 0..SEGMENTS {
            let t0 = k as f32 / SEGMENTS as f32 * TAU;
            let t1 = (k + 1) as f32 / SEGMENTS as f32 * TAU;
            let r0 = u * t0.cos() * radius + v * t0.sin() * radius;
            let r1 = u * t1.cos() * radius + v * t1.sin() * radius;
            self.quad_with_normals(
                a + r0,
                a + r1,
                b + r1,
                b + r0,
                [unit(r0), unit(r1), unit(r1), unit(r0)],
                color,
            );
        }
        self.sphere(a, radius, 5, SEGMENTS, color);
        self.sphere(b, radius, 5, SEGMENTS, color);
    }

    pub(crate) fn limb(&mut self, a: Vec3, b: Vec3, radius: f32, color: Color) {
        self.capsule(a, b, radius * 0.5, color);
    }

    pub(crate) fn cone(
        &mut self,
        base: Vec3,
        radius: f32,
        height: f32,
        segments: usize,
        color: Color,
    ) {
        let apex = base + Vec3::new(0.0, height, 0.0);
        for k in 0..segments {
            let a0 = k as f32 / segments as f32 * TAU;
            let a1 = (k + 1) as f32 / segments as f32 * TAU;
            let p0 = base + Vec3::new(a0.cos() * radius, 0.0, a0.sin() * radius);
            let p1 = base + Vec3::new(a1.cos() * radius, 0.0, a1.sin() * radius);
            let mid = (a0 + a1) * 0.5;
            let normal = unit(Vec3::new(mid.cos() * height, radius, mid.sin() * height));
            self.tri_with_normal(p0, p1, apex, normal, color);
        }
    }

    pub(crate) fn roof(
        &mut self,
        center: Vec3,
        half_x: f32,
        half_z: f32,
        height: f32,
        color: Color,
    ) {
        let apex = center + Vec3::new(0.0, height, 0.0);
        let (x0, x1) = (center.x - half_x, center.x + half_x);
        let (z0, z1) = (center.z - half_z, center.z + half_z);
        for (a, b) in [
            (Vec3::new(x0, center.y, z0), Vec3::new(x1, center.y, z0)),
            (Vec3::new(x1, center.y, z0), Vec3::new(x1, center.y, z1)),
            (Vec3::new(x1, center.y, z1), Vec3::new(x0, center.y, z1)),
            (Vec3::new(x0, center.y, z1), Vec3::new(x0, center.y, z0)),
        ] {
            let mut normal = unit((b - a).cross(apex - a));
            if normal.dot((a + b) * 0.5 - center) < 0.0 {
                normal = -normal;
            }
            self.tri_with_normal(a, b, apex, normal, color);
        }
    }

    /// Kula ma normalną promieniową dla każdego wierzchołka, a nie jedną
    /// normalną kafelka. Dzięki temu głowa, dłonie i krople światła zachowują
    /// gładką sylwetkę przy dowolnym kierunku światła.
    pub(crate) fn sphere(
        &mut self,
        center: Vec3,
        radius: f32,
        rings: usize,
        slices: usize,
        color: Color,
    ) {
        let point = |latitude: f32, longitude: f32| -> Vec3 {
            center
                + Vec3::new(
                    latitude.cos() * longitude.cos(),
                    latitude.sin(),
                    latitude.cos() * longitude.sin(),
                ) * radius
        };
        for ring in 0..rings {
            let p0 = -PI / 2.0 + PI * ring as f32 / rings as f32;
            let p1 = -PI / 2.0 + PI * (ring + 1) as f32 / rings as f32;
            for slice in 0..slices {
                let t0 = TAU * slice as f32 / slices as f32;
                let t1 = TAU * (slice + 1) as f32 / slices as f32;
                let points = [point(p0, t0), point(p0, t1), point(p1, t1), point(p1, t0)];
                let normals = points.map(|p| unit(p - center));
                self.quad_with_normals(points[0], points[1], points[2], points[3], normals, color);
            }
        }
    }

    /// Low-poly ellipsoid used by distant clouds. Keeping the normal from the
    /// inverse scale preserves soft shape shading without a second material.
    pub(crate) fn ellipsoid(
        &mut self,
        center: Vec3,
        radii: Vec3,
        rings: usize,
        slices: usize,
        color: Color,
    ) {
        let radii = Vec3::new(
            radii.x.max(1.0e-4),
            radii.y.max(1.0e-4),
            radii.z.max(1.0e-4),
        );
        let point = |latitude: f32, longitude: f32| -> (Vec3, Vec3) {
            let unit = Vec3::new(
                latitude.cos() * longitude.cos(),
                latitude.sin(),
                latitude.cos() * longitude.sin(),
            );
            let position = center + unit * radii;
            let normal = (Vec3::new(unit.x / radii.x, unit.y / radii.y, unit.z / radii.z))
                .normalize_or_zero();
            (position, normal)
        };
        for ring in 0..rings {
            let p0 = -PI / 2.0 + PI * ring as f32 / rings as f32;
            let p1 = -PI / 2.0 + PI * (ring + 1) as f32 / rings as f32;
            for slice in 0..slices {
                let t0 = TAU * slice as f32 / slices as f32;
                let t1 = TAU * (slice + 1) as f32 / slices as f32;
                let (p0t0, n0) = point(p0, t0);
                let (p0t1, n1) = point(p0, t1);
                let (p1t1, n2) = point(p1, t1);
                let (p1t0, n3) = point(p1, t0);
                self.quad_with_normals(p0t0, p0t1, p1t1, p1t0, [n0, n1, n2, n3], color);
            }
        }
    }

    pub(crate) fn glow_quad(&mut self, quad: GlowQuad) {
        self.transparent_quad(quad, false);
    }

    /// Kreski lamp idą do osobnego pokoju z addytywnym blendingiem: dwa
    /// nachodzące stożki mają dać jaśniejszy obszar przecięcia (rozproszenie
    /// we mgle się sumuje), a nie miks jak standardowy alpha-blend.
    pub(crate) fn beam_quad(&mut self, quad: GlowQuad) {
        self.transparent_quad(quad, true);
    }

    fn transparent_quad(&mut self, quad: GlowQuad, beam: bool) {
        if beam {
            self.beam_room(4, 6);
        } else {
            self.effect_room(4, 6);
        }
        let normal = unit((quad.points[1] - quad.points[0]).cross(quad.points[2] - quad.points[0]));
        for (point, color) in quad.points.into_iter().zip(quad.colors) {
            if beam {
                self.add_beam_vertex(point, normal, color);
            } else {
                self.add_effect_vertex(point, normal, color);
            }
        }
        let base = if beam {
            self.beam_vertices.len() as u16
        } else {
            self.effect_vertices.len() as u16
        };
        if beam {
            self.beam_indices.extend_from_slice(&[
                base - 4,
                base - 3,
                base - 2,
                base - 4,
                base - 2,
                base - 1,
            ]);
        } else {
            self.effect_indices.extend_from_slice(&[
                base - 4,
                base - 3,
                base - 2,
                base - 4,
                base - 2,
                base - 1,
            ]);
        }
    }

    pub(crate) fn finish(mut self) -> (Vec<Mesh>, Vec<Mesh>) {
        self.flush();
        self.flush_effects();
        self.flush_beams();
        let effects = self.sorted_effect_chunks();
        (self.chunks, effects)
    }

    pub(crate) fn reset_reusable(&mut self, eye: Vec3) {
        for mut mesh in self.chunks.drain(..) {
            mesh.vertices.clear();
            mesh.indices.clear();
            self.reusable_chunks.push(mesh);
        }
        for mut mesh in self.effect_chunks.drain(..) {
            mesh.vertices.clear();
            mesh.indices.clear();
            self.reusable_effect_chunks.push(mesh);
        }
        self.vertices.clear();
        self.indices.clear();
        self.effect_vertices.clear();
        self.effect_indices.clear();
        for mut mesh in self.beam_chunks.drain(..) {
            mesh.vertices.clear();
            mesh.indices.clear();
            self.reusable_beam_chunks.push(mesh);
        }
        self.beam_vertices.clear();
        self.beam_indices.clear();
        self.shadow_class = None;
        self.eye = eye;
        self.material = MaterialSpec::new(0.8, 0.0, MaterialFlags::NONE);
    }

    pub(crate) fn finish_reusable(&mut self) {
        self.flush();
        self.flush_effects();
        self.flush_beams();
        let effects = self.sorted_effect_chunks();
        self.effect_chunks = effects;
        let beams = self.sorted_beam_chunks();
        self.beam_chunks = beams;
    }

    pub(crate) fn opaque_meshes(&self) -> &[Mesh] {
        &self.chunks
    }

    pub(crate) fn effect_meshes(&self) -> &[Mesh] {
        &self.effect_chunks
    }

    pub(crate) fn beam_meshes(&self) -> &[Mesh] {
        &self.beam_chunks
    }

    /// Transparent triangles must be submitted back-to-front. Effects are
    /// collected separately from opaque geometry, so sorting here cannot
    /// disturb the opaque depth-writing pass or enter the shadow maps.
    fn sorted_transparent_chunks(&mut self, beams: bool) -> Vec<Mesh> {
        let eye = self.eye;
        let mut triangles: Vec<([Vertex; 3], f32)> = Vec::new();
        if beams {
            for mesh in self.beam_chunks.drain(..) {
                collect_transparent_triangles(mesh, eye, &mut triangles);
            }
        } else {
            for mesh in self.effect_chunks.drain(..) {
                collect_transparent_triangles(mesh, eye, &mut triangles);
            }
        }
        triangles.sort_by(|a, b| b.1.total_cmp(&a.1));

        let mut chunks = Vec::new();
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for (triangle, _) in triangles {
            if vertices.len() + 3 > CHUNK_V || indices.len() + 3 > CHUNK_I {
                chunks.push(Mesh {
                    vertices: std::mem::take(&mut vertices),
                    indices: std::mem::take(&mut indices),
                    texture: None,
                });
            }
            let base = vertices.len() as u16;
            vertices.extend_from_slice(&triangle);
            indices.extend_from_slice(&[base, base + 1, base + 2]);
        }
        if !vertices.is_empty() {
            chunks.push(Mesh {
                vertices,
                indices,
                texture: None,
            });
        }
        chunks
    }

    fn sorted_effect_chunks(&mut self) -> Vec<Mesh> {
        self.sorted_transparent_chunks(false)
    }

    /// Kreski lamp: sumowanie addytywne jest przemienne, więc sortowanie nie
    /// jest konieczne — utrzymujemy je dla wspólnej inwestycji w pokój i
    /// stabilnych chunków.
    fn sorted_beam_chunks(&mut self) -> Vec<Mesh> {
        self.sorted_transparent_chunks(true)
    }
}

fn collect_transparent_triangles(mesh: Mesh, eye: Vec3, triangles: &mut Vec<([Vertex; 3], f32)>) {
    for indices in mesh.indices.as_chunks::<3>().0 {
        let triangle = [
            mesh.vertices[indices[0] as usize],
            mesh.vertices[indices[1] as usize],
            mesh.vertices[indices[2] as usize],
        ];
        let center = (triangle[0].position + triangle[1].position + triangle[2].position) / 3.0;
        triangles.push((triangle, (center - eye).length_squared()));
    }
}

fn unit(vector: Vec3) -> Vec3 {
    if vector.length_squared() > 1e-8 {
        vector.normalize()
    } else {
        Vec3::Y
    }
}
