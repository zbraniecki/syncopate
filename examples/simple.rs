use std::time::Duration;

// #[tokio::main]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    // let mut interval = time::interval(time::Duration::from_secs(1));
    loop {
        // interval.tick().await;
        // println!("TICK");
        tokio::time::sleep(Duration::from_secs(1)).await;
        // println!("TICK");
        // std::thread::sleep(Duration::from_secs(1));
        // println!("TICK");
    }
}
