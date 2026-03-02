use std::time::Duration;
use syncopate::system_time::{Clock, MonoInstant, SimClock, TimeReference, WallInstant};

#[test]
fn sim_clock_starts_at_zero() {
    let clock = SimClock::new();
    assert_eq!(clock.monotonic_now(), MonoInstant::ZERO);
    assert_eq!(clock.wall_now(), WallInstant::UNIX_EPOCH);
}

#[test]
fn sim_clock_advance_moves_both_clocks() {
    let clock = SimClock::new();
    clock.advance(Duration::from_secs(1));
    assert_eq!(clock.monotonic_now().as_nanos(), 1_000_000_000);
    assert_eq!(clock.wall_now().as_nanos(), 1_000_000_000);
}

#[test]
fn sim_clock_advance_is_cumulative() {
    let clock = SimClock::new();
    clock.advance(Duration::from_millis(500));
    clock.advance(Duration::from_millis(500));
    assert_eq!(clock.monotonic_now().as_nanos(), 1_000_000_000);
    assert_eq!(clock.wall_now().as_nanos(), 1_000_000_000);
}

#[test]
fn sim_clock_positive_wall_jump_does_not_move_mono() {
    let clock = SimClock::new();
    clock.advance(Duration::from_secs(10));
    clock.jump_wall_clock(5_000_000_000); // +5s NTP correction
    assert_eq!(clock.monotonic_now().as_nanos(), 10_000_000_000);
    assert_eq!(clock.wall_now().as_nanos(), 15_000_000_000);
}

#[test]
fn sim_clock_negative_wall_jump_does_not_move_mono() {
    let clock = SimClock::new();
    clock.advance(Duration::from_secs(10));
    clock.jump_wall_clock(-3_000_000_000); // -3s NTP step-back
    assert_eq!(clock.monotonic_now().as_nanos(), 10_000_000_000);
    assert_eq!(clock.wall_now().as_nanos(), 7_000_000_000);
}

#[test]
fn sim_clock_wall_jump_saturates_at_zero() {
    let clock = SimClock::new();
    clock.advance(Duration::from_secs(1));
    clock.jump_wall_clock(-9_000_000_000); // jump back more than elapsed
    assert_eq!(clock.wall_now().as_nanos(), 0);
    // monotonic is untouched
    assert_eq!(clock.monotonic_now().as_nanos(), 1_000_000_000);
}

#[test]
fn time_reference_wall_to_mono_forward() {
    let clock = SimClock::new();
    clock.advance(Duration::from_secs(5));
    let time_ref = TimeReference::capture(&clock);

    // 3 seconds after the reference
    let wall = clock.wall_now() + Duration::from_secs(3);
    let expected_mono = clock.monotonic_now() + Duration::from_secs(3);
    assert_eq!(time_ref.wall_to_mono(wall), expected_mono);
}

#[test]
fn time_reference_mono_to_wall_forward() {
    let clock = SimClock::new();
    clock.advance(Duration::from_secs(5));
    let time_ref = TimeReference::capture(&clock);

    let mono = clock.monotonic_now() + Duration::from_secs(3);
    let expected_wall = clock.wall_now() + Duration::from_secs(3);
    assert_eq!(time_ref.mono_to_wall(mono), expected_wall);
}

#[test]
fn time_reference_round_trip_mono() {
    let clock = SimClock::new();
    clock.advance(Duration::from_secs(5));
    let time_ref = TimeReference::capture(&clock);

    clock.advance(Duration::from_secs(3));
    let mono = clock.monotonic_now();
    assert_eq!(time_ref.wall_to_mono(time_ref.mono_to_wall(mono)), mono);
}

#[test]
fn time_reference_round_trip_wall() {
    let clock = SimClock::new();
    clock.advance(Duration::from_secs(5));
    let time_ref = TimeReference::capture(&clock);

    clock.advance(Duration::from_secs(3));
    let wall = clock.wall_now();
    assert_eq!(time_ref.mono_to_wall(time_ref.wall_to_mono(wall)), wall);
}

#[test]
fn time_reference_reflects_wall_jump() {
    let clock = SimClock::new();
    clock.advance(Duration::from_secs(10));
    let time_ref = TimeReference::capture(&clock); // ref at mono=10s, wall=10s

    clock.jump_wall_clock(2_000_000_000); // wall jumps to 12s, mono stays at 10s

    // wall_to_mono: wall=12s is 2s ahead of wall_ref=10s, so mono = mono_ref(10s) + 2s = 12s
    let mono = time_ref.wall_to_mono(clock.wall_now());
    assert_eq!(mono.as_nanos(), 12_000_000_000);
}

#[test]
fn mono_instant_add_duration() {
    let t = MonoInstant::ZERO + Duration::from_millis(250);
    assert_eq!(t.as_nanos(), 250_000_000);
}

#[test]
fn mono_instant_sub_duration() {
    let t = MonoInstant(1_000_000_000) - Duration::from_millis(250);
    assert_eq!(t.as_nanos(), 750_000_000);
}

#[test]
fn mono_instant_sub_saturates_at_zero() {
    let t = MonoInstant(100) - Duration::from_secs(1);
    assert_eq!(t, MonoInstant::ZERO);
}

#[test]
fn mono_instant_checked_duration_since_some() {
    let a = MonoInstant(500_000_000);
    let b = MonoInstant(1_000_000_000);
    assert_eq!(
        b.checked_duration_since(a),
        Some(Duration::from_millis(500))
    );
}

#[test]
fn mono_instant_checked_duration_since_none() {
    let a = MonoInstant(1_000_000_000);
    let b = MonoInstant(500_000_000);
    assert_eq!(b.checked_duration_since(a), None);
}

#[test]
fn mono_instant_sub_instants_saturates() {
    let a = MonoInstant(1_000_000_000);
    let b = MonoInstant(500_000_000);
    // b - a underflows, should saturate to ZERO
    assert_eq!(b - a, Duration::ZERO);
    // a - b is well-defined
    assert_eq!(a - b, Duration::from_millis(500));
}

#[test]
fn wall_instant_checked_duration_since_some() {
    let a = WallInstant(1_000_000_000);
    let b = WallInstant(3_000_000_000);
    assert_eq!(b.checked_duration_since(a), Some(Duration::from_secs(2)));
}

#[test]
fn wall_instant_checked_duration_since_none_on_step_back() {
    let a = WallInstant(3_000_000_000);
    let b = WallInstant(1_000_000_000);
    assert_eq!(b.checked_duration_since(a), None);
}
