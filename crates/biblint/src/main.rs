use biblint_core::{
    Diagnostic, FixSafety, Rule, Severity, apply_fixes, check_source, check_sources,
    discover_files, format_source, load_settings,
};
use clap::{Args, Parser, Subcommand, ValueEnum};
use similar::TextDiff;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "biblint",
    version,
    about = "A deterministic BibTeX linter and formatter"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Check BibTeX files for syntax and lint issues.
    Check(CheckArgs),
    /// Canonically format BibTeX files.
    Format(FormatArgs),
    /// Explain a rule, or list all rules.
    Rule { name: Option<String> },
}

#[derive(Args, Debug)]
struct CheckArgs {
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,
    /// Include the canonical formatting check.
    #[arg(long)]
    format: bool,
    /// Apply safe formatting fixes in place.
    #[arg(long)]
    fix: bool,
    /// Permit configured formatting transforms that can change content or identity.
    #[arg(long, requires = "fix")]
    unsafe_fixes: bool,
    #[arg(long, value_enum, default_value = "text")]
    output: OutputFormat,
    /// Read configuration from this file instead of discovering biblint.toml.
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct FormatArgs {
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,
    /// Report files that would change without writing them.
    #[arg(long)]
    check: bool,
    /// Print a unified diff without writing files.
    #[arg(long, conflicts_with = "check")]
    diff: bool,
    /// Read configuration from this file instead of discovering biblint.toml.
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("biblint: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    match cli.command {
        Command::Check(args) => run_check(&args),
        Command::Format(args) => run_format(&args),
        Command::Rule { name } => {
            print_rules(name.as_deref())?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn run_check(args: &CheckArgs) -> Result<ExitCode, String> {
    let base = args.paths.first().map_or(Path::new("."), PathBuf::as_path);
    let (mut settings, _) = load_settings(base, args.config.as_deref())?;
    if args.format {
        settings
            .lint
            .extend_select
            .push(Rule::Formatting.name().to_string());
    }
    if is_stdin(&args.paths) {
        let mut source = read_stdin()?;
        let mut checked = check_source(&source, Path::new("<stdin>"), &settings);
        if args.fix {
            let fixed = apply_fixes(&source, &checked.diagnostics, args.unsafe_fixes);
            if fixed != source {
                if matches!(args.output, OutputFormat::Text) {
                    print!("{fixed}");
                }
                source = fixed;
                checked = check_source(&source, Path::new("<stdin>"), &settings);
            }
        }
        print_diagnostics(&checked.diagnostics, &source, args.output)?;
        if !checked.diagnostics.is_empty() {
            eprintln!("Found {} issue(s).", checked.diagnostics.len());
        }
        return Ok(exit_for_diagnostics(&checked.diagnostics));
    }

    let files = discover_files(&args.paths, &settings)?;
    let mut sources = Vec::new();
    for path in files {
        let mut source = fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        if args.fix {
            let checked = check_source(&source, &path, &settings);
            let fixed = apply_fixes(&source, &checked.diagnostics, args.unsafe_fixes);
            if fixed != source {
                fs::write(&path, &fixed)
                    .map_err(|error| format!("could not write {}: {error}", path.display()))?;
                source = fixed;
            }
        }
        sources.push((path, source));
    }
    let diagnostics = check_sources(&sources, &settings);

    match args.output {
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&diagnostics).map_err(|error| error.to_string())?
        ),
        OutputFormat::Text => {
            for diagnostic in &diagnostics {
                let source = sources
                    .iter()
                    .find(|(path, _)| path == &diagnostic.path)
                    .map_or("", |(_, source)| source.as_str());
                print_text_diagnostic(diagnostic, source, &sources);
            }
        }
    }
    if !diagnostics.is_empty() {
        eprintln!("Found {} issue(s).", diagnostics.len());
    }
    Ok(exit_for_diagnostics(&diagnostics))
}

fn run_format(args: &FormatArgs) -> Result<ExitCode, String> {
    let base = args.paths.first().map_or(Path::new("."), PathBuf::as_path);
    let (settings, _) = load_settings(base, args.config.as_deref())?;
    if is_stdin(&args.paths) {
        let source = read_stdin()?;
        let (parsed, result) = format_source(&source, &settings.format);
        if !parsed.errors.is_empty() {
            return Err(parsed.errors[0].message.clone());
        }
        if args.diff {
            print_diff("<stdin>", &source, &result.output);
        } else if !args.check {
            print!("{}", result.output);
        }
        return Ok(if args.check && result.output != source {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        });
    }

    let files = discover_files(&args.paths, &settings)?;
    let mut changed = 0usize;
    let mut had_errors = false;
    for path in files {
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let (parsed, result) = format_source(&source, &settings.format);
        if let Some(error) = parsed.errors.first() {
            eprintln!("{}:{}", path.display(), error.message);
            had_errors = true;
            continue;
        }
        if result.output == source {
            continue;
        }
        changed += 1;
        if args.diff {
            print_diff(&path.display().to_string(), &source, &result.output);
        } else if args.check {
            println!("Would reformat: {}", path.display());
        } else {
            fs::write(&path, result.output)
                .map_err(|error| format!("could not write {}: {error}", path.display()))?;
            println!("Formatted: {}", path.display());
        }
    }
    if had_errors {
        return Ok(ExitCode::from(2));
    }
    Ok(if (args.check || args.diff) && changed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn print_diff(path: &str, old: &str, new: &str) {
    println!("--- {path}");
    println!("+++ {path}");
    print!(
        "{}",
        TextDiff::from_lines(old, new)
            .unified_diff()
            .context_radius(3)
    );
}

fn print_diagnostics(
    diagnostics: &[Diagnostic],
    source: &str,
    output: OutputFormat,
) -> Result<(), String> {
    match output {
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(diagnostics).map_err(|error| error.to_string())?
        ),
        OutputFormat::Text => {
            for diagnostic in diagnostics {
                print_text_diagnostic(diagnostic, source, &[]);
            }
        }
    }
    Ok(())
}

fn print_text_diagnostic(diagnostic: &Diagnostic, source: &str, sources: &[(PathBuf, String)]) {
    let severity = match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Note => "note",
    };
    println!(
        "{severity}[{}]: {}",
        diagnostic.rule.name(),
        diagnostic.message
    );
    if diagnostic.rule == Rule::Formatting {
        println!("  --> {}", diagnostic.path.display());
    } else {
        let (line, column) = line_column(source, diagnostic.range.start);
        println!("  --> {}:{}:{}", diagnostic.path.display(), line, column);
    }
    if let Some(help) = &diagnostic.help {
        println!("  help: {help}");
    }
    for related in &diagnostic.related {
        let related_source = if related.path == diagnostic.path {
            source
        } else {
            sources
                .iter()
                .find(|(path, _)| path == &related.path)
                .map_or("", |(_, source)| source.as_str())
        };
        let (line, column) = line_column(related_source, related.range.start);
        println!(
            "  related: {}:{}:{}: {}",
            related.path.display(),
            line,
            column,
            related.message
        );
    }
}

fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .map_or(1, |tail| tail.chars().count() + 1);
    (line, column)
}

fn print_rules(name: Option<&str>) -> Result<(), String> {
    if let Some(name) = name {
        let rule = Rule::from_name(name).ok_or_else(|| format!("unknown rule '{name}'"))?;
        println!(
            "{}\n\n{}\n\nDefault: {}\nFix: {}",
            rule.name(),
            rule.summary(),
            rule.default_enabled(),
            rule.fix_safety().map_or("none", |safety| match safety {
                FixSafety::Safe => "safe",
                FixSafety::Unsafe => "unsafe",
            })
        );
        return Ok(());
    }
    for rule in Rule::ALL {
        println!("{:<24} {}", rule.name(), rule.summary());
    }
    Ok(())
}

fn exit_for_diagnostics(diagnostics: &[Diagnostic]) -> ExitCode {
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        ExitCode::from(2)
    } else if diagnostics.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn is_stdin(paths: &[PathBuf]) -> bool {
    paths.len() == 1 && paths[0] == Path::new("-")
}

fn read_stdin() -> Result<String, String> {
    let mut source = String::new();
    io::stdin()
        .read_to_string(&mut source)
        .map_err(|error| format!("could not read stdin: {error}"))?;
    Ok(source)
}
