use std::rc::Rc;
use std::time::Duration;

use serde_crate::{Deserialize, Serialize};

use crate::{
    MissedTickBehavior, PeriodicSchedule, Repeat, Scheduler, SimClock, TaskBuilder, Window,
};

// ── Fixture types ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "serde_crate")]
pub struct Fixture {
    pub name: String,
    pub description: String,
    pub input: ScenarioInput,
    pub expected: ScenarioOutput,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "serde_crate")]
pub struct ScenarioInput {
    pub tasks: Vec<TaskDef>,
    #[serde(default)]
    pub disruptions: Vec<DisruptionDef>,
    pub duration_ns: Option<u64>,
    #[serde(default)]
    pub wall_clock_offset_ns: u64,
    pub min_tick_interval_ns: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "serde_crate")]
pub struct TaskDef {
    pub name: String,
    pub period_ns: u64,
    #[serde(default)]
    pub window_early_ns: u64,
    #[serde(default)]
    pub window_late_ns: u64,
    #[serde(default)]
    pub delay_ns: u64,
    pub kind: TaskKindDef,
    #[serde(default)]
    pub on_miss: MissedTickBehavior,
    #[serde(default)]
    pub repeat: Option<Repeat>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "serde_crate", tag = "type")]
pub enum TaskKindDef {
    Relative {
        #[serde(default)]
        initial_delay_ns: u64,
        #[serde(default)]
        schedule: PeriodicSchedule,
    },
    Absolute {
        #[serde(default)]
        offset_ns: u64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(crate = "serde_crate")]
pub struct DisruptionDef {
    pub at_ns: u64,
    pub duration_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(crate = "serde_crate")]
pub struct ScenarioOutput {
    pub ticks: Vec<TickDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(crate = "serde_crate")]
pub struct TickDef {
    pub at_ns: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fired: Vec<FiredDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missed: Vec<MissedDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(crate = "serde_crate")]
pub struct FiredDef {
    pub task_name: String,
    pub drift_ns: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(crate = "serde_crate")]
pub struct MissedDef {
    pub task_name: String,
    pub deadlines_missed_ns: Vec<u64>,
}

// ── Replay engine ────────────────────────────────────────────────────────────

impl Fixture {
    pub fn run(&self) -> ScenarioOutput {
        replay(&self.input)
    }
}

pub fn replay(input: &ScenarioInput) -> ScenarioOutput {
    let clock = Rc::new(SimClock::new());

    if input.wall_clock_offset_ns != 0 {
        clock.jump_wall_clock(input.wall_clock_offset_ns as i64);
    }

    let mut scheduler = Scheduler::new_with_clock(Rc::clone(&clock));
    if let Some(ns) = input.min_tick_interval_ns {
        scheduler.set_min_tick_interval(Some(Duration::from_nanos(ns)));
    }

    // Track tasks with nonzero delay for deferred addition.
    let mut pending_tasks: Vec<(u64, &TaskDef)> =
        input.tasks.iter().map(|t| (t.delay_ns, t)).collect();
    pending_tasks.sort_by_key(|(delay, _)| *delay);

    // Add tasks with zero delay immediately.
    while pending_tasks.first().is_some_and(|(d, _)| *d == 0) {
        let (_, td) = pending_tasks.remove(0);
        add_task_def(&mut scheduler, td);
    }

    let duration_ns = input.duration_ns.unwrap_or_else(|| auto_duration(input));

    let mut ticks = Vec::new();
    let mut current_ns: u64 = 0;

    // Tick at t=0.
    record_tick(&scheduler.tick(), current_ns, &mut ticks);

    let mut disruptions: Vec<&DisruptionDef> = input.disruptions.iter().collect();
    disruptions.sort_by_key(|d| d.at_ns);
    let mut next_disruption = 0usize;

    while current_ns < duration_ns {
        // Check for disruption.
        if next_disruption < disruptions.len() {
            let d = disruptions[next_disruption];
            if current_ns >= d.at_ns {
                next_disruption += 1;
                clock.advance(Duration::from_nanos(d.duration_ns));
                current_ns += d.duration_ns;
                record_tick(&scheduler.tick(), current_ns, &mut ticks);
                continue;
            }
        }

        // Add pending tasks whose delay has been reached.
        while let Some(&(delay, _)) = pending_tasks.first() {
            if delay <= current_ns {
                let (_, td) = pending_tasks.remove(0);
                add_task_def(&mut scheduler, td);
            } else {
                break;
            }
        }

        // Calculate next tick.
        let sleep_dur = match scheduler.calculate_next_tick() {
            Some(d) => d,
            None => {
                if let Some(&(delay, _)) = pending_tasks.first() {
                    let step = delay.saturating_sub(current_ns).max(1);
                    clock.advance(Duration::from_nanos(step));
                    current_ns += step;
                    continue;
                }
                break;
            }
        };

        // Cap to remaining duration.
        let remaining = duration_ns - current_ns;
        let advance = (sleep_dur.as_nanos() as u64).min(remaining);
        if advance == 0 {
            clock.advance(Duration::from_nanos(1));
            current_ns += 1;
        } else {
            let mut target = current_ns + advance;

            // Check if a disruption fires before the advance target.
            if next_disruption < disruptions.len() {
                let d_at = disruptions[next_disruption].at_ns;
                if d_at < target && d_at > current_ns {
                    target = d_at;
                }
            }

            // Advance toward target, adding pending tasks along the way.
            while let Some(&(delay, _)) = pending_tasks.first() {
                if delay > current_ns && delay <= target {
                    let step = delay - current_ns;
                    clock.advance(Duration::from_nanos(step));
                    current_ns = delay;
                    let (_, td) = pending_tasks.remove(0);
                    add_task_def(&mut scheduler, td);
                } else {
                    break;
                }
            }

            // Advance remaining distance to target.
            let step = target - current_ns;
            if step > 0 {
                clock.advance(Duration::from_nanos(step));
                current_ns = target;
            }
        }

        let result = scheduler.tick();
        record_tick(&result, current_ns, &mut ticks);
    }

    ScenarioOutput { ticks }
}

fn add_task_def<C: crate::Clock>(scheduler: &mut Scheduler<(), C>, td: &TaskDef) {
    let window = Window::new(
        Duration::from_nanos(td.window_early_ns),
        Duration::from_nanos(td.window_late_ns),
    );
    let builder = match &td.kind {
        TaskKindDef::Relative {
            initial_delay_ns,
            schedule,
        } => TaskBuilder::every(Duration::from_nanos(td.period_ns))
            .window(window)
            .initial_delay(Duration::from_nanos(*initial_delay_ns))
            .schedule(*schedule),
        TaskKindDef::Absolute { offset_ns } => {
            let mut b =
                TaskBuilder::every_absolute(Duration::from_nanos(td.period_ns)).window(window);
            if *offset_ns != 0 {
                b = b.offset(Duration::from_nanos(*offset_ns));
            }
            b
        }
    };
    let mut builder = builder.name(&td.name).on_miss(td.on_miss);
    if let Some(repeat) = td.repeat {
        builder = builder.repeat(repeat);
    }
    let task = builder.build().unwrap();
    scheduler.add_task(task).unwrap();
}

fn auto_duration(input: &ScenarioInput) -> u64 {
    let longest_period = input
        .tasks
        .iter()
        .map(|t| t.period_ns)
        .max()
        .unwrap_or(500_000_000);

    let combined_period = input
        .tasks
        .iter()
        .map(|t| t.period_ns)
        .fold(1u64, lcm);

    let cap = longest_period.saturating_mul(10);
    let base = combined_period.saturating_mul(2).min(cap);

    let delay_adjusted = input
        .tasks
        .iter()
        .map(|t| t.delay_ns + t.period_ns * 2)
        .max()
        .unwrap_or(0);

    let disruption_end = input
        .disruptions
        .iter()
        .map(|d| d.at_ns + d.duration_ns)
        .max()
        .unwrap_or(0);

    base.max(delay_adjusted).max(disruption_end)
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn lcm(a: u64, b: u64) -> u64 {
    if a == 0 || b == 0 {
        return 0;
    }
    a / gcd(a, b) * b
}

fn record_tick(result: &crate::TickResult<'_>, at_ns: u64, ticks: &mut Vec<TickDef>) {
    let fired: Vec<FiredDef> = result
        .fired
        .iter()
        .map(|exec| {
            let name = exec.task.name.as_deref().unwrap_or("?");
            FiredDef {
                task_name: name.to_string(),
                drift_ns: exec.drift.as_nanos_signed() as i64,
            }
        })
        .collect();

    let missed: Vec<MissedDef> = result
        .missed
        .iter()
        .map(|miss| {
            let name = miss.task.name.as_deref().unwrap_or("?");
            MissedDef {
                task_name: name.to_string(),
                deadlines_missed_ns: miss
                    .deadlines_missed
                    .iter()
                    .map(|d| d.as_nanos() as u64)
                    .collect(),
            }
        })
        .collect();

    ticks.push(TickDef {
        at_ns,
        fired,
        missed,
    });
}
