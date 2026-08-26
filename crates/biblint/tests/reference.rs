use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/reference");
const PROJECT_FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/project-duplicates"
);
fn fixture(name: &str) -> PathBuf {
    Path::new(FIXTURES).join(name)
}

fn read_fixture(name: &str) -> String {
    fs::read_to_string(fixture(name)).unwrap_or_else(|error| {
        panic!("read fixture {name}: {error}");
    })
}

fn run_biblint(input: &str, config: &Path) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_biblint"))
        .args(["format", "-", "--config"])
        .arg(config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start biblint");
    child
        .stdin
        .take()
        .expect("biblint stdin")
        .write_all(input.as_bytes())
        .expect("write biblint input");
    child.wait_with_output().expect("wait for biblint")
}

fn run_biblint_file(path: &Path, current_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_biblint"))
        .args(["format", "--diff"])
        .arg(path)
        .current_dir(current_dir)
        .output()
        .expect("run biblint on fixture")
}

fn run_biblint_check_directory(path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_biblint"))
        .args(["check", "--output", "json"])
        .arg(path)
        .current_dir(std::env::temp_dir())
        .output()
        .expect("run biblint project check")
}

fn assert_fixture_case(input_name: &str, config_name: &str, expected_name: &str) {
    let input = read_fixture(input_name);
    let actual = run_biblint(&input, &fixture(config_name));
    assert!(
        actual.status.success(),
        "biblint failed: {}",
        String::from_utf8_lossy(&actual.stderr)
    );
    assert_eq!(
        String::from_utf8(actual.stdout).expect("biblint stdout"),
        read_fixture(expected_name),
        "biblint output changed for {input_name}"
    );
}

fn assert_key_generation_case() {
    let input = read_fixture("key-input.bib");
    let actual = run_biblint(&input, &fixture("key.toml"));
    assert!(
        actual.status.success(),
        "biblint failed: {}",
        String::from_utf8_lossy(&actual.stderr)
    );
    let actual = String::from_utf8(actual.stdout).expect("biblint key output");
    assert_eq!(actual, read_fixture("key.expected.bib"));
}

#[test]
fn canonical_transforms_match_fixture() {
    assert_fixture_case(
        "canonical-input.bib",
        "canonical.toml",
        "canonical.expected.bib",
    );
}

#[test]
fn cleanup_transforms_match_fixture() {
    assert_fixture_case("cleanup-input.bib", "cleanup.toml", "cleanup.expected.bib");
}

#[test]
fn value_transforms_match_fixture() {
    assert_fixture_case("value-input.bib", "value.toml", "value.expected.bib");
}

#[test]
fn duplicate_merge_matches_fixture() {
    assert_fixture_case("merge-input.bib", "merge.toml", "merge.expected.bib");
}

#[test]
fn wrapping_matches_fixture() {
    assert_fixture_case("wrap-input.bib", "wrap.toml", "wrap.expected.bib");
}

#[test]
fn unescaping_matches_fixture() {
    assert_fixture_case(
        "unescape-input.bib",
        "unescape.toml",
        "unescape.expected.bib",
    );
}

#[test]
fn url_encoding_matches_fixture() {
    assert_fixture_case("url-input.bib", "url.toml", "url.expected.bib");
}

#[test]
fn key_generation_uses_better_bibtex_formula() {
    assert_key_generation_case();
}

#[test]
fn project_check_reports_cross_file_duplicates() {
    let directory = Path::new(PROJECT_FIXTURES);
    let output = run_biblint_check_directory(directory);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).expect("project check stderr");
    assert_eq!(stderr.trim(), "Found 4 issue(s).");

    let stdout = String::from_utf8(output.stdout).expect("project check JSON");
    for rule in [
        "duplicate_key",
        "duplicate_doi",
        "duplicate_citation",
        "duplicate_abstract",
    ] {
        assert_eq!(
            stdout.matches(&format!("\"rule\": \"{rule}\"")).count(),
            1,
            "expected one cross-file {rule} diagnostic: {stdout}"
        );
    }
    assert!(stdout.contains("first.bib"));
    assert!(stdout.contains("second.bib"));
    assert!(stdout.contains("first use of"));
}

#[test]
fn discovered_configuration_is_independent_of_working_directory() {
    let input = fixture("deterministic/input.bib");
    let expected = read_fixture("deterministic/expected.bib");
    let package_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let first = run_biblint_file(&input, package_dir);
    let second = run_biblint_file(&input, &std::env::temp_dir());
    let expected_diff = expected
        .lines()
        .map(|line| format!("+{line}"))
        .collect::<Vec<_>>()
        .join("\n");

    for output in [&first, &second] {
        assert_eq!(output.status.code(), Some(1));
        assert!(
            output.stderr.is_empty(),
            "unexpected stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("    title = {A title}"));
        assert!(stdout.contains("    title = {Another title}"));
        assert!(stdout.contains(&expected_diff));
    }
    assert_eq!(first.stdout, second.stdout);
}
