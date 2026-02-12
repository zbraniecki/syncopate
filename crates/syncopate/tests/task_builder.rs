use std::time::{Duration, SystemTime, UNIX_EPOCH};
use syncopate::scheduler::Scheduler;
use syncopate::task::{TaskBuildError, TaskBuilder, Time};

const NANOS_PER_MS: u64 = 1_000_000;
const NANOS_PER_SEC: u64 = 1_000_000_000;

#[test]
fn test_builder_missing_task_type() {
    let result = TaskBuilder::<()>::new().name("test").build();

    assert!(result.is_err());
    match result {
        Err(TaskBuildError::NoTaskType) => (),
        _ => panic!("Expected NoTaskType error"),
    }
}

#[test]
fn test_builder_offset_exceeds_period() {
    let result = TaskBuilder::<()>::every_with_offset(
        Duration::from_secs(1),
        Duration::from_secs(2), // offset > period
    )
    .build();

    assert!(result.is_err());
    match result {
        Err(TaskBuildError::OffsetExceedsPeriod { period, offset }) => {
            assert_eq!(period, Duration::from_secs(1));
            assert_eq!(offset, Duration::from_secs(2));
        }
        _ => panic!("Expected OffsetExceedsPeriod error"),
    }
}

#[test]
fn test_builder_offset_equals_period() {
    // offset == period should also be an error
    let result = TaskBuilder::<()>::every_with_offset(
        Duration::from_secs(1),
        Duration::from_secs(1), // offset == period
    )
    .build();

    assert!(result.is_err());
    match result {
        Err(TaskBuildError::OffsetExceedsPeriod { .. }) => (),
        _ => panic!("Expected OffsetExceedsPeriod error"),
    }
}

#[test]
fn test_builder_fluent_constructors() {
    // Test every()
    let task = TaskBuilder::<()>::every(Duration::from_secs(1))
        .name("test")
        .build()
        .unwrap();
    assert_eq!(task.name, Some("test".to_string()));

    // Test every_at_boundary()
    let task = TaskBuilder::<()>::every_at_boundary(Duration::from_secs(1))
        .priority(5)
        .build()
        .unwrap();
    assert_eq!(task.priority, 5);

    // Test every_with_offset()
    let task =
        TaskBuilder::<()>::every_with_offset(Duration::from_secs(60), Duration::from_secs(5))
            .build()
            .unwrap();
    assert!(task.name.is_none());

    // Test once_after()
    let task = TaskBuilder::<()>::once_after(Duration::from_secs(5))
        .build()
        .unwrap();
    assert_eq!(task.priority, 0);

    // Test once_at()
    let deadline = SystemTime::now() + Duration::from_secs(10);
    let task = TaskBuilder::<()>::once_at(deadline).build().unwrap();
    assert!(task.name.is_none());
}

#[test]
fn test_one_time_relative_task() {
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, epoch);

    // Create a one-time task that fires 500ms from now
    let task = TaskBuilder::once_after(Duration::from_nanos(500 * NANOS_PER_MS))
        .name("once")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    // Verify it's scheduled for 500ms
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_nanos(500 * NANOS_PER_MS))
    );

    // Advance and verify it fires
    let ready = scheduler.tick(Duration::from_nanos(500 * NANOS_PER_MS));
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].name.as_ref().unwrap(), "once");

    // Verify it's been removed from the scheduler
    assert_eq!(scheduler.calculate_next_tick(), None);
}

#[test]
fn test_one_time_absolute_task() {
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, epoch);

    // Create a one-time task that fires 1 second in the future
    let deadline = epoch + Duration::from_nanos(1 * NANOS_PER_SEC);
    let task = TaskBuilder::once_at(deadline).name("once").build().unwrap();
    scheduler.add_task(task).unwrap();

    // Verify it's scheduled for 1s
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_nanos(1 * NANOS_PER_SEC))
    );

    // Advance and verify it fires
    scheduler.advance_time(epoch + Duration::from_nanos(1 * NANOS_PER_SEC));
    let ready = scheduler.tick(Duration::from_nanos(1 * NANOS_PER_SEC));
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].name.as_ref().unwrap(), "once");

    // Verify it's been removed
    assert_eq!(scheduler.calculate_next_tick(), None);
}

#[test]
fn test_one_time_delay_calculated_at_add_time() {
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, epoch);

    // Create task
    let task = TaskBuilder::once_after(Duration::from_nanos(500 * NANOS_PER_MS))
        .name("delayed")
        .build()
        .unwrap();

    // Simulate waiting before adding to scheduler
    scheduler.tick(Duration::from_nanos(1 * NANOS_PER_SEC));

    // Add task - delay should be calculated from NOW, not from creation
    scheduler.add_task(task).unwrap();

    // Should fire 500ms from scheduler's current position (1s), so at 1.5s total
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_nanos(500 * NANOS_PER_MS))
    );

    // Advance 500ms and verify it fires
    let ready = scheduler.tick(Duration::from_nanos(500 * NANOS_PER_MS));
    assert_eq!(ready.len(), 1);
}

#[test]
fn test_one_time_with_periodic_tasks() {
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, epoch);

    // Add periodic task
    let periodic = TaskBuilder::every(Duration::from_nanos(1 * NANOS_PER_SEC))
        .name("periodic")
        .build()
        .unwrap();
    scheduler.add_task(periodic).unwrap();

    // Add one-time task
    let once = TaskBuilder::once_after(Duration::from_nanos(500 * NANOS_PER_MS))
        .name("once")
        .build()
        .unwrap();
    scheduler.add_task(once).unwrap();

    // At 500ms, one-time fires
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_nanos(500 * NANOS_PER_MS))
    );
    let ready = scheduler.tick(Duration::from_nanos(500 * NANOS_PER_MS));
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].name.as_ref().unwrap(), "once");

    // At 1s, periodic fires
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_nanos(500 * NANOS_PER_MS))
    );
    let ready = scheduler.tick(Duration::from_nanos(500 * NANOS_PER_MS));
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].name.as_ref().unwrap(), "periodic");

    // At 2s, only periodic fires (one-time already removed)
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_nanos(1 * NANOS_PER_SEC))
    );
    let ready = scheduler.tick(Duration::from_nanos(1 * NANOS_PER_SEC));
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].name.as_ref().unwrap(), "periodic");
}

#[test]
fn test_time_helpers() {
    // Test hms
    assert_eq!(Time::hms(16, 5, 30), Duration::from_secs(57930));
    assert_eq!(Time::hms(0, 0, 0), Duration::from_secs(0));
    assert_eq!(Time::hms(23, 59, 59), Duration::from_secs(86399));

    // Test hm
    assert_eq!(Time::hm(16, 5), Duration::from_secs(57900));
    assert_eq!(Time::hm(0, 0), Duration::from_secs(0));
    assert_eq!(Time::hm(12, 30), Duration::from_secs(45000));

    // today_at and tomorrow_at are harder to test deterministically
    // but we can verify they return reasonable values
    let today = Time::today_at(12, 0, 0);
    assert!(today.is_some());

    let tomorrow = Time::tomorrow_at(12, 0, 0);
    let now = SystemTime::now();
    assert!(tomorrow > now);
}

#[test]
fn test_anchored_with_offset() {
    // Test "every hour at :05"
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut scheduler: Scheduler = Scheduler::with_test_time(
        epoch,
        epoch + Duration::from_nanos(10 * NANOS_PER_MS), // Start at 10ms
    );

    // Task fires every second at 300ms offset
    let task = TaskBuilder::every_with_offset(
        Duration::from_nanos(1 * NANOS_PER_SEC),
        Duration::from_nanos(300 * NANOS_PER_MS),
    )
    .name("offset")
    .build()
    .unwrap();
    scheduler.add_task(task).unwrap();

    // First tick: should fire at 300ms (290ms from now)
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_nanos(290 * NANOS_PER_MS))
    );

    scheduler.advance_time(epoch + Duration::from_nanos(300 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(290 * NANOS_PER_MS));
    assert_eq!(ready.len(), 1);

    // Subsequent tick: should fire 1s later at 1300ms
    assert_eq!(
        scheduler.calculate_next_tick(),
        Some(Duration::from_nanos(1 * NANOS_PER_SEC))
    );

    scheduler.advance_time(epoch + Duration::from_nanos(1300 * NANOS_PER_MS));
    let ready = scheduler.tick(Duration::from_nanos(1 * NANOS_PER_SEC));
    assert_eq!(ready.len(), 1);
}
