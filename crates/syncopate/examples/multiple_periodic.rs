use comfy_table::Table;
use std::time::Duration;
use syncopate::scheduler::Scheduler;
use syncopate::task::TaskBuilder;

fn main() {
    println!("=== Multiple Periodic Tasks Example ===\n");

    // Create a scheduler
    let mut scheduler = Scheduler::new();

    // Add multiple tasks with different periods
    let task1 = TaskBuilder::<()>::every(Duration::from_secs(60))
        .name("1min")
        .build()
        .unwrap();

    let task2 = TaskBuilder::<()>::every(Duration::from_secs(120))
        .name("2min")
        .build()
        .unwrap();

    let task3 = TaskBuilder::<()>::every(Duration::from_secs(180))
        .name("3min")
        .build()
        .unwrap();

    scheduler.add_task(task1).unwrap();
    scheduler.add_task(task2).unwrap();
    scheduler.add_task(task3).unwrap();

    println!("Tasks added:");
    println!("  - 1min (fires every 60s)");
    println!("  - 2min (fires every 120s)");
    println!("  - 3min (fires every 180s)\n");

    // Create table
    let mut table = Table::new();
    table.set_header(vec![
        "Tick",
        "Time Since Last",
        "Tasks Executed",
        "Sleep Until Next",
    ]);

    // Run for 6 minutes
    for tick_num in 1..=6 {
        let tick_duration = Duration::from_secs(60);

        // Advance time
        let fired_tasks = scheduler.tick(tick_duration);

        // Format tasks executed
        let tasks_str = if fired_tasks.is_empty() {
            "none".to_string()
        } else {
            fired_tasks
                .iter()
                .map(|t| t.name.as_deref().unwrap_or("unnamed"))
                .collect::<Vec<_>>()
                .join(", ")
        };

        // Calculate sleep until next tick
        let sleep_str = if let Some(next_tick) = scheduler.calculate_next_tick() {
            format!("{:?}", next_tick)
        } else {
            "no tasks".to_string()
        };

        // Add row to table
        table.add_row(vec![
            tick_num.to_string(),
            format!("{:?}", tick_duration),
            tasks_str,
            sleep_str,
        ]);
    }

    println!("{table}");
    println!("\n=== Summary ===");
    println!("In 6 minutes:");
    println!("  - 1min fired 6 times");
    println!("  - 2min fired 3 times");
    println!("  - 3min fired 2 times");
}
