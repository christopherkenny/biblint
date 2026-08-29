use std::io::Write;
use std::path::PathBuf;
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

fn temporary_directory() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("biblint-cli-test-{stamp}"));
    std::fs::create_dir_all(&path).expect("create temporary directory");
    path
}

fn run_file(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_biblint"))
        .args(args)
        .output()
        .expect("run biblint")
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
    let source = "@article{A,title={x}}\n@article{a,title={y}}\n";
    let output = run(&["check", "-", "--output", "json"], source);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("duplicate_key"));
    assert!(!stdout.contains("\"rule\": \"formatting\""));

    let output = run(&["check", "-", "--format", "--output", "json"], source);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"rule\": \"formatting\""));
    assert!(stdout.contains("\"severity\": \"note\""));
}

#[test]
fn check_format_diagnostic_is_file_scoped_in_text_output() {
    let output = run(&["check", "-", "--format"], "@ARTICLE{k,title=\"A\"}\n");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("note[formatting]: file would be reformatted"));
    assert!(stdout.contains("  --> <stdin>\n"));
    assert!(!stdout.contains("<stdin>:1:1"));
}

#[test]
fn unsafe_transforms_wait_for_unsafe_fixes() {
    let config = write_config("[format]\ngenerate-keys = true\n");
    let output = run(
        &["check", "-", "--format", "--fix", "--config", &config],
        "@article{old,author={Smith, John},year=2024,title={A study}}\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("formatting"));
    assert!(!stdout.contains("smith2024study"));

    let output = run(
        &[
            "check",
            "-",
            "--format",
            "--fix",
            "--unsafe-fixes",
            "--config",
            &config,
        ],
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
        &[
            "check",
            "-",
            "--format",
            "--fix",
            "--unsafe-fixes",
            "--config",
            &config,
        ],
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
fn updates_markdown_family_citations_when_generating_keys() {
    for extension in ["md", "qmd", "Rmd"] {
        let directory = temporary_directory();
        let bib = directory.join("references.bib");
        let markdown = directory.join(format!("manuscript.{extension}"));
        let config = directory.join("biblint.toml");
        std::fs::write(
            &bib,
            "@article{old,author={Smith, John},year=2024,title={A study}}\n",
        )
        .expect("write BibTeX");
        std::fs::write(
            &markdown,
            "See [@old; @other] and @old.\n\n```text\n[@old]\n```\n",
        )
        .expect("write Markdown");
        std::fs::write(&config, "[format]\ngenerate-keys = true\n").expect("write config");

        let args = vec![
            "check".to_string(),
            bib.to_string_lossy().into_owned(),
            "--format".to_string(),
            "--fix".to_string(),
            "--unsafe-fixes".to_string(),
            "--update-markdown".to_string(),
            markdown.to_string_lossy().into_owned(),
            "--config".to_string(),
            config.to_string_lossy().into_owned(),
        ];
        let output = run_file(&args);
        assert!(
            output.status.success(),
            "biblint failed for .{extension}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&std::fs::read(&bib).expect("read BibTeX"))
                .contains("@article{smith2024study,")
        );
        assert_eq!(
            std::fs::read_to_string(&markdown).expect("read Markdown"),
            "See [@smith2024study; @other] and @smith2024study.\n\n```text\n[@old]\n```\n"
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("Updated 2 citation(s)"));
        std::fs::remove_dir_all(directory).expect("remove temporary directory");
    }
}

#[test]
fn update_markdown_rejects_other_extensions() {
    let directory = temporary_directory();
    let bib = directory.join("references.bib");
    let text = directory.join("manuscript.txt");
    let config = directory.join("biblint.toml");
    std::fs::write(
        &bib,
        "@article{old,author={Smith, John},year=2024,title={A study}}\n",
    )
    .expect("write BibTeX");
    std::fs::write(&text, "[@old]\n").expect("write text");
    std::fs::write(&config, "[format]\ngenerate-keys = true\n").expect("write config");

    let args = vec![
        "check".to_string(),
        bib.to_string_lossy().into_owned(),
        "--format".to_string(),
        "--fix".to_string(),
        "--unsafe-fixes".to_string(),
        "--update-markdown".to_string(),
        text.to_string_lossy().into_owned(),
        "--config".to_string(),
        config.to_string_lossy().into_owned(),
    ];
    let output = run_file(&args);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("only supports .md, .qmd, and .Rmd"));
    assert_eq!(
        std::fs::read_to_string(&text).expect("read text"),
        "[@old]\n"
    );
    std::fs::remove_dir_all(directory).expect("remove temporary directory");
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
