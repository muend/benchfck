use crate::schema::{BaseItem, JsonlRecord};
use serde_json::Value;
use std::{collections::BTreeSet, fmt::Write as _};

pub const FORBIDDEN_PUBLIC_KEYS: &[&str] = &[
    "avalanche_map",
    "avalanche_sampling_rate",
    "avalanche_score",
    "changed",
    "compiler",
    "e0",
    "e1",
    "e1_legend",
    "e2",
    "e3",
    "e4",
    "encodings",
    "expected_answer",
    "expected_output",
    "full_trace",
    "input",
    "ir",
    "matched_digest_hex",
    "matching_expression",
    "oracle_fingerprint",
    "oracles",
    "outcome",
    "reference_solution",
    "seed",
    "semantic_fingerprint",
    "t2_nontriviality_witness",
    "t2_reference_solution",
    "trace",
];

#[derive(Clone, Debug)]
pub struct LeakAudit {
    pub public_records: usize,
    pub public_item_records: usize,
    pub public_metadata_records: usize,
    pub public_task_records: usize,
    pub private_item_records: usize,
    pub forbidden_key_hits: Vec<(String, usize)>,
    pub duplicate_metadata_ids: usize,
    pub duplicate_task_ids: usize,
    pub duplicate_private_item_ids: usize,
    pub metadata_private_id_mismatches: usize,
    pub orphan_task_item_ids: usize,
    pub private_path_ignored: bool,
    pub private_path_untracked: bool,
}

impl LeakAudit {
    pub fn release_passed(&self) -> bool {
        self.public_records > 0
            && self.public_item_records == 0
            && self.public_metadata_records == self.private_item_records
            && self.public_task_records > 0
            && self.forbidden_key_hits.iter().all(|(_, hits)| *hits == 0)
            && self.duplicate_metadata_ids == 0
            && self.duplicate_task_ids == 0
            && self.duplicate_private_item_ids == 0
            && self.metadata_private_id_mismatches == 0
            && self.orphan_task_item_ids == 0
            && self.private_path_ignored
            && self.private_path_untracked
    }
}

fn collect_forbidden_keys(value: &Value, hits: &mut [usize]) {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                if let Some(index) = FORBIDDEN_PUBLIC_KEYS
                    .iter()
                    .position(|forbidden| key == forbidden)
                {
                    hits[index] += 1;
                }
                collect_forbidden_keys(nested, hits);
            }
        }
        Value::Array(values) => {
            for nested in values {
                collect_forbidden_keys(nested, hits);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

pub fn audit(
    public_bytes: &[u8],
    private_items: &[BaseItem],
    private_path_ignored: bool,
    private_path_untracked: bool,
) -> Result<LeakAudit, String> {
    let public_text = std::str::from_utf8(public_bytes)
        .map_err(|error| format!("public JSONL is not UTF-8: {error}"))?;
    let mut forbidden_hits = vec![0usize; FORBIDDEN_PUBLIC_KEYS.len()];
    let mut public_records = 0usize;
    let mut public_item_records = 0usize;
    let mut metadata_ids = BTreeSet::new();
    let mut task_ids = BTreeSet::new();
    let mut task_item_ids = BTreeSet::new();
    let mut duplicate_metadata_ids = 0usize;
    let mut duplicate_task_ids = 0usize;

    for (line_index, line) in public_text.lines().enumerate() {
        if line.trim().is_empty() {
            return Err(format!("public JSONL line {} is blank", line_index + 1));
        }
        let value: Value = serde_json::from_str(line)
            .map_err(|error| format!("public JSONL line {}: {error}", line_index + 1))?;
        collect_forbidden_keys(&value, &mut forbidden_hits);
        let record: JsonlRecord = serde_json::from_value(value)
            .map_err(|error| format!("public JSONL line {}: {error}", line_index + 1))?;
        public_records += 1;
        match record {
            JsonlRecord::Item(_) => public_item_records += 1,
            JsonlRecord::PublicItemMetadata(metadata) => {
                if !metadata_ids.insert(metadata.item_id.clone()) {
                    duplicate_metadata_ids += 1;
                }
            }
            JsonlRecord::Task(task) => {
                task_item_ids.insert(task.item_id.clone());
                if !task_ids.insert(task.task_id.clone()) {
                    duplicate_task_ids += 1;
                }
            }
        }
    }

    let private_ids = private_items
        .iter()
        .map(|item| item.item_id.clone())
        .collect::<BTreeSet<_>>();
    let duplicate_private_item_ids = private_items.len().saturating_sub(private_ids.len());
    let metadata_private_id_mismatches = metadata_ids.symmetric_difference(&private_ids).count();
    let orphan_task_item_ids = task_item_ids.difference(&metadata_ids).count();

    Ok(LeakAudit {
        public_records,
        public_item_records,
        public_metadata_records: metadata_ids.len(),
        public_task_records: task_ids.len(),
        private_item_records: private_items.len(),
        forbidden_key_hits: FORBIDDEN_PUBLIC_KEYS
            .iter()
            .zip(forbidden_hits)
            .map(|(key, hits)| ((*key).to_string(), hits))
            .collect(),
        duplicate_metadata_ids,
        duplicate_task_ids,
        duplicate_private_item_ids,
        metadata_private_id_mismatches,
        orphan_task_item_ids,
        private_path_ignored,
        private_path_untracked,
    })
}

pub fn render(
    audit: &LeakAudit,
    public_source: &str,
    public_sha256: &str,
    private_sha256: &str,
) -> String {
    let mut report = String::new();
    writeln!(report, "# Generated-batch leak scan\n").unwrap();
    writeln!(report, "- Schema: `benchfck.leak-scan.v1`").unwrap();
    writeln!(
        report,
        "- Status: **{}**",
        if audit.release_passed() {
            "PASS"
        } else {
            "FAIL"
        }
    )
    .unwrap();
    writeln!(report, "- Public source: `{public_source}`").unwrap();
    writeln!(report, "- Public source SHA-256: `{public_sha256}`").unwrap();
    writeln!(
        report,
        "- Private source path: omitted from public evidence"
    )
    .unwrap();
    writeln!(report, "- Private source SHA-256: `{private_sha256}`").unwrap();
    writeln!(report, "- Public JSONL records: {}", audit.public_records).unwrap();
    writeln!(
        report,
        "- Public private-item records: {}",
        audit.public_item_records
    )
    .unwrap();
    writeln!(
        report,
        "- Public metadata records: {}",
        audit.public_metadata_records
    )
    .unwrap();
    writeln!(
        report,
        "- Public task records: {}",
        audit.public_task_records
    )
    .unwrap();
    writeln!(
        report,
        "- Private item records: {}",
        audit.private_item_records
    )
    .unwrap();
    writeln!(
        report,
        "- Duplicate public metadata IDs: {}",
        audit.duplicate_metadata_ids
    )
    .unwrap();
    writeln!(
        report,
        "- Duplicate public task IDs: {}",
        audit.duplicate_task_ids
    )
    .unwrap();
    writeln!(
        report,
        "- Duplicate private item IDs: {}",
        audit.duplicate_private_item_ids
    )
    .unwrap();
    writeln!(
        report,
        "- Public/private item-ID mismatches: {}",
        audit.metadata_private_id_mismatches
    )
    .unwrap();
    writeln!(
        report,
        "- Task item IDs without public metadata: {}",
        audit.orphan_task_item_ids
    )
    .unwrap();
    writeln!(
        report,
        "- Private path is Git-ignored: {}",
        audit.private_path_ignored
    )
    .unwrap();
    writeln!(
        report,
        "- Private path is Git-untracked: {}\n",
        audit.private_path_untracked
    )
    .unwrap();

    writeln!(report, "## Recursive forbidden-key audit\n").unwrap();
    writeln!(report, "The scan walks every object and array in every raw JSONL record before typed deserialization, so unknown extra fields cannot be hidden by schema parsing. Prompt strings are not treated as JSON keys.\n").unwrap();
    writeln!(report, "| Forbidden public key | Hits |").unwrap();
    writeln!(report, "|---|---:|").unwrap();
    for (key, hits) in &audit.forbidden_key_hits {
        writeln!(report, "| `{key}` | {hits} |").unwrap();
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recursive_scan_finds_answer_material_inside_arrays() {
        let value = json!({"safe": [{"payload": {"expected_answer": 7}}]});
        let mut hits = vec![0; FORBIDDEN_PUBLIC_KEYS.len()];
        collect_forbidden_keys(&value, &mut hits);
        let index = FORBIDDEN_PUBLIC_KEYS
            .iter()
            .position(|key| *key == "expected_answer")
            .unwrap();
        assert_eq!(hits[index], 1);
    }
}
