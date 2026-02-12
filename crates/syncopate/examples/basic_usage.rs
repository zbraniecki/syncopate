use std::time::Duration;
use syncopate::scheduler::Scheduler;
use syncopate::task::{TaskBuilder, Time};

fn main() {
    // Example 1: Relative task - "every second" (drift accumulates)
    let task1 = TaskBuilder::<()>::every(Duration::from_secs(1))
        .name("sensor")
        .build()
        .unwrap();

    // Example 2: Anchored task - "every minute at :00" (no drift)
    let task2 = TaskBuilder::<()>::every_at_boundary(Duration::from_secs(60))
        .name("minute_boundary")
        .build()
        .unwrap();

    // Example 3: Anchored with offset - "every minute at :05"
    let task3 =
        TaskBuilder::<()>::every_with_offset(Duration::from_secs(60), Duration::from_secs(5))
            .name("minute_at_05")
            .build()
            .unwrap();

    // Example 4: "every day at 16:05" using Time helpers
    let task4 =
        TaskBuilder::<()>::every_with_offset(Duration::from_secs(24 * 3600), Time::hm(16, 5))
            .name("daily_report")
            .build()
            .unwrap();

    // Example 5: One-time task after delay
    let task5 = TaskBuilder::<()>::once_after(Duration::from_secs(5))
        .name("delayed")
        .build()
        .unwrap();

    // Create scheduler
    let mut scheduler = Scheduler::new();

    // Add tasks (in production, handle errors properly)
    scheduler.add_task(task1).ok();
    scheduler.add_task(task2).ok();
    scheduler.add_task(task3).ok();
    scheduler.add_task(task4).ok();
    scheduler.add_task(task5).ok();

    // Main loop (simplified)
    println!("Scheduler created with {} tasks", 5);
    println!("Next tick in: {:?}", scheduler.calculate_next_tick());

    // In a real application, you would:
    loop {
        if let Some(duration) = scheduler.calculate_next_tick() {
            std::thread::sleep(duration);
            let ready_tasks = scheduler.tick(duration);
            for task in ready_tasks {
                println!("Executing task: {:?}", task.name);
                // Execute task...
            }
        }
    }
}
