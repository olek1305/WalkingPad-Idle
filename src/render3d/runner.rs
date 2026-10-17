//! Sylwetka biegacza. Rysowanie i rzucanie cienia korzystają z tej samej
//! funkcji, więc późniejszy pass cieni nie dostanie innego modelu niż kamera.

use macroquad::prelude::*;

use crate::physics::{RunnerVisual, SHOE_SIZE};

use super::PI;
use super::geometry::Bld;
use super::material::{MaterialSpec, SurfaceKind, surface_preset};

const ARM_SHOULDER_LATERAL: f32 = 0.29;
const ARM_ELBOW_LATERAL: f32 = 0.41;
const ARM_HAND_LATERAL: f32 = 0.44;
const ARM_FOREARM_RADIUS: f32 = 0.09 * 0.5;
const ARM_HAND_RADIUS: f32 = 0.105;
const ARM_UPPER_BONE_LENGTH: f32 = 0.35;
const ARM_FOREARM_BONE_LENGTH: f32 = 0.29;
const ARM_WALK_SPEED_KMH: f32 = 4.2;
const ARM_RUN_BLEND_START_KMH: f32 = 7.0;
const ARM_RUN_BLEND_END_KMH: f32 = 10.0;
const ARM_MAX_VISUAL_SPEED_KMH: f32 = 20.0;
const ARM_UPPER_WALK_SWING: f32 = 0.055;
const ARM_UPPER_RUN_SWING: f32 = 0.095;
const ARM_FOREARM_WALK_SWING: f32 = 0.025;
const ARM_FOREARM_RUN_SWING: f32 = 0.050;
const ARM_FOREARM_FORWARD_BIAS: f32 = 0.085;
const ARM_FOREARM_RUN_FORWARD_BIAS: f32 = 0.015;

#[derive(Clone, Copy, Debug, PartialEq)]
struct ArmPose {
    upper: Vec3,
    elbow: Vec3,
    hand: Vec3,
}

fn visual_speed(speed: f32) -> f32 {
    if speed.is_finite() {
        speed.clamp(0.0, ARM_MAX_VISUAL_SPEED_KMH)
    } else {
        0.0
    }
}

fn gait_angle(gait: f32) -> f32 {
    if gait.is_finite() {
        gait.rem_euclid(std::f32::consts::TAU)
    } else {
        0.0
    }
}

fn smoothstep01(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

fn torso_pose(root: Vec3, _speed: f32) -> (Vec3, Vec3, Vec3) {
    // Bez pochylenia: przy szym Simmons wysokościach 0.22 robiło złamany tors
    // (fizyka nóg zostaje pionowa) — biegacz stoi prosto.
    let torso_hip = root + Vec3::new(0.0, -0.02, 0.0);
    let shoulder = root + Vec3::new(0.10, 0.55, 0.0);
    (
        torso_hip + Vec3::new(0.0, 0.04, 0.0),
        shoulder,
        shoulder + Vec3::new(-0.02, -0.04, 0.0),
    )
}

fn fixed_bone_offset(length: f32, lateral: f32, forward: f32) -> Vec3 {
    let lateral_squared = lateral * lateral;
    let planar_squared = (length * length - lateral_squared).max(0.0);
    let planar = planar_squared.sqrt();
    let forward = forward.clamp(-planar, planar);
    let downward = (planar_squared - forward * forward).max(0.0).sqrt();
    Vec3::new(forward, -downward, lateral)
}

fn arm_pose(shoulder: Vec3, gait: f32, side: f32, speed: f32) -> ArmPose {
    let gait = gait_angle(gait);
    let speed = visual_speed(speed);
    let walk_amount = smoothstep01(speed / ARM_WALK_SPEED_KMH);
    let run_amount = smoothstep01(
        (speed - ARM_RUN_BLEND_START_KMH) / (ARM_RUN_BLEND_END_KMH - ARM_RUN_BLEND_START_KMH),
    );
    let side = if side < 0.0 { -1.0 } else { 1.0 };
    let upper_swing = ARM_UPPER_WALK_SWING * walk_amount + ARM_UPPER_RUN_SWING * run_amount;
    let forearm_swing = ARM_FOREARM_WALK_SWING * walk_amount + ARM_FOREARM_RUN_SWING * run_amount;
    let upper_forward = gait.sin() * upper_swing;
    let forearm_forward = ARM_FOREARM_FORWARD_BIAS
        + ARM_FOREARM_RUN_FORWARD_BIAS * run_amount
        + (gait - 0.45).sin() * forearm_swing;
    let upper = shoulder + Vec3::new(0.0, -0.05, side * ARM_SHOULDER_LATERAL);
    let elbow = upper
        + fixed_bone_offset(
            ARM_UPPER_BONE_LENGTH,
            side * (ARM_ELBOW_LATERAL - ARM_SHOULDER_LATERAL),
            upper_forward,
        );
    let hand = elbow
        + fixed_bone_offset(
            ARM_FOREARM_BONE_LENGTH,
            side * (ARM_HAND_LATERAL - ARM_ELBOW_LATERAL),
            forearm_forward,
        );
    ArmPose { upper, elbow, hand }
}

fn leg_hip(root: Vec3, leg: usize) -> Vec3 {
    let side = if leg == 0 { 1.0 } else { -1.0 };
    root + Vec3::new(0.0, -0.02, side * 0.12)
}

pub(crate) fn draw_runner(builder: &mut Bld, pose: RunnerVisual) {
    runner_body(builder, pose);
}

fn runner_body(builder: &mut Bld, pose: RunnerVisual) {
    let phase = gait_angle(pose.gait_phase * std::f32::consts::TAU);
    let speed = visual_speed(pose.smoothed_speed_kmh);
    // Physics owns the visual bob and solves the knees against this exact
    // root. Keeping one posed snapshot also keeps the shadow triangles exact.
    let root = Vec3::from_array(pose.root);
    let (_, shoulder, torso_top) = torso_pose(root, speed);

    let cloth = MaterialSpec::from_surface(surface_preset(SurfaceKind::Cloth, 0.0));
    let skin = MaterialSpec::from_surface(surface_preset(SurfaceKind::Skin, 0.0));
    let shoe = MaterialSpec::from_surface(surface_preset(SurfaceKind::Metal, 0.0));

    // Spójne, rozpoznawalne ubranie: niebieskie spodnie i bluza; kolor jest
    // bazowym sRGB, a nie wynikiem przedwypalonego światła.
    let trousers = Color::new(0.08, 0.20, 0.52, 1.0);
    let jacket = Color::new(0.08, 0.52, 0.70, 1.0);
    let shoe_color = Color::new(0.93, 0.73, 0.14, 1.0);
    let skin_color = Color::new(0.82, 0.50, 0.32, 1.0);

    // Szorty odchylone od spodni + odblaskowy pas: detale czytelne z tyłu,
    // skąd kamera patrzy na biegacza.
    let shorts = Color::new(0.06, 0.15, 0.38, 1.0);
    let reflect = Color::new(0.62, 0.96, 0.42, 1.0);

    builder.with_material(cloth, |builder| {
        for i in 0..2 {
            // Rapier solves each knee from this side-specific hip. Reusing
            // the same lateral offset here prevents the rendered thigh from
            // shearing back to the centre hip.
            let hip = leg_hip(root, i);
            let foot = Vec3::from_array(pose.feet[i]);
            let knee = Vec3::from_array(pose.knees[i]);
            // szorty kończą się w połowie uda
            builder.capsule_smooth(hip, hip + (knee - hip) * 0.5, 0.128, shorts);
            builder.capsule_smooth(hip, knee, 0.105, trousers);
            builder.capsule_smooth(knee, foot, 0.082, trousers);
            builder.with_material(shoe, |builder| {
                let shoe_center = foot + Vec3::new(0.08, 0.0, -0.015);
                builder.cube(shoe_center, Vec3::from_array(SHOE_SIZE), shoe_color);
                // biała podeszwa lekko wystająca poza bryłę buta
                builder.cube(
                    Vec3::new(
                        shoe_center.x + 0.005,
                        shoe_center.y - SHOE_SIZE[1] * 0.30,
                        shoe_center.z,
                    ),
                    Vec3::new(
                        SHOE_SIZE[0] + 0.012,
                        SHOE_SIZE[1] * 0.44,
                        SHOE_SIZE[2] + 0.008,
                    ),
                    Color::new(0.93, 0.93, 0.89, 1.0),
                );
            });
        }
        // Tułów zamiast jednej golawej kapsuły: miednica (szorty) → talia →
        // klatka + nawinięte barki. Radii ≤ 0.24 w strefie ramion, więc
        // asercje przyczepu rąk pozostają nadzorem.
        builder.sphere(root + Vec3::new(0.0, 0.06, 0.0), 0.245, 9, 18, shorts);
        builder.capsule_smooth(
            root + Vec3::new(0.0, 0.15, 0.0),
            root + Vec3::new(0.04, 0.36, 0.0),
            0.212,
            jacket,
        );
        builder.capsule_smooth(root + Vec3::new(0.04, 0.36, 0.0), torso_top, 0.248, jacket);
        // odblaskowy pasek przecina oba boki korpusu (+/- z)
        builder.cube(
            root + Vec3::new(0.09, 0.40, 0.0),
            Vec3::new(0.15, 0.065, 0.56),
            reflect,
        );
        for side in [-1.0_f32, 1.0] {
            builder.sphere(
                shoulder + Vec3::new(0.0, -0.02, side * 0.215),
                0.145,
                8,
                14,
                jacket,
            );
        }
    });

    // Ręce mają ubrany odcinek ramienny, a osobne dłonie zawsze używają skóry.
    for i in 0..2 {
        let gait = phase + i as f32 * PI;
        let side = if i == 0 { 1.0 } else { -1.0 };
        let arm = arm_pose(shoulder, gait, side, speed);
        builder.with_material(cloth, |builder| {
            // rękaw lekko opuffany: 6.2 cm zamiast 5.75
            builder.capsule_smooth(arm.upper, arm.elbow, 0.062, jacket);
        });
        builder.with_material(skin, |builder| {
            builder.capsule_smooth(arm.elbow, arm.hand, ARM_FOREARM_RADIUS, skin_color);
            builder.sphere(arm.hand, ARM_HAND_RADIUS, 7, 12, skin_color);
        });
    }

    builder.with_material(skin, |builder| {
        let neck = shoulder + Vec3::new(0.0, 0.04, 0.0);
        builder.capsule_smooth(neck, neck + Vec3::new(0.03, 0.10, 0.0), 0.10, skin_color);
        builder.sphere(
            shoulder + Vec3::new(0.04, 0.24, 0.0),
            0.19,
            10,
            16,
            skin_color,
        );
    });

    builder.with_material(cloth, |builder| {
        let head = shoulder + Vec3::new(0.04, 0.24, 0.0);
        builder.sphere(
            head + Vec3::new(0.0, 0.19, 0.0),
            0.205,
            8,
            14,
            Color::new(0.08, 0.10, 0.18, 1.0),
        );
        builder.cube(
            head + Vec3::new(0.12, 0.08, 0.0),
            Vec3::new(0.34, 0.055, 0.38),
            Color::new(0.06, 0.08, 0.15, 1.0),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::{GROUND_Y, SHOE_CENTER_ABOVE_FLOOR};

    // Alias testowy 1:1 z poprzednim layoutem tułowia; radii biegacza są
    // dziś ułożone pod tors budowany z miednicy/waist/chest, ale czytelność
    // asercji pozostaje przy tej samej wartości bazowej.
    const TORSO_RADIUS: f32 = 0.24;
    const ARM_UPPER_RADIUS: f32 = 0.057_5;

    #[test]
    fn shoe_bottom_uses_the_physics_foot_center() {
        let foot = Vec3::new(1.0, GROUND_Y + SHOE_CENTER_ABOVE_FLOOR, -1.0);
        let center = foot + Vec3::new(0.08, 0.0, -0.015);
        assert_eq!(center.y, foot.y);
        assert!((center.y - SHOE_SIZE[1] * 0.5 - GROUND_Y).abs() < 1.0e-6);
    }

    #[test]
    fn rendered_thigh_hips_match_the_two_physics_lateral_sides() {
        let root = Vec3::new(3.0, 1.0, -1.15);
        let left = leg_hip(root, 0);
        let right = leg_hip(root, 1);
        assert_eq!(left.y, root.y - 0.02);
        assert_eq!(right.y, root.y - 0.02);
        assert!((left.z - (-1.03)).abs() < 1.0e-6);
        assert!((right.z - (-1.27)).abs() < 1.0e-6);
    }

    fn distance_to_segment(point: Vec3, a: Vec3, b: Vec3) -> f32 {
        let ab = b - a;
        let t = ((point - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        point.distance(a + ab * t)
    }

    fn assert_segment_clears_torso(
        name: &str,
        a: Vec3,
        b: Vec3,
        radius: f32,
        torso_a: Vec3,
        torso_b: Vec3,
    ) {
        for sample in 2..=10 {
            let t = sample as f32 / 10.0;
            let center = a.lerp(b, t);
            let clearance = distance_to_segment(center, torso_a, torso_b) - TORSO_RADIUS - radius;
            assert!(
                clearance > 0.0,
                "{name} intersects torso after attachment area at t={t}: clearance={clearance}, center={center:?}"
            );
        }
    }

    fn assert_finite_pose(arm: ArmPose) {
        for point in [arm.upper, arm.elbow, arm.hand] {
            assert!(point.x.is_finite() && point.y.is_finite() && point.z.is_finite());
        }
    }

    fn pose_distance(a: ArmPose, b: ArmPose) -> f32 {
        a.upper
            .distance(b.upper)
            .max(a.elbow.distance(b.elbow))
            .max(a.hand.distance(b.hand))
    }

    #[test]
    fn arm_bones_keep_length_and_side_anchors_through_full_gait() {
        let root = Vec3::new(0.0, 1.0, -1.15);
        let speeds = [0.0, 0.049, 0.05, 0.051, 0.10, 4.2, 7.0, 8.5, 10.0, 20.0];
        for speed in speeds {
            let (_, shoulder, _) = torso_pose(root, speed);
            for sample in 0..128 {
                let phase = sample as f32 / 128.0 * std::f32::consts::TAU;
                for (side, gait) in [(1.0, phase), (-1.0, phase + PI)] {
                    let arm = arm_pose(shoulder, gait, side, speed);
                    assert_finite_pose(arm);
                    assert!(
                        (arm.upper - shoulder).distance(Vec3::new(
                            0.0,
                            -0.05,
                            side * ARM_SHOULDER_LATERAL,
                        )) < 1.0e-6
                    );
                    assert_eq!((arm.upper.z - root.z).signum(), side);
                    assert_eq!((arm.elbow.z - root.z).signum(), side);
                    assert_eq!((arm.hand.z - root.z).signum(), side);
                    assert!((arm.elbow.z - (root.z + side * ARM_ELBOW_LATERAL)).abs() < 1.0e-6);
                    assert!((arm.hand.z - (root.z + side * ARM_HAND_LATERAL)).abs() < 1.0e-6);
                    assert!((arm.upper.distance(arm.elbow) - ARM_UPPER_BONE_LENGTH).abs() < 2.0e-5);
                    assert!(
                        (arm.elbow.distance(arm.hand) - ARM_FOREARM_BONE_LENGTH).abs() < 2.0e-5
                    );
                }
            }
        }
    }

    #[test]
    fn arms_attach_clear_the_torso_and_keep_hands_at_hip_level() {
        let root = Vec3::new(0.0, 1.0, -1.15);
        let speeds = [0.0, 0.049, 0.05, 0.051, 0.10, 4.2, 7.0, 8.5, 10.0, 20.0];
        for speed in speeds {
            let (torso_a, shoulder, torso_b) = torso_pose(root, speed);
            for sample in 0..128 {
                let phase = sample as f32 / 128.0 * std::f32::consts::TAU;
                for (side, gait) in [(1.0, phase), (-1.0, phase + PI)] {
                    let arm = arm_pose(shoulder, gait, side, speed);
                    let attachment_clearance = distance_to_segment(arm.upper, torso_a, torso_b)
                        - TORSO_RADIUS
                        - ARM_UPPER_RADIUS;
                    assert!(
                        attachment_clearance < 0.0,
                        "{side} shoulder sleeve detached: clearance={attachment_clearance}"
                    );
                    assert_segment_clears_torso(
                        "upper arm",
                        arm.upper,
                        arm.elbow,
                        ARM_UPPER_RADIUS,
                        torso_a,
                        torso_b,
                    );
                    assert_segment_clears_torso(
                        "forearm",
                        arm.elbow,
                        arm.hand,
                        ARM_FOREARM_RADIUS,
                        torso_a,
                        torso_b,
                    );
                    let hand_clearance = distance_to_segment(arm.hand, torso_a, torso_b)
                        - TORSO_RADIUS
                        - ARM_HAND_RADIUS;
                    assert!(
                        hand_clearance > 0.0,
                        "hand intersects torso: clearance={hand_clearance}"
                    );
                }
            }
        }

        let (_, shoulder, _) = torso_pose(root, 0.0);
        let resting = arm_pose(shoulder, 0.0, 1.0, 0.0);
        assert!(resting.hand.y > root.y - 0.20 && resting.hand.y < root.y + 0.10);
    }

    #[test]
    fn arms_have_opposed_bounded_swing_and_forward_elbow_flexion() {
        let root = Vec3::new(0.0, 1.0, -1.15);
        let (_, shoulder, _) = torso_pose(root, 20.0);
        let max_swing = |speed: f32| {
            (0..128).fold(0.0_f32, |maximum, sample| {
                let phase = sample as f32 / 128.0 * std::f32::consts::TAU;
                let arm = arm_pose(shoulder, phase, 1.0, speed);
                maximum.max((arm.elbow.x - arm.upper.x).abs())
            })
        };
        let walk_max = max_swing(4.2);
        let run_max = max_swing(20.0);
        for speed in [4.2, 7.0, 8.5, 10.0, 20.0] {
            for sample in 0..128 {
                let phase = sample as f32 / 128.0 * std::f32::consts::TAU;
                let right = arm_pose(shoulder, phase, 1.0, speed);
                let left = arm_pose(shoulder, phase + PI, -1.0, speed);
                let right_swing = right.elbow.x - right.upper.x;
                let left_swing = left.elbow.x - left.upper.x;
                assert!((right_swing + left_swing).abs() < 1.0e-5);
                assert!(right.hand.x > right.elbow.x);
                assert!(left.hand.x > left.elbow.x);

                for arm in [right, left] {
                    let shoulder_vector = arm.upper - arm.elbow;
                    let forearm_vector = arm.hand - arm.elbow;
                    let bend_dot = shoulder_vector.dot(forearm_vector);
                    assert!(
                        bend_dot > -ARM_UPPER_BONE_LENGTH * ARM_FOREARM_BONE_LENGTH * 0.98,
                        "arm is nearly hyperextended: dot={bend_dot}"
                    );

                    let upper_vector = arm.elbow - arm.upper;
                    let theta_upper = upper_vector.x.atan2(-upper_vector.y);
                    let theta_fore = forearm_vector.x.atan2(-forearm_vector.y);
                    let bend_angle = theta_fore - theta_upper;
                    assert!(
                        bend_angle > 0.0 && bend_angle < 0.75,
                        "arm bend is outside the forward anatomical range: bend={bend_angle}"
                    );
                }
            }
        }
        assert!(run_max > walk_max * 2.0);
        assert!(run_max <= ARM_UPPER_WALK_SWING + ARM_UPPER_RUN_SWING + 1.0e-5);
    }

    #[test]
    fn arm_pose_is_continuous_at_wrap_speed_gate_and_walk_run_transition() {
        let root = Vec3::new(0.0, 1.0, -1.15);
        let (_, shoulder, _) = torso_pose(root, 8.5);
        let epsilon = 1.0e-4;
        for speed in [0.0, 0.049, 0.05, 0.051, 4.2, 7.0, 8.5, 10.0, 20.0] {
            for side in [1.0, -1.0] {
                let before_wrap = arm_pose(shoulder, std::f32::consts::TAU - epsilon, side, speed);
                let after_wrap = arm_pose(shoulder, epsilon, side, speed);
                assert!(pose_distance(before_wrap, after_wrap) < 2.0e-4);
            }
        }

        for (before, after) in [(0.049, 0.051), (6.999, 7.001), (9.999, 10.001)] {
            let a = arm_pose(shoulder, 1.3, 1.0, before);
            let b = arm_pose(shoulder, 1.3, 1.0, after);
            assert!(pose_distance(a, b) < 2.0e-4);
        }

        let mut previous = arm_pose(shoulder, 0.8, 1.0, 0.0);
        for speed in [0.0001, 0.01, 0.049, 0.05, 0.051, 0.01, 0.0] {
            let current = arm_pose(shoulder, 0.8, 1.0, speed);
            assert_finite_pose(current);
            assert!(pose_distance(previous, current) < 0.01);
            previous = current;
        }
    }

    #[test]
    fn nonfinite_motion_inputs_fall_back_to_a_finite_pose() {
        let root = Vec3::new(0.0, 1.0, -1.15);
        let (_, shoulder, _) = torso_pose(root, f32::NAN);
        let arm = arm_pose(shoulder, f32::NAN, 1.0, f32::INFINITY);
        assert_finite_pose(arm);
        assert!((arm.upper.distance(arm.elbow) - ARM_UPPER_BONE_LENGTH).abs() < 2.0e-5);
        assert!((arm.elbow.distance(arm.hand) - ARM_FOREARM_BONE_LENGTH).abs() < 2.0e-5);
    }
}
