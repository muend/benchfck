use benchfck::{
    PINNED_STEP_CAP,
    config::Defaults,
    generator::{self, BuildSpec},
    metrics,
    schema::{BaseItem, DifficultyBand, EncodingId, Family, JsonlRecord, TaskRecord},
    tasks::{
        self, DriftAfterKMock, IgnoreWrapMock, ModelAdapter, OffByOnePointerMock, PerfectMock,
        T1Answer,
    },
};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Component, Path, PathBuf},
};

#[derive(Parser)]
#[command(
    name = "benchfck",
    version,
    about = "Generate and verify benchfck items without model-based grading"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Generate {
        #[arg(long)]
        seed: u64,
        #[arg(long, default_value_t = 1)]
        count: usize,
        #[arg(long, value_enum)]
        difficulty: Band,
        #[arg(long, default_value_t = 1)]
        arity: u8,
        #[arg(long)]
        held_out: bool,
        #[arg(long, default_value = "config/defaults.toml")]
        config: PathBuf,
        #[arg(long)]
        output: PathBuf,
        /// Diagnostics may be written anywhere except evidence/. Evidence is
        /// accepted only under evidence/ and is added to its SHA-256 manifest.
        #[arg(long, value_enum, default_value = "diagnostic")]
        artifact_class: ArtifactClass,
        /// Include private items, oracle artifacts, and grading answers.
        #[arg(long, default_value_t = false)]
        with_answers: bool,
        /// Write a private export from the same generated items, avoiding a
        /// second expensive generation pass. The path is always diagnostic and
        /// therefore cannot be below evidence/.
        #[arg(long)]
        private_output: Option<PathBuf>,
        /// Items admitted per (semantic class, size tier) cell. Defaults to the
        /// nominal 8x10 grid share. The class share ceiling is enforced
        /// separately.
        #[arg(long)]
        max_per_cell: Option<usize>,
    },
    MockRun {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, value_enum, default_value = "perfect")]
        solver: Solver,
    },
    /// Score externally produced responses against a private export.
    Score {
        #[arg(long)]
        private: PathBuf,
        #[arg(long)]
        responses: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Validate {
        #[arg(long)]
        input: PathBuf,
    },
    /// Build the preregistered, disjoint T2 E0↔E2/E3 BPE-matched pair table
    /// from a private batch. The command fails unless both contrasts have 30.
    MatchedPairs {
        #[arg(long)]
        private: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "config/defaults.toml")]
        config: PathBuf,
        #[arg(long, value_enum, default_value = "diagnostic")]
        artifact_class: ArtifactClass,
    },
    /// Audit the first 20 public items for representation-independent T2/T3
    /// response caps, at least five distinct values per family, and no caps at
    /// the configured safety ceilings.
    BudgetPilot {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "config/defaults.toml")]
        config: PathBuf,
        #[arg(long, value_enum, default_value = "diagnostic")]
        artifact_class: ArtifactClass,
    },
    /// Convert a diagnostic candidate trace into a public first-rejection
    /// histogram. Evidence output requires at least 500 evaluated candidates.
    RejectionHistogram {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        probe_seed: u64,
        #[arg(long)]
        probe_count: usize,
        #[arg(long, default_value_t = 1)]
        probe_arity: u8,
        #[arg(long, value_enum)]
        probe_difficulty: Band,
        #[arg(long, default_value = "config/defaults.toml")]
        config: PathBuf,
        #[arg(long)]
        max_per_cell: Option<usize>,
        #[arg(long, value_enum, default_value = "diagnostic")]
        artifact_class: ArtifactClass,
    },
    /// Run the real acceptance pipeline under a bounded candidate budget and
    /// record every candidate, accepted or rejected. Used to measure whether a
    /// full batch is reachable before paying for one. Diagnostic only.
    Probe {
        #[arg(long)]
        seed: u64,
        /// Batch size the probe pretends to build. Shape quota and size-tier
        /// rotation follow this value, so the measured pressure is realistic.
        #[arg(long, default_value_t = 100)]
        count: usize,
        /// Hard candidate budget. The run stops here even if `count` accepted
        /// items were not reached.
        #[arg(long, default_value_t = 300)]
        candidates: usize,
        #[arg(long, value_enum, default_value = "hard")]
        difficulty: Band,
        #[arg(long, default_value_t = 1)]
        arity: u8,
        #[arg(long, default_value = "config/defaults.toml")]
        config: PathBuf,
        #[arg(long)]
        output: PathBuf,
        /// Mirrors `generate --max-per-cell` so a probe measures the same
        /// stratification pressure the real batch will face.
        #[arg(long)]
        max_per_cell: Option<usize>,
    },
    /// Search a generated closed-form template space for nontrivial semantic
    /// proposals. This is diagnostic design output, never benchmark evidence.
    ConstructorSearch {
        #[arg(long, default_value = "config/defaults.toml")]
        config: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
}
#[derive(Clone, Copy, ValueEnum)]
enum Band {
    Easy,
    Medium,
    Hard,
}
impl From<Band> for DifficultyBand {
    fn from(x: Band) -> Self {
        match x {
            Band::Easy => Self::Easy,
            Band::Medium => Self::Medium,
            Band::Hard => Self::Hard,
        }
    }
}
#[derive(Clone, Copy, ValueEnum)]
enum Solver {
    Perfect,
    OffByOnePointer,
    DriftAfterK,
    IgnoreWrap,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
enum ArtifactClass {
    Diagnostic,
    Evidence,
}

fn path_starts_with_evidence(path: &Path) -> bool {
    !path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        && matches!(
            path.components().next(),
            Some(Component::Normal(component)) if component.eq_ignore_ascii_case("evidence")
        )
}

fn validate_generate_output(path: &Path, class: ArtifactClass) -> Result<(), String> {
    let in_evidence = path_starts_with_evidence(path);
    match (class, in_evidence) {
        (ArtifactClass::Evidence, true) if path != Path::new("evidence/MANIFEST.txt") => Ok(()),
        (ArtifactClass::Evidence, _) => Err(
            "evidence output must be a relative path below evidence/ and cannot be MANIFEST.txt"
                .into(),
        ),
        (ArtifactClass::Diagnostic, false) => Ok(()),
        (ArtifactClass::Diagnostic, true) => Err(
            "diagnostic output cannot enter evidence/; pass --artifact-class evidence explicitly"
                .into(),
        ),
    }
}

fn update_evidence_manifest(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    let hash = benchfck::lower_hex(&Sha256::digest(&bytes));
    let relative = path.to_string_lossy().replace('\\', "/");
    let manifest_path = Path::new("evidence/MANIFEST.txt");
    let manifest = fs::read_to_string(manifest_path)?;
    let mut comments = Vec::new();
    let mut entries = std::collections::BTreeMap::new();
    for line in manifest.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            comments.push(line.to_string());
        } else if let Some((existing_hash, existing_path)) = line.split_once("  ") {
            entries.insert(existing_path.to_string(), existing_hash.to_string());
        } else {
            return Err(format!("malformed evidence manifest line: {line}").into());
        }
    }
    entries.insert(relative, hash);
    let mut next = comments.join("\n");
    if !next.ends_with('\n') {
        next.push('\n');
    }
    for (entry_path, entry_hash) in entries {
        next.push_str(&format!("{entry_hash}  {entry_path}\n"));
    }
    fs::write(manifest_path, next)?;
    Ok(())
}

#[derive(Deserialize)]
struct ResponseRecord {
    task_id: String,
    response: String,
    #[serde(default)]
    tokens_used: Option<u64>,
}

const BUDGET_PILOT_ITEMS: usize = 20;
const BUDGET_PILOT_DISTINCT_CAPS: usize = 5;

#[derive(Serialize)]
#[serde(tag = "record_type", content = "data", rename_all = "snake_case")]
enum BudgetPilotRecord {
    Summary(BudgetPilotSummary),
    Item(BudgetPilotItem),
}

#[derive(Serialize)]
struct BudgetPilotSummary {
    schema_version: &'static str,
    source_batch: String,
    source_sha256: String,
    selection: &'static str,
    item_count: usize,
    required_distinct_caps_per_family: usize,
    t2_distinct_caps: usize,
    t3_distinct_caps: usize,
    t2_safety_ceiling: u32,
    t3_safety_ceiling: u32,
    t2_caps_at_ceiling: usize,
    t3_caps_at_ceiling: usize,
    encoding_invariant: bool,
}

#[derive(Serialize)]
struct BudgetPilotItem {
    item_id: String,
    semantic_class: String,
    program_size_tier: u8,
    t2_response_token_cap: u32,
    t3_response_token_cap: u32,
    t2_caps_by_encoding: Vec<BudgetEncodingCap>,
    t3_caps_by_encoding: Vec<BudgetEncodingCap>,
}

#[derive(Serialize)]
struct BudgetEncodingCap {
    encoding: EncodingId,
    hard_token_cap: u32,
}

fn budget_caps_for_family(
    tasks: &[TaskRecord],
    item_id: &str,
    family: Family,
    rendered_encodings: &[EncodingId],
    expected_cap: u32,
) -> Result<Vec<BudgetEncodingCap>, String> {
    let family_tasks = tasks
        .iter()
        .filter(|task| task.item_id == item_id && task.family == family)
        .collect::<Vec<_>>();
    if family_tasks.len() != rendered_encodings.len() {
        return Err(format!(
            "{item_id} {family:?}: expected {} rendered tasks, found {}",
            rendered_encodings.len(),
            family_tasks.len()
        ));
    }
    rendered_encodings
        .iter()
        .map(|encoding| {
            let matching = family_tasks
                .iter()
                .filter(|task| task.encoding == *encoding)
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                return Err(format!(
                    "{item_id} {family:?} {encoding:?}: expected exactly one task, found {}",
                    matching.len()
                ));
            }
            let observed = matching[0]
                .hard_token_cap
                .ok_or_else(|| format!("{item_id} {family:?} {encoding:?}: missing hard cap"))?;
            if observed != expected_cap {
                return Err(format!(
                    "{item_id} {family:?} {encoding:?}: metadata cap {expected_cap}, task cap {observed}"
                ));
            }
            Ok(BudgetEncodingCap {
                encoding: *encoding,
                hard_token_cap: observed,
            })
        })
        .collect()
}

fn model_answer(solver: Solver, task: &TaskRecord, item: &BaseItem) -> String {
    match solver {
        Solver::Perfect => PerfectMock.answer(task, item),
        Solver::OffByOnePointer => OffByOnePointerMock.answer(task, item),
        Solver::DriftAfterK => DriftAfterKMock {
            k: item.annotations.n_steps / 3,
        }
        .answer(task, item),
        Solver::IgnoreWrap => IgnoreWrapMock.answer(task, item),
    }
}

fn t1_diagnostics(
    solver: Solver,
    task: &TaskRecord,
    item: &BaseItem,
) -> (Option<u64>, Option<benchfck::metrics::ErrorCriticality>) {
    if task.family != Family::T1 {
        return (None, None);
    }
    let n_ideal = task.payload["n_ideal"]
        .as_u64()
        .unwrap_or(item.annotations.n_steps);
    let divergence = metrics::first_divergence(n_ideal, |step| {
        if step == 0 {
            return true;
        }
        tasks::t1_probe_task(item, task.encoding, step).is_some_and(|probe| {
            let response = model_answer(solver, &probe, item);
            tasks::verify_t1(&probe, &response).correct
        })
    });
    let criticality = divergence.and_then(|step| {
        if !matches!(task.encoding, EncodingId::E0 | EncodingId::E1) {
            return None;
        }
        let probe = tasks::t1_probe_task(item, task.encoding, step)?;
        let claimed: T1Answer = serde_json::from_str(&model_answer(solver, &probe, item)).ok()?;
        let program = benchfck::bf::BfProgram::parse(&item.encodings.e0).ok()?;
        let mut state = program
            .state_after(&item.input, step, PINNED_STEP_CAP)
            .ok()?;
        if let Some(pointer) = claimed.pointer {
            if pointer >= state.tape.len() {
                return Some(benchfck::metrics::ErrorCriticality::Critical);
            }
            state.pointer = pointer;
        }
        if claimed.cell >= state.tape.len() {
            return Some(benchfck::metrics::ErrorCriticality::Critical);
        }
        state.tape[claimed.cell] = claimed.value;
        Some(metrics::criticality(
            &program,
            state,
            &item.input,
            &item.expected_output,
            PINNED_STEP_CAP,
        ))
    });
    (divergence, criticality)
}

const CLI_WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let worker = std::thread::Builder::new()
        .name("benchfck-cli".into())
        .stack_size(CLI_WORKER_STACK_BYTES)
        .spawn(|| run().map_err(|error| error.to_string()))?;
    match worker.join() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(message)) => Err(std::io::Error::other(message).into()),
        Err(_) => Err(std::io::Error::other("benchfck CLI worker panicked").into()),
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command {
        Command::Generate {
            seed,
            count,
            difficulty,
            arity,
            held_out,
            config,
            output,
            artifact_class,
            with_answers,
            private_output,
            max_per_cell,
        } => {
            validate_generate_output(&output, artifact_class)?;
            if artifact_class == ArtifactClass::Evidence && with_answers {
                return Err("answer-bearing private exports cannot be evidence artifacts".into());
            }
            if let Some(private_output) = &private_output {
                if with_answers {
                    return Err("--private-output cannot be combined with --with-answers".into());
                }
                validate_generate_output(private_output, ArtifactClass::Diagnostic)?;
                if private_output == &output {
                    return Err("--private-output must differ from --output".into());
                }
            }
            let defaults = Defaults::load(config)?;
            let items = generator::generate(
                &BuildSpec {
                    seed,
                    count,
                    difficulty: difficulty.into(),
                    arity,
                    held_out,
                    max_attempts: None,
                    max_items_per_cell: max_per_cell,
                },
                &defaults,
            )?;
            let mut w = BufWriter::new(File::create(&output)?);
            for r in generator::records(&items, &defaults, with_answers) {
                serde_json::to_writer(&mut w, &r)?;
                w.write_all(b"\n")?;
            }
            w.flush()?;
            drop(w);
            if let Some(private_output) = &private_output {
                let mut private_writer = BufWriter::new(File::create(private_output)?);
                for record in generator::records(&items, &defaults, true) {
                    serde_json::to_writer(&mut private_writer, &record)?;
                    private_writer.write_all(b"\n")?;
                }
                private_writer.flush()?;
            }
            if artifact_class == ArtifactClass::Evidence {
                update_evidence_manifest(&output)?;
            }
            eprintln!(
                "emitted {} accepted items as {} T1/T2/T3 {} artifact",
                items.len(),
                if with_answers { "private" } else { "public" },
                match artifact_class {
                    ArtifactClass::Diagnostic => "diagnostic",
                    ArtifactClass::Evidence => "manifested evidence",
                }
            );
        }
        Command::Validate { input } => {
            let r = BufReader::new(File::open(input)?);
            let mut n = 0;
            for line in r.lines() {
                let _: JsonlRecord = serde_json::from_str(&line?)?;
                n += 1;
            }
            println!("validated {n} schema records");
        }
        Command::MatchedPairs {
            private,
            output,
            config,
            artifact_class,
        } => {
            validate_generate_output(&output, artifact_class)?;
            let defaults = Defaults::load(config)?;
            let items = BufReader::new(File::open(private)?)
                .lines()
                .map(|line| Ok(serde_json::from_str::<JsonlRecord>(&line?)?))
                .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?
                .into_iter()
                .filter_map(|record| match record {
                    JsonlRecord::Item(item) => Some(*item),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if items.is_empty() {
                return Err("matched-pairs requires a private export".into());
            }
            let mut all_pairs = Vec::new();
            for encoding in [EncodingId::E2, EncodingId::E3] {
                let pairs = tasks::matched_t2_prompt_pairs(
                    &items,
                    encoding,
                    0.10,
                    defaults.t1_probe_count,
                    defaults.t2_token_cap,
                    defaults.t3_token_cap,
                    &defaults.prompt_tokenizer,
                )?;
                if pairs.len() < 30 {
                    return Err(format!(
                        "only {} disjoint T2 E0↔{encoding:?} pairs; 30 required",
                        pairs.len()
                    )
                    .into());
                }
                all_pairs.extend(pairs);
            }
            let mut writer = BufWriter::new(File::create(&output)?);
            writer.write_all(b"family,encoded_as,e0_item_id,encoded_item_id,e0_tokens,encoded_tokens,relative_gap,e0_size_tier,encoded_size_tier,e0_semantic_class,encoded_semantic_class\n")?;
            for pair in all_pairs {
                writeln!(
                    writer,
                    "{:?},{:?},{},{},{},{},{:.6},{},{},{},{}",
                    pair.family,
                    pair.encoded_as,
                    pair.e0_item_id,
                    pair.encoded_item_id,
                    pair.e0_tokens,
                    pair.encoded_tokens,
                    pair.relative_gap,
                    pair.e0_size_tier,
                    pair.encoded_size_tier,
                    pair.e0_semantic_class,
                    pair.encoded_semantic_class,
                )?;
            }
            writer.flush()?;
            drop(writer);
            if artifact_class == ArtifactClass::Evidence {
                update_evidence_manifest(&output)?;
            }
        }
        Command::BudgetPilot {
            input,
            output,
            config,
            artifact_class,
        } => {
            validate_generate_output(&output, artifact_class)?;
            if input == output {
                return Err("budget-pilot output must differ from its input batch".into());
            }
            let defaults = Defaults::load(config)?;
            let source_bytes = fs::read(&input)?;
            let source_sha256 = benchfck::lower_hex(&Sha256::digest(&source_bytes));
            let records = BufReader::new(source_bytes.as_slice())
                .lines()
                .map(|line| Ok(serde_json::from_str::<JsonlRecord>(&line?)?))
                .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
            if records
                .iter()
                .any(|record| matches!(record, JsonlRecord::Item(_)))
            {
                return Err("budget-pilot requires a public export without private items".into());
            }
            let metadata = records
                .iter()
                .filter_map(|record| match record {
                    JsonlRecord::PublicItemMetadata(item) => Some(&**item),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if metadata.len() < BUDGET_PILOT_ITEMS {
                return Err(format!(
                    "budget-pilot requires at least {BUDGET_PILOT_ITEMS} public items, found {}",
                    metadata.len()
                )
                .into());
            }
            let tasks = records
                .iter()
                .filter_map(|record| match record {
                    JsonlRecord::Task(task) => Some((**task).clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let selected = &metadata[..BUDGET_PILOT_ITEMS];
            let unique_ids = selected
                .iter()
                .map(|item| item.item_id.as_str())
                .collect::<HashSet<_>>();
            if unique_ids.len() != BUDGET_PILOT_ITEMS {
                return Err("budget-pilot selection contains duplicate item ids".into());
            }

            let mut t2_distinct = HashSet::new();
            let mut t3_distinct = HashSet::new();
            let mut t2_at_ceiling = 0;
            let mut t3_at_ceiling = 0;
            let mut item_rows = Vec::with_capacity(BUDGET_PILOT_ITEMS);
            for item in selected {
                t2_distinct.insert(item.t2_response_token_cap);
                t3_distinct.insert(item.t3_response_token_cap);
                t2_at_ceiling += usize::from(item.t2_response_token_cap == defaults.t2_token_cap);
                t3_at_ceiling += usize::from(item.t3_response_token_cap == defaults.t3_token_cap);
                if item.t2_response_token_cap > defaults.t2_token_cap
                    || item.t3_response_token_cap > defaults.t3_token_cap
                {
                    return Err(
                        format!("{} exceeds a configured safety ceiling", item.item_id).into(),
                    );
                }
                let t2_caps_by_encoding = budget_caps_for_family(
                    &tasks,
                    &item.item_id,
                    Family::T2,
                    &item.available_encodings,
                    item.t2_response_token_cap,
                )?;
                let t3_caps_by_encoding = budget_caps_for_family(
                    &tasks,
                    &item.item_id,
                    Family::T3,
                    &item.available_encodings,
                    item.t3_response_token_cap,
                )?;
                item_rows.push(BudgetPilotItem {
                    item_id: item.item_id.clone(),
                    semantic_class: item.grammar_shape.clone(),
                    program_size_tier: item.program_size_tier,
                    t2_response_token_cap: item.t2_response_token_cap,
                    t3_response_token_cap: item.t3_response_token_cap,
                    t2_caps_by_encoding,
                    t3_caps_by_encoding,
                });
            }
            if t2_distinct.len() < BUDGET_PILOT_DISTINCT_CAPS
                || t3_distinct.len() < BUDGET_PILOT_DISTINCT_CAPS
            {
                return Err(format!(
                    "budget-pilot lacks cap diversity: required {BUDGET_PILOT_DISTINCT_CAPS}, observed T2={}, T3={}",
                    t2_distinct.len(),
                    t3_distinct.len()
                )
                .into());
            }
            if t2_at_ceiling != 0 || t3_at_ceiling != 0 {
                return Err(format!(
                    "budget-pilot has caps pinned to a safety ceiling: T2={t2_at_ceiling}, T3={t3_at_ceiling}"
                )
                .into());
            }

            let summary = BudgetPilotSummary {
                schema_version: "benchfck.budget-pilot.v1",
                source_batch: input.to_string_lossy().replace('\\', "/"),
                source_sha256,
                selection: "first_20_public_items_in_source_order",
                item_count: BUDGET_PILOT_ITEMS,
                required_distinct_caps_per_family: BUDGET_PILOT_DISTINCT_CAPS,
                t2_distinct_caps: t2_distinct.len(),
                t3_distinct_caps: t3_distinct.len(),
                t2_safety_ceiling: defaults.t2_token_cap,
                t3_safety_ceiling: defaults.t3_token_cap,
                t2_caps_at_ceiling: t2_at_ceiling,
                t3_caps_at_ceiling: t3_at_ceiling,
                encoding_invariant: true,
            };
            let mut writer = BufWriter::new(File::create(&output)?);
            serde_json::to_writer(&mut writer, &BudgetPilotRecord::Summary(summary))?;
            writer.write_all(b"\n")?;
            for item in item_rows {
                serde_json::to_writer(&mut writer, &BudgetPilotRecord::Item(item))?;
                writer.write_all(b"\n")?;
            }
            writer.flush()?;
            drop(writer);
            if artifact_class == ArtifactClass::Evidence {
                update_evidence_manifest(&output)?;
            }
        }
        Command::RejectionHistogram {
            input,
            output,
            probe_seed,
            probe_count,
            probe_arity,
            probe_difficulty,
            config,
            max_per_cell,
            artifact_class,
        } => {
            validate_generate_output(&output, artifact_class)?;
            if input == output {
                return Err("rejection-histogram output must differ from its input trace".into());
            }
            let source_bytes = fs::read(&input)?;
            let source_sha256 = benchfck::lower_hex(&Sha256::digest(&source_bytes));
            let trace = BufReader::new(source_bytes.as_slice())
                .lines()
                .map(|line| Ok(serde_json::from_str::<generator::CandidateOutcome>(&line?)?))
                .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
            if artifact_class == ArtifactClass::Evidence && trace.len() < 500 {
                return Err(format!(
                    "release rejection histogram requires at least 500 candidates, found {}",
                    trace.len()
                )
                .into());
            }
            if artifact_class == ArtifactClass::Evidence && probe_count <= trace.len() {
                return Err(
                    "evidence histogram probe-count must exceed the candidate budget so the trace cannot stop early at its accepted-item target"
                        .into(),
                );
            }
            let unique_attempts = trace
                .iter()
                .map(|outcome| outcome.attempt)
                .collect::<HashSet<_>>();
            if unique_attempts.len() != trace.len() {
                return Err("candidate trace contains duplicate attempt ids".into());
            }
            if trace
                .iter()
                .any(|outcome| outcome.accepted != outcome.rejection_category.is_none())
            {
                return Err(
                    "candidate trace has inconsistent accepted/rejection-category fields".into(),
                );
            }
            let expected_difficulty: DifficultyBand = probe_difficulty.into();
            if trace.iter().any(|outcome| {
                outcome.item_seed != probe_seed.wrapping_add(outcome.attempt as u64 * 0x9E37_79B9)
                    || outcome.difficulty_band != expected_difficulty
            }) {
                return Err("candidate trace does not match the declared seed/difficulty".into());
            }
            let config_bytes = fs::read(&config)?;
            let config_sha256 = benchfck::lower_hex(&Sha256::digest(&config_bytes));
            let accepted = trace.iter().filter(|outcome| outcome.accepted).count();
            let rejected = trace.len() - accepted;
            let total_ms: u64 = trace.iter().map(|outcome| outcome.elapsed_ms).sum();
            let mut categories = std::collections::BTreeMap::<String, usize>::new();
            for category in generator::REJECTION_CATEGORIES {
                categories.insert((*category).into(), 0);
            }
            for outcome in &trace {
                if let Some(category) = &outcome.rejection_category {
                    *categories.entry(category.clone()).or_default() += 1;
                }
            }
            let mut tier_total = std::collections::BTreeMap::<u8, usize>::new();
            let mut tier_accepted = std::collections::BTreeMap::<u8, usize>::new();
            for outcome in &trace {
                *tier_total.entry(outcome.requested_size_tier).or_default() += 1;
                if outcome.accepted {
                    *tier_accepted
                        .entry(outcome.requested_size_tier)
                        .or_default() += 1;
                }
            }

            let mut writer = BufWriter::new(File::create(&output)?);
            writeln!(writer, "# Acceptance rejection histogram\n")?;
            writeln!(writer, "- Schema: `benchfck.rejection-histogram.v1`")?;
            writeln!(
                writer,
                "- Source trace: `{}`",
                input.to_string_lossy().replace('\\', "/")
            )?;
            writeln!(writer, "- Source SHA-256: `{source_sha256}`")?;
            writeln!(
                writer,
                "- Probe parameters: seed={probe_seed}, count={probe_count}, candidates={}, difficulty={expected_difficulty:?}, arity={probe_arity}, max_per_cell={}",
                trace.len(),
                max_per_cell
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "derived".into())
            )?;
            writeln!(
                writer,
                "- Configuration: `{}` (`{config_sha256}`)",
                config.to_string_lossy().replace('\\', "/")
            )?;
            writeln!(writer, "- Evaluated candidates: {}", trace.len())?;
            writeln!(writer, "- Accepted: {accepted}")?;
            writeln!(writer, "- Rejected: {rejected}")?;
            writeln!(
                writer,
                "- Acceptance rate: {:.2}%",
                accepted as f64 * 100.0 / trace.len().max(1) as f64
            )?;
            writeln!(
                writer,
                "- Total candidate time: {:.3} s",
                total_ms as f64 / 1000.0
            )?;
            writeln!(
                writer,
                "- Mean candidate time: {:.3} s\n",
                total_ms as f64 / trace.len().max(1) as f64 / 1000.0
            )?;
            writeln!(
                writer,
                "Each rejected candidate is attributed only to its first failing gate. A zero-hit row means the gate was not the first failure in this sample; it is reported, not treated as a defect.\n"
            )?;
            writeln!(
                writer,
                "| First rejection category | Hits | Share of all candidates |"
            )?;
            writeln!(writer, "|---|---:|---:|")?;
            for (category, hits) in categories {
                writeln!(
                    writer,
                    "| `{category}` | {hits} | {:.2}% |",
                    hits as f64 * 100.0 / trace.len().max(1) as f64
                )?;
            }
            writeln!(
                writer,
                "\n| Requested size tier | Candidates | Accepted | Acceptance rate |"
            )?;
            writeln!(writer, "|---:|---:|---:|---:|")?;
            for (tier, total) in tier_total {
                let tier_hits = tier_accepted.get(&tier).copied().unwrap_or_default();
                writeln!(
                    writer,
                    "| {tier} | {total} | {tier_hits} | {:.2}% |",
                    tier_hits as f64 * 100.0 / total as f64
                )?;
            }
            writer.flush()?;
            drop(writer);
            if artifact_class == ArtifactClass::Evidence {
                update_evidence_manifest(&output)?;
            }
        }
        Command::Probe {
            seed,
            count,
            candidates,
            difficulty,
            arity,
            config,
            output,
            max_per_cell,
        } => {
            validate_generate_output(&output, ArtifactClass::Diagnostic)?;
            let defaults = Defaults::load(config)?;
            let (result, trace) = generator::generate_traced(
                &BuildSpec {
                    seed,
                    count,
                    difficulty: difficulty.into(),
                    arity,
                    held_out: false,
                    max_attempts: Some(candidates),
                    max_items_per_cell: max_per_cell,
                },
                &defaults,
            );
            let mut writer = BufWriter::new(File::create(&output)?);
            for outcome in &trace {
                serde_json::to_writer(&mut writer, outcome)?;
                writer.write_all(b"\n")?;
            }
            writer.flush()?;
            drop(writer);
            let accepted = trace.iter().filter(|outcome| outcome.accepted).count();
            let total_ms: u64 = trace.iter().map(|outcome| outcome.elapsed_ms).sum();
            let mut categories = std::collections::BTreeMap::<&str, usize>::new();
            for outcome in &trace {
                if let Some(category) = outcome.rejection_category.as_deref() {
                    *categories.entry(category).or_default() += 1;
                }
            }
            eprintln!(
                "probe: {accepted} accepted / {} candidates in {:.1} s",
                trace.len(),
                total_ms as f64 / 1000.0
            );
            for (category, hits) in &categories {
                eprintln!("  reject {category:>44}  {hits}");
            }
            match result {
                Ok(items) => eprintln!("probe: full batch of {} reached", items.len()),
                Err(error) => eprintln!("probe: batch not reached ({error})"),
            }
        }
        Command::ConstructorSearch { config, output } => {
            validate_generate_output(&output, ArtifactClass::Diagnostic)?;
            let defaults = Defaults::load(config)?;
            let report = tasks::search_constructor_templates(defaults.t2_nontriviality_threshold);
            serde_json::to_writer_pretty(BufWriter::new(File::create(output)?), &report)?;
        }
        Command::MockRun {
            input,
            output,
            solver,
        } => {
            let r = BufReader::new(File::open(input)?);
            let mut records = Vec::new();
            for line in r.lines() {
                records.push(serde_json::from_str::<JsonlRecord>(&line?)?);
            }
            let mut items = HashMap::new();
            for rec in &records {
                if let JsonlRecord::Item(i) = rec {
                    items.insert(i.item_id.clone(), (**i).clone());
                }
            }
            let mut w = BufWriter::new(File::create(output)?);
            for rec in &records {
                if let JsonlRecord::Task(t) = rec {
                    let item = items.get(&t.item_id).ok_or(
                        "mock-run requires a private export generated with --with-answers",
                    )?;
                    let response = model_answer(solver, t, item);
                    let v = tasks::verify(t, item, &response);
                    let (first_divergence, criticality) = t1_diagnostics(solver, t, item);
                    let n_ideal = t.payload["n_ideal"]
                        .as_u64()
                        .unwrap_or(item.annotations.n_steps);
                    let m = metrics::item_metric(
                        t,
                        &v,
                        tasks::lexical_token_count(&response),
                        n_ideal,
                        first_divergence,
                        criticality,
                    );
                    serde_json::to_writer(&mut w, &m)?;
                    w.write_all(b"\n")?;
                }
            }
            w.flush()?;
        }
        Command::Score {
            private,
            responses,
            output,
        } => {
            let records = BufReader::new(File::open(private)?)
                .lines()
                .map(|line| Ok(serde_json::from_str::<JsonlRecord>(&line?)?))
                .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
            let items = records
                .iter()
                .filter_map(|record| match record {
                    JsonlRecord::Item(item) => Some((item.item_id.clone(), &**item)),
                    _ => None,
                })
                .collect::<HashMap<_, _>>();
            let tasks_by_id = records
                .iter()
                .filter_map(|record| match record {
                    JsonlRecord::Task(task) => Some((task.task_id.clone(), &**task)),
                    _ => None,
                })
                .collect::<HashMap<_, _>>();
            if items.is_empty() {
                return Err("score requires a private export generated with --with-answers".into());
            }
            let responses = BufReader::new(File::open(responses)?)
                .lines()
                .map(|line| Ok(serde_json::from_str::<ResponseRecord>(&line?)?))
                .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
            let mut writer = BufWriter::new(File::create(output)?);
            for response in responses {
                let task = tasks_by_id
                    .get(&response.task_id)
                    .ok_or_else(|| format!("unknown task_id {}", response.task_id))?;
                let item = items
                    .get(&task.item_id)
                    .ok_or_else(|| format!("missing private item {}", task.item_id))?;
                let verification = tasks::verify(task, item, &response.response);
                let tokens = response
                    .tokens_used
                    .unwrap_or_else(|| tasks::lexical_token_count(&response.response));
                let n_ideal = task.payload["n_ideal"]
                    .as_u64()
                    .unwrap_or(item.annotations.n_steps);
                let metric = metrics::item_metric(task, &verification, tokens, n_ideal, None, None);
                serde_json::to_writer(&mut writer, &metric)?;
                writer.write_all(b"\n")?;
            }
            writer.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn capped_task(family: Family, encoding: EncodingId, cap: u32) -> TaskRecord {
        TaskRecord {
            schema_version: "benchfck.task.v3".into(),
            task_id: format!("item-1-{family:?}-{encoding:?}"),
            program_id: "program-1".into(),
            item_id: "item-1".into(),
            family,
            encoding,
            prompt: String::new(),
            hard_token_cap: Some(cap),
            payload: json!({}),
        }
    }

    #[test]
    fn evidence_path_requires_explicit_class_and_safe_relative_path() {
        assert!(
            validate_generate_output(Path::new("evidence/batch.jsonl"), ArtifactClass::Evidence)
                .is_ok()
        );
        assert!(
            validate_generate_output(Path::new("evidence/batch.jsonl"), ArtifactClass::Diagnostic)
                .is_err()
        );
        assert!(
            validate_generate_output(
                Path::new("../evidence/batch.jsonl"),
                ArtifactClass::Evidence
            )
            .is_err()
        );
        assert!(
            validate_generate_output(Path::new("target/smoke.jsonl"), ArtifactClass::Diagnostic)
                .is_ok()
        );
    }

    #[test]
    fn budget_pilot_requires_exactly_one_equal_cap_per_rendered_encoding() {
        let encodings = [EncodingId::E0, EncodingId::E1];
        let mut tasks = vec![
            capped_task(Family::T2, EncodingId::E0, 220),
            capped_task(Family::T2, EncodingId::E1, 220),
        ];
        assert!(budget_caps_for_family(&tasks, "item-1", Family::T2, &encodings, 220).is_ok());

        tasks[1].hard_token_cap = Some(221);
        assert!(budget_caps_for_family(&tasks, "item-1", Family::T2, &encodings, 220).is_err());

        tasks[1].hard_token_cap = Some(220);
        tasks.push(capped_task(Family::T2, EncodingId::E1, 220));
        assert!(budget_caps_for_family(&tasks, "item-1", Family::T2, &encodings, 220).is_err());
    }
}
