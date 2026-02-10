use std::time::Duration;
use syncopate::{
    scheduler::SchedulerBuilder,
    task::{TaskConfig, TaskType},
};

#[tokio::main]
async fn main() {
    println!("Starting syncopate simple example");
    println!("Task will fire every 1.5 seconds using callbacks");
    println!();

    // Build a scheduler with unit context (no shared state needed)
    let (handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(Duration::from_secs(1))
        .max_period(Duration::from_secs(2))
        .build();

    // Spawn the scheduler loop in a background task
    let scheduler_handle = tokio::spawn(async move {
        let mut iteration = 0;
        loop {
            // Poll the scheduler - callbacks execute automatically!
            let idle = scheduler.poll();

            // Sleep for the idle duration (capped to allow responsiveness)
            if idle > Duration::ZERO {
                let sleep_duration = idle.min(Duration::from_millis(100));
                tokio::time::sleep(sleep_duration).await;
            }

            iteration += 1;

            // Stop after enough iterations to see several task executions
            if iteration > 60 {
                println!();
                println!("Example complete!");
                break;
            }
        }
    });

    // Give the scheduler loop a moment to start
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Add a task that fires every 1.5 seconds with execution callback
    let task_id = handle
        .add_task(
            TaskConfig {
                task_type: TaskType::Periodic {
                    period: Duration::from_millis(1500),
                    window_before: Duration::from_millis(100),
                    window_after: Duration::from_millis(100),
                    anchored: false,
                },
                priority: 0,
                name: Some("hello_world".into()),
                on_execute: None,
                on_miss: None,
            }
            .with_executor(|exec, _ctx| {
                println!(
                    "✓ Task {:?} executed! (drift: {:?}, was_late: {})",
                    exec.task_id, exec.drift, exec.was_late
                );
            })
            .with_miss_handler(|miss, _ctx| {
                eprintln!(
                    "✗ Task {:?} MISSED! (miss_count: {})",
                    miss.task_id, miss.miss_count
                );
            }),
        )
        .expect("Failed to add task");

    println!("Task {:?} scheduled to run every 1.5 seconds", task_id);
    println!();

    // Wait for the scheduler to complete
    scheduler_handle.await.unwrap();
}
