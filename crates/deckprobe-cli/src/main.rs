use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use base64::Engine as _;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum, error::ErrorKind};
use deckprobe_core::{
    BoxedProbeReader, Budget, BudgetOverrides, Confidence, DeckProbeError, MemorySource,
    ProbeLevel, ProbeOptions, ProbeSource,
};
use deckprobe_engine::{
    error_exit_code, error_report, formats_report, probe, report_schema, targets_report,
    values_report,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(
    name = "deckprobe",
    version,
    about = "Inspect PDF, Microsoft Office, and Apple iWork documents with bounded probes",
    long_about = "Inspect PDF, Microsoft Office, and modern Apple iWork documents with bounded, target-driven probes.\n\nDeckProbe 2.2 sends local files, stdin bytes, and JSONL inputs through the same source-independent engine. It routes by the supplied filename extension, verifies that the container and internal document type agree, reads only the paths needed for the requested targets, and writes schema-v2 JSON reports to standard output.",
    override_usage = "deckprobe [OPTIONS] <INPUT>\n       deckprobe --jsonl [OPTIONS]\n       deckprobe <COMMAND> [OPTIONS]",
    subcommand_help_heading = "Discovery commands",
    subcommand_value_name = "COMMAND",
    after_help = "Run 'deckprobe --help' for examples and exit-status details.",
    after_long_help = "EXAMPLES:\n  Inspect a local document:\n    deckprobe report.pdf\n\n  Probe raw stdin bytes using a logical filename for format routing:\n    cat report.pdf | deckprobe -n report.pdf -\n\n  Probe one path or base64 payload per JSONL record:\n    printf '%s\\n' '{\"path\":\"report.pdf\"}' | deckprobe --jsonl\n\n  Request several targets with short aliases and compact output:\n    deckprobe -t title,page_count --view values report.pdf\n\n  Combine target presets in one probe:\n    deckprobe -l m -t @summary,@security -p deck.pptx\n\n  Discover supported formats or targets:\n    deckprobe formats --pretty\n    deckprobe targets pdf --pretty\n\n  Generate a man page from this command model:\n    deckprobe generate man > deckprobe.1\n    deckprobe generate man -d ./man\n\nEXIT STATUS:\n  0  Success (including unresolved targets unless --strict is used)\n  1  Invalid request or unsupported target\n  2  Command-line syntax error or source I/O error\n  3  Unsupported or unrecognized input format\n  4  Malformed input or probe budget exceeded\n  5  Unresolved requested target with --strict\n  6  Internal parser or report-serialization failure"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Local file to inspect, or '-' to read document bytes from stdin.
    #[arg(
        value_name = "INPUT",
        conflicts_with = "jsonl",
        long_help = "Local PDF, Microsoft Office, or modern Apple iWork file to inspect. Use '-' to read one document from stdin together with -n/--stdin-name. The normalized logical filename extension selects the format path; content is then verified."
    )]
    input: Option<PathBuf>,

    /// Logical filename, including extension, for raw stdin bytes.
    #[arg(
        short = 'n',
        long,
        value_name = "NAME",
        conflicts_with = "jsonl",
        help_heading = "Input interpretation"
    )]
    stdin_name: Option<String>,

    /// Read one input descriptor per line from stdin and emit one JSON report per line.
    #[arg(
        long,
        conflicts_with_all = ["input", "pretty", "stdin_name"],
        help_heading = "Input interpretation",
        long_help = "Read JSONL records from stdin. Each non-empty line is either a JSON string containing a local path, an object {\"path\":\"...\"}, or an object {\"name\":\"report.pdf\",\"data_base64\":\"...\"}. Output is always compact JSONL and processing continues after a per-record error."
    )]
    jsonl: bool,

    /// Target names or selector presets. Repeatable and comma-separated.
    #[arg(
        short = 't',
        long,
        default_value = "@default",
        value_name = "SELECTOR",
        value_delimiter = ',',
        action = clap::ArgAction::Append,
        help_heading = "Target selection",
        long_help = "Target names, unambiguous short target names, or selector presets. Repeat the option or separate values with commas.\n\nSelectors:\n  @header   Header-level identity targets\n  @default  Driver defaults for --probe-level\n  @summary  Identity, common metadata, and primary structure\n  @security Encryption, macros, signatures, and active-content signals\n  @structure Format-owned structural targets\n  @assets   Asset, preview, media, font, and embedded-object targets\n  @quality  Integrity and conformance targets\n  @format   Format-specific targets available at --probe-level\n  @all      Every target available at --probe-level"
    )]
    targets: Vec<String>,

    /// Optional targets returned only when selected paths already produce them.
    #[arg(
        short = 'o',
        long,
        value_name = "SELECTOR",
        value_delimiter = ',',
        action = clap::ArgAction::Append,
        help_heading = "Target selection"
    )]
    optional_targets: Vec<String>,

    /// Probe budget and path-depth profile.
    #[arg(
        short = 'l',
        long,
        visible_alias = "level",
        default_value = "metadata",
        value_parser = ["header", "h", "l0", "0", "metadata", "m", "l1", "1", "deep", "d", "l2", "2"],
        value_name = "LEVEL",
        help_heading = "Resource limits",
        long_help = "Probe budget and path-depth profile. 'header' only inspects container identity, 'metadata' reads bounded metadata paths, and 'deep' enables higher-cost paths. Alias: --level."
    )]
    probe_level: String,

    /// Required minimum confidence.
    #[arg(
        short = 'c',
        long,
        visible_alias = "confidence",
        default_value = "high",
        value_parser = ["low", "l", "medium", "m", "high", "h", "exact", "x"],
        value_name = "CONFIDENCE",
        help_heading = "Target selection",
        long_help = "Required minimum confidence. Alias: --confidence."
    )]
    minimum_confidence: String,

    /// Override minimum confidence for one target. Repeatable and comma-separated.
    #[arg(
        short = 'C',
        long,
        value_name = "TARGET=CONFIDENCE",
        value_delimiter = ',',
        action = clap::ArgAction::Append,
        help_heading = "Target selection"
    )]
    target_confidence: Vec<String>,

    /// Disable zero-additional-path collection for optional targets.
    #[arg(short = 'N', long, help_heading = "Target selection")]
    no_piggyback: bool,

    /// Verify that the detected driver or profile matches this value.
    #[arg(
        short = 'f',
        long,
        value_name = "FORMAT",
        help_heading = "Input interpretation",
        long_help = "Verify that the detected driver or profile matches this value. This option does not bypass signature or extension-based routing."
    )]
    input_format: Option<String>,

    /// Driver option in namespaced key=value form. Repeatable.
    #[arg(
        short = 'O',
        long = "format-option",
        visible_alias = "option",
        value_name = "KEY=VALUE",
        help_heading = "Input interpretation",
        long_help = "Driver option in namespaced key=value form. Repeatable. Alias: --option."
    )]
    format_options: Vec<String>,

    /// Maximum physical bytes read by probe paths.
    #[arg(
        short = 'b',
        long,
        alias = "probesize",
        value_name = "BYTES",
        help_heading = "Resource limits"
    )]
    probe_size: Option<u64>,

    /// Maximum cumulative decompressed bytes.
    #[arg(
        short = 'x',
        long,
        value_name = "BYTES",
        help_heading = "Resource limits"
    )]
    max_expanded_bytes: Option<u64>,

    /// Maximum ZIP/OPC entry count.
    #[arg(
        short = 'e',
        long,
        value_name = "COUNT",
        help_heading = "Resource limits"
    )]
    max_archive_entries: Option<usize>,

    /// Wall-clock budget in milliseconds.
    #[arg(
        short = 'T',
        long,
        value_name = "MILLISECONDS",
        help_heading = "Resource limits"
    )]
    timeout_ms: Option<u64>,

    /// Exit non-zero when any requested target is unresolved.
    #[arg(short = 's', long, help_heading = "Output and execution")]
    strict: bool,

    /// Pretty-print JSON output from probes and discovery commands.
    #[arg(
        short = 'p',
        long,
        global = true,
        help_heading = "Output and execution"
    )]
    pretty: bool,

    /// Print the execution plan without running non-header paths.
    #[arg(
        short = 'P',
        long,
        visible_alias = "plan",
        help_heading = "Output and execution",
        long_help = "Print the execution plan without running non-header paths. Alias: --plan."
    )]
    plan_only: bool,

    /// Include non-deterministic wall-clock timing in the JSON cost object.
    #[arg(long, help_heading = "Output and execution")]
    telemetry: bool,

    /// Select the full evidence report or a compact target-to-value envelope.
    #[arg(
        long,
        value_enum,
        default_value = "full",
        value_name = "VIEW",
        help_heading = "Output and execution"
    )]
    view: OutputView,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List supported format profiles and driver boundaries.
    #[command(
        long_about = "List supported format profiles, their owning drivers, and the current support boundary as JSON.",
        after_long_help = "EXAMPLES:\n  deckprobe formats\n  deckprobe formats --pretty"
    )]
    Formats,
    /// List targets and format-specific parameters for a profile.
    #[command(
        long_about = "List target specifications and format-specific options for one format profile as JSON.",
        after_long_help = "EXAMPLES:\n  deckprobe targets --format pdf\n  deckprobe targets pptx --pretty"
    )]
    Targets {
        /// Format driver or profile to describe.
        #[arg(value_name = "FORMAT", conflicts_with = "format_option")]
        format: Option<String>,

        /// Format driver or profile to describe.
        #[arg(
            short = 'f',
            long = "format",
            id = "format_option",
            value_name = "FORMAT",
            long_help = "Format driver or profile to describe. Accepted names include pdf, word/docx/docm, excel/xlsx/xlsm, powerpoint/pptx/pptm, keynote/key, numbers, pages, and legacy/doc/xls/ppt."
        )]
        format_option: Option<String>,
    },
    /// Generate reference artifacts from the live command model.
    #[command(
        long_about = "Generate reference artifacts from the live command model.",
        after_long_help = "EXAMPLES:\n  deckprobe generate man > deckprobe.1\n  man ./deckprobe.1\n\n  deckprobe generate man -d ./man"
    )]
    Generate {
        #[arg(value_enum)]
        artifact: GenerateArtifact,
        #[arg(short = 'd', long, value_name = "DIRECTORY")]
        output_dir: Option<PathBuf>,
    },
    /// Print the report JSON Schema bundled with this binary.
    #[command(after_long_help = "EXAMPLES:\n  deckprobe schema\n  deckprobe schema --pretty")]
    Schema,
    /// Generate shell completion source from the live command model.
    #[command(
        after_long_help = "EXAMPLES:\n  deckprobe completion bash > deckprobe.bash\n  deckprobe completion zsh > _deckprobe"
    )]
    Completion {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum GenerateArtifact {
    /// A roff manual page suitable for man(1).
    Man,
}

#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
enum OutputView {
    Full,
    Values,
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            emit_cli_error(2, "CLI_SYNTAX", error.to_string(), false);
            return ExitCode::from(2);
        }
    };
    let pretty = cli.pretty;
    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            let exit_code = error_exit_code(&error);
            if let Err(serialization_error) = write_json(&error_report(&error), pretty) {
                eprintln!("deckprobe: {serialization_error}");
            }
            ExitCode::from(exit_code)
        }
    }
}

fn run(cli: Cli) -> deckprobe_core::Result<u8> {
    match &cli.command {
        Some(Command::Formats) => {
            write_json(&formats_report(), cli.pretty)?;
            return Ok(0);
        }
        Some(Command::Targets {
            format,
            format_option,
        }) => {
            let requested_format = format
                .as_deref()
                .or(format_option.as_deref())
                .unwrap_or("pptx");
            write_json(&targets_report(requested_format)?, cli.pretty)?;
            return Ok(0);
        }
        Some(Command::Generate {
            artifact: GenerateArtifact::Man,
            output_dir,
        }) => {
            print_man_pages(output_dir.as_deref())?;
            return Ok(0);
        }
        Some(Command::Schema) => {
            write_json(&report_schema()?, cli.pretty)?;
            return Ok(0);
        }
        Some(Command::Completion { shell }) => {
            let mut command = Cli::command();
            clap_complete::generate(*shell, &mut command, "deckprobe", &mut std::io::stdout());
            return Ok(0);
        }
        None => {}
    }

    let options = probe_options(&cli)?;
    if cli.jsonl {
        return run_jsonl(&cli, &options);
    }

    let input = cli
        .input
        .as_ref()
        .ok_or_else(|| DeckProbeError::InvalidRequest("missing input file".to_owned()))?;
    let source: std::sync::Arc<dyn ProbeSource> = if input.as_os_str() == "-" {
        let name = cli.stdin_name.as_deref().ok_or_else(|| {
            DeckProbeError::InvalidRequest(
                "--stdin-name with a filename extension is required when INPUT is '-'".to_owned(),
            )
        })?;
        let bytes = read_stdin_document(&options)?;
        std::sync::Arc::new(MemorySource::with_kind(name, "stdin", bytes))
    } else {
        if cli.stdin_name.is_some() {
            return Err(DeckProbeError::InvalidRequest(
                "--stdin-name is only valid when INPUT is '-'".to_owned(),
            ));
        }
        std::sync::Arc::new(PathSource::open(input, None)?)
    };

    let report = probe(source, options)?;
    write_probe_report(&report, cli.view, cli.pretty)?;
    Ok(if cli.strict && report.status == "partial" {
        5
    } else {
        0
    })
}

fn probe_options(cli: &Cli) -> deckprobe_core::Result<ProbeOptions> {
    Ok(ProbeOptions {
        targets: cli.targets.clone(),
        optional_targets: cli.optional_targets.clone(),
        level: ProbeLevel::parse(&cli.probe_level).expect("clap validates probe level"),
        minimum_confidence: Confidence::parse(&cli.minimum_confidence)
            .expect("clap validates confidence"),
        target_confidence: parse_target_confidence(&cli.target_confidence)?,
        allow_piggyback: !cli.no_piggyback,
        format_options: parse_key_values(&cli.format_options, "format option")?,
        input_format: cli.input_format.clone(),
        plan_only: cli.plan_only,
        telemetry: cli.telemetry,
        budget: BudgetOverrides {
            max_physical_bytes: cli.probe_size,
            max_expanded_bytes: cli.max_expanded_bytes,
            max_archive_entries: cli.max_archive_entries,
            timeout_ms: cli.timeout_ms,
        },
    })
}

fn parse_target_confidence(
    values: &[String],
) -> deckprobe_core::Result<BTreeMap<String, Confidence>> {
    let raw = parse_key_values(values, "target confidence")?;
    raw.into_iter()
        .map(|(target, value)| {
            let confidence = Confidence::parse(&value).ok_or_else(|| {
                DeckProbeError::InvalidRequest(format!(
                    "invalid confidence in target override {target}={value}"
                ))
            })?;
            if confidence == Confidence::None {
                return Err(DeckProbeError::InvalidRequest(format!(
                    "target confidence cannot be none: {target}={value}"
                )));
            }
            Ok((target, confidence))
        })
        .collect()
}

fn parse_key_values(
    values: &[String],
    label: &str,
) -> deckprobe_core::Result<BTreeMap<String, String>> {
    let mut output = BTreeMap::new();
    for value in values {
        let (key, setting) = value.split_once('=').ok_or_else(|| {
            DeckProbeError::InvalidRequest(format!("{label} must be key=value: {value}"))
        })?;
        if key.is_empty() || setting.is_empty() {
            return Err(DeckProbeError::InvalidRequest(format!(
                "invalid {label}: {value}"
            )));
        }
        output.insert(key.to_owned(), setting.to_owned());
    }
    Ok(output)
}

fn read_stdin_document(options: &ProbeOptions) -> deckprobe_core::Result<Vec<u8>> {
    let mut budget = Budget::for_level(options.level);
    options.budget.apply_to(&mut budget);
    let limit = budget.max_physical_bytes;
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(DeckProbeError::BudgetExceeded(format!(
            "stdin size exceeds physical read budget {limit}"
        )));
    }
    Ok(bytes)
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum JsonlInput {
    Path(String),
    Record {
        path: Option<PathBuf>,
        name: Option<String>,
        #[serde(alias = "base64")]
        data_base64: Option<String>,
    },
}

fn run_jsonl(cli: &Cli, options: &ProbeOptions) -> deckprobe_core::Result<u8> {
    let stdin = std::io::stdin();
    let mut exit_code = 0;
    for (index, line) in stdin.lock().lines().enumerate() {
        let line_number = index + 1;
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let outcome = serde_json::from_str::<JsonlInput>(&line)
            .map_err(|error| {
                DeckProbeError::InvalidRequest(format!(
                    "JSONL line {line_number} is invalid: {error}"
                ))
            })
            .and_then(|input| jsonl_source(input, options))
            .and_then(|source| probe(source, options.clone()));
        match outcome {
            Ok(report) => {
                let value = match cli.view {
                    OutputView::Full => serde_json::to_value(&report),
                    OutputView::Values => Ok(values_report(&report)),
                }
                .map_err(|error| {
                    DeckProbeError::Parser(format!("cannot serialize report: {error}"))
                })?;
                write_json(&value, false)?;
                if cli.strict && report.status == "partial" {
                    exit_code = exit_code.max(5);
                }
            }
            Err(error) => {
                exit_code = exit_code.max(error_exit_code(&error));
                write_json(&error_report(&error), false)?;
            }
        }
    }
    Ok(exit_code)
}

fn jsonl_source(
    input: JsonlInput,
    options: &ProbeOptions,
) -> deckprobe_core::Result<std::sync::Arc<dyn ProbeSource>> {
    match input {
        JsonlInput::Path(path) => Ok(std::sync::Arc::new(PathSource::open(path, None)?)),
        JsonlInput::Record {
            path: Some(path),
            name,
            data_base64: None,
        } => Ok(std::sync::Arc::new(PathSource::open(path, name)?)),
        JsonlInput::Record {
            path: None,
            name: Some(name),
            data_base64: Some(data),
        } => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|error| {
                    DeckProbeError::InvalidRequest(format!("invalid JSONL base64 payload: {error}"))
                })?;
            let mut budget = Budget::for_level(options.level);
            options.budget.apply_to(&mut budget);
            if bytes.len() as u64 > budget.max_physical_bytes {
                return Err(DeckProbeError::BudgetExceeded(format!(
                    "JSONL byte payload size {} exceeds physical read budget {}",
                    bytes.len(),
                    budget.max_physical_bytes
                )));
            }
            Ok(std::sync::Arc::new(MemorySource::with_kind(
                name,
                "jsonl_bytes",
                bytes,
            )))
        }
        _ => Err(DeckProbeError::InvalidRequest(
            "JSONL object must contain either path, or name plus data_base64".to_owned(),
        )),
    }
}

struct PathSource {
    path: PathBuf,
    display_name: String,
    file_size: u64,
}

impl PathSource {
    fn open(
        path: impl Into<PathBuf>,
        display_name: Option<String>,
    ) -> deckprobe_core::Result<Self> {
        let path = path.into();
        let metadata = std::fs::metadata(&path)?;
        if !metadata.is_file() {
            return Err(DeckProbeError::InvalidRequest(format!(
                "input is not a regular file: {}",
                path.display()
            )));
        }
        let display_name = display_name.unwrap_or_else(|| {
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("<input>")
                .to_owned()
        });
        Ok(Self {
            path,
            display_name,
            file_size: metadata.len(),
        })
    }
}

impl ProbeSource for PathSource {
    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn source_kind(&self) -> &str {
        "local_file"
    }

    fn len(&self) -> u64 {
        self.file_size
    }

    fn open(&self) -> deckprobe_core::Result<BoxedProbeReader> {
        Ok(Box::new(File::open(&self.path)?))
    }
}

fn write_probe_report(
    report: &deckprobe_core::ProbeReport,
    view: OutputView,
    pretty: bool,
) -> deckprobe_core::Result<()> {
    match view {
        OutputView::Full => write_json(report, pretty),
        OutputView::Values => write_json(&values_report(report), pretty),
    }
}

fn print_man_pages(output_dir: Option<&Path>) -> deckprobe_core::Result<()> {
    if let Some(output_dir) = output_dir {
        std::fs::create_dir_all(output_dir)?;
        let mut command = Cli::command().disable_help_subcommand(true);
        command.build();
        generate_man_page_tree(command, output_dir)?;
    } else {
        let (_, page) = render_man_page(Cli::command())?;
        std::io::stdout().lock().write_all(&page)?;
    }
    Ok(())
}

fn generate_man_page_tree(command: clap::Command, output_dir: &Path) -> deckprobe_core::Result<()> {
    for subcommand in command
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set())
        .cloned()
    {
        generate_man_page_tree(subcommand, output_dir)?;
    }
    let (filename, page) = render_man_page(command)?;
    std::fs::write(output_dir.join(filename), page)?;
    Ok(())
}

fn render_man_page(command: clap::Command) -> deckprobe_core::Result<(String, Vec<u8>)> {
    let man = man_page(command);
    let filename = man.get_filename();
    let mut page = Vec::new();
    man.render(&mut page)?;
    let page = String::from_utf8_lossy(&page);
    Ok((filename, tidy_roff(&page).into_bytes()))
}

fn man_page(command: clap::Command) -> clap_mangen::Man {
    let title = command
        .get_display_name()
        .unwrap_or_else(|| command.get_name())
        .to_ascii_uppercase();
    clap_mangen::Man::new(command)
        .title(title)
        .date("2026-08-02")
        .source(format!("deckprobe {}", env!("CARGO_PKG_VERSION")))
        .manual("DeckProbe Manual")
}

fn tidy_roff(page: &str) -> String {
    let page = page.replace(".br\n\n.br\n", ".sp\n");
    let mut output = String::with_capacity(page.len());
    for line in page.lines() {
        let mut remainder = line;
        while !remainder.starts_with('.') && remainder.len() > 80 {
            let split_at = remainder
                .char_indices()
                .take_while(|(index, _)| *index <= 80)
                .filter_map(|(index, character)| character.is_whitespace().then_some(index))
                .last();
            let Some(split_at) = split_at else {
                break;
            };
            output.push_str(&remainder[..split_at]);
            output.push('\n');
            remainder = remainder[split_at..].trim_start();
        }
        output.push_str(remainder);
        output.push('\n');
    }
    output
}

fn write_json(value: &impl Serialize, pretty: bool) -> deckprobe_core::Result<()> {
    let output = if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
    .map_err(|error| DeckProbeError::Parser(format!("cannot serialize report: {error}")))?;
    println!("{output}");
    Ok(())
}

#[derive(Serialize)]
struct CliErrorEnvelope {
    schema_version: u32,
    tool_version: &'static str,
    status: &'static str,
    error: CliErrorBody,
}

#[derive(Serialize)]
struct CliErrorBody {
    code: String,
    message: String,
    exit_code: u8,
}

fn emit_cli_error(exit_code: u8, code: &str, message: String, pretty: bool) {
    let report = CliErrorEnvelope {
        schema_version: 2,
        tool_version: env!("CARGO_PKG_VERSION"),
        status: "error",
        error: CliErrorBody {
            code: code.to_owned(),
            message,
            exit_code,
        },
    };
    if let Err(error) = write_json(&report, pretty) {
        eprintln!("deckprobe: cannot serialize error report: {error}");
    }
}
