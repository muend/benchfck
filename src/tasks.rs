use crate::{
    PINNED_SEMANTICS, PINNED_STEP_CAP,
    backend::Bytecode,
    bf::BfProgram,
    oracle::Observable,
    schema::{
        AdditivePeriodWitness, AnalyticalExpressionWitness, AnalyticalNontrivialityWitness,
        BaseItem, EncodingId, Family, NontrivialityWitness, TaskRecord,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct T1Answer {
    pub step: u64,
    pub pointer: Option<usize>,
    pub cell: usize,
    pub value: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Verification {
    pub correct: bool,
    pub parse_failure: bool,
    pub detail: String,
}

fn source(item: &BaseItem, e: EncodingId) -> String {
    match e {
        EncodingId::E0 => item.encodings.e0.clone(),
        EncodingId::E1 => {
            let definitions = item
                .encodings
                .e1_legend
                .iter()
                .map(|(canonical, displayed)| {
                    let meaning = match canonical {
                        '+' => "increment the current cell modulo 256",
                        '-' => "decrement the current cell modulo 256",
                        '>' => "move the pointer one cell right",
                        '<' => "move the pointer one cell left",
                        '[' => "if the current cell is zero, jump after the matching loop close",
                        ']' => {
                            "if the current cell is nonzero, jump back to the matching loop open"
                        }
                        ',' => "read the next input byte into the current cell",
                        '.' => "emit the current cell as one output byte",
                        _ => unreachable!(),
                    };
                    format!("`{displayed}`: {meaning}")
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "Operational definitions:\n{definitions}\n{}",
                item.encodings.e1
            )
        }
        EncodingId::E2 => item.encodings.e2.clone(),
        EncodingId::E3 => item.encodings.e3.clone(),
        EncodingId::E4 => item.encodings.e4.clone(),
    }
}
fn task_id(item: &BaseItem, f: Family, e: EncodingId) -> String {
    format!("{}-{:?}-{:?}", item.item_id, f, e).to_lowercase()
}
fn probe_task_id(item: &BaseItem, e: EncodingId, probe: usize) -> String {
    format!("{}-t1-{:?}-p{probe}", item.item_id, e).to_lowercase()
}
fn prompt_header() -> String {
    format!("Pinned execution semantics (apply exactly):\n{PINNED_SEMANTICS}\n\n")
}

/// Builds the production T2 prompt around an explicitly supplied rendering.
/// Carrier diagnostics use this entry point so their prompt-length comparison
/// cannot drift from the task adapter's actual wording or item-level budget.
pub fn t2_prompt_for_source(item: &BaseItem, rendered_source: &str, t2_token_cap: u32) -> String {
    let cap = item_t2_token_cap(item, t2_token_cap);
    format!(
        "{}Program:\n{}\n\nReturn executable code in the restricted Python expression subset. It must be exactly `def solve(inputs):` followed by `return [EXPR, ...]`. Allowed in expressions: integer literals 0 through 256, inputs[N], + - * // % & | ^, unary minus, and parentheses. Constant-only subexpressions are accepted and folded before the lexical-token budget is measured. Lookup tables, loops, conditionals, imports, calls, and prose are forbidden. Hard folded lexical-token cap: {cap}.",
        prompt_header(),
        rendered_source
    )
}

/// D33: the implicit-encoding trace is never stored. It is a deterministic
/// function of `e0` and `input`, and storing it dominated private export size
/// (~17 MB per item at ~200k steps, and the size ladder pushes step counts far
/// higher). Probes are recomputed on demand with O(1) extra memory.
///
/// Step semantics mirror `bf::TracePoint` exactly: `touched_cell` is the
/// pointer *before* the instruction ran, `pointer` is the pointer after it, and
/// `value` is that touched cell's value after the instruction.
fn implicit_probe_answer(item: &BaseItem, step: u64) -> Option<T1Answer> {
    let program = BfProgram::parse(&item.encodings.e0).ok()?;
    let before = program
        .state_after(&item.input, step.checked_sub(1)?, PINNED_STEP_CAP)
        .ok()?;
    let after = program
        .state_after(&item.input, step, PINNED_STEP_CAP)
        .ok()?;
    let touched = before.pointer;
    Some(T1Answer {
        step,
        pointer: Some(after.pointer),
        cell: touched,
        value: *after.tape.get(touched)?,
    })
}

pub fn adapt_all(
    item: &BaseItem,
    t1_probe_count: usize,
    t2_token_cap: u32,
    t3_token_cap: u32,
) -> Vec<TaskRecord> {
    let mut out = vec![];
    let bytecode = Bytecode::parse_e2(&item.encodings.e2).expect("accepted E2");
    let t2_item_cap = item_t2_token_cap(item, t2_token_cap);
    let t3_item_cap = item_t3_token_cap(item, t3_token_cap);
    for e in [
        EncodingId::E0,
        EncodingId::E1,
        EncodingId::E2,
        EncodingId::E3,
    ]
    .into_iter()
    .filter(|encoding| {
        crate::generator::tier_renders_encoding(item.annotations.program_size_tier, *encoding)
    }) {
        let explicit_run = matches!(e, EncodingId::E2 | EncodingId::E3).then(|| {
            bytecode
                .execute_traced(&item.input, PINNED_STEP_CAP, true)
                .expect("validated backend")
        });
        let trace_len = explicit_run
            .as_ref()
            .map_or(item.annotations.n_steps as usize, |run| run.trace.len());
        let probe_count = t1_probe_count.max(1);
        let mut numerators: Vec<usize> = (1..=probe_count).collect();
        let rotation = (item.seed as usize) % probe_count;
        numerators.rotate_left(rotation);
        for (probe, numerator) in numerators.into_iter().enumerate() {
            let index = ((trace_len - 1) * numerator / (probe_count + 1)).min(trace_len - 1);
            let (answer, question) = if let Some(run) = &explicit_run {
                let p = &run.trace[index];
                (
                    T1Answer {
                        step: p.step,
                        pointer: None,
                        cell: p.cell,
                        value: p.value,
                    },
                    format!(
                        "After completed step {}, report the explicit cell index and its value; pointer must be null.",
                        p.step
                    ),
                )
            } else {
                let step = (index + 1) as u64;
                let answer = implicit_probe_answer(item, step)
                    .expect("accepted item replays its own implicit trace");
                (
                    answer,
                    format!(
                        "At completed step {step}, report the tape pointer, touched cell index, and that cell's value."
                    ),
                )
            };
            out.push(TaskRecord{schema_version:"benchfck.task.v3".into(),task_id:probe_task_id(item,e,probe),program_id:item.program_id.clone(),item_id:item.item_id.clone(),family:Family::T1,encoding:e,prompt:format!("{}Program:\n{}\n\nInput bytes: {:?}\n{} Return JSON only: {{\"step\":N,\"pointer\":N_or_null,\"cell\":N,\"value\":N}}",prompt_header(),source(item,e),item.input,question),hard_token_cap:None,payload:json!({"expected_answer":answer,"n_ideal":trace_len,"probe_ordinal":probe})});
        }
    }
    for e in [
        EncodingId::E0,
        EncodingId::E1,
        EncodingId::E2,
        EncodingId::E3,
    ]
    .into_iter()
    .filter(|encoding| {
        crate::generator::tier_renders_encoding(item.annotations.program_size_tier, *encoding)
    }) {
        let ideal = if matches!(e, EncodingId::E0 | EncodingId::E1) {
            item.annotations.n_steps
        } else {
            bytecode
                .execute(&item.input, PINNED_STEP_CAP)
                .expect("validated backend")
                .steps
        };
        out.push(TaskRecord{schema_version:"benchfck.task.v3".into(),task_id:task_id(item,Family::T2,e),program_id:item.program_id.clone(),item_id:item.item_id.clone(),family:Family::T2,encoding:e,prompt:t2_prompt_for_source(item,&source(item,e),t2_token_cap),hard_token_cap:Some(t2_item_cap),payload:json!({"arity":item.ir.arity,"oracle_fingerprint":item.oracles.semantic_fingerprint,"n_ideal":ideal,"reference_solution":item.oracles.t2_reference_solution})});
    }
    let eligible_mutations = item
        .oracles
        .avalanche_map
        .iter()
        .filter(|m| {
            matches!(m.from, '+' | '-' | ',' | '.') && matches!(m.to, '+' | '-' | ',' | '.')
        })
        .collect::<Vec<_>>();
    if !eligible_mutations.is_empty() {
        let region = match item.seed % 3 {
            0 => "early",
            1 => "middle",
            _ => "late",
        };
        let numerator = match region {
            "early" => 1,
            "middle" => 3,
            _ => 5,
        };
        let index =
            ((eligible_mutations.len() - 1) * numerator / 6).min(eligible_mutations.len() - 1);
        let m = eligible_mutations[index];
        let row = m.position;
        for e in [
            EncodingId::E0,
            EncodingId::E1,
            EncodingId::E2,
            EncodingId::E3,
        ]
        .into_iter()
        .filter(|encoding| {
            crate::generator::tier_renders_encoding(item.annotations.program_size_tier, *encoding)
        }) {
            let mutation = match e {
                EncodingId::E0 => format!(
                    "replace instruction {} ({}) with {}",
                    m.position, m.from, m.to
                ),
                EncodingId::E1 => {
                    let map = |x| {
                        item.encodings
                            .e1_legend
                            .iter()
                            .find(|(a, _)| *a == x)
                            .unwrap()
                            .1
                    };
                    format!(
                        "replace instruction {} ({}) with {}",
                        m.position,
                        map(m.from),
                        map(m.to)
                    )
                }
                EncodingId::E2 | EncodingId::E3 => format!(
                    "at explicit instruction row {row}, replace operation {} with {} while retaining its cell operand",
                    op_name(m.from, e),
                    op_name(m.to, e)
                ),
                _ => unreachable!(),
            };
            let ideal = if matches!(e, EncodingId::E0 | EncodingId::E1) {
                item.annotations.n_steps.max(1)
            } else {
                bytecode
                    .execute(&item.input, PINNED_STEP_CAP)
                    .expect("validated backend")
                    .steps
                    .max(1)
            };
            out.push(TaskRecord{schema_version:"benchfck.task.v3".into(),task_id:task_id(item,Family::T3,e),program_id:item.program_id.clone(),item_id:item.item_id.clone(),family:Family::T3,encoding:e,prompt:format!("{}Program:\n{}\n\nInput bytes: {:?}\nCounterfactual mutation: {}. Return JSON only as one of {{\"status\":\"OUTPUT\",\"value\":[...] }}, {{\"status\":\"IDENTICAL\"}}, {{\"status\":\"NON_TERMINATING\"}}, or {{\"status\":\"ERROR\",\"value\":\"...\"}}.",prompt_header(),source(item,e),item.input,mutation),hard_token_cap:Some(t3_item_cap),payload:json!({"mutation":m,"mutation_region":region,"mutation_position_fraction":m.position as f64 / item.encodings.e0.len().max(1) as f64,"expected_answer":m.outcome,"n_ideal":ideal})});
        }
    }
    out
}

/// Response budgets are properties of an item, never of its representation.
/// Difficulty features create useful variation across items while every
/// E0/E1/E2/E3 member of one ladder receives exactly the same allowance.
pub fn item_t2_token_cap(item: &BaseItem, ceiling: u32) -> u32 {
    let floor = item.oracles.t2_reference_solution_tokens_upper_bound;
    let budget_stratum = (item.seed % 17) as u32;
    let structural = (item.annotations.nesting_depth as u32)
        .saturating_mul(5)
        .saturating_add((item.annotations.working_set as u32).saturating_mul(2))
        .saturating_add(item.annotations.n_steps.max(1).ilog2())
        .saturating_add(budget_stratum);
    floor
        .saturating_mul(3)
        .saturating_add(structural)
        .clamp(floor, ceiling.max(floor))
}

pub fn item_t3_token_cap(item: &BaseItem, ceiling: u32) -> u32 {
    let budget_stratum = (item.seed % 17) as u32;
    let structural = (item.annotations.nesting_depth as u32)
        .saturating_mul(2)
        .saturating_add(item.annotations.working_set as u32)
        .saturating_add(item.annotations.n_steps.max(1).ilog2())
        .saturating_add(budget_stratum);
    32u32.saturating_add(structural).min(ceiling)
}

/// Removes every grading-side field from a model-facing task record. Public
/// exports contain only prompts, budgets, identifiers, and non-answer task
/// metadata; private item/oracle records are opt-in at generation time.
pub fn without_answers(mut task: TaskRecord) -> TaskRecord {
    if let Some(payload) = task.payload.as_object_mut() {
        payload.remove("expected_answer");
        payload.remove("reference_solution");
        payload.remove("oracle_fingerprint");
        if let Some(mutation) = payload
            .get_mut("mutation")
            .and_then(|value| value.as_object_mut())
        {
            mutation.remove("outcome");
            mutation.remove("changed");
        }
    }
    task
}

/// Builds an exact interior probe at an arbitrary completed step. Runners use
/// this to locate monotone state drift with adaptive binary search instead of
/// pretending that one fixed midpoint is a divergence metric.
pub fn t1_probe_task(item: &BaseItem, e: EncodingId, step: u64) -> Option<TaskRecord> {
    if step == 0 {
        return None;
    }
    let (answer, question, n_ideal) = if matches!(e, EncodingId::E0 | EncodingId::E1) {
        if step > item.annotations.n_steps {
            return None;
        }
        (
            implicit_probe_answer(item, step)?,
            format!(
                "At completed step {step}, report the tape pointer, touched cell index, and that cell's value."
            ),
            item.annotations.n_steps as usize,
        )
    } else {
        let run = Bytecode::from_e0(&item.encodings.e0)
            .ok()?
            .execute_traced(&item.input, PINNED_STEP_CAP, true)
            .ok()?;
        let p = run.trace.iter().find(|p| p.step == step)?;
        (
            T1Answer {
                step,
                pointer: None,
                cell: p.cell,
                value: p.value,
            },
            format!(
                "After completed step {step}, report the explicit cell index and its value; pointer must be null."
            ),
            run.trace.len(),
        )
    };
    Some(TaskRecord {
        schema_version: "benchfck.task.v3".into(),
        task_id: format!("{}-t1-{:?}-adaptive-{step}", item.item_id, e).to_lowercase(),
        program_id: item.program_id.clone(),
        item_id: item.item_id.clone(),
        family: Family::T1,
        encoding: e,
        prompt: format!(
            "{}Program:\n{}\n\nInput bytes: {:?}\n{} Return JSON only: {{\"step\":N,\"pointer\":N_or_null,\"cell\":N,\"value\":N}}",
            prompt_header(),
            source(item, e),
            item.input,
            question
        ),
        hard_token_cap: None,
        payload: json!({"expected_answer": answer, "n_ideal": n_ideal, "adaptive": true}),
    })
}
fn op_name(c: char, e: EncodingId) -> &'static str {
    match (e, c) {
        (EncodingId::E2, '+') => "INC",
        (EncodingId::E2, '-') => "DEC",
        (EncodingId::E2, ',') => "IN",
        (EncodingId::E2, '.') => "OUT",
        (EncodingId::E3, '+') => "a",
        (EncodingId::E3, '-') => "s",
        (EncodingId::E3, ',') => "i",
        (EncodingId::E3, '.') => "o",
        _ => "UNKNOWN",
    }
}

pub fn verify_t1(task: &TaskRecord, response: &str) -> Verification {
    let parsed: Result<T1Answer, _> = serde_json::from_str(response);
    match parsed {
        Err(e) => Verification {
            correct: false,
            parse_failure: true,
            detail: e.to_string(),
        },
        Ok(a) => {
            let expected: Result<T1Answer, _> =
                serde_json::from_value(task.payload["expected_answer"].clone());
            match expected {
                Ok(x) => Verification {
                    correct: a == x,
                    parse_failure: false,
                    detail: if a == x {
                        "exact".into()
                    } else {
                        "state mismatch".into()
                    },
                },
                Err(e) => Verification {
                    correct: false,
                    parse_failure: true,
                    detail: format!("bad task oracle: {e}"),
                },
            }
        }
    }
}
pub fn verify_t3(task: &TaskRecord, response: &str) -> Verification {
    let parsed: Result<Observable, _> = serde_json::from_str(response);
    match parsed {
        Err(e) => Verification {
            correct: false,
            parse_failure: true,
            detail: e.to_string(),
        },
        Ok(a) => {
            let x: Observable =
                serde_json::from_value(task.payload["expected_answer"].clone()).expect("oracle");
            Verification {
                correct: a == x,
                parse_failure: false,
                detail: if a == x {
                    "exact".into()
                } else {
                    "counterfactual mismatch".into()
                },
            }
        }
    }
}

/// Conservative lexical count used for local budget enforcement. Unlike
/// whitespace splitting, a JSON truth table cannot collapse into one token.
pub fn lexical_token_count(source: &str) -> u64 {
    let mut count = 0;
    let mut in_word = false;
    for c in source.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            if !in_word {
                count += 1;
                in_word = true;
            }
        } else {
            in_word = false;
            if !c.is_whitespace() {
                count += 1;
            }
        }
    }
    count
}

/// Token count for the restricted expression grammar. Composite terminals
/// such as `inputs[0]` and `//` are one grammar token each; punctuation that is
/// structurally required by the AST remains explicit. This is the D24 cost
/// model used only for nontriviality, not the model-response safety cap.
pub fn expression_grammar_token_count(source: &str) -> Result<u32, String> {
    let bytes = source.as_bytes();
    let mut position = 0;
    let mut tokens = 0u32;
    while position < bytes.len() {
        if bytes[position].is_ascii_whitespace() {
            position += 1;
            continue;
        }
        if bytes[position..].starts_with(b"inputs[") {
            position += b"inputs[".len();
            let start = position;
            while bytes.get(position).is_some_and(u8::is_ascii_digit) {
                position += 1;
            }
            if position == start || bytes.get(position) != Some(&b']') {
                return Err("malformed inputs[N] grammar token".into());
            }
            position += 1;
            tokens += 1;
            continue;
        }
        if bytes[position].is_ascii_digit() {
            while bytes.get(position).is_some_and(u8::is_ascii_digit) {
                position += 1;
            }
            tokens += 1;
            continue;
        }
        if bytes[position..].starts_with(b"//") {
            position += 2;
            tokens += 1;
            continue;
        }
        if b"+-*%&|^()".contains(&bytes[position]) {
            position += 1;
            tokens += 1;
            continue;
        }
        return Err(format!("unsupported expression byte at {position}"));
    }
    Ok(tokens)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BpePromptRatios {
    /// D36: `None` means the tier does not render that encoding at all, which
    /// is different from "rendered and measured as zero". Recording 0.0 for an
    /// absent rendering would put a number in the annotations that never
    /// happened.
    pub e2_prompt_over_e0_prompt: Option<f64>,
    pub e3_prompt_over_e0_prompt: Option<f64>,
}

/// Measures the complete model-facing prompt, pairing each explicit task with
/// the same family/probe in E0. This prevents source-only size accounting from
/// hiding a task-template confound.
pub fn ladder_prompt_bpe_ratios(
    item: &BaseItem,
    t1_probe_count: usize,
    t2_token_cap: u32,
    t3_token_cap: u32,
    tokenizer_name: &str,
) -> Result<BpePromptRatios, String> {
    use std::collections::HashMap;

    let tokenizer = tiktoken::get_encoding(tokenizer_name)
        .ok_or_else(|| format!("unknown tokenizer {tokenizer_name}"))?;
    let records = adapt_all(item, t1_probe_count, t2_token_cap, t3_token_cap);
    let e0 = records
        .iter()
        .filter(|task| task.encoding == EncodingId::E0)
        .map(|task| {
            (
                task.task_id.clone(),
                tokenizer.count(&task.prompt).max(1) as u64,
            )
        })
        .collect::<HashMap<_, _>>();
    let max_ratio = |encoding: EncodingId, marker: &str| {
        records
            .iter()
            .filter(|task| task.encoding == encoding)
            .filter_map(|task| {
                let e0_id = task.task_id.replace(marker, "-e0");
                e0.get(&e0_id)
                    .map(|denominator| tokenizer.count(&task.prompt) as f64 / *denominator as f64)
            })
            .fold(None::<f64>, |best, ratio| {
                Some(best.map_or(ratio, |value: f64| value.max(ratio)))
            })
    };
    Ok(BpePromptRatios {
        e2_prompt_over_e0_prompt: max_ratio(EncodingId::E2, "-e2"),
        e3_prompt_over_e0_prompt: max_ratio(EncodingId::E3, "-e3"),
    })
}

pub fn bpe_token_count(source: &str, tokenizer_name: &str) -> Result<u64, String> {
    let tokenizer = tiktoken::get_encoding(tokenizer_name)
        .ok_or_else(|| format!("unknown tokenizer {tokenizer_name}"))?;
    Ok(tokenizer.count(source) as u64)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MatchedPromptPair {
    pub e0_item_id: String,
    pub encoded_item_id: String,
    pub encoded_as: EncodingId,
    pub family: Family,
    pub e0_tokens: u64,
    pub encoded_tokens: u64,
    pub relative_gap: f64,
    pub e0_size_tier: u8,
    pub encoded_size_tier: u8,
    pub e0_semantic_class: String,
    pub encoded_semantic_class: String,
}

/// Builds disjoint, cross-item T2 prompt pairs for the H1 controlled contrast.
/// The E0 side must come from a strictly larger declared size tier. Greedy
/// selection is ordered by the smallest BPE gap and never reuses an item.
pub fn matched_t2_prompt_pairs(
    items: &[BaseItem],
    encoded_as: EncodingId,
    maximum_relative_gap: f64,
    t1_probe_count: usize,
    t2_token_cap: u32,
    t3_token_cap: u32,
    tokenizer_name: &str,
) -> Result<Vec<MatchedPromptPair>, String> {
    if !matches!(encoded_as, EncodingId::E2 | EncodingId::E3) {
        return Err("token matching supports E2 or E3 as the encoded side".into());
    }
    let tokenizer = tiktoken::get_encoding(tokenizer_name)
        .ok_or_else(|| format!("unknown tokenizer {tokenizer_name}"))?;
    let mut lengths = Vec::with_capacity(items.len());
    for item in items {
        let tasks = adapt_all(item, t1_probe_count, t2_token_cap, t3_token_cap);
        let t2 = |encoding| {
            tasks
                .iter()
                .find(|task| task.family == Family::T2 && task.encoding == encoding)
                .map(|task| tokenizer.count(&task.prompt) as u64)
                .ok_or_else(|| format!("missing T2/{encoding:?} task for {}", item.item_id))
        };
        // D36: every item supplies the E0 side, but only tiers that actually
        // render `encoded_as` can supply the encoded side. Treating a missing
        // rendering as an error would abort the whole table; it is simply an
        // item that is ineligible for that half of the pair.
        let encoded = if crate::generator::tier_renders_encoding(
            item.annotations.program_size_tier,
            encoded_as,
        ) {
            Some(t2(encoded_as)?)
        } else {
            None
        };
        lengths.push((item, t2(EncodingId::E0)?, encoded));
    }

    let mut candidates = Vec::new();
    for (e0_index, (e0_item, e0_tokens, _)) in lengths.iter().enumerate() {
        for (encoded_index, (encoded_item, _, encoded_tokens)) in lengths.iter().enumerate() {
            let Some(encoded_tokens) = encoded_tokens else {
                continue;
            };
            if e0_item.annotations.program_size_tier <= encoded_item.annotations.program_size_tier {
                continue;
            }
            let denominator = (*e0_tokens).max(*encoded_tokens).max(1) as f64;
            let gap = e0_tokens.abs_diff(*encoded_tokens) as f64 / denominator;
            if gap <= maximum_relative_gap {
                candidates.push((gap, e0_index, encoded_index));
            }
        }
    }
    candidates.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });

    let mut used = std::collections::HashSet::new();
    let mut pairs = Vec::new();
    for (relative_gap, e0_index, encoded_index) in candidates {
        if used.contains(&e0_index) || used.contains(&encoded_index) {
            continue;
        }
        let (e0_item, e0_tokens, _) = lengths[e0_index];
        let (encoded_item, _, encoded_tokens) = lengths[encoded_index];
        // Only indices whose encoded rendering exists reach `candidates`, so
        // this is total by construction; skipping rather than unwrapping keeps
        // it that way if the candidate filter is ever changed. The check comes
        // before the `used` inserts so a skipped index is not consumed.
        let Some(encoded_tokens) = encoded_tokens else {
            continue;
        };
        used.insert(e0_index);
        used.insert(encoded_index);
        pairs.push(MatchedPromptPair {
            e0_item_id: e0_item.item_id.clone(),
            encoded_item_id: encoded_item.item_id.clone(),
            encoded_as,
            family: Family::T2,
            e0_tokens,
            encoded_tokens,
            relative_gap,
            e0_size_tier: e0_item.annotations.program_size_tier,
            encoded_size_tier: encoded_item.annotations.program_size_tier,
            e0_semantic_class: e0_item.annotations.grammar_shape.clone(),
            encoded_semantic_class: encoded_item.annotations.grammar_shape.clone(),
        });
    }
    pairs.sort_by(|a, b| {
        a.relative_gap
            .total_cmp(&b.relative_gap)
            .then_with(|| a.e0_item_id.cmp(&b.e0_item_id))
    });
    Ok(pairs)
}

pub fn task_budgets_are_encoding_invariant(
    item: &BaseItem,
    t1_probe_count: usize,
    t2_token_cap: u32,
    t3_token_cap: u32,
) -> bool {
    let records = adapt_all(item, t1_probe_count, t2_token_cap, t3_token_cap);
    task_records_have_encoding_invariant_budgets(
        item.annotations.program_size_tier,
        item.oracles.t2_reference_solution_tokens_upper_bound,
        &records,
        t2_token_cap,
        t3_token_cap,
    )
}

/// D11 + D36: response budgets remain representation-independent, but the
/// comparison domain is the set of encodings actually rendered at this tier.
/// A missing rendering is not a budget value. Every expected rendering must
/// still occur exactly once, and every rendered value must agree.
fn task_records_have_encoding_invariant_budgets(
    program_size_tier: u8,
    t2_reference_solution_tokens_upper_bound: u32,
    records: &[TaskRecord],
    t2_token_cap: u32,
    t3_token_cap: u32,
) -> bool {
    let rendered_encodings = [
        EncodingId::E0,
        EncodingId::E1,
        EncodingId::E2,
        EncodingId::E3,
    ]
    .into_iter()
    .filter(|encoding| crate::generator::tier_renders_encoding(program_size_tier, *encoding))
    .collect::<Vec<_>>();

    let family_budget = |family| {
        let family_records = records
            .iter()
            .filter(|task| task.family == family)
            .collect::<Vec<_>>();
        if family_records.len() != rendered_encodings.len() {
            return None;
        }
        let mut budgets = Vec::with_capacity(rendered_encodings.len());
        for encoding in &rendered_encodings {
            let mut matching = family_records
                .iter()
                .filter(|task| task.encoding == *encoding);
            let budget = matching.next()?.hard_token_cap?;
            if matching.next().is_some() {
                return None;
            }
            budgets.push(budget);
        }
        let first = *budgets.first()?;
        budgets
            .iter()
            .all(|budget| *budget == first)
            .then_some(first)
    };

    let Some(t2) = family_budget(Family::T2) else {
        return false;
    };
    let Some(t3) = family_budget(Family::T3) else {
        return false;
    };
    t2 >= t2_reference_solution_tokens_upper_bound
        && t2 <= t2_token_cap.max(t2_reference_solution_tokens_upper_bound)
        && t3 <= t3_token_cap
}

pub fn batch_budgets_are_diverse(items: &[BaseItem], t2_token_cap: u32, t3_token_cap: u32) -> bool {
    use std::collections::BTreeSet;

    let required = items.len().min(5);
    if required == 0 {
        return true;
    }
    let t2 = items
        .iter()
        .map(|item| item_t2_token_cap(item, t2_token_cap))
        .collect::<BTreeSet<_>>();
    let t3 = items
        .iter()
        .map(|item| item_t3_token_cap(item, t3_token_cap))
        .collect::<BTreeSet<_>>();
    t2.len() >= required && t3.len() >= required
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Expr {
    Number(i64),
    Input(usize),
    UnaryMinus(Box<Expr>),
    Binary(char, Box<Expr>, Box<Expr>),
    FloorDiv(Box<Expr>, Box<Expr>),
}

impl Expr {
    fn depends_on_input(&self) -> bool {
        match self {
            Expr::Number(_) => false,
            Expr::Input(_) => true,
            Expr::UnaryMinus(value) => value.depends_on_input(),
            Expr::Binary(_, left, right) | Expr::FloorDiv(left, right) => {
                left.depends_on_input() || right.depends_on_input()
            }
        }
    }

    fn eval(&self, input: &[u8]) -> Option<i64> {
        match self {
            Expr::Number(n) => Some(*n),
            Expr::Input(i) => input.get(*i).map(|x| *x as i64),
            Expr::UnaryMinus(x) => x.eval(input)?.checked_neg(),
            Expr::FloorDiv(a, b) => {
                let divisor = b.eval(input)?;
                if divisor == 0 {
                    None
                } else {
                    Some(a.eval(input)?.div_euclid(divisor))
                }
            }
            Expr::Binary(op, a, b) => {
                let (a, b) = (a.eval(input)?, b.eval(input)?);
                match op {
                    '+' => a.checked_add(b),
                    '-' => a.checked_sub(b),
                    '*' => a.checked_mul(b),
                    '%' if b != 0 => Some(a.rem_euclid(b)),
                    '&' => Some(a & b),
                    '|' => Some(a | b),
                    '^' => Some(a ^ b),
                    _ => None,
                }
            }
        }
    }

    fn fold_constants(self) -> Result<Self, String> {
        let folded = match self {
            Expr::Number(_) | Expr::Input(_) => self,
            Expr::UnaryMinus(value) => Expr::UnaryMinus(Box::new(value.fold_constants()?)),
            Expr::Binary(op, left, right) => Expr::Binary(
                op,
                Box::new(left.fold_constants()?),
                Box::new(right.fold_constants()?),
            ),
            Expr::FloorDiv(left, right) => Expr::FloorDiv(
                Box::new(left.fold_constants()?),
                Box::new(right.fold_constants()?),
            ),
        };
        if folded.depends_on_input() {
            Ok(folded)
        } else {
            folded
                .eval(&[])
                .map(Expr::Number)
                .ok_or_else(|| "constant expression overflowed or divided by zero".into())
        }
    }

    fn rendered(&self) -> (String, u8) {
        match self {
            Expr::Number(value) => (value.to_string(), if *value < 0 { 90 } else { 100 }),
            Expr::Input(index) => (format!("inputs[{index}]"), 100),
            Expr::UnaryMinus(value) => {
                let (source, precedence) = value.rendered();
                (
                    format!("-{}", parenthesize(&source, precedence, 90, false)),
                    90,
                )
            }
            Expr::Binary(op, left, right) => {
                let precedence = match op {
                    '*' | '%' => 80,
                    '+' | '-' => 70,
                    '&' => 60,
                    '^' => 50,
                    '|' => 40,
                    _ => unreachable!("parser emits only declared binary operators"),
                };
                let (left_source, left_precedence) = left.rendered();
                let (right_source, right_precedence) = right.rendered();
                (
                    format!(
                        "{}{}{}",
                        parenthesize(&left_source, left_precedence, precedence, false),
                        op,
                        parenthesize(&right_source, right_precedence, precedence, true)
                    ),
                    precedence,
                )
            }
            Expr::FloorDiv(left, right) => {
                let precedence = 80;
                let (left_source, left_precedence) = left.rendered();
                let (right_source, right_precedence) = right.rendered();
                (
                    format!(
                        "{}//{}",
                        parenthesize(&left_source, left_precedence, precedence, false),
                        parenthesize(&right_source, right_precedence, precedence, true)
                    ),
                    precedence,
                )
            }
        }
    }
}

struct ExprParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> ExprParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            bytes: source.as_bytes(),
            pos: 0,
        }
    }
    fn skip(&mut self) {
        while self
            .bytes
            .get(self.pos)
            .is_some_and(|b| b.is_ascii_whitespace())
        {
            self.pos += 1;
        }
    }
    fn take(&mut self, token: &[u8]) -> bool {
        self.skip();
        if self.bytes.get(self.pos..self.pos + token.len()) == Some(token) {
            self.pos += token.len();
            true
        } else {
            false
        }
    }
    fn parse(mut self) -> Result<Expr, String> {
        let expr = self.bit_or()?;
        self.skip();
        if self.pos != self.bytes.len() {
            return Err(format!("unexpected token at byte {}", self.pos));
        }
        Ok(expr)
    }
    fn bit_or(&mut self) -> Result<Expr, String> {
        let mut left = self.bit_xor()?;
        while self.take(b"|") {
            left = Expr::Binary('|', Box::new(left), Box::new(self.bit_xor()?));
        }
        Ok(left)
    }
    fn bit_xor(&mut self) -> Result<Expr, String> {
        let mut left = self.bit_and()?;
        while self.take(b"^") {
            left = Expr::Binary('^', Box::new(left), Box::new(self.bit_and()?));
        }
        Ok(left)
    }
    fn bit_and(&mut self) -> Result<Expr, String> {
        let mut left = self.sum()?;
        while self.take(b"&") {
            left = Expr::Binary('&', Box::new(left), Box::new(self.sum()?));
        }
        Ok(left)
    }
    fn sum(&mut self) -> Result<Expr, String> {
        let mut left = self.product()?;
        loop {
            if self.take(b"+") {
                left = Expr::Binary('+', Box::new(left), Box::new(self.product()?));
            } else if self.take(b"-") {
                left = Expr::Binary('-', Box::new(left), Box::new(self.product()?));
            } else {
                return Ok(left);
            }
        }
    }
    fn product(&mut self) -> Result<Expr, String> {
        let mut left = self.unary()?;
        loop {
            if self.take(b"//") {
                left = Expr::FloorDiv(Box::new(left), Box::new(self.unary()?));
            } else if self.take(b"*") {
                left = Expr::Binary('*', Box::new(left), Box::new(self.unary()?));
            } else if self.take(b"%") {
                left = Expr::Binary('%', Box::new(left), Box::new(self.unary()?));
            } else {
                return Ok(left);
            }
        }
    }
    fn unary(&mut self) -> Result<Expr, String> {
        if self.take(b"-") {
            Ok(Expr::UnaryMinus(Box::new(self.unary()?)))
        } else {
            self.primary()
        }
    }
    fn primary(&mut self) -> Result<Expr, String> {
        self.skip();
        if self.take(b"(") {
            let expr = self.bit_or()?;
            if !self.take(b")") {
                return Err("missing closing parenthesis".into());
            }
            return Ok(expr);
        }
        if self
            .bytes
            .get(self.pos..)
            .is_some_and(|x| x.starts_with(b"inputs"))
        {
            self.pos += b"inputs".len();
            if !self.take(b"[") {
                return Err("expected `[` after inputs".into());
            }
            let index = self.number()? as usize;
            if !self.take(b"]") {
                return Err("expected `]` after input index".into());
            }
            return Ok(Expr::Input(index));
        }
        let literal = self.number()?;
        if literal > 256 {
            return Err("integer literals must be in 0..=256".into());
        }
        Ok(Expr::Number(literal))
    }
    fn number(&mut self) -> Result<i64, String> {
        self.skip();
        let start = self.pos;
        while self.bytes.get(self.pos).is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
        if start == self.pos {
            return Err(format!("expected integer at byte {}", self.pos));
        }
        std::str::from_utf8(&self.bytes[start..self.pos])
            .ok()
            .and_then(|x| x.parse().ok())
            .ok_or_else(|| "invalid integer".into())
    }
}

fn parse_solution(source: &str) -> Result<Vec<Expr>, String> {
    let body = source
        .trim()
        .strip_prefix("def solve(inputs):")
        .ok_or("missing exact solve definition")?
        .trim();
    let list = body
        .strip_prefix("return [")
        .and_then(|x| x.strip_suffix(']'))
        .ok_or("solve body must be one return-list expression")?;
    let mut depth = 0i32;
    let mut start = 0;
    let mut parts = Vec::new();
    for (i, c) in list.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&list[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        if depth < 0 {
            return Err("unbalanced delimiters".into());
        }
    }
    if depth != 0 {
        return Err("unbalanced delimiters".into());
    }
    parts.push(&list[start..]);
    parts
        .into_iter()
        .map(|part| ExprParser::new(part).parse()?.fold_constants())
        .collect()
}

fn render_solution(expressions: &[Expr]) -> String {
    let body = expressions
        .iter()
        .map(|expression| expression.rendered().0)
        .collect::<Vec<_>>()
        .join(",");
    format!("def solve(inputs):\n    return [{body}]")
}

/// Parse, constant-fold, and render a T2 solution in the same canonical form
/// used by grading and reference search.
pub fn canonical_solution(source: &str) -> Result<String, String> {
    parse_solution(source).map(|expressions| render_solution(&expressions))
}

/// Evaluate a canonical T2 solution over the complete declared byte domain.
/// Reference expressions are already full-domain witnesses; exposing the table
/// lets the duplicate audit compute Hamming distance without rerunning the much
/// more expensive Brainfuck program.
pub fn solution_domain_outputs(source: &str, arity: u8) -> Result<Vec<Vec<u8>>, String> {
    if !(1..=2).contains(&arity) {
        return Err("arity must be 1 or 2".into());
    }
    let expressions = parse_solution(source)?;
    let required = 256usize.pow(arity as u32);
    (0..required)
        .map(|n| {
            let input = if arity == 1 {
                vec![n as u8]
            } else {
                vec![(n >> 8) as u8, n as u8]
            };
            evaluate_expressions(&expressions, &input)
                .ok_or_else(|| format!("expression evaluation failed at domain index {n}"))
        })
        .collect()
}

/// Complete-domain digest under the benchmark's length-delimited fingerprint
/// format. The duplicate audit uses this to fail closed if a private reference
/// expression no longer matches its stored semantic oracle.
pub fn solution_semantic_digest(source: &str, arity: u8) -> Result<String, String> {
    let expressions = parse_solution(source)?;
    expression_digest(&expressions, arity)
        .ok_or_else(|| "expression failed on the declared byte domain".into())
}

pub fn folded_solution_token_count(source: &str) -> Result<u64, String> {
    parse_solution(source).map(|expressions| lexical_token_count(&render_solution(&expressions)))
}

#[derive(Clone)]
struct EnumeratedExpr {
    values: std::sync::Arc<[i64]>,
    source: String,
    precedence: u8,
    folded_expression_tokens: u32,
    depends_on_input: bool,
}

#[derive(Clone, Copy)]
enum EnumeratedOp {
    Add,
    Sub,
    Mul,
    FloorDiv,
    Mod,
    And,
    Or,
    Xor,
}

impl EnumeratedOp {
    const ALL: [Self; 8] = [
        Self::Add,
        Self::Sub,
        Self::Mul,
        Self::FloorDiv,
        Self::Mod,
        Self::And,
        Self::Or,
        Self::Xor,
    ];

    fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::FloorDiv => "//",
            Self::Mod => "%",
            Self::And => "&",
            Self::Or => "|",
            Self::Xor => "^",
        }
    }

    fn precedence(self) -> u8 {
        match self {
            Self::Mul | Self::FloorDiv | Self::Mod => 80,
            Self::Add | Self::Sub => 70,
            Self::And => 60,
            Self::Xor => 50,
            Self::Or => 40,
        }
    }

    fn commutative(self) -> bool {
        matches!(
            self,
            Self::Add | Self::Mul | Self::And | Self::Or | Self::Xor
        )
    }

    fn apply(self, left: i64, right: i64) -> Option<i64> {
        match self {
            Self::Add => left.checked_add(right),
            Self::Sub => left.checked_sub(right),
            Self::Mul => left.checked_mul(right),
            Self::FloorDiv if right != 0 => Some(left.div_euclid(right)),
            Self::Mod if right != 0 => Some(left.rem_euclid(right)),
            Self::And => Some(left & right),
            Self::Or => Some(left | right),
            Self::Xor => Some(left ^ right),
            _ => None,
        }
    }
}

fn wrapped_solution(expr: &str) -> String {
    format!("def solve(inputs):\n    return [{expr}]")
}

fn parenthesize(source: &str, child_precedence: u8, parent_precedence: u8, right: bool) -> String {
    if child_precedence < parent_precedence || (right && child_precedence == parent_precedence) {
        format!("({source})")
    } else {
        source.to_string()
    }
}

fn enumerated_binary_source(
    op: EnumeratedOp,
    left: &EnumeratedExpr,
    right: &EnumeratedExpr,
) -> String {
    let precedence = op.precedence();
    format!(
        "{}{}{}",
        parenthesize(&left.source, left.precedence, precedence, false),
        op.symbol(),
        parenthesize(&right.source, right.precedence, precedence, true)
    )
}

fn keep_shortest_candidate(
    level: &mut std::collections::HashMap<std::sync::Arc<[i64]>, Vec<EnumeratedExpr>>,
    seen_frontier: &std::collections::HashMap<std::sync::Arc<[i64]>, Vec<(u8, u32)>>,
    candidate: EnumeratedExpr,
) {
    let dominates = |precedence: u8, tokens: u32| {
        tokens <= candidate.folded_expression_tokens && precedence >= candidate.precedence
    };
    if seen_frontier
        .get(&candidate.values)
        .is_some_and(|frontier| {
            frontier
                .iter()
                .any(|(precedence, tokens)| dominates(*precedence, *tokens))
        })
    {
        return;
    }
    let frontier = level.entry(candidate.values.clone()).or_default();
    if let Some(incumbent) = frontier.iter_mut().find(|incumbent| {
        (incumbent.precedence, incumbent.folded_expression_tokens)
            == (candidate.precedence, candidate.folded_expression_tokens)
    }) {
        if (candidate.source.len(), &candidate.source) < (incumbent.source.len(), &incumbent.source)
        {
            *incumbent = candidate;
        }
        return;
    }
    if frontier
        .iter()
        .any(|incumbent| dominates(incumbent.precedence, incumbent.folded_expression_tokens))
    {
        return;
    }
    frontier.retain(|incumbent| {
        !(candidate.folded_expression_tokens <= incumbent.folded_expression_tokens
            && candidate.precedence >= incumbent.precedence)
    });
    frontier.push(candidate);
}

fn extend_seen_frontier(
    seen_frontier: &mut std::collections::HashMap<std::sync::Arc<[i64]>, Vec<(u8, u32)>>,
    candidate: &EnumeratedExpr,
) {
    let frontier = seen_frontier.entry(candidate.values.clone()).or_default();
    if frontier.iter().any(|(precedence, tokens)| {
        *tokens <= candidate.folded_expression_tokens && *precedence >= candidate.precedence
    }) {
        return;
    }
    frontier.retain(|(precedence, tokens)| {
        !(candidate.folded_expression_tokens <= *tokens && candidate.precedence >= *precedence)
    });
    frontier.push((candidate.precedence, candidate.folded_expression_tokens));
}

fn detected_integer_polynomial_degree(values: &[i64], max_degree: u8) -> Option<u8> {
    let mut differences = values.to_vec();
    for degree in 0..=max_degree {
        if differences.windows(2).all(|pair| pair[0] == pair[1]) {
            return Some(degree);
        }
        differences = differences
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .collect();
    }
    None
}

fn detected_exact_period(values: &[i64], max_period: u16) -> Option<u16> {
    (1..=max_period.min(values.len().saturating_sub(1) as u16)).find(|period| {
        let period = *period as usize;
        (0..values.len() - period).all(|index| values[index + period] == values[index])
    })
}

fn detected_additive_period(values: &[i64], max_period: u16) -> Option<AdditivePeriodWitness> {
    (1..=max_period.min(values.len().saturating_sub(1) as u16)).find_map(|period| {
        let offset = period as usize;
        let delta = (values[offset] - values[0]).rem_euclid(256) as u8;
        ((0..values.len() - offset)
            .all(|index| (values[index + offset] - values[index]).rem_euclid(256) as u8 == delta))
        .then_some(AdditivePeriodWitness {
            period,
            delta_mod_256: delta,
        })
    })
}

fn minimum_integer_affine_pieces(values: &[i64]) -> usize {
    if values.is_empty() {
        return 0;
    }
    let mut affine = vec![vec![false; values.len() + 1]; values.len()];
    for start in 0..values.len() {
        affine[start][start + 1] = true;
        if start + 2 <= values.len() {
            affine[start][start + 2] = true;
        }
        if start + 3 <= values.len() {
            let slope = values[start + 1] - values[start];
            for end in start + 3..=values.len() {
                if values[end - 1] - values[end - 2] != slope {
                    break;
                }
                affine[start][end] = true;
            }
        }
    }
    let mut pieces = vec![usize::MAX; values.len() + 1];
    pieces[0] = 0;
    for end in 1..=values.len() {
        for start in 0..end {
            if affine[start][end] && pieces[start] != usize::MAX {
                pieces[end] = pieces[end].min(pieces[start] + 1);
            }
        }
    }
    pieces[values.len()]
}

fn is_bitwise_pointwise(values: &[i64]) -> bool {
    if values.len() != 256 || values.iter().any(|value| !(0..=255).contains(value)) {
        return false;
    }
    (0..8).all(|bit| {
        let mut output_for_input_bit = [None, None];
        values.iter().enumerate().all(|(input, output)| {
            let input_bit = (input >> bit) & 1;
            let output_bit = ((*output as u8) >> bit) & 1;
            match output_for_input_bit[input_bit] {
                Some(expected) => expected == output_bit,
                None => {
                    output_for_input_bit[input_bit] = Some(output_bit);
                    true
                }
            }
        })
    })
}

fn compact_expression_witness(
    family: &str,
    expression: String,
    values: &[i64],
    threshold_exclusive: u32,
) -> Option<AnalyticalExpressionWitness> {
    let source = wrapped_solution(&expression);
    let expressions = parse_solution(&source).ok()?;
    let folded_expression_tokens = expressions
        .first()
        .and_then(|expression| expression_grammar_token_count(&expression.rendered().0).ok())?;
    if expressions.len() != 1 || folded_expression_tokens >= threshold_exclusive {
        return None;
    }
    let matches = values.iter().enumerate().all(|(input, expected)| {
        evaluate_expressions(&expressions, &[input as u8])
            .is_some_and(|output| output.as_slice() == [*expected as u8])
            && (0..=255).contains(expected)
    });
    matches.then_some(AnalyticalExpressionWitness {
        family: family.into(),
        expression: source,
        folded_expression_grammar_tokens: folded_expression_tokens,
    })
}

fn newton_expression(samples: &[i64], variable: &str, degree: u8) -> Option<String> {
    if samples.is_empty() || degree as usize >= samples.len() {
        return None;
    }
    let mut row = samples.to_vec();
    let mut coefficients = Vec::with_capacity(degree as usize + 1);
    for _ in 0..=degree {
        coefficients.push(*row.first()?);
        row = row.windows(2).map(|pair| pair[1] - pair[0]).collect();
    }
    let mut terms = Vec::new();
    for (order, coefficient) in coefficients.into_iter().enumerate() {
        let coefficient = coefficient.rem_euclid(256);
        if coefficient == 0 {
            continue;
        }
        let basis = match order {
            0 => String::new(),
            1 => variable.to_string(),
            _ => {
                let numerator = (0..order)
                    .map(|offset| {
                        if offset == 0 {
                            variable.to_string()
                        } else {
                            format!("({variable}-{offset})")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("*");
                let factorial = (2..=order)
                    .map(|factor| factor.to_string())
                    .collect::<Vec<_>>()
                    .join("*");
                format!("({numerator}//({factorial}))")
            }
        };
        terms.push(match (coefficient, basis.is_empty()) {
            (value, true) => value.to_string(),
            (1, false) => basis,
            (value, false) => format!("{value}*{basis}"),
        });
    }
    Some(if terms.is_empty() {
        "0".into()
    } else {
        format!("({})%256", terms.join("+"))
    })
}

fn additive_affine_expression(values: &[i64], witness: &AdditivePeriodWitness) -> String {
    let period = witness.period as usize;
    let bias = values[0].rem_euclid(256);
    let residue_slope = if period > 1 {
        (values[1] - values[0]).rem_euclid(256)
    } else {
        0
    };
    let mut terms = vec![bias.to_string()];
    if residue_slope != 0 {
        terms.push(format!("{residue_slope}*(inputs[0]%{period})"));
    }
    if witness.delta_mod_256 != 0 {
        terms.push(format!("{}*(inputs[0]//{period})", witness.delta_mod_256));
    }
    format!("({})%256", terms.join("+"))
}

fn bitwise_pointwise_expression(values: &[i64]) -> Option<String> {
    if !is_bitwise_pointwise(values) {
        return None;
    }
    let mut identity_mask = 0u8;
    let mut inverted_mask = 0u8;
    let mut constant_mask = 0u8;
    for bit in 0..8 {
        let zero = ((values[0] as u8) >> bit) & 1;
        let one = ((values[1usize << bit] as u8) >> bit) & 1;
        match (zero, one) {
            (0, 1) => identity_mask |= 1 << bit,
            (1, 0) => inverted_mask |= 1 << bit,
            (1, 1) => constant_mask |= 1 << bit,
            (0, 0) => {}
            _ => unreachable!(),
        }
    }
    let mut terms = Vec::new();
    if identity_mask != 0 {
        terms.push(format!("inputs[0]&{identity_mask}"));
    }
    if inverted_mask != 0 {
        terms.push(format!("(inputs[0]^255)&{inverted_mask}"));
    }
    if constant_mask != 0 {
        terms.push(constant_mask.to_string());
    }
    Some(if terms.is_empty() {
        "0".into()
    } else {
        terms.join("|")
    })
}

/// Characterizes broad structural families, but rejects only when that
/// characterization can synthesize a concrete ExprParser solution below the
/// same folded-token threshold as the exact layer. A detected period or a low
/// piece count is diagnostic by itself, never an unconditional rejection.
pub fn analyze_named_trivial_families(
    values: &[i64],
    threshold_exclusive: u32,
) -> AnalyticalNontrivialityWitness {
    let derived_max_degree = threshold_exclusive.saturating_sub(4).div_euclid(3).min(7) as u8;
    // A period is reported only when at least two full blocks can be compared.
    // Larger offsets would make every table vacuously "additive-periodic" at
    // p = domain_size - 1 from a single endpoint pair.
    let max_period = (values.len() / 2).min(u16::MAX as usize) as u16;
    let polynomial_degree = detected_integer_polynomial_degree(values, derived_max_degree);
    let exact_period = detected_exact_period(values, max_period);
    let additive_period =
        detected_additive_period(values, max_period).filter(|witness| witness.delta_mod_256 != 0);
    let affine_pieces = minimum_integer_affine_pieces(values);
    let bitwise_pointwise = is_bitwise_pointwise(values);
    let mut compact = Vec::new();

    if let Some(degree) = polynomial_degree
        && let Some(expression) = newton_expression(values, "inputs[0]", degree)
        && let Some(witness) = compact_expression_witness(
            "compact_integer_polynomial",
            expression,
            values,
            threshold_exclusive,
        )
    {
        compact.push(witness);
    }
    if let Some(period) = exact_period
        && let Some(degree) =
            detected_integer_polynomial_degree(&values[..period as usize], derived_max_degree)
        && let Some(expression) = newton_expression(
            &values[..period as usize],
            &format!("(inputs[0]%{period})"),
            degree,
        )
        && let Some(witness) = compact_expression_witness(
            "compact_exact_periodic_residue_polynomial",
            expression,
            values,
            threshold_exclusive,
        )
    {
        compact.push(witness);
    }
    if let Some(period_witness) = &additive_period
        && let Some(witness) = compact_expression_witness(
            "compact_affine_residue_additive_period",
            additive_affine_expression(values, period_witness),
            values,
            threshold_exclusive,
        )
    {
        compact.push(witness);
    }
    if let Some(expression) = bitwise_pointwise_expression(values)
        && let Some(witness) = compact_expression_witness(
            "compact_bitwise_pointwise",
            expression,
            values,
            threshold_exclusive,
        )
    {
        compact.push(witness);
    }
    if let Some(first) = values
        .first()
        .copied()
        .filter(|value| (0..=256).contains(value))
        && let Some(witness) = compact_expression_witness(
            "compact_single_boundary_floor",
            format!("{first}//(inputs[0]+1)"),
            values,
            threshold_exclusive,
        )
    {
        compact.push(witness);
    }

    compact.sort_by_key(|witness| {
        (
            witness.folded_expression_grammar_tokens,
            witness.family.clone(),
        )
    });
    compact.dedup_by(|left, right| left.family == right.family);
    let matched = compact
        .iter()
        .map(|witness| witness.family.clone())
        .collect::<Vec<_>>();
    AnalyticalNontrivialityWitness {
        algorithm: "token_calibrated_concrete_expression_witnesses_v2".into(),
        folded_expression_grammar_token_threshold_exclusive: threshold_exclusive,
        derived_max_integer_polynomial_degree_checked: derived_max_degree,
        detected_integer_polynomial_degree: polynomial_degree,
        max_period_checked: max_period,
        detected_exact_period: exact_period,
        detected_additive_period: additive_period,
        minimum_integer_affine_pieces: affine_pieces,
        bitwise_pointwise,
        compact_expression_witnesses: compact,
        named_families_excluded: matched.is_empty(),
        matched_trivial_families: matched,
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ConstructorSearchSurvivor {
    pub template_family: String,
    pub expression: String,
    pub folded_expression_grammar_tokens: u64,
    pub semantic_digest_hex: String,
    pub semantic_cluster: String,
    pub detected_exact_period: Option<u16>,
    pub detected_additive_period: Option<AdditivePeriodWitness>,
    pub minimum_integer_affine_pieces: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConstructorSearchReport {
    pub algorithm: String,
    pub folded_expression_grammar_token_threshold_exclusive: u32,
    pub templates_generated: usize,
    pub analytically_rejected: usize,
    pub reference_too_short_rejected: usize,
    pub shallow_exact_rejected: usize,
    pub requested_exact_ast_depth: u8,
    pub proven_exhaustive_ast_depth: u8,
    pub semantic_clusters: usize,
    pub clustering_audit: ConstructorClusteringAudit,
    pub survivors: Vec<ConstructorSearchSurvivor>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConstructorClusteringAudit {
    pub profile_kind: String,
    pub candidate_records_before_global_dedup: usize,
    pub records_after_adjacent_only_dedup: usize,
    pub duplicate_semantic_records_removed: usize,
    pub nonadjacent_duplicates_previously_missed: usize,
    pub unique_semantic_functions: usize,
    pub profile_buckets: usize,
    pub singleton_profile_buckets: usize,
    pub largest_profile_bucket: usize,
    pub mixed_template_family_profile_buckets: usize,
}

fn expression_values(expression: &str) -> Option<Vec<i64>> {
    let expressions = parse_solution(&wrapped_solution(expression)).ok()?;
    (0..=255u8)
        .map(|input| {
            evaluate_expressions(&expressions, &[input])
                .and_then(|output| output.first().copied())
                .map(i64::from)
        })
        .collect()
}

fn shallow_parser_match_through_ast3(
    values: &[i64],
    threshold_exclusive: u32,
) -> Option<AnalyticalExpressionWitness> {
    for expression in [
        "inputs[0]".to_string(),
        "-inputs[0]".to_string(),
        "--inputs[0]".to_string(),
    ] {
        if let Some(witness) = compact_expression_witness(
            "exact_parser_ast_le_3",
            expression,
            values,
            threshold_exclusive,
        ) {
            return Some(witness);
        }
    }
    for literal in 0..=256 {
        for operator in ["+", "-", "*", "//", "%", "&", "|", "^"] {
            for expression in [
                format!("inputs[0]{operator}{literal}"),
                format!("{literal}{operator}inputs[0]"),
            ] {
                if let Some(witness) = compact_expression_witness(
                    "exact_parser_ast_le_3",
                    expression,
                    values,
                    threshold_exclusive,
                ) {
                    return Some(witness);
                }
            }
        }
    }
    for operator in ["+", "-", "*", "//", "%", "&", "|", "^"] {
        if let Some(witness) = compact_expression_witness(
            "exact_parser_ast_le_3",
            format!("inputs[0]{operator}inputs[0]"),
            values,
            threshold_exclusive,
        ) {
            return Some(witness);
        }
    }
    None
}

fn semantic_cardinality_bucket(cardinality: usize) -> &'static str {
    match cardinality {
        0..=8 => "0-8",
        9..=16 => "9-16",
        17..=32 => "17-32",
        33..=64 => "33-64",
        65..=128 => "65-128",
        _ => "129-256",
    }
}

/// A constructor-independent profile of the complete 256-point function.
/// Template-family names are deliberately excluded so adding labels cannot
/// manufacture apparent semantic diversity.
pub(crate) fn constructor_semantic_profile(
    values: &[i64],
    analytical: &AnalyticalNontrivialityWitness,
) -> String {
    // Integer affine pieces are sensitive to where an otherwise irrelevant
    // modular output bias crosses 255 -> 0. Normalize that translation before
    // bucketing so constructor profiles describe shape, not the sampled bias.
    let anchor = values.first().copied().unwrap_or(0);
    let bias_normalized = values
        .iter()
        .map(|value| (value - anchor).rem_euclid(256))
        .collect::<Vec<_>>();
    let output_support = values
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let first_differences = values
        .windows(2)
        .map(|window| (window[1] - window[0]).rem_euclid(256))
        .collect::<Vec<_>>();
    let first_support = first_differences
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let second_support = first_differences
        .windows(2)
        .map(|window| (window[1] - window[0]).rem_euclid(256))
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let period_class = if analytical.detected_exact_period.is_some() {
        "periodic"
    } else if analytical.detected_additive_period.is_some() {
        "additive-periodic"
    } else {
        "aperiodic-on-domain"
    };
    let piece_bucket = match minimum_integer_affine_pieces(&bias_normalized) {
        0..=8 => "0-8",
        9..=32 => "9-32",
        33..=96 => "33-96",
        _ => "97-plus",
    };
    format!(
        "{period_class}/pieces-{piece_bucket}/outputs-{}/d1-{}/d2-{}",
        semantic_cardinality_bucket(output_support),
        semantic_cardinality_bucket(first_support),
        semantic_cardinality_bucket(second_support),
    )
}

/// Generates a preregistered coprime-modulus template grid, uses the calibrated
/// analytical layer as a design oracle, then applies a genuinely exhaustive
/// parser check through AST size 3. Survivors are proposals for later IR
/// constructors, not accepted benchmark items or evidence.
pub fn search_constructor_templates(threshold_exclusive: u32) -> ConstructorSearchReport {
    let moduli = [5u16, 7, 11, 13, 17];
    let coefficients = [1u16, 3, 5, 7, 11];
    let mut templates_generated = 0;
    let mut analytically_rejected = 0;
    let mut reference_too_short_rejected = 0;
    let mut shallow_exact_rejected = 0;
    let mut survivors = Vec::new();
    for (left_index, left) in moduli.iter().enumerate() {
        for right in moduli.iter().skip(left_index + 1) {
            for quotient in moduli
                .iter()
                .filter(|modulus| *modulus != left && *modulus != right)
            {
                for coefficient in coefficients {
                    let templates = [
                        (
                            "coprime_residue_product_plus_quotient",
                            format!(
                                "((inputs[0]%{left})*(inputs[0]%{right})+(inputs[0]//{quotient})*{coefficient}+113)%256"
                            ),
                        ),
                        (
                            "residue_times_quotient_residue_coupling",
                            format!(
                                "((inputs[0]%{left})*((inputs[0]//{right})%{quotient})+(inputs[0]%{right})*{coefficient}+113)%256"
                            ),
                        ),
                        (
                            "triple_residue_product_plus_quotient",
                            format!(
                                "((inputs[0]%{left})*(inputs[0]%{right})*(inputs[0]%{quotient})+(inputs[0]//{left})*{coefficient}+113)%256"
                            ),
                        ),
                        (
                            "quotient_times_residue_plus_residue",
                            format!(
                                "((inputs[0]//{left})*(inputs[0]%{right})+(inputs[0]%{quotient})*{coefficient}+113)%256"
                            ),
                        ),
                        (
                            "nested_quotient_residue_product",
                            format!(
                                "(((inputs[0]//{left})%{right})*(inputs[0]%{quotient})+(inputs[0]//{quotient})*{coefficient}+113)%256"
                            ),
                        ),
                        (
                            "residue_square_plus_cross_product",
                            format!(
                                "((inputs[0]%{left})*(inputs[0]%{left})+(inputs[0]%{right})*(inputs[0]%{quotient})+(inputs[0]//{quotient})*{coefficient}+113)%256"
                            ),
                        ),
                        (
                            "dual_coupled_residue_products",
                            format!(
                                "((inputs[0]%{left})*((inputs[0]//{right})%{quotient})+(inputs[0]%{right})*(inputs[0]%{quotient})*{coefficient}+113)%256"
                            ),
                        ),
                        (
                            "dual_residue_product_with_nested_quotient",
                            format!(
                                "((inputs[0]%{left})*(inputs[0]%{right})+(inputs[0]%{quotient})*((inputs[0]//{left})%{right})*{coefficient}+113)%256"
                            ),
                        ),
                        (
                            "self_quotient_plus_residue_product",
                            format!(
                                "(inputs[0]*(inputs[0]//{left})+(inputs[0]%{right})*(inputs[0]%{quotient})*{coefficient}+113)%256"
                            ),
                        ),
                        (
                            "quotient_chain_product_plus_residue",
                            format!(
                                "(((inputs[0]//{left})%{right})*((inputs[0]//{right})%{quotient})+(inputs[0]%{left})*{coefficient}+113)%256"
                            ),
                        ),
                        (
                            "single_product_residue_quotient",
                            format!(
                                "((inputs[0]%{left})*(inputs[0]//{left})*{coefficient}+(inputs[0]%{left})*3+(inputs[0]//{left})+113)%256"
                            ),
                        ),
                        (
                            "single_product_residue_quotient_varied_linears",
                            format!(
                                "((inputs[0]%{left})*(inputs[0]//{left})*{coefficient}+(inputs[0]%{left})*{right}+(inputs[0]//{left})*{quotient}+113)%256"
                            ),
                        ),
                        (
                            "single_product_residue_quotient_even_product_c2",
                            format!(
                                "((inputs[0]%{left})*(inputs[0]//{left})*2+(inputs[0]%{left})*{right}+(inputs[0]//{left})*{quotient}+113)%256"
                            ),
                        ),
                        (
                            "single_product_residue_square",
                            format!(
                                "((inputs[0]%{left})*(inputs[0]%{left})*{coefficient}+(inputs[0]//{left})*3+(inputs[0]%{left})+113)%256"
                            ),
                        ),
                        (
                            "single_product_quotient_square",
                            format!(
                                "((inputs[0]//{left})*(inputs[0]//{left})*{coefficient}+(inputs[0]%{left})*3+(inputs[0]//{left})+113)%256"
                            ),
                        ),
                        (
                            "single_product_shifted_residue",
                            format!(
                                "(((inputs[0]%{left})+{coefficient})*(inputs[0]//{left})+(inputs[0]%{left})*3+(inputs[0]//{left})*5+113)%256"
                            ),
                        ),
                        (
                            "single_product_shifted_quotient",
                            format!(
                                "((inputs[0]%{left})*((inputs[0]//{left})+{coefficient})+(inputs[0]%{left})*5+(inputs[0]//{left})*3+113)%256"
                            ),
                        ),
                        (
                            "single_product_dual_shift",
                            format!(
                                "(((inputs[0]%{left})+{coefficient})*((inputs[0]//{left})+3)+(inputs[0]%{left})*5+(inputs[0]//{left})+113)%256"
                            ),
                        ),
                        (
                            "single_product_residue_complement",
                            format!(
                                "((inputs[0]%{left})*({left}-1-(inputs[0]%{left}))+(inputs[0]//{left})*{coefficient}+(inputs[0]%{left})*3+113)%256"
                            ),
                        ),
                        (
                            "single_product_quotient_residue_sum",
                            format!(
                                "((inputs[0]//{left})*((inputs[0]//{left})+(inputs[0]%{left}))+(inputs[0]%{left})*{coefficient}+(inputs[0]//{left})*3+113)%256"
                            ),
                        ),
                        (
                            "single_residue_control",
                            format!("((inputs[0]%{left})*{coefficient}+113)%256"),
                        ),
                        (
                            "quotient_affine_control",
                            format!("((inputs[0]//{left})*{coefficient}+113)%256"),
                        ),
                    ];
                    for (template_family, expression) in templates {
                        templates_generated += 1;
                        let values = expression_values(&expression)
                            .expect("constructor search emits parser-valid total expressions");
                        let analytical =
                            analyze_named_trivial_families(&values, threshold_exclusive);
                        if !analytical.named_families_excluded {
                            analytically_rejected += 1;
                            continue;
                        }
                        let folded_expression_grammar_tokens =
                            folded_solution_token_count(&wrapped_solution(&expression))
                                .expect("constructor search emits foldable parser expressions");
                        if folded_expression_grammar_tokens < u64::from(threshold_exclusive) {
                            reference_too_short_rejected += 1;
                            continue;
                        }
                        if shallow_parser_match_through_ast3(&values, threshold_exclusive).is_some()
                        {
                            shallow_exact_rejected += 1;
                            continue;
                        }
                        let digest = crate::lower_hex(&Sha256::digest(
                            values.iter().map(|v| *v as u8).collect::<Vec<_>>(),
                        ));
                        let semantic_cluster = constructor_semantic_profile(&values, &analytical);
                        survivors.push(ConstructorSearchSurvivor {
                            template_family: template_family.into(),
                            expression,
                            folded_expression_grammar_tokens,
                            semantic_digest_hex: digest,
                            semantic_cluster,
                            detected_exact_period: analytical.detected_exact_period,
                            detected_additive_period: analytical.detected_additive_period,
                            minimum_integer_affine_pieces: analytical.minimum_integer_affine_pieces,
                        });
                    }
                }
            }
        }
    }
    survivors.sort_by(|left, right| {
        (&left.semantic_cluster, &left.expression)
            .cmp(&(&right.semantic_cluster, &right.expression))
    });
    let candidate_records_before_global_dedup = survivors.len();
    let records_after_adjacent_only_dedup = {
        let mut previous_digest = None;
        let mut records = 0;
        for survivor in &survivors {
            if previous_digest != Some(survivor.semantic_digest_hex.as_str()) {
                records += 1;
                previous_digest = Some(survivor.semantic_digest_hex.as_str());
            }
        }
        records
    };
    let mut seen_semantic_digests = std::collections::BTreeSet::new();
    survivors.retain(|survivor| seen_semantic_digests.insert(survivor.semantic_digest_hex.clone()));
    let mut profile_members =
        std::collections::BTreeMap::<String, (usize, std::collections::BTreeSet<String>)>::new();
    for survivor in &survivors {
        let entry = profile_members
            .entry(survivor.semantic_cluster.clone())
            .or_default();
        entry.0 += 1;
        entry.1.insert(survivor.template_family.clone());
    }
    let semantic_clusters = profile_members.len();
    let clustering_audit = ConstructorClusteringAudit {
        profile_kind: "coarse_bucket_signature_not_distance_cluster".into(),
        candidate_records_before_global_dedup,
        records_after_adjacent_only_dedup,
        duplicate_semantic_records_removed: candidate_records_before_global_dedup - survivors.len(),
        nonadjacent_duplicates_previously_missed: records_after_adjacent_only_dedup
            - survivors.len(),
        unique_semantic_functions: survivors.len(),
        profile_buckets: semantic_clusters,
        singleton_profile_buckets: profile_members
            .values()
            .filter(|(members, _)| *members == 1)
            .count(),
        largest_profile_bucket: profile_members
            .values()
            .map(|(members, _)| *members)
            .max()
            .unwrap_or(0),
        mixed_template_family_profile_buckets: profile_members
            .values()
            .filter(|(_, families)| families.len() > 1)
            .count(),
    };
    ConstructorSearchReport {
        algorithm:
            "generated_coprime_template_grid_bias_invariant_profile_exact_ast3_global_dedup_v4"
                .into(),
        folded_expression_grammar_token_threshold_exclusive: threshold_exclusive,
        templates_generated,
        analytically_rejected,
        reference_too_short_rejected,
        shallow_exact_rejected,
        requested_exact_ast_depth: 7,
        proven_exhaustive_ast_depth: 3,
        semantic_clusters,
        clustering_audit,
        survivors,
    }
}

fn finalize_hybrid_gate(witness: &mut NontrivialityWitness, minimum_depth: u8) {
    witness.hybrid_gate_passed = witness.proven_exhaustive_ast_depth >= minimum_depth
        && !witness.short_expression_match_found
        && witness.analytical.named_families_excluded;
}

/// Exhaustively enumerates the same AST accepted by `ExprParser`, bottom-up by
/// node count. Deduplication uses exact full-domain i64 value vectors so it is
/// a congruence for every parser operator, including division and remainder.
/// Resource limits are fail-closed and can never produce an exhaustive flag.
#[derive(Clone, Copy, Debug)]
pub struct NontrivialitySearchLimits {
    pub folded_expression_grammar_token_threshold_exclusive: u32,
    pub target_ast_depth: u8,
    pub minimum_proven_ast_depth: u8,
    pub max_semantics: usize,
    pub max_operator_applications: u64,
}

pub fn prove_no_short_parser_expression(
    e0: &str,
    arity: u8,
    target: &crate::oracle::SemanticFingerprint,
    step_cap: u64,
    limits: NontrivialitySearchLimits,
) -> Result<NontrivialityWitness, String> {
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    let NontrivialitySearchLimits {
        folded_expression_grammar_token_threshold_exclusive: threshold_exclusive,
        target_ast_depth,
        minimum_proven_ast_depth,
        max_semantics,
        max_operator_applications,
    } = limits;
    if arity != 1 {
        return Err("hybrid T2 nontriviality certificate currently supports arity 1".into());
    }
    if target_ast_depth == 0 || minimum_proven_ast_depth > target_ast_depth {
        return Err("invalid requested/minimum AST depth configuration".into());
    }
    let bf = BfProgram::parse(e0).map_err(|error| error.to_string())?;
    let mut target_values = Vec::with_capacity(256);
    for input in 0..=255u8 {
        let output = bf
            .execute(&[input], step_cap, false)
            .map_err(|error| error.to_string())?
            .state
            .output;
        if output.len() != 1 {
            return Err("bottom-up T2 proof currently requires output_arity=1".into());
        }
        target_values.push(output[0] as i64);
    }
    let target_values: Arc<[i64]> = target_values.into();
    let analytical = analyze_named_trivial_families(&target_values, threshold_exclusive);
    let mut witness = NontrivialityWitness {
        enumeration_algorithm:
            "parser_ast_bottom_up_observational_equivalence_v3_hybrid_layer".into(),
        acceptance_grammar: "ExprParser-v3: literals 0..256, inputs[N], unary -, + - * // % & | ^, parentheses; constant-only subexpressions accepted and folded for budget accounting".into(),
        folded_expression_grammar_token_threshold_exclusive: threshold_exclusive,
        requested_ast_depth: target_ast_depth,
        proven_exhaustive_ast_depth: 0,
        short_expression_match_found: false,
        matching_expression_ast_depth: None,
        unique_semantics_enumerated: 0,
        operator_applications: 0,
        enumeration_resource_limit_hit: None,
        matching_expression: None,
        analytical,
        hybrid_gate_passed: false,
        reference_search_algorithm: String::new(),
        reference_candidates_enumerated: 0,
        reference_candidates_full_domain_checked: 0,
        domain_size: target.domain_size,
        matched_digest_hex: target.digest_hex.clone(),
    };
    // A single representative per value vector is sufficient for semantic
    // reachability, but not for a lexical-cost proof: precedence changes the
    // parentheses a parent needs. Keep the cheapest representative for each
    // (value vector, precedence) context and count unique semantics separately.
    let mut seen_frontier = HashMap::<Arc<[i64]>, Vec<(u8, u32)>>::new();
    let mut unique_semantics = HashSet::<Arc<[i64]>>::new();
    let mut levels = vec![Vec::<EnumeratedExpr>::new(); target_ast_depth as usize + 1];
    let mut first = HashMap::<Arc<[i64]>, Vec<EnumeratedExpr>>::new();
    for literal in 0..=256i64 {
        let source = literal.to_string();
        let candidate = EnumeratedExpr {
            values: vec![literal; 256].into(),
            folded_expression_tokens: expression_grammar_token_count(&source)
                .expect("enumerator emits grammar tokens"),
            source,
            precedence: 100,
            depends_on_input: false,
        };
        keep_shortest_candidate(&mut first, &seen_frontier, candidate);
    }
    let input_source = "inputs[0]".to_string();
    keep_shortest_candidate(
        &mut first,
        &seen_frontier,
        EnumeratedExpr {
            values: (0..=255i64).collect::<Vec<_>>().into(),
            folded_expression_tokens: expression_grammar_token_count(&input_source)
                .expect("enumerator emits grammar tokens"),
            source: input_source,
            precedence: 100,
            depends_on_input: true,
        },
    );
    levels[1] = first.into_values().flatten().collect();
    levels[1].sort_by(|a, b| a.source.cmp(&b.source));

    for nodes in 1..=target_ast_depth as usize {
        if nodes > 1 {
            let mut level = HashMap::<Arc<[i64]>, Vec<EnumeratedExpr>>::new();
            for child in &levels[nodes - 1] {
                witness.operator_applications += 1;
                if witness.operator_applications > max_operator_applications {
                    witness.enumeration_resource_limit_hit = Some(format!(
                        "operator application ceiling {max_operator_applications} reached"
                    ));
                    finalize_hybrid_gate(&mut witness, minimum_proven_ast_depth);
                    return Ok(witness);
                }
                let Some(values) = child
                    .values
                    .iter()
                    .map(|value| value.checked_neg())
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                let depends_on_input = child.depends_on_input;
                let (source, precedence) = if depends_on_input {
                    let source = parenthesize(&child.source, child.precedence, 90, false);
                    (format!("-{source}"), 90)
                } else {
                    let value = values[0];
                    (value.to_string(), if value < 0 { 90 } else { 100 })
                };
                let candidate = EnumeratedExpr {
                    values: values.into(),
                    folded_expression_tokens: expression_grammar_token_count(&source)
                        .expect("enumerator emits grammar tokens"),
                    source,
                    precedence,
                    depends_on_input,
                };
                if candidate.values == target_values
                    && candidate.folded_expression_tokens < threshold_exclusive
                {
                    witness.short_expression_match_found = true;
                    witness.matching_expression_ast_depth = Some(nodes as u8);
                    witness.matching_expression = Some(wrapped_solution(&candidate.source));
                    witness.unique_semantics_enumerated = unique_semantics.len() + level.len();
                    finalize_hybrid_gate(&mut witness, minimum_proven_ast_depth);
                    return Ok(witness);
                }
                if candidate.folded_expression_tokens < threshold_exclusive {
                    keep_shortest_candidate(&mut level, &seen_frontier, candidate);
                }
            }
            for left_nodes in 1..nodes - 1 {
                let right_nodes = nodes - 1 - left_nodes;
                if right_nodes == 0 {
                    continue;
                }
                for (left_index, left) in levels[left_nodes].iter().enumerate() {
                    for (right_index, right) in levels[right_nodes].iter().enumerate() {
                        for op in EnumeratedOp::ALL {
                            if op.commutative()
                                && (left_nodes > right_nodes
                                    || (left_nodes == right_nodes && left_index > right_index))
                            {
                                continue;
                            }
                            witness.operator_applications += 1;
                            if witness.operator_applications > max_operator_applications {
                                witness.enumeration_resource_limit_hit = Some(format!(
                                    "operator application ceiling {max_operator_applications} reached"
                                ));
                                finalize_hybrid_gate(&mut witness, minimum_proven_ast_depth);
                                return Ok(witness);
                            }
                            let Some(values) = left
                                .values
                                .iter()
                                .zip(right.values.iter())
                                .map(|(a, b)| op.apply(*a, *b))
                                .collect::<Option<Vec<_>>>()
                            else {
                                continue;
                            };
                            let depends_on_input = left.depends_on_input || right.depends_on_input;
                            let (source, precedence) = if depends_on_input {
                                let mut source = enumerated_binary_source(op, left, right);
                                if op.commutative() {
                                    let reversed = enumerated_binary_source(op, right, left);
                                    if (
                                        expression_grammar_token_count(&reversed)
                                            .expect("enumerator emits grammar tokens"),
                                        reversed.len(),
                                        &reversed,
                                    ) < (
                                        expression_grammar_token_count(&source)
                                            .expect("enumerator emits grammar tokens"),
                                        source.len(),
                                        &source,
                                    ) {
                                        source = reversed;
                                    }
                                }
                                (source, op.precedence())
                            } else {
                                let value = values[0];
                                (value.to_string(), if value < 0 { 90 } else { 100 })
                            };
                            let candidate = EnumeratedExpr {
                                values: values.into(),
                                folded_expression_tokens: expression_grammar_token_count(&source)
                                    .expect("enumerator emits grammar tokens"),
                                source,
                                precedence,
                                depends_on_input,
                            };
                            if candidate.values == target_values
                                && candidate.folded_expression_tokens < threshold_exclusive
                            {
                                witness.short_expression_match_found = true;
                                witness.matching_expression_ast_depth = Some(nodes as u8);
                                witness.matching_expression =
                                    Some(wrapped_solution(&candidate.source));
                                witness.unique_semantics_enumerated =
                                    unique_semantics.len() + level.len();
                                finalize_hybrid_gate(&mut witness, minimum_proven_ast_depth);
                                return Ok(witness);
                            }
                            if candidate.folded_expression_tokens < threshold_exclusive {
                                keep_shortest_candidate(&mut level, &seen_frontier, candidate);
                            }
                        }
                    }
                }
            }
            levels[nodes] = level.into_values().flatten().collect();
            levels[nodes].sort_by(|a, b| a.source.cmp(&b.source));
        }

        if let Some(candidate) = levels[nodes]
            .iter()
            .find(|candidate| candidate.values == target_values)
            && candidate.folded_expression_tokens < threshold_exclusive
        {
            witness.short_expression_match_found = true;
            witness.matching_expression_ast_depth = Some(nodes as u8);
            witness.matching_expression = Some(wrapped_solution(&candidate.source));
            witness.unique_semantics_enumerated = unique_semantics.len() + levels[nodes].len();
            finalize_hybrid_gate(&mut witness, minimum_proven_ast_depth);
            return Ok(witness);
        }
        let new_semantics = levels[nodes]
            .iter()
            .map(|candidate| candidate.values.clone())
            .filter(|values| !unique_semantics.contains(values))
            .collect::<HashSet<_>>();
        if unique_semantics.len().saturating_add(new_semantics.len()) > max_semantics {
            witness.enumeration_resource_limit_hit = Some(format!(
                "unique semantic ceiling {max_semantics} reached at AST size {nodes}"
            ));
            witness.unique_semantics_enumerated = unique_semantics.len();
            finalize_hybrid_gate(&mut witness, minimum_proven_ast_depth);
            return Ok(witness);
        }
        unique_semantics.extend(new_semantics);
        for candidate in &levels[nodes] {
            extend_seen_frontier(&mut seen_frontier, candidate);
        }
        witness.proven_exhaustive_ast_depth = nodes as u8;
        witness.unique_semantics_enumerated = unique_semantics.len();
        if nodes == 1 && !witness.analytical.named_families_excluded {
            finalize_hybrid_gate(&mut witness, minimum_proven_ast_depth);
            return Ok(witness);
        }
    }
    finalize_hybrid_gate(&mut witness, minimum_proven_ast_depth);
    Ok(witness)
}

#[derive(Clone, Debug)]
pub struct ReferenceSearchResult {
    pub solution: String,
    pub tokens_upper_bound: u32,
    pub candidates_enumerated: usize,
    pub candidates_full_domain_checked: usize,
    pub matched_digest_hex: String,
}

/// Declared-family G2 search retained only to obtain a private perfect mock
/// answer. It is not an acceptance proof and makes no minimality claim. The
/// hybrid gate never consumes the result of this search.
pub fn search_g2_reference_expression(
    e0: &str,
    arity: u8,
    target: &crate::oracle::SemanticFingerprint,
    step_cap: u64,
) -> Option<ReferenceSearchResult> {
    use std::collections::BTreeSet;

    if !(1..=2).contains(&arity) {
        return None;
    }
    let wrap = |expr: String| format!("def solve(inputs):\n    return [{expr}]");
    let mut unique = BTreeSet::new();
    for constant in 0..=255u16 {
        unique.insert(wrap(constant.to_string()));
    }
    for input_index in 0..arity {
        let x = format!("inputs[{input_index}]");
        unique.insert(wrap(x.clone()));
        for constant in 0..=255u16 {
            for op in ["+", "-", "*", "//", "%", "&", "|", "^"] {
                unique.insert(wrap(format!("({x}{op}{constant})%256")));
                unique.insert(wrap(format!("({constant}{op}{x})%256")));
            }
            for coefficient in 2..=31u16 {
                unique.insert(wrap(format!("({constant}+{x}*{coefficient})%256")));
                unique.insert(wrap(format!("({constant}-{x}*{coefficient})%256")));
            }
        }
    }
    if arity == 2 {
        let (x, y) = ("inputs[0]", "inputs[1]");
        for constant in 0..=255u16 {
            unique.insert(wrap(format!("({constant}+{x}+{y})%256")));
            unique.insert(wrap(format!("({constant}+{x}*{y})%256")));
            unique.insert(wrap(format!("({constant}+({x}%2)+({y}%2))%256")));
        }
        for constant in 0..=31u16 {
            for a in 2..=15u16 {
                unique.insert(wrap(format!("({constant}+{x}*{a}+{y})%256")));
                unique.insert(wrap(format!("({constant}+{x}+{y}*{a})%256")));
                for b in 2..=15u16 {
                    unique.insert(wrap(format!("({constant}+{x}*{a}+{y}*{b})%256")));
                }
            }
        }
    }
    let y_terms: Vec<String> = if arity == 1 {
        vec![String::new()]
    } else {
        vec![
            "+inputs[1]*197".into(),
            "+inputs[1]*203".into(),
            "+inputs[1]*209".into(),
        ]
    };
    for bias in 0..=255u16 {
        for y in &y_terms {
            let x = "inputs[0]";
            for expression in [
                format!("({bias}+(({x}%5)+1)*({x}//5)+({x}%5)*3+({x}//5)*5{y})%256"),
                format!("({bias}+({x}%5)*({x}%5)*11+({x}//5)*3+({x}%5){y})%256"),
                format!("({bias}+({x}%7)*({x}//7)*5+({x}%7)*11+({x}//7)*17{y})%256"),
                format!("({bias}+({x}%5)*(5-1-({x}%5))+({x}//5)*3+({x}%5)*3{y})%256"),
                format!("({bias}+(({x}%5)+5)*({x}//5)+({x}%5)*3+({x}//5)*5{y})%256"),
                format!("({bias}+({x}%7)*({x}//7)*2+({x}%7)*11+({x}//7)*13{y})%256"),
                format!("({bias}+({x}%7)*({x}//7)*2+({x}%7)*11+({x}//7)*17{y})%256"),
                format!("({bias}+({x}%5)*(5-1-({x}%5))+({x}//5)+({x}%5)*3{y})%256"),
                format!("({bias}+({x}%7)*255+({x}//7)*223+{x}*255{y})%256"),
                format!("({bias}+({x}%2)*255+({x}//2)*223+{x}*255{y})%256"),
                format!("({bias}+({x}//128)*255+({x}%128)*223+{x}*255{y})%256"),
                format!("({bias}+({x}&15)*255+({x}//16)*223+{x}*255{y})%256"),
                format!("({bias}+({x}%7)*({x}//7)*255+({x}//7)*223+{x}*255{y})%256"),
                format!("({bias}+({x}%5)*255-({x}//5)*223+{x}*255{y})%256"),
                format!("({bias}+({x}%5)*({x}%5)*255+({x}%5)*223+({x}//5)*191+{x}*255{y})%256"),
                format!("({bias}+({x}%3)*({x}//3)*255+({x}%3)*223-({x}//3)*191+{x}*255{y})%256"),
                format!("({bias}-({x}%7)-({x}//7)*33+{x}*255{y})%256"),
                format!("({bias}-({x}%2)-({x}//2)*33+{x}*255{y})%256"),
                format!("({bias}-({x}//128)-({x}%128)*33+{x}*255{y})%256"),
                format!("({bias}-({x}&15)-({x}//16)*33+{x}*255{y})%256"),
                format!("({bias}-({x}%7)*({x}//7)-({x}//7)*33+{x}*255{y})%256"),
                format!("({bias}-({x}%5)+({x}//5)*33+{x}*255{y})%256"),
                format!("({bias}-({x}%5)*({x}%5)-({x}%5)*33-({x}//5)*65+{x}*255{y})%256"),
                format!("({bias}-({x}%3)*({x}//3)-({x}%3)*33+({x}//3)*65+{x}*255{y})%256"),
            ] {
                unique.insert(wrap(expression.clone()));
                unique.insert(wrap(
                    expression.replace(&format!("+{x}*255"), &format!("-{x}")),
                ));
            }
        }
    }

    let mut candidates = unique.into_iter().collect::<Vec<_>>();
    candidates.sort_by_key(|source| (lexical_token_count(source), source.len(), source.clone()));
    let bf = BfProgram::parse(e0).ok()?;
    let probes: Vec<Vec<u8>> = if arity == 1 {
        [0, 1, 2, 17, 63, 127, 254, 255]
            .into_iter()
            .map(|x| vec![x])
            .collect()
    } else {
        [
            (0, 0),
            (0, 1),
            (1, 0),
            (1, 1),
            (2, 3),
            (17, 63),
            (127, 254),
            (255, 255),
        ]
        .into_iter()
        .map(|(a, b)| vec![a, b])
        .collect()
    };
    let probe_truth = probes
        .iter()
        .map(|input| {
            bf.execute(input, step_cap, false)
                .ok()
                .map(|run| run.state.output)
        })
        .collect::<Option<Vec<_>>>()?;
    let mut full_checks = 0usize;
    for source in &candidates {
        let expressions = parse_solution(source).ok()?;
        let normalized_source = render_solution(&expressions);
        let source_tokens = lexical_token_count(&normalized_source) as u32;
        let probe_outputs = probes
            .iter()
            .map(|input| evaluate_expressions(&expressions, input))
            .collect::<Option<Vec<_>>>();
        if probe_outputs.as_ref() != Some(&probe_truth) {
            continue;
        }
        full_checks += 1;
        let digest = expression_digest(&expressions, arity)?;
        if digest == target.digest_hex {
            return Some(ReferenceSearchResult {
                solution: normalized_source,
                tokens_upper_bound: source_tokens,
                candidates_enumerated: candidates.len(),
                candidates_full_domain_checked: full_checks,
                matched_digest_hex: digest,
            });
        }
    }
    None
}

fn evaluate_expressions(expressions: &[Expr], input: &[u8]) -> Option<Vec<u8>> {
    expressions
        .iter()
        .map(|expr| expr.eval(input).and_then(|value| u8::try_from(value).ok()))
        .collect()
}

fn expression_digest(expressions: &[Expr], arity: u8) -> Option<String> {
    let required = 256usize.pow(arity as u32);
    let mut hasher = Sha256::new();
    for n in 0..required {
        let input = if arity == 1 {
            vec![n as u8]
        } else {
            vec![(n >> 8) as u8, n as u8]
        };
        let output = evaluate_expressions(expressions, &input)?;
        hasher.update((input.len() as u32).to_le_bytes());
        hasher.update(&input);
        hasher.update((output.len() as u32).to_le_bytes());
        hasher.update(&output);
    }
    Some(crate::lower_hex(&hasher.finalize()))
}

/// Exact, sandbox-free verifier for compact symbolic programs. Validation
/// evaluates a local AST and never executes model-provided Python.
pub fn verify_t2_expression(task: &TaskRecord, item: &BaseItem, response: &str) -> Verification {
    let expressions = match parse_solution(response) {
        Ok(x) => x,
        Err(detail) => {
            return Verification {
                correct: false,
                parse_failure: true,
                detail,
            };
        }
    };
    let folded_source = render_solution(&expressions);
    if task
        .hard_token_cap
        .is_some_and(|cap| lexical_token_count(&folded_source) > cap as u64)
    {
        return Verification {
            correct: false,
            parse_failure: false,
            detail: "hard folded token cap exceeded".into(),
        };
    }
    if expressions.len() != item.ir.output_arity as usize {
        return Verification {
            correct: false,
            parse_failure: false,
            detail: "wrong output arity".into(),
        };
    }
    let bf = BfProgram::parse(&item.encodings.e0).expect("accepted E0");
    let required = 256usize.pow(item.ir.arity as u32);
    for n in 0..required {
        let input = if item.ir.arity == 1 {
            vec![n as u8]
        } else {
            vec![(n >> 8) as u8, n as u8]
        };
        let candidate: Option<Vec<u8>> = expressions
            .iter()
            .map(|expr| expr.eval(&input).and_then(|x| u8::try_from(x).ok()))
            .collect();
        let Some(candidate) = candidate else {
            return Verification {
                correct: false,
                parse_failure: false,
                detail: format!("expression left byte range at input {input:?}"),
            };
        };
        let truth = bf
            .execute(&input, PINNED_STEP_CAP, false)
            .expect("accepted domain")
            .state
            .output;
        if candidate != truth {
            return Verification {
                correct: false,
                parse_failure: false,
                detail: format!("first mismatch at input {input:?}"),
            };
        }
    }
    Verification {
        correct: true,
        parse_failure: false,
        detail: "exhaustive symbolic match".into(),
    }
}

pub trait ModelAdapter {
    fn answer(&self, task: &TaskRecord, item: &BaseItem) -> String;
}
pub struct PerfectMock;
impl ModelAdapter for PerfectMock {
    fn answer(&self, task: &TaskRecord, item: &BaseItem) -> String {
        match task.family {
            Family::T1 | Family::T3 => {
                serde_json::to_string(&task.payload["expected_answer"]).unwrap()
            }
            Family::T2 => item.oracles.t2_reference_solution.clone(),
            _ => String::new(),
        }
    }
}

pub struct OffByOnePointerMock;
impl ModelAdapter for OffByOnePointerMock {
    fn answer(&self, task: &TaskRecord, item: &BaseItem) -> String {
        let exact = PerfectMock.answer(task, item);
        match task.family {
            Family::T1 => {
                let mut a: T1Answer = serde_json::from_str(&exact).unwrap();
                if let Some(p) = a.pointer.as_mut() {
                    *p += 1
                } else {
                    a.cell += 1
                }
                serde_json::to_string(&a).unwrap()
            }
            Family::T2 => "def solve(inputs):\n    return [0]".into(),
            Family::T3 => wrong_counterfactual(&exact),
            _ => exact,
        }
    }
}

pub struct DriftAfterKMock {
    pub k: u64,
}
impl ModelAdapter for DriftAfterKMock {
    fn answer(&self, task: &TaskRecord, item: &BaseItem) -> String {
        let exact = PerfectMock.answer(task, item);
        match task.family {
            Family::T1 => {
                let mut a: T1Answer = serde_json::from_str(&exact).unwrap();
                if a.step >= self.k {
                    a.value = a.value.wrapping_add(1)
                }
                serde_json::to_string(&a).unwrap()
            }
            Family::T2 => "def solve(inputs):\n    return [1]".into(),
            Family::T3 => wrong_counterfactual(&exact),
            _ => exact,
        }
    }
}

pub struct IgnoreWrapMock;
impl ModelAdapter for IgnoreWrapMock {
    fn answer(&self, task: &TaskRecord, item: &BaseItem) -> String {
        let exact = PerfectMock.answer(task, item);
        match task.family {
            Family::T1 => {
                let mut a: T1Answer = serde_json::from_str(&exact).unwrap();
                a.value = if a.value == 0 {
                    255
                } else {
                    a.value.wrapping_add(1)
                };
                serde_json::to_string(&a).unwrap()
            }
            Family::T2 => "def solve(inputs):\n    return [256]".into(),
            Family::T3 => wrong_counterfactual(&exact),
            _ => exact,
        }
    }
}

pub fn verify(task: &TaskRecord, item: &BaseItem, response: &str) -> Verification {
    match task.family {
        Family::T1 => verify_t1(task, response),
        Family::T2 => verify_t2_expression(task, item, response),
        Family::T3 => verify_t3(task, response),
        _ => Verification {
            correct: false,
            parse_failure: true,
            detail: "family reserved in MVP".into(),
        },
    }
}

fn wrong_counterfactual(exact: &str) -> String {
    let mut observable: Observable = serde_json::from_str(exact).unwrap();
    observable = match observable {
        Observable::Output(mut value) => {
            if value.is_empty() {
                value.push(1);
            } else {
                value[0] = value[0].wrapping_add(1);
            }
            Observable::Output(value)
        }
        _ => Observable::Output(vec![0]),
    };
    serde_json::to_string(&observable).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_symbolic_solution_is_parsed_without_python_execution() {
        let expressions =
            parse_solution("def solve(inputs):\n    return [(29 + inputs[0]) % 256]").unwrap();
        assert_eq!(expressions[0].eval(&[227]), Some(0));
    }

    #[test]
    fn constant_binary_subtrees_are_folded_without_narrowing_acceptance() {
        let expanded = "def solve(inputs):\n    return [(3*64+inputs[0])%256]";
        let canonical = "def solve(inputs):\n    return [(192+inputs[0])%256]";
        let parsed = parse_solution(expanded).unwrap();
        assert_eq!(render_solution(&parsed), canonical);
        assert_eq!(
            folded_solution_token_count(expanded).unwrap(),
            lexical_token_count(canonical)
        );
        assert!(lexical_token_count(expanded) > folded_solution_token_count(expanded).unwrap());
        assert_eq!(
            expression_digest(&parsed, 1),
            expression_digest(&parse_solution(canonical).unwrap(), 1)
        );
    }

    #[test]
    fn g2_external_short_expression_is_rejected_by_hybrid_layer() {
        let known = parse_solution("def solve(inputs):\n    return [1//(inputs[0]+1)]").unwrap();
        let target = crate::oracle::SemanticFingerprint {
            algorithm: "sha256_length_delimited_domain_table".into(),
            domain_size: 256,
            digest_hex: expression_digest(&known, 1).unwrap(),
        };
        let witness = prove_no_short_parser_expression(
            ",>+<[[-]>[-]<]>.",
            1,
            &target,
            1_000_000,
            NontrivialitySearchLimits {
                folded_expression_grammar_token_threshold_exclusive: 25,
                target_ast_depth: 7,
                minimum_proven_ast_depth: 3,
                max_semantics: 1_000_000,
                max_operator_applications: 1_000_000,
            },
        )
        .unwrap();
        assert_eq!(witness.domain_size, 256);
        assert!(!witness.hybrid_gate_passed);
        assert!(
            witness
                .analytical
                .matched_trivial_families
                .contains(&"compact_single_boundary_floor".to_string())
        );
    }

    #[test]
    fn exact_layer_detects_a_shallow_matching_expression() {
        let known = parse_solution("def solve(inputs):\n    return [inputs[0]]").unwrap();
        let target = crate::oracle::SemanticFingerprint {
            algorithm: "sha256-domain-v1".into(),
            digest_hex: expression_digest(&known, 1).unwrap(),
            domain_size: 256,
        };
        let witness = prove_no_short_parser_expression(
            ",.",
            1,
            &target,
            1_000_000,
            NontrivialitySearchLimits {
                folded_expression_grammar_token_threshold_exclusive: 25,
                target_ast_depth: 3,
                minimum_proven_ast_depth: 0,
                max_semantics: 1_000_000,
                max_operator_applications: 0,
            },
        )
        .unwrap();
        assert!(witness.short_expression_match_found);
        assert_eq!(witness.matching_expression_ast_depth, Some(1));
    }

    #[test]
    fn analytical_shortcut_reports_the_exact_depth_completed_first() {
        let known = parse_solution("def solve(inputs):\n    return [1//(inputs[0]+1)]").unwrap();
        let target = crate::oracle::SemanticFingerprint {
            algorithm: "sha256-domain-v1".into(),
            digest_hex: expression_digest(&known, 1).unwrap(),
            domain_size: 256,
        };
        let witness = prove_no_short_parser_expression(
            ",>+<[[ -]>[-]<]>.".replace(' ', "").as_str(),
            1,
            &target,
            1_000_000,
            NontrivialitySearchLimits {
                folded_expression_grammar_token_threshold_exclusive: 25,
                target_ast_depth: 7,
                minimum_proven_ast_depth: 3,
                max_semantics: 1_000_000,
                max_operator_applications: 0,
            },
        )
        .unwrap();
        assert_eq!(witness.proven_exhaustive_ast_depth, 1);
        assert!(!witness.hybrid_gate_passed);
        assert!(witness.enumeration_resource_limit_hit.is_none());
        assert!(!witness.analytical.named_families_excluded);
    }

    #[test]
    fn analytical_layer_names_each_preregistered_trivial_family() {
        let polynomial = (0..256).map(|x| (x * x) as i64).collect::<Vec<_>>();
        let periodic = (0..256).map(|x| (x % 7) as i64).collect::<Vec<_>>();
        let composition = (0..256)
            .map(|x| (((x / 7) * 5 + x % 7) % 256) as i64)
            .collect::<Vec<_>>();
        let piecewise = (0..256).map(|x| (x / 32) as i64).collect::<Vec<_>>();
        let bitwise = (0..256).map(|x| (x ^ 0xaa) as i64).collect::<Vec<_>>();

        let analyze = |values: &[i64]| analyze_named_trivial_families(values, 25);
        assert_eq!(
            analyze(&polynomial).detected_integer_polynomial_degree,
            Some(2)
        );
        assert_eq!(analyze(&periodic).detected_exact_period, Some(7));
        assert_eq!(
            analyze(&composition)
                .detected_additive_period
                .as_ref()
                .map(|witness| witness.period),
            Some(7)
        );
        assert!(analyze(&piecewise).minimum_integer_affine_pieces <= 8);
        assert!(analyze(&bitwise).bitwise_pointwise);
    }

    #[test]
    fn analytical_layer_can_exclude_all_named_families() {
        use rand::{RngExt, SeedableRng};
        use rand_chacha::ChaCha20Rng;

        let mut rng = ChaCha20Rng::seed_from_u64(0xB3EC_FCC0);
        let values = (0..256)
            .map(|_| rng.random_range(0..=255) as i64)
            .collect::<Vec<_>>();
        let witness = analyze_named_trivial_families(&values, 25);
        assert!(witness.named_families_excluded, "{witness:?}");
        assert!(witness.matched_trivial_families.is_empty());
    }

    #[test]
    fn exact_period_is_diagnostic_without_a_compact_expression_witness() {
        let residues = (0..100)
            .map(|index| ((index * 73 + index * index * 19 + 41) % 251) as i64)
            .collect::<Vec<_>>();
        let values = (0..256)
            .map(|index| residues[index % residues.len()])
            .collect::<Vec<_>>();
        let witness = analyze_named_trivial_families(&values, 25);
        assert_eq!(witness.detected_exact_period, Some(100));
        assert!(witness.named_families_excluded, "{witness:?}");
        assert!(witness.compact_expression_witnesses.is_empty());
    }

    #[test]
    fn every_analytical_rejection_has_a_parser_valid_under_threshold_witness() {
        let values = (0..256).map(|x| (x % 7) as i64).collect::<Vec<_>>();
        let witness = analyze_named_trivial_families(&values, 25);
        assert!(!witness.compact_expression_witnesses.is_empty());
        for compact in &witness.compact_expression_witnesses {
            assert!(compact.folded_expression_grammar_tokens < 25);
            let parsed = parse_solution(&compact.expression).unwrap();
            assert!((0..=255u8).all(|input| {
                evaluate_expressions(&parsed, &[input]) == Some(vec![values[input as usize] as u8])
            }));
        }
    }

    #[test]
    fn proposed_coprime_coupling_survives_the_calibrated_analytic_oracle() {
        let expression = "((inputs[0]%7)*(inputs[0]%11)+(inputs[0]//13)*5+113)%256";
        let values = expression_values(expression).unwrap();
        let witness = analyze_named_trivial_families(&values, 25);
        assert!(witness.named_families_excluded, "{witness:?}");
        assert!(shallow_parser_match_through_ast3(&values, 25).is_none());
    }

    #[test]
    fn constructor_search_discovers_eight_name_independent_semantic_profiles() {
        let report = search_constructor_templates(25);
        assert!(report.templates_generated >= 3_300, "{report:?}");
        assert!(report.analytically_rejected >= 300, "{report:?}");
        assert!(report.semantic_clusters >= 8, "{report:?}");
        assert_eq!(report.survivors.len(), 1_730, "{report:?}");
        assert_eq!(
            report
                .survivors
                .iter()
                .map(|survivor| &survivor.semantic_digest_hex)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            report.survivors.len(),
            "{report:?}"
        );
        assert_eq!(report.clustering_audit.unique_semantic_functions, 1_730);
        assert_eq!(
            report
                .clustering_audit
                .candidate_records_before_global_dedup,
            3_000
        );
        assert_eq!(
            report.clustering_audit.records_after_adjacent_only_dedup,
            1_838
        );
        assert_eq!(
            report.clustering_audit.duplicate_semantic_records_removed,
            1_270
        );
        assert_eq!(
            report
                .clustering_audit
                .nonadjacent_duplicates_previously_missed,
            108
        );
        assert_eq!(report.clustering_audit.profile_buckets, 51);
        assert_eq!(report.clustering_audit.singleton_profile_buckets, 7);
        assert_eq!(report.clustering_audit.largest_profile_bucket, 250);
        assert_eq!(
            report
                .clustering_audit
                .mixed_template_family_profile_buckets,
            30
        );
        assert!(
            report
                .survivors
                .iter()
                .all(|survivor| survivor.folded_expression_grammar_tokens >= 25),
            "{report:?}"
        );
        assert!(
            report
                .survivors
                .iter()
                .map(|survivor| &survivor.template_family)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                >= 8,
            "{report:?}"
        );
    }

    #[test]
    fn constructor_semantic_profiles_ignore_modular_output_bias() {
        let values = (0..256)
            .map(|x| ((x % 5) * (x / 5) * 2 + (x % 5) * 17 + (x / 5) * 7) as i64 % 256)
            .collect::<Vec<_>>();
        let shifted = values
            .iter()
            .map(|value| (value + 197).rem_euclid(256))
            .collect::<Vec<_>>();
        assert_eq!(
            constructor_semantic_profile(&values, &analyze_named_trivial_families(&values, 25)),
            constructor_semantic_profile(&shifted, &analyze_named_trivial_families(&shifted, 25))
        );
    }

    #[test]
    fn g2_search_is_reference_only_and_reports_an_upper_bound() {
        let known = parse_solution("def solve(inputs):\n    return [(inputs[0]+5)%256]").unwrap();
        let target = crate::oracle::SemanticFingerprint {
            algorithm: "sha256_length_delimited_domain_table".into(),
            domain_size: 256,
            digest_hex: expression_digest(&known, 1).unwrap(),
        };
        let reference = search_g2_reference_expression(",+++++.", 1, &target, 1_000_000).unwrap();
        assert_eq!(
            expression_digest(&parse_solution(&reference.solution).unwrap(), 1).unwrap(),
            target.digest_hex
        );
        assert_eq!(
            reference.tokens_upper_bound as u64,
            lexical_token_count(&reference.solution)
        );
    }

    #[test]
    fn bpe_counter_does_not_treat_each_repeated_brainfuck_symbol_as_a_token() {
        let source = ">>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>";
        assert!(bpe_token_count(source, "cl100k_base").unwrap() < lexical_token_count(source));
    }

    #[test]
    fn truth_table_cannot_hide_as_one_whitespace_token() {
        let table = format!("TABLE=[{}]", "[0],".repeat(256));
        assert!(lexical_token_count(&table) > 192);
        assert!(parse_solution(&table).is_err());
    }

    #[test]
    fn tier_without_e3_checks_only_rendered_budgets_and_still_rejects_a_difference() {
        let tier = crate::generator::HIGHEST_TIER_RENDERED_AS_E3 + 1;
        assert!(crate::generator::tier_renders_encoding(
            tier,
            EncodingId::E2
        ));
        assert!(!crate::generator::tier_renders_encoding(
            tier,
            EncodingId::E3
        ));

        let rendered = [
            EncodingId::E0,
            EncodingId::E1,
            EncodingId::E2,
            EncodingId::E3,
        ]
        .into_iter()
        .filter(|encoding| crate::generator::tier_renders_encoding(tier, *encoding));
        let mut records = Vec::new();
        for encoding in rendered {
            for (family, cap) in [(Family::T2, 40), (Family::T3, 50)] {
                records.push(TaskRecord {
                    schema_version: "test".into(),
                    task_id: format!("{family:?}-{encoding:?}"),
                    program_id: "program".into(),
                    item_id: "tier-4-item".into(),
                    family,
                    encoding,
                    prompt: String::new(),
                    hard_token_cap: Some(cap),
                    payload: serde_json::Value::Null,
                });
            }
        }

        assert!(task_records_have_encoding_invariant_budgets(
            tier, 30, &records, 384, 96
        ));

        records
            .iter_mut()
            .find(|task| task.family == Family::T2 && task.encoding == EncodingId::E2)
            .unwrap()
            .hard_token_cap = Some(41);
        assert!(!task_records_have_encoding_invariant_budgets(
            tier, 30, &records, 384, 96
        ));
    }

    #[test]
    fn public_task_strips_direct_and_nested_answer_material() {
        let task = TaskRecord {
            schema_version: "benchfck.task.v3".into(),
            task_id: "t".into(),
            program_id: "p".into(),
            item_id: "i".into(),
            family: Family::T3,
            encoding: EncodingId::E0,
            prompt: "prompt".into(),
            hard_token_cap: Some(32),
            payload: json!({
                "expected_answer": {"status":"IDENTICAL"},
                "reference_solution": "secret",
                "oracle_fingerprint": "secret",
                "mutation": {"position": 1, "from": "+", "to": "-", "changed": true, "outcome": {"status":"IDENTICAL"}}
            }),
        };
        let public = serde_json::to_string(&without_answers(task)).unwrap();
        for forbidden in [
            "expected_answer",
            "reference_solution",
            "oracle_fingerprint",
            "changed",
            "outcome",
        ] {
            assert!(!public.contains(forbidden), "leaked {forbidden}");
        }
    }
}
