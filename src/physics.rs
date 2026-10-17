//! Fixed-step Rapier simulation for the visible route.
//!
//! This module deliberately has no rendering dependency.  Rapier uses its
//! own glamx math types internally; the public snapshots below use small
//! arrays so the macroquad/glam version used by the renderer never becomes a
//! physics API requirement.

use std::collections::{HashMap, HashSet};

use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::prelude::*;

use crate::rng::{REGION_M, Rng, hash1, region_of};
use crate::world::{self, ColliderKind, Geometry, ObjectDesc};

pub const FIXED_DT: f32 = 1.0 / 60.0;
pub const GRAVITY: f32 = -9.81;
pub const GROUND_Y: f32 = 0.02;
pub const RUNNER_LANE_Z: f32 = -1.15;

const MAX_FRAME_DT: f32 = 0.25;
const MAX_CATCH_UP_STEPS: usize = 8;
const ORIGIN_REBASE_M: f64 = 256.0;
const FLOOR_HALF_X: f32 = 650.0;
const FLOOR_HALF_Z: f32 = 32.0;
const FLOOR_HALF_THICKNESS: f32 = 0.02;
const CHARACTER_HALF_HEIGHT: f32 = 0.72;
const CHARACTER_RADIUS: f32 = 0.26;
const CHARACTER_CENTER_Y: f32 = GROUND_Y + CHARACTER_HALF_HEIGHT + CHARACTER_RADIUS;
pub const SHOE_SIZE: [f32; 3] = [0.28, 0.085, 0.16];
pub const SHOE_CENTER_ABOVE_FLOOR: f32 = SHOE_SIZE[1] * 0.5;
const MAX_DYNAMIC_OBJECTS: usize = 24;
const LEG_UPPER: f32 = 0.50;
const LEG_LOWER: f32 = 0.47;
const MAX_SAFE_REBASE_SHIFT_M: f32 = 100_000.0;
const ACTIVE_BACK_M: f64 = 180.0;
const ACTIVE_FORWARD_M: f64 = 500.0;

/// The part of the runner pose that the renderer needs for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunnerVisual {
    pub root: [f32; 3],
    pub feet: [[f32; 3]; 2],
    pub knees: [[f32; 3]; 2],
    pub grounded: bool,
    pub gait_phase: f32,
    pub smoothed_speed_kmh: f32,
}

/// A dynamic object's interpolated rigid-body transform and render shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DynamicVisual {
    pub id: u64,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub size: [f32; 3],
    pub kind: u8,
}

/// Render state sampled between two completed fixed physics steps.
#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsSnapshot {
    pub runner: RunnerVisual,
    pub dynamics: Vec<DynamicVisual>,
    pub alpha: f32,
    pub origin_m: f64,
    pub active_static_colliders: usize,
    pub dynamic_bodies: usize,
}

#[derive(Clone, Copy)]
struct PoseState {
    root: Vector,
    feet: [Vector; 2],
    knees: [Vector; 2],
    grounded: bool,
    gait_phase: f32,
    smoothed_speed_kmh: f32,
}

impl PoseState {
    fn at(root: Vector, grounded: bool) -> Self {
        let feet = [
            Vector::new(
                root.x - 0.14,
                GROUND_Y + SHOE_CENTER_ABOVE_FLOOR,
                RUNNER_LANE_Z + 0.12,
            ),
            Vector::new(
                root.x + 0.14,
                GROUND_Y + SHOE_CENTER_ABOVE_FLOOR,
                RUNNER_LANE_Z - 0.12,
            ),
        ];
        Self {
            root,
            feet,
            knees: [root; 2],
            grounded,
            gait_phase: 0.0,
            smoothed_speed_kmh: 0.0,
        }
    }
}

#[derive(Clone, Copy)]
struct BodyTransform {
    position: Vector,
    rotation: Rotation,
}

#[derive(Clone, Copy)]
struct StaticBody {
    body: RigidBodyHandle,
}

#[derive(Clone, Copy)]
struct DynamicSpawn {
    id: u64,
    absolute_m: f64,
    lateral_m: f32,
    height_m: f32,
    size: [f32; 3],
    kind: u8,
}

struct DynamicSlot {
    id: u64,
    body: RigidBodyHandle,
    size: [f32; 3],
    kind: u8,
    previous: BodyTransform,
    current: BodyTransform,
}

/// The route physics owner.  `route_m` remains owned by the game loop; this
/// state only follows it with a kinematic target and never awards distance or
/// economy progress.
pub struct Physics {
    world: PhysicsWorld,
    accumulator: f32,
    origin_m: f64,
    runner_body: RigidBodyHandle,
    runner_collider: ColliderHandle,
    floor_body: RigidBodyHandle,
    controller: KinematicCharacterController,
    vertical_velocity: f32,
    grounded: bool,
    smoothed_speed_kmh: f32,
    gait_phase: f32,
    foot_anchors: [Vector; 2],
    foot_positions: [Vector; 2],
    previous_pose: PoseState,
    current_pose: PoseState,
    character_target: Vector,
    active_static: HashMap<u64, StaticBody>,
    static_regions: Option<(i64, i64)>,
    dynamic_slots: Vec<DynamicSlot>,
    dynamic_regions: Option<(i64, i64)>,
    diagnostic_drop: bool,
    route_sample_m: f64,
    speed_sample_kmh: f32,
    #[cfg(test)]
    step_displacements: Vec<f32>,
}

impl Physics {
    /// Create a local physics world whose origin is the current absolute route
    /// position.  The optional diagnostic body is save-free and deterministic.
    pub fn new(initial_route_m: f64, diagnostic_drop: bool) -> Self {
        let mut world = PhysicsWorld::new();
        world.gravity = Vector::new(0.0, GRAVITY, 0.0);
        world.integration_parameters.dt = FIXED_DT;
        world.integration_parameters.num_solver_iterations = 8;
        world.integration_parameters.max_ccd_substeps = 4;

        // The visible asphalt has its top at y = 0.02.  It is intentionally
        // bounded; the local origin is re-anchored before this support runs out.
        let (floor_body, _) = world.insert(
            RigidBodyBuilder::fixed().translation(Vector::new(0.0, 0.0, 0.0)),
            ColliderBuilder::cuboid(FLOOR_HALF_X, FLOOR_HALF_THICKNESS, FLOOR_HALF_Z)
                .friction(0.95)
                .restitution(0.0),
        );

        let character_position = Vector::new(0.0, CHARACTER_CENTER_Y, RUNNER_LANE_Z);
        let (runner_body, runner_collider) = world.insert(
            RigidBodyBuilder::kinematic_position_based().translation(character_position),
            ColliderBuilder::capsule_y(CHARACTER_HALF_HEIGHT, CHARACTER_RADIUS)
                .friction(0.0)
                .restitution(0.0),
        );

        let controller = KinematicCharacterController {
            up: Vector::Y,
            offset: CharacterLength::Absolute(0.015),
            slide: true,
            autostep: Some(CharacterAutostep {
                max_height: CharacterLength::Absolute(0.16),
                min_width: CharacterLength::Absolute(0.25),
                include_dynamic_bodies: true,
            }),
            max_slope_climb_angle: 42.0_f32.to_radians(),
            min_slope_slide_angle: 50.0_f32.to_radians(),
            snap_to_ground: Some(CharacterLength::Absolute(0.10)),
            normal_nudge_factor: 1.0e-4,
        };

        let mut physics = Self {
            world,
            accumulator: 0.0,
            origin_m: initial_route_m,
            runner_body,
            runner_collider,
            floor_body,
            controller,
            vertical_velocity: 0.0,
            grounded: true,
            smoothed_speed_kmh: 0.0,
            gait_phase: 0.0,
            foot_anchors: [
                Vector::new(
                    -0.14,
                    GROUND_Y + SHOE_CENTER_ABOVE_FLOOR,
                    RUNNER_LANE_Z + 0.12,
                ),
                Vector::new(
                    0.14,
                    GROUND_Y + SHOE_CENTER_ABOVE_FLOOR,
                    RUNNER_LANE_Z - 0.12,
                ),
            ],
            foot_positions: [
                Vector::new(
                    -0.14,
                    GROUND_Y + SHOE_CENTER_ABOVE_FLOOR,
                    RUNNER_LANE_Z + 0.12,
                ),
                Vector::new(
                    0.14,
                    GROUND_Y + SHOE_CENTER_ABOVE_FLOOR,
                    RUNNER_LANE_Z - 0.12,
                ),
            ],
            previous_pose: PoseState::at(character_position, true),
            current_pose: PoseState::at(character_position, true),
            character_target: character_position,
            active_static: HashMap::new(),
            static_regions: None,
            dynamic_slots: Vec::new(),
            dynamic_regions: None,
            diagnostic_drop,
            route_sample_m: initial_route_m,
            speed_sample_kmh: 0.0,
            #[cfg(test)]
            step_displacements: Vec::new(),
        };

        physics.sync_static_regions(initial_route_m);
        physics.sync_dynamic_pool(initial_route_m);
        physics.refresh_spatial_state();
        physics.current_pose = physics.make_pose();
        physics.previous_pose = physics.current_pose;
        physics
    }

    /// Advance using a render-frame duration.  At most eight 1/60 s steps are
    /// consumed after a hitch; excess time is discarded instead of causing a
    /// long blocking catch-up.
    pub fn advance(&mut self, route_m: f64, speed_kmh: f32, frame_dt: f32) -> usize {
        let route_m = if route_m.is_finite() {
            route_m
        } else {
            self.origin_m
        };
        self.rebase_if_needed(route_m);
        self.sync_static_regions(route_m);
        self.sync_dynamic_pool(route_m);

        let dt = if frame_dt.is_finite() {
            frame_dt.clamp(0.0, MAX_FRAME_DT)
        } else {
            0.0
        };
        let carry = self.accumulator;
        self.accumulator = (self.accumulator + dt).min(FIXED_DT * MAX_CATCH_UP_STEPS as f32);

        let previous_route_m = self.route_sample_m;
        let previous_speed_kmh = self.speed_sample_kmh;
        let mut steps = 0;
        while self.accumulator + 1.0e-6 >= FIXED_DT && steps < MAX_CATCH_UP_STEPS {
            // The step endpoint is measured from the previous render sample.
            // `carry` accounts for time left in the accumulator before this
            // frame, so a 30 Hz frame feeds two evenly spaced targets instead
            // of moving to the final target on the first substep.
            let sample = if dt > 0.0 {
                (((steps + 1) as f32 * FIXED_DT - carry) / dt).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let step_route = lerp_f64(previous_route_m, route_m, sample as f64);
            let step_speed =
                previous_speed_kmh + (speed_kmh.max(0.0) - previous_speed_kmh) * sample;
            self.fixed_step(step_route, step_speed);
            self.accumulator -= FIXED_DT;
            if self.accumulator < 0.0 {
                self.accumulator = 0.0;
            }
            steps += 1;
        }
        self.route_sample_m = route_m;
        self.speed_sample_kmh = speed_kmh.max(0.0);
        steps
    }

    /// Build a renderer-facing interpolation snapshot relative to the same
    /// absolute route coordinate used by the camera.
    pub fn snapshot(&self, camera_m: f64) -> PhysicsSnapshot {
        let alpha = (self.accumulator / FIXED_DT).clamp(0.0, 1.0);
        let pose = interpolate_pose(self.previous_pose, self.current_pose, alpha);
        let runner = RunnerVisual {
            root: self.relative_array(pose.root, camera_m),
            feet: [
                self.relative_array(pose.feet[0], camera_m),
                self.relative_array(pose.feet[1], camera_m),
            ],
            knees: [
                self.relative_array(pose.knees[0], camera_m),
                self.relative_array(pose.knees[1], camera_m),
            ],
            grounded: pose.grounded,
            gait_phase: pose.gait_phase,
            smoothed_speed_kmh: pose.smoothed_speed_kmh,
        };

        let dynamics = self
            .dynamic_slots
            .iter()
            .map(|slot| {
                let transform = interpolate_transform(slot.previous, slot.current, alpha);
                DynamicVisual {
                    id: slot.id,
                    position: self.relative_array(transform.position, camera_m),
                    rotation: quaternion_array(transform.rotation),
                    size: slot.size,
                    kind: slot.kind,
                }
            })
            .collect();

        PhysicsSnapshot {
            runner,
            dynamics,
            alpha,
            origin_m: self.origin_m,
            active_static_colliders: self.active_static.len(),
            dynamic_bodies: self.dynamic_slots.len(),
        }
    }

    fn fixed_step(&mut self, route_m: f64, speed_kmh: f32) {
        self.previous_pose = self.current_pose;
        for slot in &mut self.dynamic_slots {
            slot.previous = slot.current;
        }

        let root = self.world.bodies[self.runner_body].translation();
        let target_x = (route_m - self.origin_m) as f32;
        self.character_target = Vector::new(target_x, CHARACTER_CENTER_Y, RUNNER_LANE_Z);

        if self.grounded {
            // A small downward request keeps contact stable over tiny seams and
            // lets Rapier's snap-to-ground path remain authoritative.
            self.vertical_velocity = -1.0;
        } else {
            self.vertical_velocity += GRAVITY * FIXED_DT;
        }
        let desired = Vector::new(
            self.character_target.x - root.x,
            self.vertical_velocity * FIXED_DT,
            self.character_target.z - root.z,
        );

        let mut collisions = Vec::new();
        let movement = {
            let query = self.world.query_pipeline_with_filter(
                QueryFilter::default().exclude_rigid_body(self.runner_body),
            );
            let shape = self.world.colliders[self.runner_collider].shape();
            let position = *self.world.bodies[self.runner_body].position();
            self.controller
                .move_shape(FIXED_DT, &query, shape, &position, desired, |collision| {
                    collisions.push(collision)
                })
        };
        if !collisions.is_empty() {
            let shape = SharedShape::capsule_y(CHARACTER_HALF_HEIGHT, CHARACTER_RADIUS);
            let mut query = self.world.broad_phase.as_query_pipeline_mut(
                self.world.narrow_phase.query_dispatcher(),
                &mut self.world.bodies,
                &mut self.world.colliders,
                QueryFilter::default().exclude_rigid_body(self.runner_body),
            );
            self.controller.solve_character_collision_impulses(
                FIXED_DT,
                &mut query,
                shape.as_ref(),
                80.0,
                &collisions,
            );
        }
        self.grounded = movement.grounded;
        if self.grounded && self.vertical_velocity < 0.0 {
            self.vertical_velocity = 0.0;
        }

        let next = root + movement.translation;
        self.world.bodies[self.runner_body].set_next_kinematic_translation(next);
        self.world.step();

        self.update_gait(speed_kmh);
        self.current_pose = self.make_pose();
        for slot in &mut self.dynamic_slots {
            slot.current = body_transform(&self.world.bodies[slot.body]);
        }
        #[cfg(test)]
        self.step_displacements.push(next.x - root.x);
    }

    fn update_gait(&mut self, speed_kmh: f32) {
        let smoothing = 1.0 - (-FIXED_DT * 8.0).exp();
        self.smoothed_speed_kmh += (speed_kmh - self.smoothed_speed_kmh) * smoothing;
        let old_phase = self.gait_phase;
        let (stride_m, desired_stance, placement_fraction) =
            gait_parameters(self.smoothed_speed_kmh);
        if self.smoothed_speed_kmh > 0.05 {
            let metres_per_second = self.smoothed_speed_kmh / 3.6;
            self.gait_phase =
                (self.gait_phase + metres_per_second * FIXED_DT / stride_m).rem_euclid(1.0);
        } else if speed_kmh <= 0.05 && self.smoothed_speed_kmh < 0.05 {
            self.gait_phase = 0.0;
        }

        let root = self.world.bodies[self.runner_body].translation();
        if self.smoothed_speed_kmh < 0.05 {
            for leg in 0..2 {
                let side = if leg == 0 { 1.0 } else { -1.0 };
                let x = root.x + if leg == 0 { -0.14 } else { 0.14 };
                let z = RUNNER_LANE_Z + side * 0.12;
                let y = self.floor_height(x, z) + SHOE_CENTER_ABOVE_FLOOR;
                self.foot_anchors[leg] = Vector::new(x, y, z);
                self.foot_positions[leg] = self.foot_anchors[leg];
            }
            return;
        }

        for leg in 0..2 {
            let offset = if leg == 0 { 0.0 } else { 0.5 };
            let old_local = (old_phase + offset).rem_euclid(1.0);
            let local = (self.gait_phase + offset).rem_euclid(1.0);
            let z = RUNNER_LANE_Z + if leg == 0 { 0.12 } else { -0.12 };
            let floor = self.floor_height(root.x, z);
            let hip = Vector::new(root.x, root.y - 0.02, z);
            let planted_y = floor + SHOE_CENTER_ABOVE_FLOOR;
            let reach = horizontal_foot_reach(hip, planted_y, z, LEG_UPPER, LEG_LOWER);
            let stance_distance = (stride_m * desired_stance).min(reach * 1.96);
            let stance_fraction = (stance_distance / stride_m).clamp(0.08, 0.72);
            let placement =
                (stride_m * placement_fraction).clamp(stance_distance - reach * 0.98, reach * 0.98);

            // Swing -> stance: plant once in world coordinates, then keep the
            // anchor fixed while the route/body moves past it.
            if old_local >= stance_fraction && local < stance_fraction {
                let x = root.x + placement;
                self.foot_anchors[leg] =
                    Vector::new(x, self.floor_height(x, z) + SHOE_CENTER_ABOVE_FLOOR, z);
            }

            if local < stance_fraction {
                let anchor = self.foot_anchors[leg];
                let y = self.floor_height(anchor.x, anchor.z) + SHOE_CENTER_ABOVE_FLOOR;
                self.foot_positions[leg] = Vector::new(anchor.x, y, anchor.z);
            } else {
                let swing = ((local - stance_fraction) / (1.0 - stance_fraction)).clamp(0.0, 1.0);
                let smooth = swing * swing * (3.0 - 2.0 * swing);
                let anchor = self.foot_anchors[leg];
                let landing_x = root.x + placement;
                let x = anchor.x + (landing_x - anchor.x) * smooth;
                let floor = self.floor_height(x, z);
                // Mid-swing: marsz unosi stopę tuż nad ziemię, bieg kapuje
                // pięta ku pośladkowi (zgięcie kolana do ~125° w połowie
                // wahadła — Physiopedia "Running Biomechanics").
                let swing_lift =
                    0.075 + 0.265 * smoothstep01((self.smoothed_speed_kmh - 6.0) / 4.0);
                let y = floor
                    + SHOE_CENTER_ABOVE_FLOOR
                    + (std::f32::consts::PI * swing).sin() * swing_lift;
                self.foot_positions[leg] = Vector::new(x, y, z);
            }
        }
    }

    fn make_pose(&self) -> PoseState {
        let physics_root = self.world.bodies[self.runner_body].translation();
        let bob = pose_bob(self.grounded, self.gait_phase, self.smoothed_speed_kmh);
        let root = physics_root - Vector::new(0.0, bob, 0.0);
        let mut feet = self.foot_positions;
        let mut knees = [root; 2];
        for leg in 0..2 {
            let side = if leg == 0 { 1.0 } else { -1.0 };
            let hip = Vector::new(root.x, root.y - 0.02, RUNNER_LANE_Z + side * 0.12);
            feet[leg] = constrain_foot_target(hip, feet[leg], LEG_UPPER, LEG_LOWER);
            knees[leg] = two_bone_knee(hip, feet[leg], LEG_UPPER, LEG_LOWER);
        }
        PoseState {
            root,
            feet,
            knees,
            grounded: self.grounded,
            gait_phase: self.gait_phase,
            smoothed_speed_kmh: self.smoothed_speed_kmh,
        }
    }

    fn floor_height(&self, x: f32, z: f32) -> f32 {
        let ray = Ray::new(Vector::new(x, 8.0, z), Vector::new(0.0, -1.0, 0.0));
        let filter = QueryFilter::default().exclude_rigid_body(self.runner_body);
        self.world
            .query_pipeline_with_filter(filter)
            .cast_ray(&ray, 12.0, true)
            .map(|(_, toi)| 8.0 - toi)
            .unwrap_or(GROUND_Y)
            .max(GROUND_Y)
    }

    fn relative_array(&self, position: Vector, camera_m: f64) -> [f32; 3] {
        [
            (self.origin_m + position.x as f64 - camera_m) as f32,
            position.y,
            position.z,
        ]
    }

    fn rebase_if_needed(&mut self, route_m: f64) {
        if (route_m - self.origin_m).abs() > ORIGIN_REBASE_M {
            self.rebase_to(route_m);
        }
    }

    fn rebase_to(&mut self, new_origin_m: f64) {
        let shift = (self.origin_m - new_origin_m) as f32;
        if shift == 0.0 {
            return;
        }

        if shift.abs() > MAX_SAFE_REBASE_SHIFT_M {
            self.reset_for_large_rebase(new_origin_m);
            return;
        }

        // Static colliders are attached to fixed bodies.  Shifting the body
        // moves each child collider exactly once; no child-local transform is
        // rewritten independently.
        let handles: Vec<_> = self.world.bodies.iter().map(|(handle, _)| handle).collect();
        for handle in handles {
            let body = &mut self.world.bodies[handle];
            let translation = body.translation();
            let linvel = body.linvel();
            let angvel = body.angvel();
            let wake_up = body.is_dynamic() || body.is_kinematic();
            body.set_translation(translation + Vector::new(shift, 0.0, 0.0), wake_up);
            body.set_linvel(linvel, wake_up);
            body.set_angvel(angvel, wake_up);
        }

        shift_pose(&mut self.previous_pose, shift);
        shift_pose(&mut self.current_pose, shift);
        for anchor in &mut self.foot_anchors {
            anchor.x += shift;
        }
        for foot in &mut self.foot_positions {
            foot.x += shift;
        }
        self.character_target.x += shift;
        for slot in &mut self.dynamic_slots {
            slot.previous.position.x += shift;
            slot.current.position.x += shift;
        }

        self.origin_m = new_origin_m;

        // The floor is route-local support, not a collectible/world object.
        // Re-centering this bounded support at the new origin keeps the visible
        // asphalt covered while all persistent object bodies retain their
        // absolute placement after the common shift above.
        self.world.bodies[self.floor_body].set_translation(Vector::new(0.0, 0.0, 0.0), false);
        self.refresh_spatial_state();
    }

    fn reset_for_large_rebase(&mut self, new_origin_m: f64) {
        let static_bodies: Vec<_> = self
            .active_static
            .values()
            .map(|static_body| static_body.body)
            .collect();
        for body in static_bodies {
            self.world.remove_body(body);
        }
        let dynamic_bodies: Vec<_> = self.dynamic_slots.iter().map(|slot| slot.body).collect();
        for body in dynamic_bodies {
            self.world.remove_body(body);
        }
        self.active_static.clear();
        self.dynamic_slots.clear();
        self.static_regions = None;
        self.dynamic_regions = None;

        let runner = self.world.bodies[self.runner_body].translation();
        let local_runner = Vector::new(0.0, runner.y, RUNNER_LANE_Z);
        self.world.bodies[self.runner_body].set_translation(local_runner, true);
        self.world.bodies[self.runner_body].set_next_kinematic_translation(local_runner);
        self.world.bodies[self.floor_body].set_translation(Vector::ZERO, false);
        self.origin_m = new_origin_m;
        self.character_target = local_runner;
        self.route_sample_m = new_origin_m;
        self.speed_sample_kmh = 0.0;
        self.vertical_velocity = 0.0;
        self.grounded = true;
        self.smoothed_speed_kmh = 0.0;
        self.gait_phase = 0.0;
        self.foot_anchors = [
            Vector::new(
                -0.14,
                GROUND_Y + SHOE_CENTER_ABOVE_FLOOR,
                RUNNER_LANE_Z + 0.12,
            ),
            Vector::new(
                0.14,
                GROUND_Y + SHOE_CENTER_ABOVE_FLOOR,
                RUNNER_LANE_Z - 0.12,
            ),
        ];
        self.foot_positions = self.foot_anchors;
        self.previous_pose = PoseState::at(local_runner, self.grounded);
        self.current_pose = self.previous_pose;
        self.refresh_spatial_state();
    }

    fn sync_static_regions(&mut self, route_m: f64) {
        let range = (
            region_of(route_m - ACTIVE_BACK_M),
            region_of(route_m + ACTIVE_FORWARD_M),
        );
        if self.static_regions == Some(range) {
            return;
        }

        let mut desired = HashSet::new();
        let mut descriptions = Vec::new();
        for region in range.0..=range.1 {
            let description = world::region_description(region);
            for object in description.objects {
                if object.collider == ColliderKind::Static && static_shape(&object).is_some() {
                    desired.insert(object.id);
                    descriptions.push(object);
                }
            }
        }

        let removed: Vec<_> = self
            .active_static
            .iter()
            .filter_map(|(id, body)| (!desired.contains(id)).then_some(body.body))
            .collect();
        for body in removed {
            self.world.remove_body(body);
        }
        self.active_static.retain(|id, _| desired.contains(id));

        for object in descriptions {
            if self.active_static.contains_key(&object.id) {
                continue;
            }
            let body_position = Vector::new(
                (object.position.along_m - self.origin_m) as f32,
                0.0,
                object.position.lateral_m,
            );
            let body = self
                .world
                .bodies
                .insert(RigidBodyBuilder::fixed().translation(body_position));
            let collider = static_shape(&object)
                .expect("static_shape was checked before insertion")
                .user_data(object.id as u128)
                .friction(0.85)
                .restitution(0.0);
            self.world
                .colliders
                .insert_with_parent(collider, body, &mut self.world.bodies);
            self.active_static.insert(object.id, StaticBody { body });
        }
        self.static_regions = Some(range);
        self.refresh_spatial_state();
    }

    fn sync_dynamic_pool(&mut self, route_m: f64) {
        let range = (
            region_of(route_m - ACTIVE_BACK_M),
            region_of(route_m + ACTIVE_FORWARD_M),
        );
        if self.dynamic_regions == Some(range) {
            return;
        }

        let mut spawns = Vec::new();
        for region in range.0..=range.1 {
            spawns.extend(dynamic_spawns(region));
        }
        if self.diagnostic_drop {
            spawns.push(DynamicSpawn {
                id: 0xD1A6_0500,
                absolute_m: route_m + 2.4,
                lateral_m: 1.75,
                height_m: 3.8,
                size: [0.48, 0.48, 0.48],
                kind: 0,
            });
        }
        spawns.sort_by_key(|spawn| spawn.id);
        spawns.truncate(MAX_DYNAMIC_OBJECTS);
        let desired: HashSet<u64> = spawns.iter().map(|spawn| spawn.id).collect();

        let removed: Vec<_> = self
            .dynamic_slots
            .iter()
            .filter_map(|slot| (!desired.contains(&slot.id)).then_some(slot.body))
            .collect();
        for body in removed {
            self.world.remove_body(body);
        }
        self.dynamic_slots.retain(|slot| desired.contains(&slot.id));

        for spawn in spawns {
            if self.dynamic_slots.iter().any(|slot| slot.id == spawn.id) {
                continue;
            }
            let position = Vector::new(
                (spawn.absolute_m - self.origin_m) as f32,
                spawn.height_m,
                spawn.lateral_m,
            );
            let body_builder = RigidBodyBuilder::dynamic()
                .translation(position)
                .linear_damping(0.16)
                .angular_damping(0.34)
                .ccd_enabled(true)
                .can_sleep(true);
            let collider_builder = dynamic_shape(spawn)
                .user_data(spawn.id as u128)
                .friction(0.72)
                .restitution(0.24);
            let (body, _collider) = self.world.insert(body_builder, collider_builder);
            let transform = body_transform(&self.world.bodies[body]);
            self.dynamic_slots.push(DynamicSlot {
                id: spawn.id,
                body,
                size: spawn.size,
                kind: spawn.kind,
                previous: transform,
                current: transform,
            });
        }
        self.dynamic_regions = Some(range);
        self.refresh_spatial_state();
    }

    fn refresh_spatial_state(&mut self) {
        // `detect_collisions` refreshes queries but deliberately does not
        // register newly inserted dynamic/kinematic bodies in island state.
        // A zero-duration PhysicsWorld step performs both the broad-phase
        // refresh and Rapier's normal user-change/island bookkeeping without
        // advancing time.
        let dt = self.world.integration_parameters.dt;
        self.world.integration_parameters.dt = 0.0;
        self.world.step();
        self.world.integration_parameters.dt = dt;
    }
}

fn static_shape(object: &ObjectDesc) -> Option<ColliderBuilder> {
    let shape = match object.geometry {
        Geometry::Fir { scale } => {
            let height = 1.6 * scale;
            ColliderBuilder::capsule_y(height * 0.5, 0.18 * scale).translation(Vector::new(
                0.0,
                height * 0.5,
                0.0,
            ))
        }
        Geometry::Bush { radius } => {
            ColliderBuilder::ball(radius).translation(Vector::new(0.0, radius * 0.6, 0.0))
        }
        Geometry::House {
            height,
            half_x,
            half_z,
            ..
        } => ColliderBuilder::cuboid(half_x, height * 0.5, half_z).translation(Vector::new(
            0.0,
            height * 0.5,
            0.0,
        )),
        Geometry::SettlementSign { .. } => {
            ColliderBuilder::cuboid(0.10, 1.1, 0.65).translation(Vector::new(0.0, 1.1, 0.0))
        }
        Geometry::Lamp { .. } => ColliderBuilder::capsule_y(world::LAMP_POLE_HEIGHT_M * 0.5, 0.07)
            .translation(Vector::new(0.0, world::LAMP_POLE_HEIGHT_M * 0.5, 0.0)),
        Geometry::Signal { .. } => {
            ColliderBuilder::capsule_y(1.75, 0.08).translation(Vector::new(0.0, 1.75, 0.0))
        }
        Geometry::Mushroom { .. } | Geometry::Animal { .. } => return None,
        Geometry::Rock { scale } => {
            let radius = 0.45 * scale.max(0.35);
            ColliderBuilder::ball(radius).translation(Vector::new(0.0, radius * 0.62, 0.0))
        }
        // Kwiaty są dekoracją (kolidery None w generacji), parkan leży
        // poza pasem biegu — oba bez zderzenia jak grzyby/zwierzę.
        Geometry::FlowerPatch { .. } | Geometry::FenceSegment { .. } => return None,
    };
    Some(shape)
}

fn dynamic_spawns(region: i64) -> Vec<DynamicSpawn> {
    let mut rng = Rng::new(region as u64 ^ 0xD1A6_0501);
    let mut out = Vec::new();
    for index in 0..3_u64 {
        let h = rng.next();
        let absolute_m = region as f64 * REGION_M + 24.0 + (h % 250) as f64;
        let side = if h & 1 == 0 { -1.0 } else { 1.0 };
        // Objects sit on the verge rather than all sharing the runner lane.
        let lateral_m = side * (3.45 + ((h >> 12) % 13) as f32 * 0.12);
        let kind = ((h >> 24) % 3) as u8;
        let size = match kind {
            0 => [0.42, 0.42, 0.42],
            1 => [0.52, 0.52, 0.52],
            _ => [0.34, 0.62, 0.34],
        };
        out.push(DynamicSpawn {
            id: hash1(region as u64 ^ 0xD1A6_05FF ^ index),
            absolute_m,
            lateral_m,
            height_m: 2.0 + ((h >> 32) % 180) as f32 / 100.0,
            size,
            kind,
        });
    }
    out
}

fn dynamic_shape(spawn: DynamicSpawn) -> ColliderBuilder {
    match spawn.kind {
        0 => ColliderBuilder::cuboid(
            spawn.size[0] * 0.5,
            spawn.size[1] * 0.5,
            spawn.size[2] * 0.5,
        ),
        1 => ColliderBuilder::ball(spawn.size[0] * 0.5),
        _ => ColliderBuilder::capsule_y(
            (spawn.size[1] - spawn.size[0]).max(0.04) * 0.5,
            spawn.size[0] * 0.5,
        ),
    }
}

fn lerp_f64(a: f64, b: f64, alpha: f64) -> f64 {
    a + (b - a) * alpha
}

fn smoothstep01(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

/// `stride_m` is the distance covered by one complete cycle of the same leg.
/// The two feet are half a cycle apart, so total footfalls are twice the cycle
/// rate rather than twice the route speed divided by a single footfall.
fn gait_parameters(speed_kmh: f32) -> (f32, f32, f32) {
    let running = smoothstep01((speed_kmh - 6.0) / 4.0);
    let walking_stride = (1.10 + speed_kmh * 0.065).clamp(1.10, 1.62);
    let running_stride = (1.70 + (speed_kmh - 8.0).max(0.0) * 0.19).clamp(1.70, 4.0);
    let stride = walking_stride + (running_stride - walking_stride) * running;
    let stance_fraction = 0.54 + (0.14 - 0.54) * running;
    let placement_fraction = 0.18 + (0.07 - 0.18) * running;
    (stride, stance_fraction, placement_fraction)
}

fn pose_bob(grounded: bool, gait_phase: f32, smoothed_speed_kmh: f32) -> f32 {
    if grounded && smoothed_speed_kmh > 0.05 {
        (gait_phase * std::f32::consts::TAU * 2.0).sin().abs() * 0.045
    } else {
        0.0
    }
}

fn horizontal_foot_reach(hip: Vector, foot_y: f32, foot_z: f32, upper: f32, lower: f32) -> f32 {
    let max_distance = upper + lower - 1.0e-4;
    let dy = foot_y - hip.y;
    let dz = foot_z - hip.z;
    (max_distance * max_distance - dy * dy - dz * dz)
        .max(0.0)
        .sqrt()
}

/// Keep a floor-planted target inside the two-bone annulus without lifting
/// its shoe. Normal gait targets only need the horizontal x adjustment; the
/// final fallback also handles arbitrary diagnostic inputs safely.
fn constrain_foot_target(hip: Vector, foot: Vector, upper: f32, lower: f32) -> Vector {
    let offset = foot - hip;
    let distance = offset.length();
    let min_distance = (upper - lower).abs() + 1.0e-4;
    let max_distance = upper + lower - 1.0e-4;
    if (min_distance..=max_distance).contains(&distance) {
        return foot;
    }

    let yz_squared = offset.y * offset.y + offset.z * offset.z;
    let max_horizontal_squared = max_distance * max_distance - yz_squared;
    if max_horizontal_squared >= 0.0 {
        let max_horizontal = max_horizontal_squared.sqrt();
        let x = offset.x.clamp(-max_horizontal, max_horizontal);
        let mut result = Vector::new(hip.x + x, foot.y, foot.z);
        let result_distance = (result - hip).length();
        if result_distance < min_distance {
            let min_horizontal_squared = min_distance * min_distance - yz_squared;
            if min_horizontal_squared > 0.0 {
                let min_horizontal = min_horizontal_squared.sqrt();
                let sign = if offset.x < 0.0 { -1.0 } else { 1.0 };
                result.x = hip.x + sign * min_horizontal;
            }
        }
        return result;
    }

    let direction = if distance > 1.0e-5 {
        offset / distance
    } else {
        Vector::Y
    };
    hip + direction * distance.clamp(min_distance, max_distance)
}

fn body_transform(body: &RigidBody) -> BodyTransform {
    BodyTransform {
        position: body.translation(),
        rotation: *body.rotation(),
    }
}

fn shift_pose(pose: &mut PoseState, shift: f32) {
    pose.root.x += shift;
    for point in &mut pose.feet {
        point.x += shift;
    }
    for point in &mut pose.knees {
        point.x += shift;
    }
}

fn interpolate_vec(a: Vector, b: Vector, alpha: f32) -> Vector {
    a + (b - a) * alpha
}

fn interpolate_pose(a: PoseState, b: PoseState, alpha: f32) -> PoseState {
    let root = interpolate_vec(a.root, b.root, alpha);
    let mut feet = [
        interpolate_vec(a.feet[0], b.feet[0], alpha),
        interpolate_vec(a.feet[1], b.feet[1], alpha),
    ];
    let mut knees = [root; 2];
    for leg in 0..2 {
        let side = if leg == 0 { 1.0 } else { -1.0 };
        let hip = Vector::new(root.x, root.y - 0.02, RUNNER_LANE_Z + side * 0.12);
        feet[leg] = constrain_foot_target(hip, feet[leg], LEG_UPPER, LEG_LOWER);
        knees[leg] = two_bone_knee(hip, feet[leg], LEG_UPPER, LEG_LOWER);
    }
    PoseState {
        root,
        feet,
        knees,
        grounded: if alpha < 0.5 { a.grounded } else { b.grounded },
        gait_phase: lerp_wrapped(a.gait_phase, b.gait_phase, alpha),
        smoothed_speed_kmh: a.smoothed_speed_kmh
            + (b.smoothed_speed_kmh - a.smoothed_speed_kmh) * alpha,
    }
}

fn lerp_wrapped(a: f32, b: f32, alpha: f32) -> f32 {
    let mut delta = b - a;
    if delta > 0.5 {
        delta -= 1.0;
    } else if delta < -0.5 {
        delta += 1.0;
    }
    (a + delta * alpha).rem_euclid(1.0)
}

fn interpolate_rotation(a: Rotation, b: Rotation, alpha: f32) -> Rotation {
    let q = a + (b - a) * alpha;
    if q.length_squared() > 1.0e-8 {
        q.normalize()
    } else {
        Rotation::IDENTITY
    }
}

fn interpolate_transform(a: BodyTransform, b: BodyTransform, alpha: f32) -> BodyTransform {
    BodyTransform {
        position: interpolate_vec(a.position, b.position, alpha),
        rotation: interpolate_rotation(a.rotation, b.rotation, alpha),
    }
}

fn quaternion_array(rotation: Rotation) -> [f32; 4] {
    [rotation.x, rotation.y, rotation.z, rotation.w]
}

fn two_bone_knee(hip: Vector, foot: Vector, upper: f32, lower: f32) -> Vector {
    let offset = foot - hip;
    let true_distance = offset.length();
    let distance = true_distance.clamp((upper - lower).abs() + 1.0e-4, upper + lower - 1.0e-4);
    let direction = if true_distance > 1.0e-5 {
        offset / true_distance
    } else {
        Vector::Y
    };
    let along = (upper * upper - lower * lower + distance * distance) / (2.0 * distance);
    let bend_height = (upper * upper - along * along).max(0.0).sqrt();
    // The knee bends toward the forward (route +x) side of the character.
    let mut bend = Vector::new(-direction.y, direction.x, 0.0);
    if bend.length_squared() < 1.0e-8 {
        bend = Vector::X;
    } else {
        bend = bend.normalize();
    }
    if bend.x < 0.0 {
        bend = -bend;
    }
    hip + direction * along + bend * bend_height
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_world() -> PhysicsWorld {
        let mut world = PhysicsWorld::new();
        world.gravity = Vector::new(0.0, GRAVITY, 0.0);
        world.integration_parameters.dt = FIXED_DT;
        world.integration_parameters.max_ccd_substeps = 4;
        world.integration_parameters.num_solver_iterations = 8;
        world.insert(
            RigidBodyBuilder::fixed(),
            ColliderBuilder::cuboid(20.0, FLOOR_HALF_THICKNESS, 20.0),
        );
        world.detect_collisions(&(), &());
        world
    }

    #[test]
    fn dropped_body_accelerates_and_settles_without_tunnelling() {
        let mut world = test_world();
        let (body, _) = world.insert(
            RigidBodyBuilder::dynamic()
                .translation(Vector::new(0.0, 8.0, 0.0))
                .ccd_enabled(true),
            ColliderBuilder::ball(0.35).friction(0.8).restitution(0.12),
        );
        let start = world.bodies[body].translation().y;
        for _ in 0..240 {
            world.step();
        }
        let end = world.bodies[body].translation().y;
        assert!(end < start - 1.0, "body did not fall: {start} -> {end}");
        assert!(end > 0.30 && end < 0.43, "body tunneled or floated: {end}");
        assert!(world.bodies[body].linvel().y.abs() < 0.25);
    }

    #[test]
    fn character_stays_grounded_and_wall_stops_corrected_movement() {
        let mut world = test_world();
        let (wall_body, _) = world.insert(
            RigidBodyBuilder::fixed().translation(Vector::new(1.0, 1.0, 0.0)),
            ColliderBuilder::cuboid(0.08, 1.0, 2.0),
        );
        let (character_body, character_collider) = world.insert(
            RigidBodyBuilder::kinematic_position_based().translation(Vector::new(
                0.0,
                CHARACTER_CENTER_Y,
                0.0,
            )),
            ColliderBuilder::capsule_y(CHARACTER_HALF_HEIGHT, CHARACTER_RADIUS),
        );
        let controller = KinematicCharacterController {
            snap_to_ground: Some(CharacterLength::Absolute(0.1)),
            ..Default::default()
        };
        let mut grounded_frames = 0;
        let mut wall_x = 0.0;
        for _ in 0..120 {
            let current = *world.bodies[character_body].position();
            let movement = {
                let queries = world.query_pipeline_with_filter(
                    QueryFilter::default().exclude_rigid_body(character_body),
                );
                controller.move_shape(
                    FIXED_DT,
                    &queries,
                    world.colliders[character_collider].shape(),
                    &current,
                    Vector::new(2.0 * FIXED_DT, -FIXED_DT, 0.0),
                    |_| {},
                )
            };
            if movement.grounded {
                grounded_frames += 1;
            }
            let next = world.bodies[character_body].translation() + movement.translation;
            world.bodies[character_body].set_next_kinematic_translation(next);
            world.step();
            wall_x = world.bodies[character_body].translation().x;
        }
        assert!(grounded_frames > 100, "character lost ground too often");
        assert!(
            wall_x < 1.0 - CHARACTER_RADIUS + 0.02,
            "wall crossed: {wall_x}"
        );
        assert!(wall_x > 0.45, "character never approached wall: {wall_x}");
        let _ = wall_body;
    }

    #[test]
    fn integrated_runner_stays_grounded_at_rest_and_walking_speed() {
        let mut physics = Physics::new(0.0, false);
        for _ in 0..120 {
            physics.advance(0.0, 0.0, FIXED_DT);
            assert!(physics.snapshot(0.0).runner.grounded);
        }
        let metres_per_step = 20.0 / 3.6 * FIXED_DT;
        let mut route = 0.0;
        for _ in 0..240 {
            route += metres_per_step as f64;
            physics.advance(route, 20.0, FIXED_DT);
            assert!(physics.snapshot(route).runner.grounded);
        }
    }

    fn fixed_fps_outcome(fps: u32) -> [f32; 3] {
        let mut physics = Physics::new(0.0, true);
        for _ in 0..(fps * 2) {
            physics.advance(0.0, 0.0, 1.0 / fps as f32);
        }
        physics
            .snapshot(0.0)
            .dynamics
            .iter()
            .find(|body| body.id == 0xD1A6_0500)
            .expect("diagnostic body")
            .position
    }

    fn isolate_floor(physics: &mut Physics) {
        let static_bodies: Vec<_> = physics
            .active_static
            .values()
            .map(|static_body| static_body.body)
            .collect();
        for body in static_bodies {
            physics.world.remove_body(body);
        }
        physics.active_static.clear();
        physics.refresh_spatial_state();
    }

    #[test]
    fn fixed_step_is_stable_at_30_60_and_144_render_fps() {
        let a = fixed_fps_outcome(30);
        let b = fixed_fps_outcome(60);
        let c = fixed_fps_outcome(144);
        for (left, right) in [(&a, &b), (&a, &c)] {
            let error = (left[1] - right[1]).abs()
                + (left[0] - right[0]).abs()
                + (left[2] - right[2]).abs();
            assert!(
                error < 0.025,
                "render FPS changed fixed outcome: {left:?} vs {right:?}"
            );
        }
    }

    #[test]
    fn moving_character_has_consistent_fixed_step_velocity_at_render_fps() {
        fn steps_at(fps: u32) -> Vec<f32> {
            let speed = 20.0_f32;
            let mut physics = Physics::new(0.0, false);
            isolate_floor(&mut physics);
            let mut route = 0.0_f64;
            let mut displacements = Vec::new();
            for _ in 0..fps * 2 {
                route += speed as f64 / 3.6 / fps as f64;
                physics.advance(route, speed, 1.0 / fps as f32);
                displacements.extend(std::mem::take(&mut physics.step_displacements));
            }
            assert!(physics.current_pose.root.x <= route as f32 + 1.0e-4);
            assert!(physics.current_pose.root.x >= route as f32 - 0.05);
            assert!(physics.grounded);
            displacements
        }

        let expected = 20.0 / 3.6 * FIXED_DT;
        for (fps, displacements) in [(30, steps_at(30)), (60, steps_at(60)), (144, steps_at(144))] {
            assert!(displacements.len() > 100, "too few fixed steps at {fps} Hz");
            for displacement in displacements {
                assert!(
                    (displacement - expected).abs() < 0.01,
                    "{fps} Hz produced a non-uniform fixed-step movement: {displacement}"
                );
            }
        }
    }

    #[test]
    fn moving_character_stops_at_the_authoritative_route_without_overshoot() {
        let speed = 4.0_f64;
        let mut physics = Physics::new(0.0, false);
        let mut route = 0.0;
        for _ in 0..120 {
            route += speed / 3.6 * FIXED_DT as f64;
            physics.advance(route, speed as f32, FIXED_DT);
        }
        let stop_route = route;
        for _ in 0..180 {
            physics.advance(stop_route, 0.0, FIXED_DT);
        }
        let root = physics.current_pose.root.x;
        assert!(root <= stop_route as f32 + 1.0e-4);
        assert!(
            (stop_route as f32 - root).abs() < 0.01,
            "route drift: {root}"
        );
        assert!(physics.grounded);
    }

    #[test]
    fn leg_lengths_and_ground_contact_stay_valid_through_the_full_gait() {
        for speed in [0.0_f32, 4.0, 20.0] {
            let mut physics = Physics::new(0.0, false);
            let mut route = 0.0_f64;
            for _ in 0..600 {
                route += speed as f64 / 3.6 * FIXED_DT as f64;
                physics.advance(route, speed, FIXED_DT);
                let pose = physics.current_pose;
                let physical_root = physics.world.bodies[physics.runner_body].translation();
                let expected_bob =
                    pose_bob(pose.grounded, pose.gait_phase, pose.smoothed_speed_kmh);
                assert!((pose.root.y - (physical_root.y - expected_bob)).abs() < 1.0e-5);
                for leg in 0..2 {
                    let side = if leg == 0 { 1.0 } else { -1.0 };
                    let hip =
                        Vector::new(pose.root.x, pose.root.y - 0.02, RUNNER_LANE_Z + side * 0.12);
                    let upper = (pose.knees[leg] - hip).length();
                    let lower = (pose.feet[leg] - pose.knees[leg]).length();
                    assert!(
                        (upper - LEG_UPPER).abs() < 0.001,
                        "upper leg at {speed}: {upper}"
                    );
                    assert!(
                        (lower - LEG_LOWER).abs() < 0.001,
                        "lower leg at {speed}: {lower}"
                    );
                    let floor = physics.floor_height(pose.feet[leg].x, pose.feet[leg].z);
                    assert!(
                        pose.feet[leg].y >= floor + SHOE_CENTER_ABOVE_FLOOR - 0.001,
                        "shoe penetrated floor at {speed}: {} vs {floor}",
                        pose.feet[leg].y
                    );
                }
                assert!(pose.grounded, "character lost ground at {speed}");
            }
            if speed == 0.0 {
                for foot in physics.current_pose.feet {
                    let floor = physics.floor_height(foot.x, foot.z);
                    assert!((foot.y - floor - SHOE_CENTER_ABOVE_FLOOR).abs() < 0.001);
                }
            }
        }
    }

    #[test]
    fn unreachable_knee_target_is_constrained_before_rendering() {
        let hip = Vector::new(0.0, 0.98, RUNNER_LANE_Z);
        let requested_foot = Vector::new(2.0, GROUND_Y + SHOE_CENTER_ABOVE_FLOOR, RUNNER_LANE_Z);
        let foot = constrain_foot_target(hip, requested_foot, LEG_UPPER, LEG_LOWER);
        let knee = two_bone_knee(hip, foot, LEG_UPPER, LEG_LOWER);
        assert!(((knee - hip).length() - LEG_UPPER).abs() < 0.001);
        assert!(((foot - knee).length() - LEG_LOWER).abs() < 0.001);
        assert!((foot.y - requested_foot.y).abs() < 1.0e-6);
    }

    #[test]
    fn gait_uses_full_cycle_step_lengths_at_walking_and_running_speeds() {
        let (walk_stride, _, _) = gait_parameters(4.0);
        let (run_stride, _, _) = gait_parameters(20.0);
        assert!(run_stride > walk_stride * 2.0);
        let run_footfalls_per_minute = 2.0 * (20.0 / 3.6) / run_stride * 60.0;
        assert!((120.0..=220.0).contains(&run_footfalls_per_minute));
    }

    #[test]
    fn dynamic_capsule_collider_matches_its_declared_dimensions() {
        let spawn = DynamicSpawn {
            id: 7,
            absolute_m: 0.0,
            lateral_m: 0.0,
            height_m: 1.0,
            size: [0.34, 0.62, 0.34],
            kind: 2,
        };
        let collider = dynamic_shape(spawn).build();
        let capsule = collider.shape().as_capsule().expect("capsule collider");
        assert!((capsule.radius - spawn.size[0] * 0.5).abs() < 1.0e-6);
        assert!((capsule.segment.length() - (spawn.size[1] - spawn.size[0])).abs() < 1.0e-6);
    }

    #[test]
    fn rebasing_preserves_absolute_pose_velocity_and_ground_contact() {
        let origin = 1_000_000_000.0;
        let mut physics = Physics::new(origin, true);
        isolate_floor(&mut physics);
        for _ in 0..180 {
            physics.advance(origin, 0.0, FIXED_DT);
        }
        let slot = physics
            .dynamic_slots
            .iter()
            .find(|slot| slot.id == 0xD1A6_0500)
            .expect("diagnostic body");
        let body_handle = slot.body;
        let before_local = physics.world.bodies[body_handle].translation();
        let before_velocity = physics.world.bodies[body_handle].linvel();
        let before_absolute = physics.origin_m + before_local.x as f64;
        let before_previous = physics.previous_pose;
        let before_current = physics.current_pose;
        let before_target = physics.character_target;
        physics.rebase_to(origin + 256.0);
        let after = physics.world.bodies[body_handle].translation();
        let after_absolute = physics.origin_m + after.x as f64;
        assert!((before_absolute - after_absolute).abs() < 0.001);
        assert!((before_velocity - physics.world.bodies[body_handle].linvel()).length() < 0.001);
        assert!((after.y - before_local.y).abs() < 0.001);
        assert!((after.x - (before_local.x - 256.0)).abs() < 0.001);
        assert!((physics.previous_pose.root.x - (before_previous.root.x - 256.0)).abs() < 0.001);
        assert!((physics.current_pose.root.x - (before_current.root.x - 256.0)).abs() < 0.001);
        assert!((physics.character_target.x - (before_target.x - 256.0)).abs() < 0.001);
        // The direct rebase call models a new authoritative route sample;
        // production rebases happen inside `advance` before this interpolation.
        physics.route_sample_m = origin;
        for _ in 0..4 {
            physics.advance(origin, 0.0, FIXED_DT);
        }
        assert!(
            physics.current_pose.grounded,
            "lost contact after rebase: root={:?} body={:?}",
            physics.current_pose.root,
            physics.world.bodies[physics.runner_body].translation()
        );
    }

    #[test]
    fn large_diagnostic_teleport_resets_old_local_objects_safely() {
        let mut physics = Physics::new(0.0, true);
        physics.advance(0.0, 0.0, FIXED_DT);
        physics.advance(1_000_000_000.0, 0.0, FIXED_DT);
        assert_eq!(physics.origin_m, 1_000_000_000.0);
        assert!(
            physics.world.bodies[physics.runner_body]
                .translation()
                .x
                .abs()
                < 0.1
        );
        let diagnostic = physics
            .dynamic_slots
            .iter()
            .find(|slot| slot.id == 0xD1A6_0500)
            .expect("new local diagnostic body");
        let body_x = physics.world.bodies[diagnostic.body].translation().x;
        assert!((body_x - 2.4).abs() < 0.1);
        assert!(physics.current_pose.grounded);
    }

    #[test]
    fn dynamic_bodies_continue_settling_when_route_speed_is_zero() {
        let mut physics = Physics::new(0.0, true);
        let initial_y = physics.snapshot(0.0).dynamics[0].position[1];
        for _ in 0..12 {
            physics.advance(0.0, 0.0, 0.5);
        }
        let settled_y = physics.snapshot(0.0).dynamics[0].position[1];
        assert!(
            settled_y < initial_y - 0.5,
            "body did not settle while stopped"
        );
    }

    #[test]
    fn active_body_and_collider_pools_stay_bounded_across_regions() {
        let mut physics = Physics::new(0.0, false);
        for region in 0..200_i64 {
            let route = region as f64 * REGION_M + 1.0;
            physics.advance(route, 20.0, 0.2);
            assert!(physics.dynamic_slots.len() <= MAX_DYNAMIC_OBJECTS);
            assert!(physics.active_static.len() <= 160);
        }
    }
}
