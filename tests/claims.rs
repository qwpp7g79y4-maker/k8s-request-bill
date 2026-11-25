//! README and QUALITY claims, checked against the real binary and files.

use assert_cmd::Command;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn bin() -> Command {
    let mut cmd = Command::cargo_bin("k8s-request-bill").unwrap();
    cmd.current_dir(root());
    cmd
}

/// All `--long-flag` words in a text.
fn flags_in(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i + 2 < bytes.len() {
        let boundary = i == 0 || !(bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '-');
        if boundary && bytes[i] == '-' && bytes[i + 1] == '-' && bytes[i + 2].is_ascii_lowercase() {
            let mut j = i + 2;
            while j < bytes.len() && (bytes[j].is_ascii_lowercase() || bytes[j] == '-') {
                j += 1;
            }
            found.insert(
                bytes[i..j]
                    .iter()
                    .collect::<String>()
                    .trim_end_matches('-')
                    .to_string(),
            );
            i = j;
        } else {
            i += 1;
        }
    }
    found
}

/// README text without the lines that run `cargo` itself (its flags are not ours).
fn readme_without_cargo_lines() -> String {
    read("README.md")
        .lines()
        .filter(|l| !l.trim_start().starts_with("cargo "))
        .collect::<Vec<_>>()
        .join("\n")
}

fn help_text() -> String {
    let out = bin()
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap()
}

#[test]
fn every_flag_in_the_readme_exists() {
    let help = flags_in(&help_text());
    for flag in flags_in(&readme_without_cargo_lines()) {
        assert!(
            help.contains(&flag),
            "README mentions {flag}, but --help does not"
        );
    }
}

#[test]
fn every_flag_in_help_is_in_the_readme() {
    let readme = flags_in(&readme_without_cargo_lines());
    for flag in flags_in(&help_text()) {
        if flag == "--help" || flag == "--version" {
            continue;
        }
        assert!(
            readme.contains(&flag),
            "--help lists {flag}, README does not"
        );
    }
}

#[test]
fn the_rate_flags_are_documented_in_the_readme() {
    let readme = read("README.md");
    for flag in [
        "--cpu-hour",
        "--mem-gib-hour",
        "--node-hour",
        "--node-vcpu",
        "--node-gib",
        "--cpu-share",
        "--hours",
        "--currency",
    ] {
        assert!(readme.contains(flag), "README lacks {flag}");
    }
}

/// Commands from the README's `sh` blocks that start with `k8s-request-bill`,
/// continuation lines joined, with the bare sample file names pointed at `examples/`.
fn readme_commands() -> Vec<Vec<String>> {
    let readme = read("README.md");
    let mut commands = Vec::new();
    let mut in_sh = false;
    let mut current = String::new();
    for line in readme.lines() {
        if line.starts_with("```") {
            in_sh = line.trim() == "```sh";
            current.clear();
            continue;
        }
        if !in_sh {
            continue;
        }
        let trimmed = line.trim();
        if let Some(head) = trimmed.strip_suffix('\\') {
            current.push_str(head);
            current.push(' ');
            continue;
        }
        current.push_str(trimmed);
        let command = std::mem::take(&mut current);
        if command.starts_with("k8s-request-bill ") {
            let words = command.split_whitespace().skip(1).map(|w| match w {
                "pods.json" => "examples/pods.json".to_string(),
                "metrics.json" => "examples/metrics.json".to_string(),
                other => other.to_string(),
            });
            commands.push(words.collect());
        }
    }
    commands
}

#[test]
fn readme_commands_run_on_the_examples() {
    let commands = readme_commands();
    assert!(
        commands.len() >= 4,
        "found {} README commands",
        commands.len()
    );
    for args in commands {
        bin().args(&args).assert().success();
    }
}

#[test]
fn readme_commands_cover_both_rate_styles() {
    let all: Vec<String> = readme_commands().into_iter().flatten().collect();
    for flag in [
        "--cpu-hour",
        "--mem-gib-hour",
        "--node-hour",
        "--node-vcpu",
        "--node-gib",
        "--hours",
    ] {
        assert!(
            all.iter().any(|w| w == flag),
            "no README command uses {flag}"
        );
    }
}

#[test]
fn readme_output_block_is_the_golden_table() {
    let readme = read("README.md").replace("\r\n", "\n");
    let golden = read("examples/expected.txt").replace("\r\n", "\n");
    let table: Vec<&str> = golden.lines().take(6).collect();
    assert_eq!(table.len(), 6);
    assert!(
        readme.contains(&table.join("\n")),
        "README table differs from examples/expected.txt"
    );
}

#[test]
fn readme_sort_keys_are_accepted() {
    let readme = read("README.md");
    for key in [
        "name",
        "cpu-request",
        "memory-request",
        "cpu-over",
        "memory-over",
        "over-cost",
    ] {
        assert!(
            readme.contains(&format!("`{key}`")),
            "README lacks sort key {key}"
        );
        bin()
            .args([
                "examples/pods.json",
                "--usage",
                "examples/metrics.json",
                "--cpu-hour",
                "1",
                "--sort",
                key,
            ])
            .assert()
            .success();
    }
}

#[test]
fn readme_exit_codes_hold() {
    bin().arg("examples/pods.json").assert().code(0);
    bin().arg("examples/missing.json").assert().code(1);
    bin().arg("--nope").assert().code(2);
    assert!(read("README.md").contains("`0` success, `1` unreadable or invalid input"));
}

#[test]
fn licences_are_present_and_declared() {
    assert!(read("LICENSE-MIT").contains("MIT License"));
    assert!(read("LICENSE-APACHE").contains("Apache License"));
    assert!(read("Cargo.toml").contains("license = \"MIT OR Apache-2.0\""));
    assert!(read("README.md").contains("MIT OR Apache-2.0"));
}

#[test]
fn changelog_has_the_package_version() {
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(version, "0.1.0");
    let changelog = read("CHANGELOG.md");
    assert!(
        changelog.contains(&format!("## [{version}]")),
        "{changelog}"
    );
    assert!(changelog.contains("--node-hour") && changelog.contains("--cpu-hour"));
}

#[test]
fn no_network_crates_in_the_lockfile() {
    let lock = read("Cargo.lock");
    for name in [
        "reqwest",
        "hyper",
        "ureq",
        "tokio",
        "curl",
        "isahc",
        "kube",
        "k8s-openapi",
    ] {
        assert!(
            !lock.contains(&format!("name = \"{name}\"")),
            "{name} is in Cargo.lock"
        );
    }
}

fn count_tests_in(dir: &Path) -> usize {
    let mut total = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() && path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).unwrap();
            total += text.lines().filter(|l| l.trim() == "#[test]").count();
        }
    }
    total
}

#[test]
fn test_count_in_readme_and_quality_matches_the_suite() {
    let n = count_tests_in(&root().join("src")) + count_tests_in(&root().join("tests"));
    assert!(n >= 60, "only {n} tests");
    let claim = format!("{n} tests");
    assert!(
        read("README.md").contains(&claim),
        "README does not say {claim:?}"
    );
    assert!(
        read("QUALITY.md").contains(&claim),
        "QUALITY.md does not say {claim:?}"
    );
}
