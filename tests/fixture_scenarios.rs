#[cfg(feature = "serde")]
mod tests {
    use std::path::Path;

    use syncopate::fixture::Fixture;

    #[test]
    fn replay_all_fixtures() {
        let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        if !fixtures_dir.exists() {
            // No fixtures directory yet — not a failure.
            return;
        }

        let mut count = 0;
        for entry in std::fs::read_dir(&fixtures_dir).expect("failed to read fixtures directory") {
            let entry = entry.expect("failed to read directory entry");
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let contents =
                std::fs::read_to_string(&path).unwrap_or_else(|e| {
                    panic!("failed to read {}: {e}", path.display())
                });
            let fixture: Fixture = serde_json::from_str(&contents).unwrap_or_else(|e| {
                panic!("failed to parse {}: {e}", path.display())
            });

            let actual = fixture.run();

            if actual.events.len() != fixture.expected.events.len() {
                panic!(
                    "Fixture '{}' ({}): event count mismatch — expected {}, got {}\n\
                     Expected events: {:#?}\n\
                     Actual events:   {:#?}",
                    fixture.name,
                    path.display(),
                    fixture.expected.events.len(),
                    actual.events.len(),
                    fixture.expected.events,
                    actual.events,
                );
            }

            for (i, (expected, got)) in fixture
                .expected
                .events
                .iter()
                .zip(actual.events.iter())
                .enumerate()
            {
                assert_eq!(
                    expected, got,
                    "Fixture '{}' ({}): event {i} mismatch\n  expected: {expected:?}\n  actual:   {got:?}",
                    fixture.name,
                    path.display(),
                );
            }

            assert_eq!(
                fixture.expected.tick_times_ns, actual.tick_times_ns,
                "Fixture '{}' ({}): tick_times mismatch",
                fixture.name,
                path.display(),
            );

            count += 1;
        }

        if count > 0 {
            eprintln!("Replayed {count} fixture(s) successfully");
        }
    }
}
