use crate::{
    backend::Bytecode,
    bf::BfProgram,
    compiler::{LayoutDiscipline, compile, structural_obfuscate},
    config::Defaults,
    ir::{LoopClass, Program, Statement},
    oracle,
    schema::{
        Annotations, BaseItem, DifficultyBand, EncodingId, Encodings, Family, JsonlRecord,
        OracleArtifacts, PublicItemMetadata, SCHEMA_VERSION,
    },
    tasks,
};
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha20Rng;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildSpec {
    pub seed: u64,
    pub count: usize,
    pub difficulty: DifficultyBand,
    pub arity: u8,
    pub held_out: bool,
}

pub const PROGRAM_SIZE_TIERS: u8 = 8;
const CONSTRUCTOR_VARIABLES: usize = 9;
const SIZE_TIER_WORK_ROUNDS: [usize; PROGRAM_SIZE_TIERS as usize] = [0, 3, 6, 12, 17, 22, 31, 41];

#[derive(Debug, Error)]
pub enum GenerateError {
    #[error("arity must be 1 or 2")]
    BadArity,
    #[error("compile failed: {0}")]
    Compile(#[from] crate::compiler::CompileError),
    #[error("explicit backend failed: {0}")]
    Backend(#[from] crate::backend::BackendError),
    #[error("oracle failed: {0}")]
    Oracle(#[from] crate::oracle::OracleError),
    #[error("tokenizer failed: {0}")]
    Tokenizer(String),
    #[error(
        "unable to produce accepted item after {attempts} deterministic attempts; last rejection: {last}; histogram: {histogram:?}"
    )]
    Exhausted {
        attempts: usize,
        last: String,
        histogram: std::collections::BTreeMap<String, usize>,
    },
    #[error(
        "batch response budgets lack item-level diversity: required {required} distinct caps, observed T2={t2_distinct}, T3={t3_distinct}"
    )]
    BatchBudgetDiversity {
        required: usize,
        t2_distinct: usize,
        t3_distinct: usize,
    },
    #[error(
        "batch program-size ladder is incomplete: required {required} tiers, observed {observed}"
    )]
    BatchSizeLadder { required: usize, observed: usize },
    #[error(
        "batch has too few disjoint token-matched T2 pairs for {encoding}: required {required}, observed {observed}"
    )]
    BatchMatchedPairs {
        encoding: String,
        required: usize,
        observed: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GeneratedCase {
    program: Program,
    shape: &'static str,
    size_tier: u8,
}

fn mod_div_block(body: &mut Vec<Statement>, src: usize, modulus: u8) {
    // Scratch layout: counter=3, remainder=5, quotient=6,
    // modulus/zero-test gate=7, difference=8. The modulus cell is refreshed
    // before subtraction and then reused as the gate, keeping the live set to
    // nine variables without changing the loop semantics.
    body.extend([
        Statement::Copy { dst: 3, src },
        Statement::Set { dst: 5, value: 0 },
        Statement::Set { dst: 6, value: 0 },
        Statement::While {
            cond: 3,
            class: LoopClass::S1,
            body: vec![
                Statement::Set { dst: 7, value: 1 },
                Statement::DrainScaled {
                    dst: 3,
                    src: 7,
                    factor: 1,
                    subtract: true,
                },
                Statement::Set { dst: 7, value: 1 },
                Statement::DrainScaled {
                    dst: 5,
                    src: 7,
                    factor: 1,
                    subtract: false,
                },
                Statement::Copy { dst: 8, src: 5 },
                Statement::Set {
                    dst: 7,
                    value: modulus,
                },
                Statement::DrainScaled {
                    dst: 8,
                    src: 7,
                    factor: 1,
                    subtract: true,
                },
                Statement::Set { dst: 7, value: 1 },
                Statement::IfNonZeroDrain {
                    cond: 8,
                    body: vec![Statement::Set { dst: 7, value: 0 }],
                },
                Statement::IfNonZeroDrain {
                    cond: 7,
                    body: vec![
                        Statement::Set { dst: 5, value: 0 },
                        Statement::Set { dst: 7, value: 1 },
                        Statement::DrainScaled {
                            dst: 6,
                            src: 7,
                            factor: 1,
                            subtract: false,
                        },
                    ],
                },
            ],
        },
    ]);
}

fn add_scaled(body: &mut Vec<Statement>, dst: usize, src: usize, count: usize, subtract: bool) {
    body.push(Statement::DrainScaled {
        dst,
        src,
        factor: count as u8,
        subtract,
    });
}

fn multiply_into(body: &mut Vec<Statement>, dst: usize, left: usize, right: usize) {
    // mod_div_block has finished when multiplication begins, so its counter
    // and one cells can be reused. Keeping the live set compact materially
    // reduces Brainfuck pointer traffic across every compiler layout.
    const COUNTER: usize = 3;
    const ONE: usize = 4;
    body.extend([
        Statement::Copy {
            dst: COUNTER,
            src: left,
        },
        Statement::Set { dst: ONE, value: 1 },
        Statement::Set { dst, value: 0 },
        Statement::While {
            cond: COUNTER,
            class: LoopClass::S1,
            body: vec![
                Statement::Sub {
                    dst: COUNTER,
                    src: ONE,
                },
                Statement::Add { dst, src: right },
            ],
        },
    ]);
}

fn multiply_into_consuming_left(body: &mut Vec<Statement>, dst: usize, left: usize, right: usize) {
    const COUNTER: usize = 3;
    body.extend([
        Statement::Copy {
            dst: COUNTER,
            src: left,
        },
        Statement::Set { dst, value: 0 },
        Statement::While {
            cond: COUNTER,
            class: LoopClass::S1,
            body: vec![
                Statement::Set {
                    dst: left,
                    value: 1,
                },
                Statement::DrainScaled {
                    dst: COUNTER,
                    src: left,
                    factor: 1,
                    subtract: true,
                },
                Statement::Add { dst, src: right },
            ],
        },
    ]);
}

fn add_constant(body: &mut Vec<Statement>, dst: usize, value: u8) {
    const CONSTANT: usize = 8;
    body.extend([
        Statement::Set {
            dst: CONSTANT,
            value,
        },
        Statement::DrainScaled {
            dst,
            src: CONSTANT,
            factor: 1,
            subtract: false,
        },
    ]);
}

/// Eight terminating constructors promoted from the generated v3 template
/// search. Each occupies a distinct name-independent semantic profile over the
/// complete 256-input domain and survives the calibrated hybrid T2 gate.
fn generated_program(seed: u64, arity: u8, size_tier: u8) -> GeneratedCase {
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    let bias = rng.random::<u8>();
    let mut body = vec![Statement::In { dst: 0 }];
    if arity == 2 {
        body.push(Statement::In { dst: 1 });
    }
    body.push(Statement::Set {
        dst: 2,
        value: bias,
    });
    let shape = match seed % 8 {
        0 => {
            mod_div_block(&mut body, 0, 5);
            body.push(Statement::Copy { dst: 7, src: 5 });
            add_constant(&mut body, 7, 1);
            multiply_into_consuming_left(&mut body, 8, 7, 6);
            add_scaled(&mut body, 2, 8, 1, false);
            add_scaled(&mut body, 2, 5, 3, false);
            add_scaled(&mut body, 2, 6, 5, false);
            "shifted_residue_p5_s1"
        }
        1 => {
            mod_div_block(&mut body, 0, 5);
            multiply_into(&mut body, 8, 5, 5);
            add_scaled(&mut body, 2, 8, 11, false);
            add_scaled(&mut body, 2, 6, 3, false);
            add_scaled(&mut body, 2, 5, 1, false);
            "residue_square_p5_c11"
        }
        2 => {
            mod_div_block(&mut body, 0, 7);
            multiply_into(&mut body, 8, 5, 6);
            add_scaled(&mut body, 2, 8, 5, false);
            add_scaled(&mut body, 2, 5, 11, false);
            add_scaled(&mut body, 2, 6, 17, false);
            "residue_quotient_p7_c5_r11_q17"
        }
        3 => {
            mod_div_block(&mut body, 0, 5);
            multiply_into(&mut body, 8, 5, 5);
            add_scaled(&mut body, 2, 8, 1, true);
            add_scaled(&mut body, 2, 6, 3, false);
            add_scaled(&mut body, 2, 5, 7, false);
            "residue_complement_p5_q3"
        }
        4 => {
            mod_div_block(&mut body, 0, 5);
            // Expanded form of (r + 5)q + 3r + 5q. Computing rq + 3r + 10q
            // avoids manufacturing pointer traffic for the constant shift.
            multiply_into(&mut body, 8, 5, 6);
            add_scaled(&mut body, 2, 8, 1, false);
            add_scaled(&mut body, 2, 5, 3, false);
            add_scaled(&mut body, 2, 6, 10, false);
            "shifted_residue_p5_s5"
        }
        5 => {
            mod_div_block(&mut body, 0, 7);
            multiply_into(&mut body, 8, 5, 6);
            add_scaled(&mut body, 2, 8, 2, false);
            add_scaled(&mut body, 2, 5, 11, false);
            add_scaled(&mut body, 2, 6, 13, false);
            "residue_quotient_p7_c2_r11_q13"
        }
        6 => {
            mod_div_block(&mut body, 0, 7);
            multiply_into(&mut body, 8, 5, 6);
            add_scaled(&mut body, 2, 8, 2, false);
            add_scaled(&mut body, 2, 5, 11, false);
            add_scaled(&mut body, 2, 6, 17, false);
            "residue_quotient_p7_c2_r11_q17"
        }
        _ => {
            mod_div_block(&mut body, 0, 5);
            multiply_into(&mut body, 8, 5, 5);
            add_scaled(&mut body, 2, 8, 1, true);
            add_scaled(&mut body, 2, 6, 1, false);
            add_scaled(&mut body, 2, 5, 7, false);
            "residue_complement_p5_q1"
        }
    };
    // Controlled program-size ladder. Each tier adds an executed reversible
    // workload: two independent copies of an input are accumulated into the
    // output with the same factor and opposite signs. This changes state and
    // executes real loops, but preserves the item function exactly. It is a
    // labeled size intervention for token matching, not empty tape padding.
    let size_tier = size_tier.min(PROGRAM_SIZE_TIERS - 1);
    let work_rounds = SIZE_TIER_WORK_ROUNDS[size_tier as usize];
    for round in 0..work_rounds {
        let addend = CONSTRUCTOR_VARIABLES + round * 2;
        let subtrahend = addend + 1;
        let src = round % arity as usize;
        let factor = 17;
        body.extend([
            Statement::Copy { dst: addend, src },
            Statement::Copy {
                dst: subtrahend,
                src,
            },
            Statement::DrainScaled {
                dst: 2,
                src: addend,
                factor,
                subtract: false,
            },
            Statement::DrainScaled {
                dst: 2,
                src: subtrahend,
                factor,
                subtract: true,
            },
        ]);
    }
    if arity == 2 {
        add_scaled(&mut body, 2, 1, 197 + (seed as usize % 3) * 6, false);
    }
    body.push(Statement::Out { src: 2 });
    GeneratedCase {
        program: Program {
            arity,
            output_arity: 1,
            variables: (0..CONSTRUCTOR_VARIABLES + work_rounds * 2)
                .map(|i| format!("v{i}"))
                .collect(),
            body,
        },
        shape,
        size_tier,
    }
}
fn hash(parts: &[&[u8]]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update((p.len() as u64).to_le_bytes());
        h.update(p)
    }
    crate::lower_hex(&h.finalize())[..20].to_string()
}
fn target(b: DifficultyBand) -> u64 {
    match b {
        DifficultyBand::Easy => 20_000,
        DifficultyBand::Medium => 75_000,
        DifficultyBand::Hard => 180_000,
    }
}
fn measured_band(n: u64) -> DifficultyBand {
    if n < 30_000 {
        DifficultyBand::Easy
    } else if n < 120_000 {
        DifficultyBand::Medium
    } else {
        DifficultyBand::Hard
    }
}
fn choose_input(
    ir: &Program,
    e0: &str,
    arity: u8,
    band: DifficultyBand,
    step_cap: u64,
    seed: u64,
) -> Result<(Vec<u8>, crate::bf::BfRun), crate::bf::BfError> {
    let p = BfProgram::parse(e0)?;
    let mut best: Option<(u64, Vec<u8>)> = None;
    let mut candidates = if arity == 1 {
        (1..=255u8).map(|x| vec![x]).collect::<Vec<_>>()
    } else {
        let mut rng = ChaCha20Rng::seed_from_u64(seed ^ 0xA217_17C9_5EED);
        let mut values = vec![
            vec![1, 1],
            vec![1, 255],
            vec![255, 1],
            vec![255, 255],
            vec![2, 127],
            vec![127, 2],
            vec![17, 239],
            vec![239, 17],
        ];
        values.extend((0..1024).map(|_| vec![rng.random::<u8>(), rng.random::<u8>()]));
        values
    };
    let candidate_count = candidates.len();
    candidates.rotate_left((seed % candidate_count as u64) as usize);
    for input in candidates {
        if !crate::ir::execution_profile(ir, &input, step_cap)
            .is_ok_and(|profile| profile.is_fully_exercised())
        {
            continue;
        }
        if let Ok(run) = p.execute(&input, step_cap, false) {
            let d = run.state.steps.abs_diff(target(band));
            if best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, input.clone()));
            }
            if measured_band(run.state.steps) == band {
                let traced = p.execute(&input, step_cap, true)?;
                return Ok((input, traced));
            }
        }
    }
    let (_, input) = best.ok_or(crate::bf::BfError::NonTerminating)?;
    let run = p.execute(&input, step_cap, true)?;
    Ok((input, run))
}

pub fn generate(spec: &BuildSpec, defaults: &Defaults) -> Result<Vec<BaseItem>, GenerateError> {
    if !(1..=2).contains(&spec.arity) {
        return Err(GenerateError::BadArity);
    }
    let mut accepted = vec![];
    let mut attempt = 0;
    let max_attempts = spec.count.max(1) * 64;
    let mut last = "none".to_string();
    let mut accepted_shapes = std::collections::BTreeSet::new();
    let mut shape_counts = std::collections::BTreeMap::<String, usize>::new();
    let max_per_shape = spec.count.div_ceil(4).max(1);
    let mut rejection_histogram = std::collections::BTreeMap::<String, usize>::new();
    macro_rules! reject {
        ($category:expr, $reason:expr) => {{
            last = $reason;
            *rejection_histogram.entry($category.into()).or_default() += 1;
            attempt += 1;
            continue;
        }};
    }
    while accepted.len() < spec.count && attempt < max_attempts {
        let item_seed = spec.seed.wrapping_add(attempt as u64 * 0x9E37_79B9);
        let requested_size_tier = (accepted.len() % PROGRAM_SIZE_TIERS as usize) as u8;
        let generated = generated_program(item_seed, spec.arity, requested_size_tier);
        if (accepted_shapes.contains(generated.shape) && accepted_shapes.len() < spec.count.min(8))
            || shape_counts.get(generated.shape).copied().unwrap_or(0) >= max_per_shape
        {
            reject!(
                "duplicate_shape",
                format!("duplicate constructor shape {}", generated.shape)
            );
        }
        let (ir, passes) = structural_obfuscate(&generated.program, item_seed);
        let discipline = if spec.held_out {
            LayoutDiscipline::HeldOut
        } else {
            match item_seed % 3 {
                0 => LayoutDiscipline::Contiguous,
                1 => LayoutDiscipline::Interleaved,
                _ => LayoutDiscipline::Strided,
            }
        };
        let mut compiled = compile(&ir, item_seed, discipline)?;
        compiled.metadata.obfuscation_passes = passes;
        let e0_ops = compiled
            .e0
            .chars()
            .filter(|c| "+-<>[],.".contains(*c))
            .collect::<Vec<_>>();
        let text_semantic_density = e0_ops.iter().filter(|c| !matches!(c, '>' | '<')).count()
            as f64
            / e0_ops.len().max(1) as f64;
        if text_semantic_density < defaults.minimum_text_semantic_density {
            reject!(
                "text_semantic_density",
                format!("text semantic density {text_semantic_density:.3}")
            );
        }
        let idiom = oracle::off_idiom_rate(&compiled.e0);
        if idiom >= defaults.off_idiom_threshold {
            reject!("off_idiom_rate", format!("off-idiom rate {idiom:.3}"));
        }
        let bytecode = Bytecode::from_e0_with_carrier(&compiled.e0, defaults.move_carrier)?;
        let (input, run) = match choose_input(
            &ir,
            &compiled.e0,
            ir.arity,
            spec.difficulty,
            defaults.step_cap,
            item_seed,
        ) {
            Ok(x) => x,
            Err(e) => {
                reject!("input_selection", e.to_string());
            }
        };
        if measured_band(run.state.steps) != spec.difficulty {
            reject!(
                "difficulty_band",
                format!(
                    "could not solve input into requested band; closest had {} steps",
                    run.state.steps
                )
            );
        }
        let semantic_steps = run
            .trace
            .iter()
            .filter(|point| !matches!(point.instruction, '>' | '<'))
            .count() as u64;
        let trace_semantic_density = semantic_steps as f64 / run.state.steps.max(1) as f64;
        if trace_semantic_density < defaults.minimum_trace_semantic_density {
            reject!(
                "trace_semantic_density",
                format!("trace semantic density {trace_semantic_density:.3}")
            );
        }
        if !oracle::each_argument_sensitive(&compiled.e0, &input, defaults.step_cap)? {
            reject!(
                "per_argument_sensitivity",
                "per-argument input sensitivity".into()
            );
        }
        let worst_case = vec![255; ir.arity as usize];
        if let Err(error) = BfProgram::parse(&compiled.e0)
            .expect("compiler emits syntactically valid E0")
            .execute(&worst_case, defaults.step_cap, false)
        {
            reject!(
                "worst_case_preflight",
                format!("worst-case preflight for {}: {error}", generated.shape)
            );
        }
        let e2_source = bytecode.e2_source();
        let e3_source = bytecode.e3_source();
        let fingerprint = match oracle::exhaustive_validate(
            &ir,
            &compiled.e0,
            &e2_source,
            &e3_source,
            defaults.step_cap,
        ) {
            Ok(x) => x,
            Err(e) => {
                reject!(
                    "cross_backend_domain",
                    format!("cross-backend/domain rejection: {e}")
                );
            }
        };
        let mut nontriviality_witness = match tasks::prove_no_short_parser_expression(
            &compiled.e0,
            ir.arity,
            &fingerprint,
            defaults.step_cap,
            tasks::NontrivialitySearchLimits {
                folded_expression_grammar_token_threshold_exclusive: defaults
                    .t2_nontriviality_threshold,
                target_ast_depth: defaults.t2_enumerator_target_ast_depth,
                minimum_proven_ast_depth: defaults.t2_enumerator_min_proven_ast_depth,
                max_semantics: defaults.t2_enumerator_max_semantics,
                max_operator_applications: defaults.t2_enumerator_max_operator_applications,
            },
        ) {
            Ok(witness) => witness,
            Err(error) => reject!("nontriviality_enumerator_error", error),
        };
        if nontriviality_witness.short_expression_match_found {
            reject!(
                "short_expression_match_within_enumerated_layer",
                format!(
                    "ExprParser grammar contains a matching T2 expression below {} folded lexical tokens at AST depth {:?}",
                    defaults.t2_nontriviality_threshold,
                    nontriviality_witness.matching_expression_ast_depth
                )
            );
        }
        if !nontriviality_witness.analytical.named_families_excluded {
            reject!(
                "analytical_triviality_family_match",
                format!(
                    "matched named trivial families: {:?}",
                    nontriviality_witness.analytical.matched_trivial_families
                )
            );
        }
        if nontriviality_witness.proven_exhaustive_ast_depth
            < defaults.t2_enumerator_min_proven_ast_depth
        {
            reject!(
                "insufficient_proven_exhaustive_ast_depth",
                format!(
                    "proved AST depth {}, required {}; resource={:?}",
                    nontriviality_witness.proven_exhaustive_ast_depth,
                    defaults.t2_enumerator_min_proven_ast_depth,
                    nontriviality_witness.enumeration_resource_limit_hit
                )
            );
        }
        debug_assert!(nontriviality_witness.hybrid_gate_passed);
        let Some(reference) = tasks::search_g2_reference_expression(
            &compiled.e0,
            ir.arity,
            &fingerprint,
            defaults.step_cap,
        ) else {
            reject!(
                "reference_expression_not_found",
                "declared G2 search found no matching reference candidate".into()
            );
        };
        nontriviality_witness.reference_search_algorithm =
            "declared_family_G2_reference_only_v2".into();
        nontriviality_witness.reference_candidates_enumerated = reference.candidates_enumerated;
        nontriviality_witness.reference_candidates_full_domain_checked =
            reference.candidates_full_domain_checked;
        debug_assert_eq!(
            nontriviality_witness.matched_digest_hex,
            reference.matched_digest_hex
        );
        let avalanche = oracle::avalanche_map(
            &compiled.e0,
            &input,
            defaults.step_cap,
            defaults.minimum_avalanche_positions,
            item_seed,
        )?;
        if avalanche.score < defaults.avalanche_threshold {
            reject!(
                "avalanche",
                format!("post-obfuscation avalanche {:.3}", avalanche.score)
            );
        }
        let program_json = serde_json::to_vec(&ir).expect("IR serializes");
        let program_id = format!("program-{}", hash(&[&program_json]));
        let item_id = format!("item-{}", hash(&[program_id.as_bytes(), &input]));
        let (s0, s1, s2) = ir.loop_counts();
        let pointer_changes = run
            .trace
            .windows(2)
            .filter(|w| w[0].pointer != w[1].pointer)
            .count();
        let volatility = if run.trace.len() < 2 {
            0.0
        } else {
            pointer_changes as f64 / (run.trace.len() - 1) as f64
        };
        let working_set = run
            .trace
            .iter()
            .map(|p| p.touched_cell)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        let output = run.state.output.clone();
        let item_profile = crate::ir::execution_profile(&ir, &input, defaults.step_cap)
            .expect("accepted item was coverage-checked");
        let enc = Encodings {
            e0: compiled.e0.clone(),
            e1: compiled.e1.clone(),
            e1_legend: compiled.permutation.clone(),
            e2: e2_source,
            e3: e3_source,
            e4: bytecode.e4_source(),
            e4_residual_unvalidated_risk: true,
        };
        let e0_program_bpe_tokens = tasks::bpe_token_count(&enc.e0, &defaults.prompt_tokenizer)
            .map_err(GenerateError::Tokenizer)?;
        let e2_program_bpe_tokens = tasks::bpe_token_count(&enc.e2, &defaults.prompt_tokenizer)
            .map_err(GenerateError::Tokenizer)?;
        let e3_program_bpe_tokens = tasks::bpe_token_count(&enc.e3, &defaults.prompt_tokenizer)
            .map_err(GenerateError::Tokenizer)?;
        let mut item = BaseItem {
            schema_version: SCHEMA_VERSION.into(),
            program_id,
            item_id,
            seed: item_seed,
            input,
            expected_output: output,
            ir: ir.clone(),
            encodings: enc,
            compiler: compiled.metadata,
            annotations: Annotations {
                grammar_shape: generated.shape.into(),
                program_size_tier: generated.size_tier,
                e0_program_bpe_tokens,
                e2_program_bpe_tokens,
                e3_program_bpe_tokens,
                nesting_depth: ir.nesting_depth(),
                working_set,
                n_steps: run.state.steps,
                semantic_steps,
                trace_semantic_density,
                text_semantic_density,
                prompt_tokenizer: defaults.prompt_tokenizer.clone(),
                e2_e0_prompt_bpe_ratio: 0.0,
                e3_e0_prompt_bpe_ratio: 0.0,
                e2_e0_program_bpe_ratio: e2_program_bpe_tokens as f64
                    / e0_program_bpe_tokens.max(1) as f64,
                e3_e0_program_bpe_ratio: e3_program_bpe_tokens as f64
                    / e0_program_bpe_tokens.max(1) as f64,
                move_carrier: defaults.move_carrier,
                rejection_histogram_before_acceptance: rejection_histogram.clone(),
                minimum_loop_iterations: item_profile.minimum_loop_iterations,
                s0_loops: s0,
                s1_loops: s1,
                s2_loops: s2,
                s1_s2_ratio: if s2 == 0 {
                    None
                } else {
                    Some(s1 as f64 / s2 as f64)
                },
                pointer_volatility: volatility,
                difficulty_band: spec.difficulty,
            },
            oracles: OracleArtifacts {
                full_trace: run.trace,
                avalanche_map: avalanche.records,
                avalanche_score: avalanche.score,
                avalanche_sampling_rate: avalanche.sampling_rate,
                semantic_fingerprint: fingerprint,
                t2_reference_solution: reference.solution,
                t2_reference_solution_tokens_upper_bound: reference.tokens_upper_bound,
                t2_nontriviality_witness: nontriviality_witness,
            },
            reserved_families: vec![Family::T4, Family::T5, Family::T6],
        };
        let prompt_ratios = tasks::ladder_prompt_bpe_ratios(
            &item,
            defaults.t1_probe_count,
            defaults.t2_token_cap,
            defaults.t3_token_cap,
            &defaults.prompt_tokenizer,
        )
        .map_err(GenerateError::Tokenizer)?;
        if prompt_ratios.e2_prompt_over_e0_prompt > defaults.maximum_e2_prompt_bpe_ratio {
            reject!(
                "e2_prompt_bpe_ratio",
                format!(
                    "E2 prompt BPE ratio ({}) E2/E0={:.3}; E3/E0={:.3} is descriptive and controlled by matched pairs",
                    defaults.prompt_tokenizer,
                    prompt_ratios.e2_prompt_over_e0_prompt,
                    prompt_ratios.e3_prompt_over_e0_prompt
                )
            );
        }
        item.annotations.e2_e0_prompt_bpe_ratio = prompt_ratios.e2_prompt_over_e0_prompt;
        item.annotations.e3_e0_prompt_bpe_ratio = prompt_ratios.e3_prompt_over_e0_prompt;
        if !tasks::task_budgets_are_encoding_invariant(
            &item,
            defaults.t1_probe_count,
            defaults.t2_token_cap,
            defaults.t3_token_cap,
        ) {
            reject!(
                "encoding_dependent_task_budgets",
                "T2/T3 response budgets differ across encodings or exceed their item-level limits"
                    .into()
            );
        }
        accepted.push(item);
        accepted_shapes.insert(generated.shape.to_string());
        *shape_counts.entry(generated.shape.to_string()).or_default() += 1;
        attempt += 1;
    }
    if accepted.len() != spec.count {
        return Err(GenerateError::Exhausted {
            attempts: attempt,
            last,
            histogram: rejection_histogram,
        });
    }
    if !tasks::batch_budgets_are_diverse(&accepted, defaults.t2_token_cap, defaults.t3_token_cap) {
        use std::collections::BTreeSet;
        return Err(GenerateError::BatchBudgetDiversity {
            required: accepted.len().min(5),
            t2_distinct: accepted
                .iter()
                .map(|item| tasks::item_t2_token_cap(item, defaults.t2_token_cap))
                .collect::<BTreeSet<_>>()
                .len(),
            t3_distinct: accepted
                .iter()
                .map(|item| tasks::item_t3_token_cap(item, defaults.t3_token_cap))
                .collect::<BTreeSet<_>>()
                .len(),
        });
    }
    if spec.count >= 100 {
        let observed_tiers = accepted
            .iter()
            .map(|item| item.annotations.program_size_tier)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        if observed_tiers < PROGRAM_SIZE_TIERS as usize {
            return Err(GenerateError::BatchSizeLadder {
                required: PROGRAM_SIZE_TIERS as usize,
                observed: observed_tiers,
            });
        }
        for encoding in [EncodingId::E2, EncodingId::E3] {
            let pairs = tasks::matched_t2_prompt_pairs(
                &accepted,
                encoding,
                0.10,
                defaults.t1_probe_count,
                defaults.t2_token_cap,
                defaults.t3_token_cap,
                &defaults.prompt_tokenizer,
            )
            .map_err(GenerateError::Tokenizer)?;
            if pairs.len() < 30 {
                return Err(GenerateError::BatchMatchedPairs {
                    encoding: format!("{encoding:?}"),
                    required: 30,
                    observed: pairs.len(),
                });
            }
        }
    }
    Ok(accepted)
}

pub fn records(items: &[BaseItem], defaults: &Defaults, with_answers: bool) -> Vec<JsonlRecord> {
    let mut out = vec![];
    for item in items {
        if with_answers {
            out.push(JsonlRecord::Item(Box::new(item.clone())));
        } else {
            out.push(JsonlRecord::PublicItemMetadata(Box::new(
                public_item_metadata(item, defaults),
            )));
        }
        out.extend(
            tasks::adapt_all(
                item,
                defaults.t1_probe_count,
                defaults.t2_token_cap,
                defaults.t3_token_cap,
            )
            .into_iter()
            .map(|task| {
                if with_answers {
                    task
                } else {
                    tasks::without_answers(task)
                }
            })
            .map(|task| JsonlRecord::Task(Box::new(task))),
        );
    }
    out
}

fn public_item_metadata(item: &BaseItem, defaults: &Defaults) -> PublicItemMetadata {
    let a = &item.annotations;
    PublicItemMetadata {
        schema_version: "benchfck.public-item.v1".into(),
        program_id: item.program_id.clone(),
        item_id: item.item_id.clone(),
        arity: item.ir.arity,
        output_arity: item.ir.output_arity,
        grammar_shape: a.grammar_shape.clone(),
        program_size_tier: a.program_size_tier,
        e0_program_bpe_tokens: a.e0_program_bpe_tokens,
        e2_program_bpe_tokens: a.e2_program_bpe_tokens,
        e3_program_bpe_tokens: a.e3_program_bpe_tokens,
        difficulty_band: a.difficulty_band,
        nesting_depth: a.nesting_depth,
        working_set: a.working_set,
        n_steps: a.n_steps,
        semantic_steps: a.semantic_steps,
        trace_semantic_density: a.trace_semantic_density,
        text_semantic_density: a.text_semantic_density,
        prompt_tokenizer: a.prompt_tokenizer.clone(),
        e2_e0_prompt_bpe_ratio: a.e2_e0_prompt_bpe_ratio,
        e3_e0_prompt_bpe_ratio: a.e3_e0_prompt_bpe_ratio,
        e2_e0_program_bpe_ratio: a.e2_e0_program_bpe_ratio,
        e3_e0_program_bpe_ratio: a.e3_e0_program_bpe_ratio,
        move_carrier: a.move_carrier,
        minimum_loop_iterations: a.minimum_loop_iterations,
        s0_loops: a.s0_loops,
        s1_loops: a.s1_loops,
        s2_loops: a.s2_loops,
        s1_s2_ratio: a.s1_s2_ratio,
        pointer_volatility: a.pointer_volatility,
        t2_response_token_cap: tasks::item_t2_token_cap(item, defaults.t2_token_cap),
        t3_response_token_cap: tasks::item_t3_token_cap(item, defaults.t3_token_cap),
        available_families: vec![Family::T1, Family::T2, Family::T3],
        available_encodings: vec![
            EncodingId::E0,
            EncodingId::E1,
            EncodingId::E2,
            EncodingId::E3,
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_input_returns_first_seed_rotated_candidate_in_requested_band() {
        let ir = Program {
            arity: 1,
            output_arity: 1,
            variables: vec!["input".into()],
            body: vec![Statement::In { dst: 0 }, Statement::Out { src: 0 }],
        };

        let (input, run) = choose_input(&ir, ",.", 1, DifficultyBand::Easy, 100, 7).unwrap();

        assert_eq!(input, vec![8]);
        assert_eq!(run.state.output, vec![8]);
        assert_eq!(measured_band(run.state.steps), DifficultyBand::Easy);
        assert!(!run.trace.is_empty());
    }

    #[test]
    fn same_program_seed_is_structurally_stable() {
        let a = generated_program(7, 1, 0);
        let b = generated_program(7, 1, 0);
        assert_eq!(a, b);
    }
    #[test]
    fn seed_42_compiler_regression() {
        let raw = generated_program(42, 1, 0);
        let (p, _) = structural_obfuscate(&raw.program, 42);
        let c = compile(&p, 42, LayoutDiscipline::Contiguous).unwrap();
        for x in 0..=8u8 {
            let a = crate::ir::execute(&p, &[x], 1_000_000).unwrap().output;
            let b = crate::bf::execute(&c.e0, &[x], 1_000_000, false)
                .unwrap()
                .state
                .output;
            assert_eq!(
                a, b,
                "input={x}, templates={:?}, IR={:?}",
                c.metadata.statement_templates, p.body
            );
        }
    }
    #[test]
    fn generated_compiler_property_sample() {
        for attempt in 0..16u64 {
            let seed = 42u64.wrapping_add(attempt * 0x9E37_79B9);
            let raw = generated_program(seed, 1, 0);
            let (p, _) = structural_obfuscate(&raw.program, seed);
            let d = match seed % 3 {
                0 => LayoutDiscipline::Contiguous,
                1 => LayoutDiscipline::Interleaved,
                _ => LayoutDiscipline::Strided,
            };
            let c = compile(&p, seed, d).unwrap();
            for x in 0..=32u8 {
                let a = crate::ir::execute(&p, &[x], 1_000_000).unwrap().output;
                let b = crate::bf::execute(&c.e0, &[x], 1_000_000, false)
                    .unwrap()
                    .state
                    .output;
                assert_eq!(
                    a, b,
                    "attempt={attempt}, seed={seed}, input={x}, layout={d:?}, templates={:?}",
                    c.metadata.statement_templates
                );
            }
        }
    }
    #[test]
    fn grammar_spans_eight_non_affine_semantic_shapes() {
        let shapes: std::collections::BTreeSet<_> = (0..32)
            .map(|seed| generated_program(seed, 1, 0).shape)
            .collect();
        assert_eq!(shapes.len(), 8);
        assert!(shapes.iter().all(|shape| !shape.contains("affine")));
    }

    #[test]
    fn promoted_constructors_match_their_selected_closed_forms() {
        for seed in 0..8u64 {
            let generated = generated_program(seed, 1, 0);
            let bias = generated
                .program
                .body
                .iter()
                .find_map(|statement| match statement {
                    Statement::Set { dst: 2, value } => Some(u32::from(*value)),
                    _ => None,
                })
                .unwrap();
            for input in 0..=255u8 {
                let x = u32::from(input);
                let expected = match seed % 8 {
                    0 => {
                        let r = x % 5;
                        let q = x / 5;
                        bias + (r + 1) * q + r * 3 + q * 5
                    }
                    1 => {
                        let r = x % 5;
                        let q = x / 5;
                        bias + r * r * 11 + q * 3 + r
                    }
                    2 => {
                        let r = x % 7;
                        let q = x / 7;
                        bias + r * q * 5 + r * 11 + q * 17
                    }
                    3 => {
                        let r = x % 5;
                        let q = x / 5;
                        bias + r * (4 - r) + q * 3 + r * 3
                    }
                    4 => {
                        let r = x % 5;
                        let q = x / 5;
                        bias + (r + 5) * q + r * 3 + q * 5
                    }
                    5 => {
                        let r = x % 7;
                        let q = x / 7;
                        bias + r * q * 2 + r * 11 + q * 13
                    }
                    6 => {
                        let r = x % 7;
                        let q = x / 7;
                        bias + r * q * 2 + r * 11 + q * 17
                    }
                    _ => {
                        let r = x % 5;
                        let q = x / 5;
                        bias + r * (4 - r) + q + r * 3
                    }
                } as u8;
                let actual = crate::ir::execute(&generated.program, &[input], 1_000_000)
                    .unwrap()
                    .output[0];
                assert_eq!(actual, expected, "shape={} input={input}", generated.shape);
            }
        }
    }

    #[test]
    fn promoted_constructors_occupy_eight_hybrid_safe_semantic_profiles() {
        let mut profiles = std::collections::BTreeSet::new();
        for seed in 0..8u64 {
            let generated = generated_program(seed, 1, 0);
            let values = (0..=255u8)
                .map(|input| {
                    i64::from(
                        crate::ir::execute(&generated.program, &[input], 1_000_000)
                            .unwrap()
                            .output[0],
                    )
                })
                .collect::<Vec<_>>();
            let analytical = tasks::analyze_named_trivial_families(&values, 25);
            assert!(
                analytical.named_families_excluded,
                "shape={} analytical={analytical:?}",
                generated.shape
            );
            profiles.insert(tasks::constructor_semantic_profile(&values, &analytical));
        }
        assert_eq!(profiles.len(), 8, "profiles={profiles:#?}");
    }

    #[test]
    fn promoted_constructors_have_private_reference_search_witnesses() {
        for seed in 0..8u64 {
            let generated = generated_program(seed, 1, 0);
            let (program, _) = structural_obfuscate(&generated.program, seed);
            let compiled = compile(&program, seed, LayoutDiscipline::Contiguous).unwrap();
            let mut hasher = Sha256::new();
            for input in 0..=255u8 {
                let binding = [input];
                let output = crate::ir::execute(&program, &binding, 1_000_000)
                    .unwrap()
                    .output;
                hasher.update((binding.len() as u32).to_le_bytes());
                hasher.update(binding);
                hasher.update((output.len() as u32).to_le_bytes());
                hasher.update(output);
            }
            let target = crate::oracle::SemanticFingerprint {
                algorithm: "sha256_length_delimited_domain_table".into(),
                domain_size: 256,
                digest_hex: crate::lower_hex(&hasher.finalize()),
            };
            let bf = BfProgram::parse(&compiled.e0).unwrap();
            for input in [0u8, 1, 2, 17, 63, 127, 254, 255] {
                let expected = crate::ir::execute(&program, &[input], 1_000_000)
                    .unwrap()
                    .output;
                let actual = bf
                    .execute(&[input], 1_000_000, false)
                    .unwrap_or_else(|error| {
                        panic!("shape={} input={input} BF error={error}", generated.shape)
                    })
                    .state
                    .output;
                assert_eq!(actual, expected, "shape={} input={input}", generated.shape);
            }
            let reference =
                tasks::search_g2_reference_expression(&compiled.e0, 1, &target, 1_000_000);
            assert!(
                reference.is_some(),
                "shape={} had no private reference witness",
                generated.shape
            );
        }
    }

    #[test]
    fn all_size_tiers_preserve_the_complete_arity_one_function() {
        for seed in 0..8u64 {
            let baseline = generated_program(seed, 1, 0).program;
            for tier in 1..PROGRAM_SIZE_TIERS {
                let candidate = generated_program(seed, 1, tier).program;
                assert!(candidate.body.len() > baseline.body.len());
                for input in 0..=255u8 {
                    let expected = crate::ir::execute(&baseline, &[input], 1_000_000)
                        .unwrap()
                        .output;
                    let actual = crate::ir::execute(&candidate, &[input], 1_000_000)
                        .unwrap()
                        .output;
                    assert_eq!(actual, expected, "seed={seed} tier={tier} input={input}");
                }
            }
        }
    }

    #[test]
    fn size_tiers_increase_all_program_bpe_lengths() {
        let tokenizer = "cl100k_base";
        let mut previous: Option<(u64, u64, u64)> = None;
        for tier in 0..PROGRAM_SIZE_TIERS {
            let raw = generated_program(0, 1, tier);
            let compiled = compile(&raw.program, 0, LayoutDiscipline::Contiguous).unwrap();
            let bytecode =
                Bytecode::from_e0_with_carrier(&compiled.e0, crate::backend::MoveCarrier::Rle)
                    .unwrap();
            let lengths = (
                tasks::bpe_token_count(&compiled.e0, tokenizer).unwrap(),
                tasks::bpe_token_count(&bytecode.e2_source(), tokenizer).unwrap(),
                tasks::bpe_token_count(&bytecode.e3_source(), tokenizer).unwrap(),
            );
            println!(
                "tier={tier} e0={} e2={} e3={}",
                lengths.0, lengths.1, lengths.2
            );
            if let Some(previous) = previous {
                assert!(lengths.0 > previous.0, "E0 tier {tier} was not larger");
                assert!(lengths.1 > previous.1, "E2 tier {tier} was not larger");
                assert!(lengths.2 > previous.2, "E3 tier {tier} was not larger");
            }
            previous = Some(lengths);
        }
    }

    #[test]
    fn raw_size_ladder_supports_thirty_disjoint_pairs() {
        #[derive(Clone, Copy)]
        struct Observation {
            tier: u8,
            e0: u64,
            e2: u64,
            e3: u64,
        }
        fn count_pairs(
            observations: &[Observation],
            encoded: fn(Observation) -> u64,
        ) -> (usize, std::collections::BTreeMap<(u8, u8), usize>) {
            let mut candidates = Vec::new();
            for (left_index, left) in observations.iter().copied().enumerate() {
                for (right_index, right) in observations.iter().copied().enumerate() {
                    if left.tier <= right.tier {
                        continue;
                    }
                    let right_tokens = encoded(right);
                    let gap = left.e0.abs_diff(right_tokens) as f64
                        / left.e0.max(right_tokens).max(1) as f64;
                    if gap <= 0.10 {
                        candidates.push((gap, left_index, right_index));
                    }
                }
            }
            candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut used = std::collections::HashSet::new();
            let mut count = 0;
            let mut tier_pairs = std::collections::BTreeMap::new();
            for (_, left, right) in candidates {
                if used.insert(left) {
                    if used.insert(right) {
                        count += 1;
                        *tier_pairs
                            .entry((observations[left].tier, observations[right].tier))
                            .or_default() += 1;
                    } else {
                        used.remove(&left);
                    }
                }
            }
            (count, tier_pairs)
        }

        let mut observations = Vec::new();
        for index in 0..120u64 {
            let tier = (index % PROGRAM_SIZE_TIERS as u64) as u8;
            let seed = 42u64.wrapping_add(index * 0x9E37_79B9);
            let raw = generated_program(seed, 1, tier);
            let discipline = match seed % 3 {
                0 => LayoutDiscipline::Contiguous,
                1 => LayoutDiscipline::Interleaved,
                _ => LayoutDiscipline::Strided,
            };
            let compiled = compile(&raw.program, seed, discipline).unwrap();
            let bytecode =
                Bytecode::from_e0_with_carrier(&compiled.e0, crate::backend::MoveCarrier::Rle)
                    .unwrap();
            observations.push(Observation {
                tier,
                e0: tasks::bpe_token_count(&compiled.e0, "cl100k_base").unwrap(),
                e2: tasks::bpe_token_count(&bytecode.e2_source(), "cl100k_base").unwrap(),
                e3: tasks::bpe_token_count(&bytecode.e3_source(), "cl100k_base").unwrap(),
            });
        }
        let (e2_pairs, e2_tiers) = count_pairs(&observations, |observation| observation.e2);
        let (e3_pairs, e3_tiers) = count_pairs(&observations, |observation| observation.e3);
        println!(
            "raw_token_matched_pairs e2={e2_pairs} e3={e3_pairs} e2_tiers={e2_tiers:?} e3_tiers={e3_tiers:?}"
        );
        assert!(e2_pairs >= 30, "only {e2_pairs} raw E0/E2 pairs");
        assert!(e3_pairs >= 30, "only {e3_pairs} raw E0/E3 pairs");
    }

    #[test]
    #[ignore = "manual profiling of the hybrid nontriviality gate"]
    fn diagnose_constructor_families_against_hybrid_gate() {
        let mut rejected = 0;
        for seed in 0..8 {
            let raw = generated_program(seed, 1, 0);
            let values = (0..=255u8)
                .map(|input| {
                    crate::ir::execute(&raw.program, &[input], 1_000_000)
                        .unwrap()
                        .output[0] as i64
                })
                .collect::<Vec<_>>();
            let analytical = tasks::analyze_named_trivial_families(&values, 25);
            println!("shape={} analytical={analytical:?}", raw.shape);
            rejected += usize::from(!analytical.named_families_excluded);
        }
        println!("analytically_rejected={rejected}/8");
        assert!(rejected > 0);
    }

    #[test]
    fn generated_e0_text_density_meets_the_validity_floor() {
        let mut violations = Vec::new();
        for seed in 0..8 {
            let raw = generated_program(seed, 1, 0);
            for discipline in [
                LayoutDiscipline::Contiguous,
                LayoutDiscipline::Interleaved,
                LayoutDiscipline::Strided,
                LayoutDiscipline::HeldOut,
            ] {
                let c = compile(&raw.program, seed, discipline).unwrap();
                let ops =
                    c.e0.chars()
                        .filter(|ch| "+-<>[],.".contains(*ch))
                        .collect::<Vec<_>>();
                let density = ops.iter().filter(|ch| !matches!(ch, '>' | '<')).count() as f64
                    / ops.len() as f64;
                let semantic = ops.iter().filter(|ch| !matches!(ch, '>' | '<')).count();
                if density < 0.35 {
                    violations.push(format!(
                        "seed={seed} layout={discipline:?} density={density:.3} semantic={semantic} total={}",
                        ops.len()
                    ));
                }
            }
        }
        assert!(violations.is_empty(), "{}", violations.join("\n"));
    }
}
