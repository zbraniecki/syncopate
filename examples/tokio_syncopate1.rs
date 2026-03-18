use std::time::{Duration, Instant};

use syncopate::{Scheduler, TaskBuilder, TickResult, Window};

fn print_tick_result(result: TickResult<'_>) {
    println!("[tick] executed tick at {:?}", Instant::now());
    for exec in &result.fired {
        let name = exec.task.name.as_deref().unwrap_or("unnamed");
        println!("  [fired]  {name}  with a drift: {:?}", exec.drift);
    }
    for exec in &result.missed {
        let name = exec.task.name.as_deref().unwrap_or("unnamed");
        println!("  [missed]  {name}  at: {:?}", exec.deadlines_missed);
    }
}

#[tokio::main]
async fn main() {
    let mut scheduler = Scheduler::<()>::new();
    scheduler.add_task(
        TaskBuilder::every(Duration::from_secs(1))
            .window(Window::symmetric(Duration::from_millis(100)))
            .build(),
    );

    while let Some(dur) = scheduler.calculate_next_tick() {
        tokio::time::sleep(dur).await;
        let result = scheduler.tick();
        print_tick_result(result);
    }
}
