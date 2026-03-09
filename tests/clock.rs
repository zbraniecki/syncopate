use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use syncopate::*;

#[test]
fn sim_clock_starts_at_zero() {
    let clock = SimClock::new();
    assert_eq!(clock.monotonic_now(), MonoInstant::ZERO);
    assert_eq!(clock.wall_now(), WallInstant::UNIX_EPOCH);
}

#[test]
fn sim_clock_advance_moves_both() {
    let clock = SimClock::new();
    clock.advance(Duration::from_secs(1));
    assert_eq!(clock.monotonic_now().as_nanos(), 1_000_000_000);
    assert_eq!(clock.wall_now().as_nanos(), 1_000_000_000);
}

#[test]
fn sim_clock_wall_jump_does_not_move_mono() {
    let clock = SimClock::new();
    clock.advance(Duration::from_secs(10));
    clock.jump_wall_clock(5_000_000_000);
    assert_eq!(clock.monotonic_now().as_nanos(), 10_000_000_000);
    assert_eq!(clock.wall_now().as_nanos(), 15_000_000_000);
}

#[test]
fn sim_clock_negative_wall_jump() {
    let clock = SimClock::new();
    clock.advance(Duration::from_secs(10));
    clock.jump_wall_clock(-3_000_000_000);
    assert_eq!(clock.monotonic_now().as_nanos(), 10_000_000_000);
    assert_eq!(clock.wall_now().as_nanos(), 7_000_000_000);
}

#[test]
fn time_reference_round_trip() {
    let clock = SimClock::new();
    clock.advance(Duration::from_secs(5));
    let time_ref = TimeReference::capture(&clock);

    clock.advance(Duration::from_secs(3));
    let mono = clock.monotonic_now();
    let wall = clock.wall_now();

    assert_eq!(time_ref.wall_to_mono(time_ref.mono_to_wall(mono)), mono);
    assert_eq!(time_ref.mono_to_wall(time_ref.wall_to_mono(wall)), wall);
}

#[test]
fn time_reference_after_wall_jump() {
    let clock = SimClock::new();
    clock.advance(Duration::from_secs(10));
    let time_ref = TimeReference::capture(&clock);

    clock.jump_wall_clock(2_000_000_000);

    let wall_after_jump = clock.wall_now();
    let expected_mono = MonoInstant(12_000_000_000);
    assert_eq!(time_ref.wall_to_mono(wall_after_jump), expected_mono);
}

#[test]
fn mono_instant_arithmetic() {
    let a = MonoInstant(1_000_000_000);
    let b = a + Duration::from_millis(500);
    assert_eq!(b.as_nanos(), 1_500_000_000);
    assert_eq!((b - a), Duration::from_millis(500));
    assert_eq!(
        b.checked_duration_since(a),
        Some(Duration::from_millis(500))
    );
    assert_eq!(a.checked_duration_since(b), None);
}

#[test]
fn real_clock_monotonic_advances() {
    let clock = RealClock::new();
    let t0 = clock.monotonic_now();
    std::thread::sleep(Duration::from_millis(10));
    let t1 = clock.monotonic_now();
    assert!(t1 > t0);
}

#[test]
fn rc_shared_sim_clock() {
    let clock = Rc::new(SimClock::new());
    let clock2 = Rc::clone(&clock);

    clock.advance(Duration::from_secs(1));
    assert_eq!(clock2.monotonic_now().as_nanos(), 1_000_000_000);
    assert_eq!(clock2.wall_now().as_nanos(), 1_000_000_000);

    clock2.jump_wall_clock(-250_000_000);
    assert_eq!(clock.wall_now().as_nanos(), 750_000_000);
    assert_eq!(clock.monotonic_now().as_nanos(), 1_000_000_000);
}

#[test]
fn arc_shared_sim_clock() {
    let clock = Arc::new(SimClock::new());
    let clock2 = Arc::clone(&clock);

    clock.advance(Duration::from_secs(1));
    assert_eq!(clock2.monotonic_now().as_nanos(), 1_000_000_000);
    assert_eq!(clock2.wall_now().as_nanos(), 1_000_000_000);

    clock2.jump_wall_clock(-250_000_000);
    assert_eq!(clock.wall_now().as_nanos(), 750_000_000);
    assert_eq!(clock.monotonic_now().as_nanos(), 1_000_000_000);
}

#[test]
fn arc_sim_clock_is_send() {
    let clock = Arc::new(SimClock::new());
    let clock2 = Arc::clone(&clock);
    let handle = std::thread::spawn(move || {
        clock2.advance(Duration::from_secs(1));
    });
    handle.join().unwrap();
    assert_eq!(clock.monotonic_now().as_nanos(), 1_000_000_000);
}
