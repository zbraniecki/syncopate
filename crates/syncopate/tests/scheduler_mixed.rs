use std::time::{Duration, UNIX_EPOCH};
use syncopate::scheduler::Scheduler;
use syncopate::task::TaskBuilder;

const NANOS_PER_MS: u64 = 1_000_000;
const NANOS_PER_SEC: u64 = 1_000_000_000;

#[test]
fn test_one_anchored_one_relative() {
    // Scenario: Clock at 300ms into the second
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut scheduler: Scheduler =
        Scheduler::with_test_time(epoch, epoch + Duration::from_nanos(300 * NANOS_PER_MS));

    // Add anchored task (fires at 1s boundaries)
    let anchored_task = TaskBuilder::every_at_boundary(Duration::from_nanos(1 * NANOS_PER_SEC))
        .name("anchored")
        .build()
        .unwrap();
    scheduler.add_task(anchored_task).unwrap();

    // Add relative task at same time (fires 1s from now)
    let relative_task = TaskBuilder::every(Duration::from_nanos(1 * NANOS_PER_SEC))
        .name("relative")
        .build()
        .unwrap();
    scheduler.add_task(relative_task).unwrap();

    // Next tick: anchored fires at 700ms (next 1s boundary)
    let next_tick = scheduler.calculate_next_tick();
    assert_eq!(next_tick, Some(Duration::from_nanos(700 * NANOS_PER_MS)));

    scheduler.advance_time(epoch + Duration::from_nanos(1000 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(700 * NANOS_PER_MS));
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].name.as_ref().unwrap(), "anchored");

    // Next tick: relative fires at 1000ms (300ms later)
    let next_tick = scheduler.calculate_next_tick();
    assert_eq!(next_tick, Some(Duration::from_nanos(300 * NANOS_PER_MS)));

    scheduler.advance_time(epoch + Duration::from_nanos(1300 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(300 * NANOS_PER_MS));
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].name.as_ref().unwrap(), "relative");

    // Next tick: anchored fires at 1700ms (700ms later)
    let next_tick = scheduler.calculate_next_tick();
    assert_eq!(next_tick, Some(Duration::from_nanos(700 * NANOS_PER_MS)));

    scheduler.advance_time(epoch + Duration::from_nanos(2000 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(700 * NANOS_PER_MS));
    assert_eq!(ready.len(), 1); // Only anchored fires at 1700ms
    assert_eq!(ready[0].name.as_ref().unwrap(), "anchored");

    // Next tick: relative fires at 2000ms (300ms later)
    let next_tick = scheduler.calculate_next_tick();
    assert_eq!(next_tick, Some(Duration::from_nanos(300 * NANOS_PER_MS)));

    scheduler.advance_time(epoch + Duration::from_nanos(2300 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(300 * NANOS_PER_MS));
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].name.as_ref().unwrap(), "relative");
}

#[test]
fn test_two_anchored_one_relative() {
    // Clock starts at 200ms
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut scheduler: Scheduler =
        Scheduler::with_test_time(epoch, epoch + Duration::from_nanos(200 * NANOS_PER_MS));

    // Anchored task 1: fires at 1s boundaries
    let anchored1 = TaskBuilder::every_at_boundary(Duration::from_nanos(1 * NANOS_PER_SEC))
        .name("anchored1")
        .build()
        .unwrap();
    scheduler.add_task(anchored1).unwrap();

    // Relative task: fires every 500ms from scheduler start
    let relative = TaskBuilder::every(Duration::from_nanos(500 * NANOS_PER_MS))
        .name("relative")
        .build()
        .unwrap();
    scheduler.add_task(relative).unwrap();

    // Advance 300ms (clock at 500ms)
    scheduler.advance_time(epoch + Duration::from_nanos(500 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(300 * NANOS_PER_MS));
    assert_eq!(ready.len(), 0);

    // Anchored task 2: fires at 1s boundaries
    let anchored2 = TaskBuilder::every_at_boundary(Duration::from_nanos(1 * NANOS_PER_SEC))
        .name("anchored2")
        .build()
        .unwrap();
    scheduler.add_task(anchored2).unwrap();

    // t=500ms: relative fires (200ms from now)
    let next_tick = scheduler.calculate_next_tick();
    assert_eq!(next_tick, Some(Duration::from_nanos(200 * NANOS_PER_MS)));

    scheduler.advance_time(epoch + Duration::from_nanos(700 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(200 * NANOS_PER_MS));
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].name.as_ref().unwrap(), "relative");

    // t=800ms: both anchored fire at 1000ms boundary (300ms from now)
    let next_tick = scheduler.calculate_next_tick();
    assert_eq!(next_tick, Some(Duration::from_nanos(300 * NANOS_PER_MS)));

    scheduler.advance_time(epoch + Duration::from_nanos(1000 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(300 * NANOS_PER_MS));
    assert_eq!(ready.len(), 2); // anchored1 and anchored2 at 1s boundary
}

#[test]
fn test_one_anchored_two_relative_different_periods() {
    // Clock at 100ms
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut scheduler: Scheduler =
        Scheduler::with_test_time(epoch, epoch + Duration::from_nanos(100 * NANOS_PER_MS));

    // Anchored: 1s period, fires at boundaries
    let anchored = TaskBuilder::every_at_boundary(Duration::from_nanos(1 * NANOS_PER_SEC))
        .name("anchored")
        .build()
        .unwrap();
    scheduler.add_task(anchored).unwrap();

    // Relative 1: 400ms period
    let relative1 = TaskBuilder::every(Duration::from_nanos(400 * NANOS_PER_MS))
        .name("rel1")
        .build()
        .unwrap();
    scheduler.add_task(relative1).unwrap();

    // Relative 2: 600ms period
    let relative2 = TaskBuilder::every(Duration::from_nanos(600 * NANOS_PER_MS))
        .name("rel2")
        .build()
        .unwrap();
    scheduler.add_task(relative2).unwrap();

    // Verify first few ticks
    // t=400ms: rel1 fires
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_nanos(400 * NANOS_PER_MS))
    );
    scheduler.advance_time(epoch + Duration::from_nanos(500 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(400 * NANOS_PER_MS));
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].name.as_ref().unwrap(), "rel1");

    // t=600ms: rel2 fires
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_nanos(200 * NANOS_PER_MS))
    );
    scheduler.advance_time(epoch + Duration::from_nanos(700 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(200 * NANOS_PER_MS));
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].name.as_ref().unwrap(), "rel2");

    // t=800ms: rel1 fires
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_nanos(200 * NANOS_PER_MS))
    );
    scheduler.advance_time(epoch + Duration::from_nanos(900 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(200 * NANOS_PER_MS));
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].name.as_ref().unwrap(), "rel1");

    // t=900ms: anchored fires (at 1s boundary, 100ms later)
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_nanos(100 * NANOS_PER_MS))
    );
    scheduler.advance_time(epoch + Duration::from_nanos(1000 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(100 * NANOS_PER_MS));
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].name.as_ref().unwrap(), "anchored");
}
