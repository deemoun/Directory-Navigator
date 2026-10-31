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
fn release_includes_all_platforms_and_write_permission_is_isolated() -> io::Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml");
    let text = fs::read_to_string(path)?;
    assert_release_policy(&text);
    Ok(())
}

fn assert_release_policy(text: &str) {
    // Git may check YAML out as CRLF on Windows; policy is independent of EOLs.
    let text = text.replace("\r\n", "\n");
    let targets = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("target: "))
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), 5);
    for target in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
    ] {
        assert!(targets.contains(&target));
    }
    // Identify the publishing job by its operation, not its display name or ID.
    let (preamble, jobs) = text.split_once("\njobs:\n").expect("Jobs mapping");
    let mut boundaries = jobs
        .match_indices("  ")
        .filter_map(|(index, _)| {
            let line = jobs[index..].lines().next()?;
            let at_line_start = index == 0 || jobs.as_bytes()[index - 1] == b'\n';
            (at_line_start && !line[2..].starts_with(' ') && line.ends_with(':')).then_some(index)
        })
        .collect::<Vec<_>>();
    boundaries.push(jobs.len());
    let jobs = boundaries
        .windows(2)
        .map(|bounds| &jobs[bounds[0]..bounds[1]])
        .collect::<Vec<_>>();
    let publishers = jobs
        .iter()
        .filter(|job| job.contains("gh release create"))
        .collect::<Vec<_>>();
    assert_eq!(publishers.len(), 1, "Exactly one release publishing job");
    let publishing_job = *publishers[0];
    assert!(!preamble.contains("contents: write"));
    let build_jobs = jobs
        .iter()
        .filter(|job| !job.contains("gh release create"))
        .copied()
        .collect::<String>();
    assert!(!build_jobs.contains("contents: write"));
    assert!(!build_jobs.contains("persist-credentials: true"));
    assert!(!build_jobs.contains("git push"));
    assert!(publishing_job.contains("needs: [validate, build]"));
    assert!(publishing_job.contains("contents: write"));
    assert!(publishing_job.contains("--verify-tag --draft"));
    assert!(publishing_job.contains("--prerelease"));
    assert!(publishing_job.contains("verify-assets"));
    assert!(publishing_job.contains("--platform all"));
    let attach = publishing_job
        .lines()
        .find(|line| line.contains("gh release create"))
        .expect("Release attachment command");
    assert!(attach.contains("dist/*.tar.gz dist/*.zip"));
    assert!(!attach.contains("sha256"));
    assert!(!attach.contains("SHA256SUMS"));
    assert!(!attach.contains("dist/* "));
    assert!(preamble.contains("shell: bash"));
    assert!(build_jobs.contains("runs-on: ${{ matrix.os }}"));
    for check in [
        "cargo fmt --check",
        "cargo clippy --locked --all-targets",
        "cargo test --locked --target",
        "cargo build --release --locked --target",
    ] {
        assert!(build_jobs.contains(check));
    }
    assert!(publishing_job.contains("check-unused"));
    assert!(!text.contains("--clobber"));
    assert!(!text.contains("--draft=false"));
    assert!(!text.contains("git push --force"));
    assert!(text.contains("cancel-in-progress: false"));
    assert!(text.contains("type: string"));
    assert!(text.contains("git write-tree"));
}

fn release_workflow() -> io::Result<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml");
    // Normalize before editing fixtures as well as before checking policy.
    Ok(fs::read_to_string(path)?.replace("\r\n", "\n"))
}

fn line_ending_variants(lf: &str) -> [String; 3] {
    let mixed = lf
        .lines()
        .enumerate()
        .map(|(index, line)| format!("{line}{}", if index % 2 == 0 { "\r\n" } else { "\n" }))
        .collect::<String>();
    [lf.to_owned(), lf.replace('\n', "\r\n"), mixed]
}

#[test]
fn release_policy_accepts_windows_and_mixed_line_endings() -> io::Result<()> {
    for text in line_ending_variants(&release_workflow()?) {
        assert_release_policy(&text);
    }
    Ok(())
}

#[test]
fn release_policy_is_independent_of_publishing_job_name() -> io::Result<()> {
    let text = release_workflow()?;
    let renamed = text.replace("  release:\n", "  publish_downloads:\n");
    assert_ne!(renamed, text, "Renaming must exercise the policy");
    for variant in line_ending_variants(&renamed) {
        assert_release_policy(&variant);
    }
    Ok(())
}

#[test]
fn release_policy_rejects_unsafe_publishing_changes() -> io::Result<()> {
    let text = release_workflow()?;
    for unsafe_text in [
        text.replacen("contents: read", "contents: write", 1),
        text.replace("needs: [validate, build]", "needs: validate"),
        text.replace("--verify-tag --draft", "--verify-tag"),
        text.replace(
            "  build:\n",
            "  build:\n    permissions:\n      contents: write\n",
        ),
    ] {
        assert_ne!(unsafe_text, text, "Mutation must exercise the policy");
        for variant in line_ending_variants(&unsafe_text) {
            assert!(std::panic::catch_unwind(|| assert_release_policy(&variant)).is_err());
        }
    }
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
