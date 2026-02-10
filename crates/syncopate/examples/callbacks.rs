use std::sync::{Arc, Mutex};
use std::time::Duration;
use syncopate::{
    scheduler::SchedulerBuilder,
    task::{TaskConfig, TaskType},
};

/// Application context shared between tasks
#[derive(Clone)]
struct AppContext {
    execution_count: Arc<Mutex<usize>>,
}

#[tokio::main]
async fn main() {
    println!("Starting syncopate callback example");
    println!("Demonstrates automatic task execution via callbacks with context");
    println!();

    // Create application context
    let context = AppContext {
        execution_count: Arc::new(Mutex::new(0)),
    };

    // Build a scheduler with context
    let (handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(Duration::from_secs(1))
        .max_period(Duration::from_secs(2))
        .with_context(context.clone())
        .build();

    // Spawn the scheduler loop in a background task
    let scheduler_handle = tokio::spawn(async move {
        let mut iteration = 0;
        loop {
            // Poll the scheduler - callbacks are invoked automatically!
            let idle = scheduler.poll();

            // Sleep for the idle duration (capped to allow responsiveness)
            if idle > Duration::ZERO {
                let sleep_duration = idle.min(Duration::from_millis(100));
                tokio::time::sleep(sleep_duration).await;
            }

            iteration += 1;

            // Stop after some iterations (run for ~6 seconds)
            if iteration > 60 {
                println!();
                println!("Example complete!");
                break;
            }
        }
    });

    // Give the scheduler loop a moment to start
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Add a task with execution callback that uses the context
    let sensor_id = handle
        .add_task(
            TaskConfig::<AppContext> {
                task_type: TaskType::Periodic {
                    period: Duration::from_millis(1000),
                    window_before: Duration::from_millis(50),
                    window_after: Duration::from_millis(50),
                    anchored: false,
                },
                priority: 0,
                name: Some("sensor".into()),
                on_execute: None,
                on_miss: None,
            }
            .with_executor(move |exec, ctx| {
                let mut count = ctx.execution_count.lock().unwrap();
                *count += 1;
                println!(
                    "  [Sensor] Execution #{}: drift={:?}, was_late={}",
                    *count, exec.drift, exec.was_late
                );
                // Simulate sensor reading
                read_sensor();
            })
            .with_miss_handler(|miss, _ctx| {
                eprintln!(
                    "  [Sensor] MISSED! count={}, deadline={:?}",
                    miss.miss_count, miss.ideal_time
                );
            }),
        )
        .expect("Failed to add task");

    println!("Task {:?} (sensor) scheduled with callbacks", sensor_id);

    // Add another task with a different period
    let heartbeat_id = handle
        .add_task(
            TaskConfig::<AppContext> {
                task_type: TaskType::Periodic {
                    period: Duration::from_millis(1750),
                    window_before: Duration::from_millis(100),
                    window_after: Duration::from_millis(100),
                    anchored: false,
                },
                priority: 1,
                name: Some("heartbeat".into()),
                on_execute: None,
                on_miss: None,
            }
            .with_executor(|exec, _ctx| {
                println!(
                    "  [Heartbeat] Execution at {:?}, drift={:?}",
                    exec.actual_time, exec.drift
                );
                send_heartbeat();
            })
            .with_miss_handler(|miss, _ctx| {
                eprintln!("  [Heartbeat] MISSED {} times!", miss.miss_count);
            }),
        )
        .expect("Failed to add task");

    println!(
        "Task {:?} (heartbeat) scheduled with callbacks",
        heartbeat_id
    );
    println!();

    // Wait for the scheduler to complete
    scheduler_handle.await.unwrap();

    // Print final stats from context
    let final_count = *context.execution_count.lock().unwrap();
    println!("Total sensor executions: {}", final_count);
}

fn read_sensor() {
    // Simulate sensor reading work
    // In a real application, this would read from hardware
}

fn send_heartbeat() {
    // Simulate heartbeat work
    // In a real application, this would send a network packet
}
