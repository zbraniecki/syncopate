use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use std::rc::Rc;
use std::time::Duration;
use syncopate::fixture::{Fixture, TaskKindDef};
use syncopate::{Scheduler, SimClock, TaskBuilder, Window};

fn load_fixtures_from(subdir: &str) -> Vec<Fixture> {
    let dir = format!(
        "{}/tests/fixtures/{}",
        env!("CARGO_MANIFEST_DIR"),
        subdir
    );
    let mut fixtures = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("fixtures dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let contents = std::fs::read_to_string(&path).expect("fixture file");
        let fixture: Fixture = serde_json::from_str(&contents).expect("parse fixture");
        fixtures.push(fixture);
    }
    fixtures
}

fn build_scheduler(fixture: &Fixture) -> (Rc<SimClock>, Scheduler<(), Rc<SimClock>>) {
    let clock = Rc::new(SimClock::new());
    let mut scheduler = Scheduler::new_with_clock(Rc::clone(&clock));
    for td in &fixture.input.tasks {
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
        let task = builder.name(&td.name).on_miss(td.on_miss).build().unwrap();
        scheduler.add_task(task).unwrap();
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
fn setup_for_tick(
    fixtures: &[Fixture],
) -> Vec<(Rc<SimClock>, Scheduler<(), Rc<SimClock>>)> {
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

fn bench_periodic_relative(c: &mut Criterion) {
    let fixtures = load_fixtures_from("periodic/relative");
    add_benches(c, "periodic_relative", &fixtures);
}

fn bench_periodic_absolute(c: &mut Criterion) {
    let fixtures = load_fixtures_from("periodic/absolute");
    add_benches(c, "periodic_absolute", &fixtures);
}

fn bench_mixed(c: &mut Criterion) {
    let fixtures = load_fixtures_from("mixed");
    add_benches(c, "mixed", &fixtures);
}

criterion_group!(benches, bench_periodic_relative, bench_periodic_absolute, bench_mixed);
criterion_main!(benches);
