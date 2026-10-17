//! Sylwetka biegacza. Rysowanie i rzucanie cienia korzystają z tej samej
//! funkcji, więc późniejszy pass cieni nie dostanie innego modelu niż kamera.
//!
//! Kanon proporcji z deep researchu (revisiting Vitruvian Man — JAMA 2020,
//! antropometria lotnictwa: głowa 1/8 wysokości, barki ~1/4, krocze ~1/2;
//! 7.5-8 główek łącznie; biomechanika biegu: Physiopedia "Running
//! Biomechanics", Runner's World arm-swing guide, Yegian & Lieberman 2019
//! "Straight arm walking, bent arm running", Pontzer "Control and Function
//! of Arm Swing in Human Walking and Running"):

use macroquad::prelude::*;

use crate::physics::{RunnerVisual, SHOE_SIZE};

use super::PI;
use super::geometry::Bld;
use super::material::{MaterialSpec, SurfaceKind, surface_preset};

// ---- Ramiona: proporcje anatomiczne -------------------------------------
const ARM_SHOULDER_LATERAL: f32 = 0.165; // środek głowy kości ramiennej
const ARM_ELBOW_LATERAL: f32 = 0.19; // łokieć lekko na zewnątrz linii barków
const ARM_FOREARM_RADIUS: f32 = 0.084 * 0.5;
const ARM_HAND_RADIUS: f32 = 0.052; // zaciśnięta pięść, nie piłka golfowa
const ARM_UPPER_BONE_LENGTH: f32 = 0.33; // bark→łokieć
const ARM_FOREARM_BONE_LENGTH: f32 = 0.27; // łokieć→kciuk bazowy
// Chód: ramię prawie proste, bieg: ~90° w łokciu, trzymane przez cały cykl
const ARM_UPPER_WALK_SWING: f32 = 0.17;
const ARM_UPPER_RUN_SWING: f32 = 0.40;
const ARM_STAND_ELBOW_FLEXION: f32 = 0.26;
const ARM_WALK_ELBOW_DELTA: f32 = 0.20;
const ARM_RUN_ELBOW_DELTA: f32 = 0.99; // razem w biegu: ~1.45 rad (~83°)
const ARM_WALK_SPEED_KMH: f32 = 4.2;
const ARM_RUN_BLEND_START_KMH: f32 = 7.0;
const ARM_RUN_BLEND_END_KMH: f32 = 10.0;
const ARM_MAX_VISUAL_SPEED_KMH: f32 = 20.0;

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

/// Kąty wyliczone: ~4-10° pochylenia tułowia w biegu (Sagittal trunk kinematics,
/// Medicine & Science in Sports & Exercise 2015). Tułów lekko sztywny: barki
/// ~7.5° przed kośćmi miednicy, nigdy "na złamany" komentarz.
fn torso_pose(root: Vec3, _speed: f32) -> (Vec3, Vec3, Vec3) {
    let torso_hip = root + Vec3::new(0.0, -0.02, 0.0);
    let shoulder = root + Vec3::new(0.075, 0.55, 0.0);
    (
        torso_hip + Vec3::new(0.0, 0.04, 0.0),
        shoulder,
        shoulder + Vec3::new(-0.02, -0.04, 0.0),
    )
}

fn arm_pose(shoulder: Vec3, gait: f32, side: f32, speed: f32) -> ArmPose {
    let gait = gait_angle(gait);
    let speed = visual_speed(speed);
    let walk_amount = smoothstep01(speed / ARM_WALK_SPEED_KMH);
    let run_amount = smoothstep01(
        (speed - ARM_RUN_BLEND_START_KMH) / (ARM_RUN_BLEND_END_KMH - ARM_RUN_BLEND_START_KMH),
    );
    let side = if side < 0.0 { -1.0 } else { 1.0 };

    // Biegacz pompuje rękami dokładnie w fazie przeciwnej do nogi: maksimum
    // ramienia w tył = moment postawienia stopy tej samej strony w przód.
    // Blend amplitudy: przy staniu ~0, przy chodzie 0.17 rad, w biegu pełne
    // 0.40 rad (długość łuku od linii bioder do linii klatki).
    let swing = ARM_UPPER_WALK_SWING * walk_amount
        + (ARM_UPPER_RUN_SWING - ARM_UPPER_WALK_SWING) * run_amount;
    let elbow_flexion = ARM_STAND_ELBOW_FLEXION
        + ARM_WALK_ELBOW_DELTA * walk_amount
        + ARM_RUN_ELBOW_DELTA * run_amount;

    let upper_angle = -swing * gait.cos();
    let upper = shoulder + Vec3::new(0.0, -0.05, side * ARM_SHOULDER_LATERAL);
    // Długość kości musi być dokładna mimo drobnego odsunięcia na zewnątrz
    // (test pilnuje 1:1 długości), więc część XY kwadruje się do reszty.
    let upper_z = side * (ARM_ELBOW_LATERAL - ARM_SHOULDER_LATERAL);
    let upper_planar = (ARM_UPPER_BONE_LENGTH * ARM_UPPER_BONE_LENGTH - upper_z * upper_z)
        .max(0.0)
        .sqrt();
    let upper_bone = Vec3::new(
        upper_planar * upper_angle.sin(),
        -upper_planar * upper_angle.cos(),
        upper_z,
    );
    let elbow = upper + upper_bone;

    // Łokieć to zawias w jednej płaszczyźnie XY (Zero dryfu z-z, jak w badaniach
    // arm swing: przedramię odbite pod stałym kątem do ramienia). Gdy ramię
    // kopie w tył, dłoń ląduje przy linii bioder; kiedy w przód — podnosi się
    // nad klatkę. To daje klasyczny pump biegacza w obu kierunkach.
    let flexion_cos = elbow_flexion.cos();
    let flexion_sin = elbow_flexion.sin();
    let forearm_xy = Vec3::new(
        flexion_cos * upper_bone.x - flexion_sin * upper_bone.y,
        flexion_sin * upper_bone.x + flexion_cos * upper_bone.y,
        0.0,
    );
    let planar = forearm_xy.truncate().length().max(1.0e-6);
    let hand = elbow
        + Vec3::new(
            forearm_xy.x / planar * ARM_FOREARM_BONE_LENGTH,
            forearm_xy.y / planar * ARM_FOREARM_BONE_LENGTH,
            0.0,
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
    let (_, shoulder, _) = torso_pose(root, speed);

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
            // szorty łączą się z miednicą, kończą wyżej niż kolano
            builder.capsule_smooth(hip, hip + (knee - hip) * 0.4, 0.106, shorts);
            builder.capsule_smooth(hip, knee, 0.090, trousers);
            builder.capsule_smooth(knee, foot, 0.068, trousers);
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
        // Tułów z anatomicznymi proporcjami (JAMA 2020 / antropometria): klatka
        // ~34-35 cm szeroka przy ~22 cm głębokości, talia zwężona, miednica
        // szersza niż talia. Elipsoidy zamiast kul — człowiek nie jest beczką.
        builder.ellipsoid(
            root + Vec3::new(0.01, 0.055, 0.0),
            Vec3::new(0.095, 0.13, 0.142),
            9,
            16,
            shorts,
        );
        builder.ellipsoid(
            root + Vec3::new(0.035, 0.22, 0.0),
            Vec3::new(0.088, 0.115, 0.118),
            9,
            16,
            jacket,
        );
        builder.ellipsoid(
            root + Vec3::new(0.075, 0.40, 0.0),
            Vec3::new(0.112, 0.155, 0.16),
            10,
            18,
            jacket,
        );
        // odblaskowy pas przecina oba boki korpusu (+/- z)
        builder.cube(
            root + Vec3::new(0.075, 0.40, 0.0),
            Vec3::new(0.16, 0.06, 0.36),
            reflect,
        );
        for side in [-1.0_f32, 1.0] {
            builder.sphere(
                shoulder + Vec3::new(-0.005, -0.025, side * 0.115),
                0.085,
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
            builder.capsule_smooth(arm.upper, arm.elbow, 0.055, jacket);
        });
        builder.with_material(skin, |builder| {
            builder.capsule_smooth(arm.elbow, arm.hand, ARM_FOREARM_RADIUS, skin_color);
            builder.sphere(arm.hand, ARM_HAND_RADIUS, 7, 12, skin_color);
        });
    }

    builder.with_material(skin, |builder| {
        // Szyja: ~10 cm średnicy i WIDOCZNA między zwężeniem klatki a głową
        // (wcześniej głowa przylegała bezpośrednio do bryły piersiowej).
        let neck_base = root + Vec3::new(0.05, 0.53, 0.0);
        let neck_top = root + Vec3::new(0.06, 0.63, 0.0);
        builder.capsule_smooth(neck_base, neck_top, 0.052, skin_color);
        // Głowa: 1/7.5 wysokości ciała (średni człowiek: 7-7.5 główek).
        // Elipsoida: głębsza (x) niż szersza (z), jak prawdziwa czaszka,
        // nie piłka.
        builder.ellipsoid(
            root + Vec3::new(0.08, 0.72, 0.0),
            Vec3::new(0.105, 0.126, 0.084),
            10,
            16,
            skin_color,
        );
    });

    builder.with_material(cloth, |builder| {
        let head = root + Vec3::new(0.08, 0.72, 0.0);
        // Czapka = kopuła na samej czaszce + daszek przed buzią, a nie balon
        builder.sphere(
            head + Vec3::new(0.0, 0.04, 0.0),
            0.108,
            8,
            14,
            Color::new(0.08, 0.10, 0.18, 1.0),
        );
        builder.cube(
            head + Vec3::new(0.075, 0.085, 0.0),
            Vec3::new(0.15, 0.024, 0.185),
            Color::new(0.06, 0.08, 0.15, 1.0),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::{GROUND_Y, SHOE_CENTER_ABOVE_FLOOR};

    // Pół szerokości klatki elipsoidy — limit, żeby pompa rąk nie wchodziła
    // w bryłę klatki.
    const TORSO_HALF_WIDTH: f32 = 0.16;

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
                    // łokciowa płaszczyzna = finalna dłoń (brak dryfu z-z):
                    assert!((arm.hand.z - (root.z + side * ARM_ELBOW_LATERAL)).abs() < 1.0e-6);
                    assert!((arm.upper.distance(arm.elbow) - ARM_UPPER_BONE_LENGTH).abs() < 2.0e-5);
                    assert!(
                        (arm.elbow.distance(arm.hand) - ARM_FOREARM_BONE_LENGTH).abs() < 2.0e-5
                    );
                }
            }
        }
    }

    #[test]
    fn arms_hug_the_slim_torso_without_swallowing_it() {
        let root = Vec3::new(0.0, 1.0, -1.15);
        let speeds = [0.0, 0.05, 4.2, 7.0, 8.5, 10.0, 20.0];
        for speed in speeds {
            let (torso_a, shoulder, torso_b) = torso_pose(root, speed);
            for sample in 0..128 {
                let phase = sample as f32 / 128.0 * std::f32::consts::TAU;
                for (side, gait) in [(1.0, phase), (-1.0, phase + PI)] {
                    let arm = arm_pose(shoulder, gait, side, speed);
                    // Ludzka ręka ociera o klatkę: rękaw przylega do torsu
                    // (całe ramię wisi przy tułowiu), ale nie może odpłynąć
                    // poza bryłę barków.
                    let attach = distance_to_segment(arm.upper, torso_a, torso_b);
                    assert!(
                        attach < 0.21,
                        "{side} shoulder sleeve detached from the chest: {attach}"
                    );
                    // przedramię i dłoń zostają poza bryłą klatki (|z|):
                    assert!(
                        arm.elbow.z.abs() > TORSO_HALF_WIDTH + 0.01,
                        "forearm sank into the chest: {arm:?}"
                    );
                    assert!(
                        arm.hand.z.abs() > TORSO_HALF_WIDTH + 0.01,
                        "hand sank into the chest: {arm:?}"
                    );
                    // Dłoń podniesiona ponad linię bioder zawsze zostaje przed
                    // mostkiem (klasyczny pump: żadne "ręce za plecami").
                    if arm.hand.y > root.y + 0.15 {
                        assert!(
                            arm.hand.x > shoulder.x + 0.02,
                            "hand parked behind the chest: {arm:?}"
                        );
                    }
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
                        bend_angle > 0.0 && bend_angle < 1.62,
                        "arm bend is outside the anatomical range (~90 deg in run): bend={bend_angle}"
                    );
                }
            }
        }
        assert!(run_max > walk_max * 2.0);
        assert!(run_max <= ARM_UPPER_BONE_LENGTH * ARM_UPPER_RUN_SWING + 1.0e-5);
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
