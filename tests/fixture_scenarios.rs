#![cfg(feature = "serde")]

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

        let contents = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let fixture: Fixture = serde_json::from_str(&contents)
            .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));

        let actual = fixture.run();

        assert_eq!(
            fixture.expected.ticks.len(),
            actual.ticks.len(),
            "Fixture '{}' ({}): tick count mismatch — expected {}, got {}",
            fixture.name,
            path.display(),
            fixture.expected.ticks.len(),
            actual.ticks.len(),
        );

        for (i, (expected, got)) in fixture
            .expected
            .ticks
            .iter()
            .zip(actual.ticks.iter())
            .enumerate()
        {
            assert_eq!(
                expected,
                got,
                "Fixture '{}' ({}): tick {i} mismatch\n  expected: {expected:?}\n  actual:   {got:?}",
                fixture.name,
                path.display(),
            );
        }

        count += 1;
    }

    assert!(
        count > 0,
        "No fixture files found in {}",
        fixtures_dir.display()
    );
    eprintln!("Replayed {count} fixture(s) successfully");
}
