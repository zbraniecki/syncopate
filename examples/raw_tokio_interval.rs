use std::time::{Duration, Instant};

fn tick() {
    println!("[tick] doing scheduled work at {:?}", Instant::now());
}

#[tokio::main]
async fn main() {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        interval.tick().await;
        tick();
    }
}
