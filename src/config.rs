use crate::backend::MoveCarrier;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Defaults {
    pub if_without_else: bool,
    pub max_arity: u8,
    pub layout_disciplines: u8,
    pub held_out_layout: String,
    pub templates_per_statement: u8,
    pub avalanche_threshold: f64,
    pub trace_demand: String,
    pub pointer_volatility_as_covariate: bool,
    pub step_cap: u64,
    pub minimum_avalanche_positions: usize,
    pub off_idiom_threshold: f64,
    pub move_carrier: MoveCarrier,
    pub minimum_trace_semantic_density: f64,
    pub minimum_text_semantic_density: f64,
    pub prompt_tokenizer: String,
    pub maximum_e2_prompt_bpe_ratio: f64,
    pub t1_probe_count: usize,
    pub t2_nontriviality_threshold: u32,
    pub t2_enumerator_target_ast_depth: u8,
    pub t2_enumerator_min_proven_ast_depth: u8,
    pub t2_enumerator_max_semantics: usize,
    pub t2_enumerator_max_operator_applications: u64,
    pub t2_token_cap: u32,
    pub t3_token_cap: u32,
}

impl Defaults {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(toml::from_str(&fs::read_to_string(path)?)?)
    }
}
