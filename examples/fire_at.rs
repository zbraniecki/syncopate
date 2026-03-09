use std::time::{Duration, Instant};

use clap::Parser;
use jiff::Zoned;
use syncopate::{Scheduler, TaskBuilder, Window};

#[derive(Parser, Debug)]
#[command(about = "Schedule a one-shot task that fires at a specific time or after a delay")]
struct Args {
    /// Fire at a wall-clock time today (or tomorrow if past), e.g. "16:30:00"
    #[arg(long = "at", conflicts_with = "delay")]
    at: Option<String>,

    /// Fire after a relative delay, e.g. "1m", "30s", "1h30m5s"
    #[arg(long = "in", conflicts_with = "at")]
    delay: Option<String>,
}

fn main() {
    let args = Args::parse();

    let delay = match (&args.at, &args.delay) {
        (Some(at), None) => parse_at(at),
        (None, Some(d)) => parse_duration(d),
        _ => {
            eprintln!("Provide exactly one of --at or --in");
            std::process::exit(1);
        }
    };

    println!(
        "Scheduling one-shot task to fire in {}",
        fmt_duration(delay)
    );
    println!("Now:    {}", Zoned::now().strftime("%H:%M:%S%.3f"));

    let mut scheduler: Scheduler = Scheduler::new();
    scheduler.set_timer_delay(Duration::from_millis(5));

    let task = TaskBuilder::once_after(delay)
        .window(Window::symmetric(Duration::from_millis(50)))
        .name("fire_at")
        .build()
        .unwrap();
    scheduler.add_task(task).unwrap();

    loop {
        let Some(sleep_dur) = scheduler.calculate_next_tick() else {
            break;
        };

        let t = Instant::now();
        std::thread::sleep(sleep_dur);
        let actual = t.elapsed();

        let result = scheduler.tick();

        for exec in &result.fired {
            let name = exec.task.name.as_deref().unwrap_or("unnamed");
            println!(
                "Fired:  {} at {} (drift: {})",
                name,
                Zoned::now().strftime("%H:%M:%S%.3f"),
                exec.drift,
            );
            println!(
                "        slept {}  (requested {})",
                fmt_duration(actual),
                fmt_duration(sleep_dur),
            );
        }
        for exec in &result.missed {
            let name = exec.task.name.as_deref().unwrap_or("unnamed");
            println!(
                "Missed: {} at {} ({} deadlines missed)",
                name,
                Zoned::now().strftime("%H:%M:%S%.3f"),
                exec.deadlines_missed.len(),
            );
        }
    }
}

/// Parse "HH:MM:SS" into a delay from now. If the time is past today, targets tomorrow.
fn parse_at(s: &str) -> Duration {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        eprintln!("Expected HH:MM:SS, got '{s}'");
        std::process::exit(1);
    }
    let h: u32 = parts[0].parse().unwrap_or_else(|_| {
        eprintln!("Invalid hours in '{s}'");
        std::process::exit(1);
    });
    let m: u32 = parts[1].parse().unwrap_or_else(|_| {
        eprintln!("Invalid minutes in '{s}'");
        std::process::exit(1);
    });
    let sec: u32 = parts[2].parse().unwrap_or_else(|_| {
        eprintln!("Invalid seconds in '{s}'");
        std::process::exit(1);
    });

    let now = Zoned::now();
    let today = now
        .date()
        .at(h as i8, m as i8, sec as i8, 0)
        .to_zoned(now.time_zone().clone())
        .unwrap_or_else(|e| {
            eprintln!("Invalid time '{s}': {e}");
            std::process::exit(1);
        });

    let target = if today > now {
        today
    } else {
        today
            .checked_add(jiff::SignedDuration::from_hours(24))
            .unwrap()
    };

    let sdur: jiff::SignedDuration = target.duration_since(&now);
    sdur.unsigned_abs()
}

/// Parse a human duration string like "1m", "30s", "1h30m5s", "500ms".
fn parse_duration(s: &str) -> Duration {
    let mut total = Duration::ZERO;
    let mut num_buf = String::new();
    let mut chars = s.chars().peekable();

    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            num_buf.push(c);
            chars.next();
        } else {
            let mut unit = String::new();
            while let Some(&u) = chars.peek() {
                if u.is_ascii_alphabetic() {
                    unit.push(u);
                    chars.next();
                } else {
                    break;
                }
            }

            if num_buf.is_empty() || unit.is_empty() {
                eprintln!("Invalid duration '{s}'");
                std::process::exit(1);
            }

            let n: u64 = num_buf.parse().unwrap_or_else(|_| {
                eprintln!("Invalid number in duration '{s}'");
                std::process::exit(1);
            });
            num_buf.clear();

            total += match unit.as_str() {
                "h" => Duration::from_secs(n * 3600),
                "m" => Duration::from_secs(n * 60),
                "s" => Duration::from_secs(n),
                "ms" => Duration::from_millis(n),
                _ => {
                    eprintln!("Unknown unit '{unit}' in duration '{s}'");
                    std::process::exit(1);
                }
            };
        }
    }

    if !num_buf.is_empty() {
        eprintln!("Trailing number without unit in '{s}'");
        std::process::exit(1);
    }
    if total.is_zero() {
        eprintln!("Duration must be > 0");
        std::process::exit(1);
    }
    total
}

fn fmt_duration(d: Duration) -> String {
    let nanos = d.as_nanos();
    if nanos < 1_000 {
        format!("{}ns", nanos)
    } else if nanos < 1_000_000 {
        format!("{:.1}µs", nanos as f64 / 1_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.3}ms", nanos as f64 / 1_000_000.0)
    } else {
        let secs = d.as_secs();
        let ms = d.subsec_millis();
        if ms == 0 {
            format!("{}s", secs)
        } else {
            format!("{}.{:03}s", secs, ms)
        }
    }
}
