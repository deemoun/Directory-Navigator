//! All checked-in Actions must require an explicit dispatch, including future workflows.
use std::{fs, io, path::Path};
#[test]
fn every_workflow_is_dispatch_only() -> io::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    let mut checked = 0;
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if !matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("yml" | "yaml")
        ) {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let lines = text.lines().collect::<Vec<_>>();
        let starts = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| **l == "on:")
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        assert_eq!(
            starts.len(),
            1,
            "{} must have one explicit on: mapping",
            path.display()
        );
        let mut events = Vec::new();
        for line in &lines[starts[0] + 1..] {
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            if !line.starts_with(' ') {
                break;
            }
            let indent = line.len() - line.trim_start().len();
            if indent == 2 {
                events.push(line.trim().to_owned());
            }
        }
        assert_eq!(
            events,
            ["workflow_dispatch:"],
            "{} must not run automatically",
            path.display()
        );
        checked += 1;
    }
    assert!(
        checked >= 2,
        "Expected checks and cross-platform build workflows"
    );
    Ok(())
}
