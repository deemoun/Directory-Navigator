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

#[test]
fn draft_release_is_mac_only_and_write_permission_is_isolated() -> io::Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml");
    let text = fs::read_to_string(path)?;
    assert_draft_release_policy(&text);
    Ok(())
}

fn assert_draft_release_policy(text: &str) {
    // Git may check YAML out as CRLF on Windows; policy is independent of EOLs.
    let text = text.replace("\r\n", "\n");
    let targets = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("target: "))
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), 2);
    for target in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
        assert!(targets.contains(&target));
    }
    let (build_jobs, draft) = text.split_once("  draft:\n").expect("Dedicated draft job");
    assert!(!build_jobs.contains("contents: write"));
    assert!(!build_jobs.contains("git push"));
    assert!(draft.contains("needs: [validate, build]"));
    assert!(draft.contains("contents: write"));
    assert!(draft.contains("--verify-tag --draft"));
    assert!(draft.contains("--prerelease"));
    assert!(draft.contains("verify-assets"));
    assert!(draft.contains("--platform macos"));
    assert!(draft.contains("check-unused"));
    assert!(!text.contains("--clobber"));
    assert!(!text.contains("--draft=false"));
    assert!(!text.contains("git push --force"));
    assert!(text.contains("cancel-in-progress: false"));
    assert!(text.contains("type: string"));
    assert!(text.contains("git write-tree"));
}

#[test]
fn draft_release_policy_accepts_windows_and_mixed_line_endings() -> io::Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml");
    let lf = fs::read_to_string(path)?.replace("\r\n", "\n");
    assert_draft_release_policy(&lf);
    assert_draft_release_policy(&lf.replace('\n', "\r\n"));
    let mixed = lf
        .lines()
        .enumerate()
        .map(|(index, line)| format!("{line}{}", if index % 2 == 0 { "\r\n" } else { "\n" }))
        .collect::<String>();
    assert_draft_release_policy(&mixed);
    Ok(())
}

#[test]
fn linux_builds_are_independent_and_cannot_publish() -> io::Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/linux.yml");
    let text = fs::read_to_string(path)?;
    let targets = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("target: "))
        .collect::<Vec<_>>();
    assert_eq!(
        targets,
        ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]
    );
    assert!(text.contains("contents: read"));
    assert!(text.contains("--platform linux"));
    assert!(text.contains("scripts/release.py prepare"));
    assert!(text.contains("cargo clippy --locked --all-targets"));
    assert!(text.contains("cargo test --locked"));
    assert!(text.contains("cargo build --release --locked"));
    assert!(text.contains("linux-downloads"));
    for forbidden in [
        "contents: write",
        "persist-credentials: true",
        "git push",
        "git tag",
        "gh release",
        "needs: validate",
        "apple-darwin",
    ] {
        assert!(
            !text.contains(forbidden),
            "Unexpected Linux workflow capability: {forbidden}"
        );
    }
    Ok(())
}

#[test]
fn windows_builds_are_independent_and_package_the_executable() -> io::Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/windows.yml");
    let text = fs::read_to_string(path)?;
    for required in [
        "runs-on: windows-2022",
        "BUILD_TARGET: x86_64-pc-windows-msvc",
        "contents: read",
        "ref: ${{ github.sha }}",
        "type: string",
        "scripts/release.py prepare",
        "cargo fmt --check",
        "cargo clippy --locked --all-targets",
        "cargo test --locked",
        "cargo build --release --locked",
        "release/dn.exe",
        "--version",
        "$LASTEXITCODE -ne 0",
        "Compress-Archive",
        "Get-FileHash",
        "SHA256SUMS.txt",
        "LICENSE, README.md",
        "docs/USER_GUIDE.md",
        "windows-downloads",
        "actions/upload-artifact@",
        "retention-days: 30",
    ] {
        assert!(
            text.contains(required),
            "Missing Windows build requirement: {required}"
        );
    }
    for forbidden in [
        "contents: write",
        "persist-credentials: true",
        "git push",
        "git tag",
        "gh release",
        "needs:",
        "apple-darwin",
        "unknown-linux",
    ] {
        assert!(
            !text.contains(forbidden),
            "Unexpected Windows workflow dependency/capability: {forbidden}"
        );
    }
    Ok(())
}
