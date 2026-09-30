//! The drive view, without a GPU: controls, the seat on the banking, the head, the frame.

use super::*;
use sim_physics::TrackDef;

fn charlotte() -> Track {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/race/tracks/charlotte.ron");
    let def: TrackDef = ron::from_str(&std::fs::read_to_string(path).expect("the track file")).expect("a TrackDef");
    Track::new(&def).expect("it closes")
}

fn controls() -> Controls {
    let axis = |key: Option<&str>, neg: Option<&str>, pos: Option<&str>, sens: Option<(f32, f32)>| Axis {
        key: key.map(str::to_string),
        neg: neg.map(str::to_string),
        pos: pos.map(str::to_string),
        rise_ms: 200.0,
        fall_ms: 100.0,
        speed_sensitive: sens,
        scale: 1000.0,
    };
    Controls {
        action: "drive".into(),
        axes: BTreeMap::from([
            ("throttle".to_string(), axis(Some("up"), None, None, None)),
            ("steer".to_string(), axis(None, Some("right"), Some("left"), Some((60.0, 0.1)))),
        ]),
        buttons: Vec::new(),
    }
}

#[test]
fn a_held_key_ramps_the_pedal_and_letting_go_brings_it_back_faster() {
    let c = controls();
    let mut a = Axes::default();
    let held = BTreeSet::from(["up".to_string()]);
    a.update(&c, &held, 0.1);
    assert!((a.values["throttle"] - 0.5).abs() < 1e-4, "half way in 100 of 200 ms: {:?}", a.values);
    a.update(&c, &held, 0.2);
    assert_eq!(a.args(&c, 0.0)["throttle"], 1000, "full after its rise time");
    a.update(&c, &BTreeSet::new(), 0.05);
    assert_eq!(a.args(&c, 0.0)["throttle"], 500, "half gone after 50 of 100 ms");
}

#[test]
fn steering_reverses_through_zero_and_is_gentler_at_speed() {
    let c = controls();
    let mut a = Axes::default();
    let left = BTreeSet::from(["left".to_string()]);
    a.update(&c, &left, 1.0);
    assert_eq!(a.args(&c, 0.0)["steer"], 1000, "full left at a standstill");
    assert_eq!(a.args(&c, 60.0)["steer"], 100, "a tenth of it at 60 m/s");
    assert_eq!(a.args(&c, 30.0)["steer"], 550, "in between, in between");
    // Swapping to right goes back to zero at the fall rate, then out at the rise rate.
    let right = BTreeSet::from(["right".to_string()]);
    a.update(&c, &right, 0.1);
    assert!(a.values["steer"].abs() < 1e-4, "back to straight in 100 ms: {:?}", a.values);
    a.update(&c, &right, 0.1);
    assert!((a.values["steer"] + 0.5).abs() < 1e-4, "{:?}", a.values);
}

fn car_at(track: &Track, s: f32, offset: f32, yaw_off: f32) -> CarView {
    let c = centre(track, s);
    let (sn, cs) = c.heading.sin_cos();
    CarView {
        id: 1,
        you: true,
        x: c.x - offset * sn,
        y: c.y + offset * cs,
        yaw: c.heading + yaw_off,
        speed: 70.0,
        rpm: Some(8500.0),
        gear: Some(5),
        steer: 0.0,
        throttle: 1.0,
        brake: 0.0,
        lap: None,
        lap_ms: None,
        last_lap_ms: None,
        best_lap_ms: None,
        position: None,
        number: 24,
        impact: 0.0,
        line: None,
        accel: None,
        body: None,
        s,
        offset,
        seg: 0,
    }
}

#[test]
fn a_car_sits_on_the_banking_facing_any_way_and_its_up_leans_into_the_turn() {
    let t = charlotte();
    let look = Look::default();
    let g = scene::ground(&t, &look);
    // In the middle of turns 1-2 the track is banked 24°: the car's up leans 24° toward the infield (its left).
    let mid = 550.0;
    assert!((centre(&t, mid).bank.to_degrees() - 24.0).abs() < 0.1);
    for yaw_off in [0.0, 0.3, -0.3, std::f32::consts::PI] {
        let c = car_at(&t, mid, 0.0, yaw_off);
        let fr = seat(&t, &g, &c, (0.0, 0.0));
        let tilt = fr.u.dot(V3(0.0, 1.0, 0.0)).acos().to_degrees();
        assert!((tilt - 24.0).abs() < 0.6, "facing {yaw_off}: up tilts {tilt}°");
        // Forward follows the yaw (projected onto the surface), right completes the frame.
        let fh = V3(c.yaw.cos(), 0.0, c.yaw.sin());
        assert!(fr.f.dot(fh) > 0.9, "facing {yaw_off}");
        assert!(fr.r.dot(fr.f).abs() < 1e-4 && fr.r.dot(fr.u).abs() < 1e-4);
        // The seat is on the surface: its origin at the ground's height there.
        let h = g.height(&centre(&t, mid), 0.0);
        assert!((fr.o.1 - h).abs() < 1e-3);
    }
    // Facing the way of travel on the banking, up leans left (the infield), so the car's right side is higher.
    let fr = seat(&t, &g, &car_at(&t, mid, 0.0, 0.0), (0.0, 0.0));
    assert!(fr.r.1 > 0.3, "right side up: {:?}", fr.r);
}

fn composer<'a>(d: &'a Drive, t: &'a Track, g: &'a Ground) -> Composer<'a> {
    Composer { drive: d, track: t, ground: g, photos: &photos::NONE, car: None, sun: scene::sun_dir(&d.look), w: 1600.0, h: 900.0 }
}

fn drive() -> Drive {
    ron::from_str("(cars: \"car\")").expect("a drive view with defaults")
}

#[test]
fn in_a_banked_turn_the_horizon_tilts_by_half_the_banking_and_the_head_stays_small() {
    let t = charlotte();
    let d = drive();
    let g = scene::ground(&t, &d.look);
    let c = composer(&d, &t, &g);
    let mut rig = Rig::default();
    let car = car_at(&t, 550.0, 0.0, 0.0);
    // A steady turn at 70 m/s on its radius: yaw rate v / R, sideways pull v² / R.
    let r = 208.8;
    let kin = Kin { speed: 70.0, yaw_rate: 70.0 / r, a_long: 0.0, a_lat: 70.0 * 70.0 / r };
    let mut eye = None;
    for i in 0..240 {
        eye = Some(c.camera(&car, &kin, &mut rig, i as f32 / 60.0, 1.0 / 60.0).0);
    }
    let eye = eye.unwrap();
    let o = rig.out;
    // The banking, less the body rolling toward the outside under the sideways g (about 2° at 1.8 g).
    assert!(o.car_roll.abs() > 20.5 && o.car_roll.abs() < 23.5, "the car rolls with the banking: {}", o.car_roll);
    assert!((o.roll - o.car_roll * 0.5).abs() < 0.5, "the neck takes half back: {} of {}", o.roll, o.car_roll);
    assert!((eye.roll - o.roll).abs() < 1.0, "the eye rolls as the rig says (vibration aside)");
    // Bounded: centimetres, not decimetres, even at over 2 g.
    let head = (o.head.dot(o.head)).sqrt();
    assert!(head > 0.001 && head < 0.05, "head moves {head} m at {:?} g", o.felt);
    assert!(o.head.0 > 0.0, "a left turn pushes the head right: {:?}", o.head);
    // The eyes go a few degrees into the turn (left).
    assert!(o.look > 1.0 && o.look <= d.cockpit.head.look_max, "looks {}°", o.look);
}

#[test]
fn a_frame_has_the_cockpit_the_mirror_and_the_sky() {
    let t = charlotte();
    let d = drive();
    let g = scene::ground(&t, &d.look);
    let c = composer(&d, &t, &g);
    let mut rig = Rig::default();
    let cars = vec![car_at(&t, 100.0, 0.0, 0.0), CarView { id: 2, you: false, ..car_at(&t, 130.0, 2.0, 0.0) }];
    let f = c.compose(&cars, &Kin::default(), &Hud::default(), &mut rig, 0.0, 1.0 / 60.0);
    assert!(f.mirror.is_some(), "the mirror's own picture is asked for");
    assert!(f.meshes.iter().any(|m| m.image == MIRROR && !m.verts.is_empty()), "and hung in the cockpit");
    assert!(f.mirror_meshes < f.meshes.len() && f.meshes[..f.mirror_meshes].iter().all(|m| m.image != MIRROR), "never in itself");
    assert!(!f.back.is_empty(), "sky");
    // The car ahead is in view: its middle projects inside the screen.
    let ahead = seat(&t, &g, &cars[1], (0.0, 0.0));
    let (x, y) = f.eye.project(ahead.o, 1600.0, 900.0).expect("in front");
    assert!((0.0..1600.0).contains(&x) && (0.0..900.0).contains(&y), "at {x}, {y}");
    // From outside, no cockpit and no mirror.
    rig.view = 1;
    let chase = c.compose(&cars, &Kin::default(), &Hud::default(), &mut rig, 0.0, 1.0 / 60.0);
    assert!(chase.mirror.is_none());
}
