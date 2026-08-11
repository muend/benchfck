use crate::{
    backend::MoveCarrier,
    bf::TracePoint,
    compiler::CompilerMetadata,
    ir::Program,
    oracle::{AvalancheRecord, SemanticFingerprint},
};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "benchfck.item.v3";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum EncodingId {
    E0,
    E1,
    E2,
    E3,
    E4,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Family {
    T1,
    T2,
    T3,
    T4,
    T5,
    T6,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DifficultyBand {
    Easy,
    Medium,
    Hard,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Encodings {
    pub e0: String,
    pub e1: String,
    pub e1_legend: Vec<(char, char)>,
    pub e2: String,
    pub e3: String,
    pub e4: String,
    pub e4_residual_unvalidated_risk: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Annotations {
    pub grammar_shape: String,
    pub program_size_tier: u8,
    pub e0_program_bpe_tokens: u64,
    pub e2_program_bpe_tokens: u64,
    pub e3_program_bpe_tokens: u64,
    pub nesting_depth: usize,
    pub working_set: usize,
    pub n_steps: u64,
    pub semantic_steps: u64,
    pub trace_semantic_density: f64,
    pub text_semantic_density: f64,
    pub prompt_tokenizer: String,
    pub e2_e0_prompt_bpe_ratio: f64,
    pub e3_e0_prompt_bpe_ratio: f64,
    pub e2_e0_program_bpe_ratio: f64,
    pub e3_e0_program_bpe_ratio: f64,
    pub move_carrier: MoveCarrier,
    pub rejection_histogram_before_acceptance: std::collections::BTreeMap<String, usize>,
    pub minimum_loop_iterations: u64,
    pub s0_loops: usize,
    pub s1_loops: usize,
    pub s2_loops: usize,
    pub s1_s2_ratio: Option<f64>,
    pub pointer_volatility: f64,
    pub difficulty_band: DifficultyBand,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OracleArtifacts {
    pub full_trace: Vec<TracePoint>,
    pub avalanche_map: Vec<AvalancheRecord>,
    pub avalanche_score: f64,
    pub avalanche_sampling_rate: f64,
    pub semantic_fingerprint: SemanticFingerprint,
    /// A compact solution found by the declared independent enumerator. This
    /// is an upper bound, not a global-minimum claim. Kept only in private
    /// exports and used by the local perfect mock.
    pub t2_reference_solution: String,
    pub t2_reference_solution_tokens_upper_bound: u32,
    pub t2_nontriviality_witness: NontrivialityWitness,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NontrivialityWitness {
    pub enumeration_algorithm: String,
    pub acceptance_grammar: String,
    pub folded_expression_grammar_token_threshold_exclusive: u32,
    pub requested_ast_depth: u8,
    pub proven_exhaustive_ast_depth: u8,
    pub short_expression_match_found: bool,
    pub matching_expression_ast_depth: Option<u8>,
    pub unique_semantics_enumerated: usize,
    pub operator_applications: u64,
    pub enumeration_resource_limit_hit: Option<String>,
    pub matching_expression: Option<String>,
    pub analytical: AnalyticalNontrivialityWitness,
    pub hybrid_gate_passed: bool,
    pub reference_search_algorithm: String,
    pub reference_candidates_enumerated: usize,
    pub reference_candidates_full_domain_checked: usize,
    pub domain_size: u64,
    pub matched_digest_hex: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AdditivePeriodWitness {
    pub period: u16,
    pub delta_mod_256: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AnalyticalExpressionWitness {
    pub family: String,
    pub expression: String,
    pub folded_expression_grammar_tokens: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AnalyticalNontrivialityWitness {
    pub algorithm: String,
    pub folded_expression_grammar_token_threshold_exclusive: u32,
    pub derived_max_integer_polynomial_degree_checked: u8,
    pub detected_integer_polynomial_degree: Option<u8>,
    pub max_period_checked: u16,
    pub detected_exact_period: Option<u16>,
    pub detected_additive_period: Option<AdditivePeriodWitness>,
    pub minimum_integer_affine_pieces: usize,
    pub bitwise_pointwise: bool,
    pub compact_expression_witnesses: Vec<AnalyticalExpressionWitness>,
    pub matched_trivial_families: Vec<String>,
    pub named_families_excluded: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BaseItem {
    pub schema_version: String,
    pub program_id: String,
    pub item_id: String,
    pub seed: u64,
    pub input: Vec<u8>,
    pub expected_output: Vec<u8>,
    pub ir: Program,
    pub encodings: Encodings,
    pub compiler: CompilerMetadata,
    pub annotations: Annotations,
    pub oracles: OracleArtifacts,
    pub reserved_families: Vec<Family>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TaskRecord {
    pub schema_version: String,
    pub task_id: String,
    pub program_id: String,
    pub item_id: String,
    pub family: Family,
    pub encoding: EncodingId,
    pub prompt: String,
    pub hard_token_cap: Option<u32>,
    pub payload: serde_json::Value,
}

/// Non-secret item descriptors shipped with public task exports. This makes
/// difficulty and coverage auditable without exposing programs, inputs,
/// outputs, compiler internals, fingerprints, traces, or answer material.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PublicItemMetadata {
    pub schema_version: String,
    pub program_id: String,
    pub item_id: String,
    pub arity: u8,
    pub output_arity: u8,
    pub grammar_shape: String,
    pub program_size_tier: u8,
    pub e0_program_bpe_tokens: u64,
    pub e2_program_bpe_tokens: u64,
    pub e3_program_bpe_tokens: u64,
    pub difficulty_band: DifficultyBand,
    pub nesting_depth: usize,
    pub working_set: usize,
    pub n_steps: u64,
    pub semantic_steps: u64,
    pub trace_semantic_density: f64,
    pub text_semantic_density: f64,
    pub prompt_tokenizer: String,
    pub e2_e0_prompt_bpe_ratio: f64,
    pub e3_e0_prompt_bpe_ratio: f64,
    pub e2_e0_program_bpe_ratio: f64,
    pub e3_e0_program_bpe_ratio: f64,
    pub move_carrier: MoveCarrier,
    pub minimum_loop_iterations: u64,
    pub s0_loops: usize,
    pub s1_loops: usize,
    pub s2_loops: usize,
    pub s1_s2_ratio: Option<f64>,
    pub pointer_volatility: f64,
    pub t2_response_token_cap: u32,
    pub t3_response_token_cap: u32,
    pub available_families: Vec<Family>,
    pub available_encodings: Vec<EncodingId>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "record_type", content = "data", rename_all = "snake_case")]
pub enum JsonlRecord {
    Item(Box<BaseItem>),
    PublicItemMetadata(Box<PublicItemMetadata>),
    Task(Box<TaskRecord>),
}
