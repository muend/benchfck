use crate::{
    leak_scan::FORBIDDEN_PUBLIC_KEYS,
    lower_hex,
    schema::{EncodingId, Family, JsonlRecord, PublicItemMetadata, TaskRecord},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const REQUEST_SCHEMA_VERSION: &str = "benchfck.model-request.v1";
pub const SYSTEM_MANIFEST_SCHEMA_VERSION: &str = "benchfck.model-systems.v1";
pub const FULL_REPEATS: u8 = 5;
const PILOT_STAGE_A_ITEMS: usize = 20;
const PILOT_STAGE_B_TIERS: std::ops::RangeInclusive<u8> = 0..=3;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Openai,
    Anthropic,
    Google,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SystemSpec {
    pub system_id: String,
    pub provider: Provider,
    pub model_id: String,
    pub reasoning_setting: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SystemManifest {
    pub schema_version: String,
    pub systems: Vec<SystemSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanScope {
    Pilot,
    MatrixOnce,
    Full,
}

#[derive(Clone, Debug)]
pub struct AnswerStrippedPacket {
    pub metadata: Vec<PublicItemMetadata>,
    pub tasks: Vec<TaskRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UserMessage {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RequestPayload {
    pub model: String,
    pub messages: Vec<UserMessage>,
    pub transport_output_token_cap: u32,
    pub reasoning_setting: String,
    pub sampling_parameters: BTreeMap<String, Value>,
    pub tools: Vec<Value>,
    pub cache_policy: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelRequestRecord {
    pub schema_version: String,
    pub run_id: String,
    pub stage: String,
    pub system_manifest_sha256: String,
    pub system_id: String,
    pub provider: Provider,
    pub model_id: String,
    pub repeat: u8,
    pub task_id: String,
    pub item_id: String,
    pub family: Family,
    pub encoding: EncodingId,
    pub prompt_sha256: String,
    pub request_sha256: String,
    pub visible_answer_token_cap: u32,
    pub request: RequestPayload,
}

pub const ATTEMPT_SCHEMA_VERSION: &str = "benchfck.model-attempt.v1";
pub const MAX_TRANSPORT_RETRIES: u8 = 3;
pub const MAX_ATTEMPTS: u8 = 1 + MAX_TRANSPORT_RETRIES;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttemptOutcome {
    Delivered,
    TransportError,
    RateLimited,
    ProviderError,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ProviderUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AttemptRecord {
    pub schema_version: String,
    pub run_id: String,
    pub attempt_index: u8,
    pub request_sha256: String,
    pub outcome: AttemptOutcome,
    pub started_at: String,
    pub finished_at: String,
    pub latency_ms: u64,
    pub provider_request_id: Option<String>,
    pub model_snapshot: Option<String>,
    pub finish_reason: Option<String>,
    pub response: Option<String>,
    pub response_sha256: Option<String>,
    pub usage: ProviderUsage,
    pub cost_usd: Option<f64>,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScheduledAttempt {
    pub attempt_index: u8,
    pub run: ModelRequestRecord,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResumeDecision {
    pub pending: Vec<ScheduledAttempt>,
    pub completed_runs: usize,
    pub exhausted_runs: usize,
}

fn collect_forbidden_keys(value: &Value, path: &str, hits: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                let nested_path = format!("{path}/{key}");
                if FORBIDDEN_PUBLIC_KEYS.contains(&key.as_str()) {
                    hits.push(nested_path.clone());
                }
                collect_forbidden_keys(nested, &nested_path, hits);
            }
        }
        Value::Array(values) => {
            for (index, nested) in values.iter().enumerate() {
                collect_forbidden_keys(nested, &format!("{path}/{index}"), hits);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

pub fn load_answer_stripped_packet(bytes: &[u8]) -> Result<AnswerStrippedPacket, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| format!("answer-stripped packet is not UTF-8: {error}"))?;
    let mut metadata = Vec::new();
    let mut tasks = Vec::new();
    let mut metadata_ids = HashSet::new();
    let mut task_ids = HashSet::new();

    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            return Err(format!(
                "answer-stripped packet line {} is blank",
                index + 1
            ));
        }
        let value: Value = serde_json::from_str(line)
            .map_err(|error| format!("answer-stripped packet line {}: {error}", index + 1))?;
        let mut hits = Vec::new();
        collect_forbidden_keys(&value, "", &mut hits);
        if !hits.is_empty() {
            return Err(format!(
                "answer-stripped packet line {} contains forbidden key paths: {}",
                index + 1,
                hits.join(", ")
            ));
        }
        match serde_json::from_value::<JsonlRecord>(value)
            .map_err(|error| format!("answer-stripped packet line {}: {error}", index + 1))?
        {
            JsonlRecord::Item(_) => {
                return Err(format!(
                    "answer-stripped packet line {} contains a private item record",
                    index + 1
                ));
            }
            JsonlRecord::PublicItemMetadata(item) => {
                if !metadata_ids.insert(item.item_id.clone()) {
                    return Err(format!("duplicate metadata item_id: {}", item.item_id));
                }
                metadata.push(*item);
            }
            JsonlRecord::Task(task) => {
                if !task_ids.insert(task.task_id.clone()) {
                    return Err(format!("duplicate task_id: {}", task.task_id));
                }
                tasks.push(*task);
            }
        }
    }

    if metadata.len() < PILOT_STAGE_A_ITEMS {
        return Err(format!(
            "answer-stripped packet requires at least {PILOT_STAGE_A_ITEMS} metadata records, found {}",
            metadata.len()
        ));
    }
    if tasks.is_empty() {
        return Err("answer-stripped packet contains no tasks".into());
    }
    for task in &tasks {
        if !metadata_ids.contains(&task.item_id) {
            return Err(format!(
                "task {} references missing metadata item {}",
                task.task_id, task.item_id
            ));
        }
    }
    for item in &metadata {
        if !tasks.iter().any(|task| task.item_id == item.item_id) {
            return Err(format!("metadata item {} has no tasks", item.item_id));
        }
    }

    Ok(AnswerStrippedPacket { metadata, tasks })
}

pub fn validate_frozen_systems(manifest: &SystemManifest) -> Result<(), String> {
    if manifest.schema_version != SYSTEM_MANIFEST_SCHEMA_VERSION {
        return Err(format!(
            "unsupported system manifest schema: {}",
            manifest.schema_version
        ));
    }
    let expected = [
        ("M1", Provider::Openai, "gpt-5.6-terra", "none"),
        ("M2", Provider::Anthropic, "claude-sonnet-5", "disabled"),
        ("M3", Provider::Google, "gemini-3.5-flash", "minimal"),
    ];
    if manifest.systems.len() != expected.len() {
        return Err(format!(
            "frozen preregistration requires exactly {} systems, found {}",
            expected.len(),
            manifest.systems.len()
        ));
    }
    for (observed, (system_id, provider, model_id, reasoning)) in
        manifest.systems.iter().zip(expected)
    {
        if observed.system_id != system_id
            || observed.provider != provider
            || observed.model_id != model_id
            || observed.reasoning_setting != reasoning
        {
            return Err(format!(
                "system {} does not match the frozen preregistration",
                observed.system_id
            ));
        }
    }
    Ok(())
}

fn transport_output_token_cap(family: Family) -> Result<u32, String> {
    match family {
        Family::T1 | Family::T3 => Ok(512),
        Family::T2 => Ok(1_024),
        Family::T4 | Family::T5 | Family::T6 => Err(format!(
            "reserved task family {family:?} is not runnable in v0.4"
        )),
    }
}

fn visible_answer_token_cap(task: &TaskRecord) -> Result<u32, String> {
    match task.family {
        Family::T1 => {
            if task.hard_token_cap.is_some() {
                return Err(format!(
                    "T1 task {} unexpectedly has a hard cap",
                    task.task_id
                ));
            }
            Ok(128)
        }
        Family::T2 | Family::T3 => task
            .hard_token_cap
            .ok_or_else(|| format!("task {} is missing its hard token cap", task.task_id)),
        Family::T4 | Family::T5 | Family::T6 => Err(format!(
            "reserved task family {:?} is not runnable",
            task.family
        )),
    }
}

fn request_record(
    task: &TaskRecord,
    system: &SystemSpec,
    repeat: u8,
    stage: &'static str,
    system_manifest_sha256: &str,
) -> Result<ModelRequestRecord, String> {
    let visible_answer_token_cap = visible_answer_token_cap(task)?;
    let request = RequestPayload {
        model: system.model_id.clone(),
        messages: vec![UserMessage {
            role: "user".into(),
            content: task.prompt.clone(),
        }],
        transport_output_token_cap: transport_output_token_cap(task.family)?,
        reasoning_setting: system.reasoning_setting.clone(),
        sampling_parameters: BTreeMap::new(),
        tools: Vec::new(),
        cache_policy: "disabled_when_configurable".into(),
    };
    let request_bytes = serde_json::to_vec(&request)
        .map_err(|error| format!("failed to serialize request {}: {error}", task.task_id))?;
    let request_sha256 = lower_hex(&Sha256::digest(&request_bytes));
    let prompt_sha256 = lower_hex(&Sha256::digest(task.prompt.as_bytes()));
    let run_id = lower_hex(&Sha256::digest(
        format!(
            "benchfck.v0.4.model-run.v1\0{}\0{}\0{}",
            system.system_id, repeat, task.task_id
        )
        .as_bytes(),
    ));
    Ok(ModelRequestRecord {
        schema_version: REQUEST_SCHEMA_VERSION.into(),
        run_id,
        stage: stage.into(),
        system_manifest_sha256: system_manifest_sha256.to_string(),
        system_id: system.system_id.clone(),
        provider: system.provider,
        model_id: system.model_id.clone(),
        repeat,
        task_id: task.task_id.clone(),
        item_id: task.item_id.clone(),
        family: task.family,
        encoding: task.encoding,
        prompt_sha256,
        request_sha256,
        visible_answer_token_cap,
        request,
    })
}

pub fn build_plan(
    packet: &AnswerStrippedPacket,
    manifest: &SystemManifest,
    system_manifest_sha256: &str,
    scope: PlanScope,
) -> Result<Vec<ModelRequestRecord>, String> {
    validate_frozen_systems(manifest)?;
    if system_manifest_sha256.len() != 64
        || !system_manifest_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("system manifest SHA-256 must be 64 lowercase hex characters".into());
    }

    let tasks_by_item = packet.tasks.iter().fold(
        HashMap::<&str, Vec<&TaskRecord>>::new(),
        |mut grouped, task| {
            grouped.entry(&task.item_id).or_default().push(task);
            grouped
        },
    );
    let mut records = Vec::new();
    let mut run_ids = HashSet::new();
    let mut append = |task: &TaskRecord, repeat: u8, stage: &'static str| -> Result<(), String> {
        for system in &manifest.systems {
            let record = request_record(task, system, repeat, stage, system_manifest_sha256)?;
            if !run_ids.insert(record.run_id.clone()) {
                return Err(format!("duplicate run_id: {}", record.run_id));
            }
            records.push(record);
        }
        Ok(())
    };

    match scope {
        PlanScope::Pilot => {
            for item in packet.metadata.iter().take(PILOT_STAGE_A_ITEMS) {
                for task in tasks_by_item
                    .get(item.item_id.as_str())
                    .ok_or_else(|| format!("metadata item {} has no tasks", item.item_id))?
                {
                    append(task, 1, "pilot_stage_a")?;
                }
            }
            for tier in PILOT_STAGE_B_TIERS {
                let item = packet
                    .metadata
                    .iter()
                    .find(|item| item.program_size_tier == tier)
                    .ok_or_else(|| format!("pilot stage B requires size tier {tier}"))?;
                for task in tasks_by_item
                    .get(item.item_id.as_str())
                    .ok_or_else(|| format!("metadata item {} has no tasks", item.item_id))?
                {
                    for repeat in 2..=FULL_REPEATS {
                        append(task, repeat, "pilot_stage_b")?;
                    }
                }
            }
        }
        PlanScope::MatrixOnce => {
            for task in &packet.tasks {
                append(task, 1, "matrix_once")?;
            }
        }
        PlanScope::Full => {
            for task in &packet.tasks {
                for repeat in 1..=FULL_REPEATS {
                    append(task, repeat, "full")?;
                }
            }
        }
    }

    Ok(records)
}

pub fn load_plan(bytes: &[u8]) -> Result<Vec<ModelRequestRecord>, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|error| format!("model plan is not UTF-8: {error}"))?;
    let mut records = Vec::new();
    let mut run_ids = HashSet::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            return Err(format!("model plan line {} is blank", index + 1));
        }
        let record: ModelRequestRecord = serde_json::from_str(line)
            .map_err(|error| format!("model plan line {}: {error}", index + 1))?;
        if record.schema_version != REQUEST_SCHEMA_VERSION {
            return Err(format!(
                "model plan line {} has unsupported schema {}",
                index + 1,
                record.schema_version
            ));
        }
        if !run_ids.insert(record.run_id.clone()) {
            return Err(format!("duplicate model-plan run_id: {}", record.run_id));
        }
        let (provider, model_id, reasoning_setting) = match record.system_id.as_str() {
            "M1" => (Provider::Openai, "gpt-5.6-terra", "none"),
            "M2" => (Provider::Anthropic, "claude-sonnet-5", "disabled"),
            "M3" => (Provider::Google, "gemini-3.5-flash", "minimal"),
            _ => return Err(format!("unknown frozen system_id: {}", record.system_id)),
        };
        if record.provider != provider
            || record.model_id != model_id
            || record.request.model != model_id
            || record.request.reasoning_setting != reasoning_setting
        {
            return Err(format!(
                "run {} does not match its frozen system identity",
                record.run_id
            ));
        }
        let repeat_matches_stage = match record.stage.as_str() {
            "pilot_stage_a" => record.repeat == 1,
            "pilot_stage_b" => (2..=FULL_REPEATS).contains(&record.repeat),
            "matrix_once" => record.repeat == 1,
            "full" => (1..=FULL_REPEATS).contains(&record.repeat),
            _ => false,
        };
        if !repeat_matches_stage {
            return Err(format!(
                "run {} has invalid stage/repeat combination",
                record.run_id
            ));
        }
        if record.request.transport_output_token_cap != transport_output_token_cap(record.family)?
            || !record.request.tools.is_empty()
            || !record.request.sampling_parameters.is_empty()
            || record.request.cache_policy != "disabled_when_configurable"
        {
            return Err(format!(
                "run {} violates the frozen request protocol",
                record.run_id
            ));
        }
        let visible_cap_valid = match record.family {
            Family::T1 => record.visible_answer_token_cap == 128,
            Family::T2 => (1..=384).contains(&record.visible_answer_token_cap),
            Family::T3 => (1..=96).contains(&record.visible_answer_token_cap),
            Family::T4 | Family::T5 | Family::T6 => false,
        };
        if !visible_cap_valid {
            return Err(format!(
                "run {} has an invalid visible answer cap",
                record.run_id
            ));
        }
        if record.system_manifest_sha256.len() != 64
            || !record
                .system_manifest_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(format!(
                "run {} has an invalid system manifest SHA",
                record.run_id
            ));
        }
        let request_sha256 = lower_hex(&Sha256::digest(
            serde_json::to_vec(&record.request)
                .map_err(|error| format!("cannot hash request {}: {error}", record.run_id))?,
        ));
        if request_sha256 != record.request_sha256 {
            return Err(format!("request SHA mismatch for run {}", record.run_id));
        }
        let prompt = record
            .request
            .messages
            .as_slice()
            .first()
            .filter(|_| record.request.messages.len() == 1)
            .filter(|message| message.role == "user")
            .ok_or_else(|| format!("run {} must contain one user message", record.run_id))?;
        if lower_hex(&Sha256::digest(prompt.content.as_bytes())) != record.prompt_sha256 {
            return Err(format!("prompt SHA mismatch for run {}", record.run_id));
        }
        let expected_run_id = lower_hex(&Sha256::digest(
            format!(
                "benchfck.v0.4.model-run.v1\0{}\0{}\0{}",
                record.system_id, record.repeat, record.task_id
            )
            .as_bytes(),
        ));
        if record.run_id != expected_run_id {
            return Err(format!(
                "deterministic run ID mismatch for {}",
                record.task_id
            ));
        }
        records.push(record);
    }
    Ok(records)
}

pub fn load_attempts(bytes: &[u8]) -> Result<Vec<AttemptRecord>, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|error| format!("attempt log is not UTF-8: {error}"))?;
    text.lines()
        .enumerate()
        .map(|(index, line)| {
            if line.trim().is_empty() {
                return Err(format!("attempt log line {} is blank", index + 1));
            }
            let record: AttemptRecord = serde_json::from_str(line)
                .map_err(|error| format!("attempt log line {}: {error}", index + 1))?;
            if record.schema_version != ATTEMPT_SCHEMA_VERSION {
                return Err(format!(
                    "attempt log line {} has unsupported schema {}",
                    index + 1,
                    record.schema_version
                ));
            }
            Ok(record)
        })
        .collect()
}

fn validate_attempt(attempt: &AttemptRecord, run: &ModelRequestRecord) -> Result<(), String> {
    if attempt.attempt_index == 0 || attempt.attempt_index > MAX_ATTEMPTS {
        return Err(format!(
            "run {} has invalid attempt index {}",
            attempt.run_id, attempt.attempt_index
        ));
    }
    if attempt.request_sha256 != run.request_sha256 {
        return Err(format!(
            "run {} attempt request SHA mismatch",
            attempt.run_id
        ));
    }
    if attempt.started_at.is_empty() || attempt.finished_at.is_empty() {
        return Err(format!(
            "run {} attempt timestamps are required",
            attempt.run_id
        ));
    }
    match attempt.outcome {
        AttemptOutcome::Delivered => {
            let response = attempt
                .response
                .as_deref()
                .ok_or_else(|| format!("delivered run {} is missing response", attempt.run_id))?;
            let response_sha256 = attempt.response_sha256.as_deref().ok_or_else(|| {
                format!("delivered run {} is missing response SHA", attempt.run_id)
            })?;
            if lower_hex(&Sha256::digest(response.as_bytes())) != response_sha256 {
                return Err(format!(
                    "delivered run {} response SHA mismatch",
                    attempt.run_id
                ));
            }
            if attempt
                .provider_request_id
                .as_deref()
                .is_none_or(str::is_empty)
                || attempt.model_snapshot.as_deref().is_none_or(str::is_empty)
                || attempt.finish_reason.as_deref().is_none_or(str::is_empty)
            {
                return Err(format!(
                    "delivered run {} requires provider request, model snapshot, and finish reason",
                    attempt.run_id
                ));
            }
            if attempt.error_code.is_some() {
                return Err(format!(
                    "delivered run {} cannot carry an operational error",
                    attempt.run_id
                ));
            }
            if attempt.usage.input_tokens.is_none()
                || attempt.usage.output_tokens.is_none()
                || attempt
                    .cost_usd
                    .is_none_or(|cost| !cost.is_finite() || cost < 0.0)
            {
                return Err(format!(
                    "delivered run {} requires provider token usage and non-negative cost",
                    attempt.run_id
                ));
            }
        }
        AttemptOutcome::TransportError
        | AttemptOutcome::RateLimited
        | AttemptOutcome::ProviderError => {
            if attempt.response.is_some() || attempt.response_sha256.is_some() {
                return Err(format!(
                    "operationally failed run {} cannot carry a model response",
                    attempt.run_id
                ));
            }
            if attempt.error_code.as_deref().is_none_or(str::is_empty) {
                return Err(format!(
                    "operationally failed run {} requires an error code",
                    attempt.run_id
                ));
            }
        }
    }
    Ok(())
}

pub fn schedule_resume(
    plan: Vec<ModelRequestRecord>,
    attempts: &[AttemptRecord],
) -> Result<ResumeDecision, String> {
    let plan_by_id = plan
        .iter()
        .map(|run| (run.run_id.as_str(), run))
        .collect::<HashMap<_, _>>();
    let mut attempts_by_run = HashMap::<&str, Vec<&AttemptRecord>>::new();
    let mut seen = HashSet::new();
    for attempt in attempts {
        let run = plan_by_id
            .get(attempt.run_id.as_str())
            .ok_or_else(|| format!("attempt references unknown run_id: {}", attempt.run_id))?;
        if !seen.insert((attempt.run_id.as_str(), attempt.attempt_index)) {
            return Err(format!(
                "duplicate attempt {} for run {}",
                attempt.attempt_index, attempt.run_id
            ));
        }
        validate_attempt(attempt, run)?;
        attempts_by_run
            .entry(attempt.run_id.as_str())
            .or_default()
            .push(attempt);
    }

    let mut pending = Vec::new();
    let mut completed_runs = 0;
    let mut exhausted_runs = 0;
    for run in plan {
        let mut prior = attempts_by_run
            .remove(run.run_id.as_str())
            .unwrap_or_default();
        prior.sort_by_key(|attempt| attempt.attempt_index);
        for (index, attempt) in prior.iter().enumerate() {
            if attempt.attempt_index as usize != index + 1 {
                return Err(format!(
                    "run {} has a non-contiguous retry chain",
                    run.run_id
                ));
            }
            if attempt.outcome == AttemptOutcome::Delivered && index + 1 != prior.len() {
                return Err(format!("run {} was retried after delivery", run.run_id));
            }
        }
        if prior
            .last()
            .is_some_and(|attempt| attempt.outcome == AttemptOutcome::Delivered)
        {
            completed_runs += 1;
        } else if prior.len() >= MAX_ATTEMPTS as usize {
            exhausted_runs += 1;
        } else {
            pending.push(ScheduledAttempt {
                attempt_index: prior.len() as u8 + 1,
                run,
            });
        }
    }
    Ok(ResumeDecision {
        pending,
        completed_runs,
        exhausted_runs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{backend::MoveCarrier, schema::DifficultyBand};
    use serde_json::json;

    fn frozen_manifest() -> SystemManifest {
        SystemManifest {
            schema_version: SYSTEM_MANIFEST_SCHEMA_VERSION.into(),
            systems: vec![
                SystemSpec {
                    system_id: "M1".into(),
                    provider: Provider::Openai,
                    model_id: "gpt-5.6-terra".into(),
                    reasoning_setting: "none".into(),
                },
                SystemSpec {
                    system_id: "M2".into(),
                    provider: Provider::Anthropic,
                    model_id: "claude-sonnet-5".into(),
                    reasoning_setting: "disabled".into(),
                },
                SystemSpec {
                    system_id: "M3".into(),
                    provider: Provider::Google,
                    model_id: "gemini-3.5-flash".into(),
                    reasoning_setting: "minimal".into(),
                },
            ],
        }
    }

    fn metadata(index: usize) -> PublicItemMetadata {
        PublicItemMetadata {
            schema_version: "benchfck.public-item.v1".into(),
            program_id: format!("program-{index}"),
            item_id: format!("item-{index}"),
            arity: 1,
            output_arity: 1,
            grammar_shape: "opaque".into(),
            program_size_tier: (index % 4) as u8,
            e0_program_bpe_tokens: 1,
            e2_program_bpe_tokens: 1,
            e3_program_bpe_tokens: 1,
            difficulty_band: DifficultyBand::Hard,
            nesting_depth: 1,
            working_set: 1,
            n_steps: 1,
            semantic_steps: 1,
            trace_semantic_density: 1.0,
            text_semantic_density: 1.0,
            prompt_tokenizer: "cl100k_base".into(),
            e2_e0_prompt_bpe_ratio: None,
            e3_e0_prompt_bpe_ratio: None,
            e2_e0_program_bpe_ratio: None,
            e3_e0_program_bpe_ratio: None,
            move_carrier: MoveCarrier::Rle,
            minimum_loop_iterations: 1,
            s0_loops: 0,
            s1_loops: 1,
            s2_loops: 0,
            s1_s2_ratio: None,
            pointer_volatility: 0.0,
            t2_response_token_cap: 100,
            t3_response_token_cap: 50,
            available_families: vec![Family::T1],
            available_encodings: vec![EncodingId::E0],
        }
    }

    fn exact_pilot_packet() -> AnswerStrippedPacket {
        let metadata = (0..20).map(metadata).collect::<Vec<_>>();
        let tasks = metadata
            .iter()
            .flat_map(|item| {
                (0..20).map(move |task| TaskRecord {
                    schema_version: "benchfck.task.v3".into(),
                    task_id: format!("{}-task-{task}", item.item_id),
                    program_id: item.program_id.clone(),
                    item_id: item.item_id.clone(),
                    family: Family::T1,
                    encoding: EncodingId::E0,
                    prompt: format!("prompt {} {task}", item.item_id),
                    hard_token_cap: None,
                    payload: json!({"n_ideal": 1}),
                })
            })
            .collect();
        AnswerStrippedPacket { metadata, tasks }
    }

    #[test]
    fn frozen_pilot_builds_exactly_2160_answer_stripped_requests() {
        let records = build_plan(
            &exact_pilot_packet(),
            &frozen_manifest(),
            &"a".repeat(64),
            PlanScope::Pilot,
        )
        .unwrap();
        assert_eq!(records.len(), 2_160);
        assert_eq!(
            records
                .iter()
                .map(|record| &record.run_id)
                .collect::<HashSet<_>>()
                .len(),
            records.len()
        );
        let serialized = serde_json::to_value(&records[0]).unwrap();
        let mut hits = Vec::new();
        collect_forbidden_keys(&serialized, "", &mut hits);
        assert!(hits.is_empty());
        assert_eq!(records[0].request.messages.len(), 1);
        assert_eq!(records[0].request.messages[0].role, "user");
        assert!(records[0].request.tools.is_empty());
        assert!(records[0].request.sampling_parameters.is_empty());
    }

    #[test]
    fn packet_rejects_unknown_nested_answer_material_before_typed_parsing() {
        let line = json!({
            "record_type": "task",
            "data": {
                "schema_version": "benchfck.task.v3",
                "task_id": "task-1",
                "program_id": "program-1",
                "item_id": "item-1",
                "family": "T1",
                "encoding": "E0",
                "prompt": "safe",
                "hard_token_cap": null,
                "payload": {"nested": [{"expected_answer": 7}]}
            }
        });
        let error = load_answer_stripped_packet(line.to_string().as_bytes()).unwrap_err();
        assert!(error.contains("forbidden key paths"));
    }

    #[test]
    fn frozen_system_identity_cannot_be_silently_substituted() {
        let mut manifest = frozen_manifest();
        manifest.systems[0].model_id = "replacement".into();
        assert!(validate_frozen_systems(&manifest).is_err());
    }

    fn failed_attempt(run: &ModelRequestRecord, attempt_index: u8) -> AttemptRecord {
        AttemptRecord {
            schema_version: ATTEMPT_SCHEMA_VERSION.into(),
            run_id: run.run_id.clone(),
            attempt_index,
            request_sha256: run.request_sha256.clone(),
            outcome: AttemptOutcome::RateLimited,
            started_at: "2026-08-16T20:00:00Z".into(),
            finished_at: "2026-08-16T20:00:01Z".into(),
            latency_ms: 1_000,
            provider_request_id: None,
            model_snapshot: None,
            finish_reason: None,
            response: None,
            response_sha256: None,
            usage: ProviderUsage::default(),
            cost_usd: None,
            error_code: Some("rate_limit".into()),
        }
    }

    fn delivered_attempt(run: &ModelRequestRecord) -> AttemptRecord {
        let response = "{\"status\":\"OUTPUT\",\"value\":[1]}";
        AttemptRecord {
            schema_version: ATTEMPT_SCHEMA_VERSION.into(),
            run_id: run.run_id.clone(),
            attempt_index: 1,
            request_sha256: run.request_sha256.clone(),
            outcome: AttemptOutcome::Delivered,
            started_at: "2026-08-16T20:00:00Z".into(),
            finished_at: "2026-08-16T20:00:01Z".into(),
            latency_ms: 1_000,
            provider_request_id: Some("provider-request-1".into()),
            model_snapshot: Some(run.model_id.clone()),
            finish_reason: Some("stop".into()),
            response: Some(response.into()),
            response_sha256: Some(lower_hex(&Sha256::digest(response.as_bytes()))),
            usage: ProviderUsage {
                input_tokens: Some(10),
                output_tokens: Some(5),
                ..ProviderUsage::default()
            },
            cost_usd: Some(0.001),
            error_code: None,
        }
    }

    #[test]
    fn resume_never_reschedules_delivered_or_retry_exhausted_runs() {
        let plan = build_plan(
            &exact_pilot_packet(),
            &frozen_manifest(),
            &"a".repeat(64),
            PlanScope::Pilot,
        )
        .unwrap()
        .into_iter()
        .take(3)
        .collect::<Vec<_>>();
        let mut attempts = vec![delivered_attempt(&plan[0]), failed_attempt(&plan[1], 1)];
        for attempt_index in 1..=MAX_ATTEMPTS {
            attempts.push(failed_attempt(&plan[2], attempt_index));
        }
        let decision = schedule_resume(plan, &attempts).unwrap();
        assert_eq!(decision.completed_runs, 1);
        assert_eq!(decision.exhausted_runs, 1);
        assert_eq!(decision.pending.len(), 1);
        assert_eq!(decision.pending[0].attempt_index, 2);
    }

    #[test]
    fn resume_rejects_a_retry_after_delivery() {
        let run = build_plan(
            &exact_pilot_packet(),
            &frozen_manifest(),
            &"a".repeat(64),
            PlanScope::Pilot,
        )
        .unwrap()
        .remove(0);
        let attempts = vec![delivered_attempt(&run), failed_attempt(&run, 2)];
        assert!(schedule_resume(vec![run], &attempts).is_err());
    }
}
