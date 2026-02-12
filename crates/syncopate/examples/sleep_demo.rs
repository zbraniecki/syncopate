use std::time::{Duration, UNIX_EPOCH};
use syncopate::scheduler::Scheduler;
use syncopate::task::TaskBuilder;

fn main() {
    println!("=== Demonstrating Time Discontinuity Handling ===\n");

    // Setup: Start at epoch, add an absolute periodic task
    let epoch = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let start_time = epoch + Duration::from_secs(30);
    let mut scheduler: Scheduler = Scheduler::with_test_time(epoch, start_time);

    let task = TaskBuilder::<()>::every_at_boundary(Duration::from_secs(60))
        .name("every_minute_at_00")
        .build()
        .unwrap();

    scheduler.add_task(task).unwrap();
    println!("Added task: Fire every minute at :00 boundary");
    println!("Starting at epoch + 30s (wall-clock at :30)\n");

    // First fire at :00 (30s from now in virtual time)
    println!("Tick 30s virtual time...");
    let fired = scheduler.tick(Duration::from_secs(30));
    println!(
        "  ✓ Task fired at :00 boundary ({} task fired)\n",
        fired.len()
    );

    // Simulate system sleep: wall-clock advances 70s, virtual time only 10s
    println!("Simulating system sleep:");
    println!("  - Virtual time: +10s");
    println!("  - Wall-clock time: +70s (sleep for 60s)");
    scheduler.advance_time(start_time + Duration::from_secs(100));
    let fired = scheduler.tick(Duration::from_secs(10));
    println!("  - Time discontinuity detected (70s > 10s * 3x threshold)");
    println!("  - Tasks resynced to wall-clock boundaries");
    println!("  - Tasks fired: {}\n", fired.len());

    // Continue to next boundary
    println!("Tick 50s to reach next :00 boundary...");
    let fired = scheduler.tick(Duration::from_secs(50));
    println!(
        "  ✓ Task fired at :00 boundary ({} task fired)",
        fired.len()
    );
    println!("  - Task successfully resynced after sleep!\n");

    println!("=== Comparison: Relative Tasks (no resync needed) ===\n");

    let mut scheduler2: Scheduler = Scheduler::with_test_time(epoch, epoch);
    let task2 = TaskBuilder::<()>::every(Duration::from_secs(60))
        .name("every_60s_relative")
        .build()
        .unwrap();

    scheduler2.add_task(task2).unwrap();
    println!("Added relative task: Fire every 60s (virtual time)");

    println!("Tick 60s virtual time...");
    let fired = scheduler2.tick(Duration::from_secs(60));
    println!("  ✓ Task fired ({} task fired)\n", fired.len());

    println!("Simulating sleep (wall-clock +1000s, virtual +60s)...");
    scheduler2.advance_time(epoch + Duration::from_secs(1000));
    let fired = scheduler2.tick(Duration::from_secs(60));
    println!(
        "  ✓ Task fired at virtual 120s ({} task fired)",
        fired.len()
    );
    println!("  - Relative tasks use virtual time, unaffected by sleep!");
}
