use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::rc::Rc;
use std::time::Duration;
use syncopate::fixture::{Fixture, TaskDef, TaskKindDef};
use syncopate::{Scheduler, SimClock, TaskBuilder, Window};

// Explicit fixture lists — add new fixtures here intentionally to include them
// in benchmark measurements. Directory scanning is intentionally avoided so that
// adding test fixtures does not silently change benchmark timings.
const PERIODIC_RELATIVE: &[&str] = &[
    "periodic/relative/100ms.json",
    "periodic/relative/100ms_150ms.json",
    "periodic/relative/50ms_33ms.json",
    "periodic/relative/100ms_20ms_with_delay.json",
];
const PERIODIC_ABSOLUTE: &[&str] = &[
    "periodic/absolute/100ms.json",
    "periodic/absolute/100ms_150ms.json",
];
const MIXED: &[&str] = &["mixed/basic.json", "mixed/burst.json", "mixed/multi.json"];

fn load_fixture(path: &str) -> Fixture {
    let full = format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), path);
    let contents = std::fs::read_to_string(&full).expect("fixture file");
    serde_json::from_str(&contents).expect("parse fixture")
}

fn load_fixtures(paths: &[&str]) -> Vec<Fixture> {
    paths.iter().map(|p| load_fixture(p)).collect()
}

fn task_from_def(td: &TaskDef) -> syncopate::task::Task {
    let window = Window::new(
        Duration::from_nanos(td.window_early_ns),
        Duration::from_nanos(td.window_late_ns),
    );
    match &td.kind {
        TaskKindDef::Relative {
            initial_delay_ns,
            schedule,
        } => TaskBuilder::every(Duration::from_nanos(td.period_ns))
            .window(window)
            .initial_delay(Duration::from_nanos(*initial_delay_ns))
            .schedule(*schedule)
            .name(&td.name)
            .on_miss(td.on_miss)
            .build(),
        TaskKindDef::Absolute { offset_ns } => {
            let mut b =
                TaskBuilder::every_absolute(Duration::from_nanos(td.period_ns)).window(window);
            if *offset_ns != 0 {
                b = b.offset(Duration::from_nanos(*offset_ns));
            }
            b.name(&td.name).on_miss(td.on_miss).build()
        }
    }
}

fn build_scheduler(fixture: &Fixture) -> (Rc<SimClock>, Scheduler<(), Rc<SimClock>>) {
    let clock = Rc::new(SimClock::new());
    let mut scheduler = Scheduler::new_with_clock(Rc::clone(&clock));
    for td in &fixture.input.tasks {
        scheduler.add_task(task_from_def(td));
    }
    (clock, scheduler)
}

/// Build schedulers with the initial tick already called (tasks ready).
fn setup_for_calculate_next_tick(
    fixtures: &[Fixture],
) -> Vec<(Rc<SimClock>, Scheduler<(), Rc<SimClock>>)> {
    fixtures
        .iter()
        .map(|f| {
            let (clock, mut sched) = build_scheduler(f);
            sched.tick();
            (clock, sched)
        })
        .collect()
}

/// Build schedulers with clock pre-advanced to the next deadline so tick()
/// fires tasks immediately.
fn setup_for_tick(fixtures: &[Fixture]) -> Vec<(Rc<SimClock>, Scheduler<(), Rc<SimClock>>)> {
    fixtures
        .iter()
        .map(|f| {
            let (clock, mut sched) = build_scheduler(f);
            sched.tick();
            if let Some(next) = sched.calculate_next_tick() {
                clock.advance(next);
            }
            (clock, sched)
        })
        .collect()
}

fn add_benches(c: &mut Criterion, group_name: &str, fixtures: &[Fixture]) {
    // calculate_next_tick is &self — build schedulers once and reuse them.
    let schedulers = setup_for_calculate_next_tick(fixtures);
    c.bench_function(&format!("{group_name}/calculate_next_tick"), |b| {
        b.iter(|| {
            for (_, sched) in &schedulers {
                black_box(sched.calculate_next_tick());
            }
        })
    });

    // tick() mutates state — rebuild schedulers (pre-advanced) each iteration.
    c.bench_function(&format!("{group_name}/tick"), |b| {
        b.iter_batched(
            || setup_for_tick(fixtures),
            |mut schedulers| {
                for (_, sched) in &mut schedulers {
                    black_box(sched.tick());
                }
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_add_task(c: &mut Criterion) {
    // Collect fixtures from all categories that have tasks added during runtime.
    let fixtures: Vec<Fixture> = [PERIODIC_RELATIVE, PERIODIC_ABSOLUTE, MIXED]
        .iter()
        .flat_map(|paths| load_fixtures(paths))
        .filter(|f| f.input.tasks.iter().any(|t| t.delay_ns > 0))
        .collect();

    if fixtures.is_empty() {
        return;
    }

    c.bench_function("add_task", |b| {
        b.iter_batched(
            || {
                fixtures
                    .iter()
                    .map(|f| {
                        // Build scheduler with only the immediate (delay=0) tasks.
                        let clock = Rc::new(SimClock::new());
                        let mut scheduler = Scheduler::new_with_clock(Rc::clone(&clock));
                        for td in f.input.tasks.iter().filter(|t| t.delay_ns == 0) {
                            scheduler.add_task(task_from_def(td));
                        }
                        scheduler.tick();

                        // Advance to the earliest deferred task's add time.
                        let first_delay = f
                            .input
                            .tasks
                            .iter()
                            .filter(|t| t.delay_ns > 0)
                            .map(|t| t.delay_ns)
                            .min()
                            .unwrap();
                        clock.advance(Duration::from_nanos(first_delay));

                        // Pre-build the deferred tasks.
                        let deferred: Vec<_> = f
                            .input
                            .tasks
                            .iter()
                            .filter(|t| t.delay_ns > 0)
                            .map(task_from_def)
                            .collect();

                        (scheduler, deferred)
                    })
                    .collect::<Vec<_>>()
            },
            |mut items| {
                for (scheduler, deferred) in &mut items {
                    for task in deferred.drain(..) {
                        let _ = black_box(scheduler.add_task(task));
                    }
                }
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_periodic_relative(c: &mut Criterion) {
    add_benches(c, "periodic_relative", &load_fixtures(PERIODIC_RELATIVE));
}

fn bench_periodic_absolute(c: &mut Criterion) {
    add_benches(c, "periodic_absolute", &load_fixtures(PERIODIC_ABSOLUTE));
}

fn bench_mixed(c: &mut Criterion) {
    add_benches(c, "mixed", &load_fixtures(MIXED));
}

criterion_group!(
    benches,
    bench_periodic_relative,
    bench_periodic_absolute,
    bench_mixed,
    bench_add_task
);
criterion_main!(benches);
