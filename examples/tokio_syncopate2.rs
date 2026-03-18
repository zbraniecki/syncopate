use std::time::{Duration, Instant};

use syncopate::{Repeat, Scheduler, TaskBuilder, TickResult, Window};

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

async fn wait_tick(scheduler: &mut Scheduler) -> Option<TickResult<'_>> {
    let dur = scheduler.calculate_next_tick()?;
    tokio::time::sleep(dur).await;
    Some(scheduler.tick())
}

#[tokio::main]
async fn main() {
    let mut scheduler = Scheduler::<()>::new();
    scheduler.add_task(
        TaskBuilder::every(Duration::from_secs(1))
            .name("heartbeat")
            .window(Window::symmetric(Duration::from_millis(100)))
            .repeat(Repeat::Times(5))
            .build(),
    );

    loop {
        tokio::select! {
            Some(result) = wait_tick(&mut scheduler) => {
                print_tick_result(result);
            }
            _ = tokio::signal::ctrl_c() => {
                println!("shutting down");
                break;
            }
            else => break,
        }
    }
}
