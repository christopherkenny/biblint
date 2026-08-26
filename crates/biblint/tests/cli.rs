use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn run(args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_biblint"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start biblint");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait for biblint")
}

fn write_config(contents: &str) -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("biblint-cli-{stamp}.toml"));
    std::fs::write(&path, contents).expect("write config");
    path.to_string_lossy().into_owned()
}

#[test]
fn format_stdin_is_canonical() {
    let output = run(&["format", "-"], "@ARTICLE{k,title=\"A\"}\n");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "@article{k,\n  title = \"A\"\n}\n"
    );
}

#[test]
fn check_json_reports_duplicate_keys() {
    let output = run(
        &["check", "-", "--output", "json"],
        "@article{A,title={x}}\n@article{a,title={y}}\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("duplicate_key"));
    assert!(stdout.contains("formatting"));
}

#[test]
fn unsafe_transforms_wait_for_unsafe_fixes() {
    let config = write_config("[format]\ngenerate-keys = true\n");
    let output = run(
        &["check", "-", "--fix", "--config", &config],
        "@article{old,author={Smith, John},year=2024,title={A study}}\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("formatting"));
    assert!(!stdout.contains("smith2024study"));

    let output = run(
        &["check", "-", "--fix", "--unsafe-fixes", "--config", &config],
        "@article{old,author={Smith, John},year=2024,title={A study}}\n",
    );
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("smith2024study")
    );
    std::fs::remove_file(config).expect("remove config");
}

#[test]
fn key_format_is_selected_in_configuration() {
    let config = write_config(
        "[lint]\nextend-select = [\"key_format\"]\n\n[lint.key-format]\nstyle = \"better-bibtex\"\n",
    );
    let output = run(
        &["check", "-", "--output", "json", "--config", &config],
        "@article{wrong,author={Smith, John},title={A Study of Things},year=2024}\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"rule\": \"key_format\""));
    assert!(stdout.contains("smith2024Study"));
    std::fs::remove_file(config).expect("remove config");
}

#[test]
fn configured_better_bibtex_formula_controls_generation() {
    let config = write_config(
        "[format]\ngenerate-keys = true\n\n[format.key-generation]\nformula = \"auth.lower + '-' + year\"\n",
    );
    let output = run(
        &["check", "-", "--fix", "--unsafe-fixes", "--config", &config],
        "@article{old,author={Smith, John},year=2024,title={A study}}\n",
    );
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("smith-2024")
    );
    std::fs::remove_file(config).expect("remove config");
}

#[test]
fn unsupported_formula_features_are_configuration_errors() {
    let config = write_config("[format.key-generation]\nformula = \"group('Methods') + auth\"\n");
    let output = run(
        &["check", "-", "--config", &config],
        "@article{key,author={Smith, John}}\n",
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("group"));
    assert!(stderr.contains("key-generation"));
    std::fs::remove_file(config).expect("remove config");
}
