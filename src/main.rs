use benchfck::{
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
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
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
        let mut state = program.state_after(&item.input, step, 1_000_000).ok()?;
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
            1_000_000,
        ))
    });
    (divergence, criticality)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
        } => {
            validate_generate_output(&output, artifact_class)?;
            let defaults = Defaults::load(config)?;
            let items = generator::generate(
                &BuildSpec {
                    seed,
                    count,
                    difficulty: difficulty.into(),
                    arity,
                    held_out,
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
}
