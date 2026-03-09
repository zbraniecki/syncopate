use std::thread;
use std::time::{Duration, Instant};

use tokio::runtime::Builder;
use tokio::sync::mpsc;

fn schedule_next_tick() -> Duration {
    Duration::from_secs(1)
}

fn tick() {
    println!("[tick] doing scheduled work at {:?}", Instant::now());
}

fn start_scheduler_thread(tx: mpsc::Sender<()>) {
    thread::spawn(move || {
        loop {
            let dur = schedule_next_tick();
            thread::sleep(dur);
            // ignore send errors if receiver is gone
            if tx.blocking_send(()).is_err() {
                break;
            }
        }
    });
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build a *current-thread* runtime with I/O but NO time driver.
    let rt = Builder::new_current_thread()
        .enable_io() // async I/O only; NO enable_time()
        .build()?;

    // Channel for ticks from the scheduler thread into async world.
    let (tx, mut rx) = mpsc::channel::<()>(16);

    // Start scheduler thread that uses std::thread::sleep.
    start_scheduler_thread(tx);

    rt.block_on(async move {
        loop {
            println!("Waiting for tick (10s) or keypress...");

            tokio::select! {
                _ = rx.recv() => {
                    tick();
                }
            }
        }
    });
    Ok(())
}
