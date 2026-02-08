use std::time::Duration;
use syncopate::{
    scheduler::SchedulerBuilder,
    task::{TaskConfig, TaskId, TaskType},
};

#[tokio::main]
async fn main() {
    println!("Starting syncopate simple example");
    println!("Task will fire every 5 seconds");
    println!();

    // Build a scheduler
    let (handle, mut scheduler) = SchedulerBuilder::new()
        .min_period(Duration::from_secs(1))
        .max_period(Duration::from_secs(2))
        .build();

    // Spawn the scheduler loop in a background task
    let scheduler_handle = tokio::spawn(async move {
        let mut iteration = 0;
        loop {
            // Poll the scheduler
            let plan = scheduler.poll();

            println!(
                "Iteration {}: idle_duration = {:?}",
                iteration, plan.idle_duration
            );

            // Handle any tasks that are due
            if !plan.due_tasks.is_empty() {
                for task in &plan.due_tasks {
                    println!(
                        "  ✓ Task {:?} is DUE! (ideal_time: {:?})",
                        task.id, task.ideal_time
                    );
                    println!("    Executing task...");
                    // This is where application-defined execution happens
                    execute_task(task.id);
                }

                // Mark tasks as completed
                let completed: Vec<_> = plan.due_tasks.iter().map(|t| t.id).collect();
                scheduler.mark_completed(&completed);
            }

            // Handle any missed tasks
            if !plan.missed_tasks.is_empty() {
                for miss in &plan.missed_tasks {
                    eprintln!(
                        "  ✗ Task {:?} MISSED (miss_count: {})",
                        miss.id, miss.miss_count
                    );
                }
            }

            // Sleep for the idle duration (capped to allow command processing)
            if plan.idle_duration > Duration::ZERO {
                let sleep_duration = plan.idle_duration.min(Duration::from_millis(100));
                tokio::time::sleep(sleep_duration).await;
            }

            iteration += 1;

            // Stop after 6 iterations (about 30 seconds)
            if iteration > 6 {
                println!();
                println!("Example complete!");
                break;
            }
        }
    });

    // Give the scheduler loop a moment to start
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Add a task that fires every 5 seconds
    let task_id = handle
        .add_task(TaskConfig {
            task_type: TaskType::Periodic {
                period: Duration::from_millis(1500),
                window_before: Duration::from_millis(100),
                window_after: Duration::from_millis(100),
            },
            priority: 0,
            name: Some("hello_world".into()),
        })
        .expect("Failed to add task");

    println!("Task {:?} scheduled to run every 500 ms", task_id);
    println!();

    // Wait for the scheduler to complete
    scheduler_handle.await.unwrap();
}

fn execute_task(task_id: TaskId) {
    // Application-specific task execution
    println!("    Hello from task {:?}!", task_id);
}
