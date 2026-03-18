use std::time::{Duration, Instant};

use syncopate::{Scheduler, TaskBuilder};

fn tick() {
    println!("[tick] doing scheduled work at {:?}", Instant::now());
}

#[tokio::main]
async fn main() {
    let mut scheduler = Scheduler::<()>::new();
    scheduler.add_task(TaskBuilder::every(Duration::from_secs(1)).build());

    while let Some(dur) = scheduler.calculate_next_tick() {
        tokio::time::sleep(dur).await;
        scheduler.tick();
        tick();
    }
}
