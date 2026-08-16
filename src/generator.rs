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

/// One deterministic candidate evaluation, recorded whether or not the
/// candidate was accepted. Emitted only by `generate_traced`; the acceptance
/// gates themselves are untouched, so a probe run and a batch run exercise the
/// exact same code path and cannot drift apart.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CandidateOutcome {
    pub attempt: usize,
    pub item_seed: u64,
    pub grammar_shape: String,
    pub requested_size_tier: u8,
    pub difficulty_band: DifficultyBand,
    pub accepted: bool,
    pub rejection_category: Option<String>,
    pub rejection_detail: Option<String>,
    pub elapsed_ms: u64,
    pub n_steps: Option<u64>,
    pub text_semantic_density: Option<f64>,
    pub trace_semantic_density: Option<f64>,
    pub proven_exhaustive_ast_depth: Option<u8>,
    pub avalanche_score: Option<f64>,
    pub e2_e0_prompt_bpe_ratio: Option<f64>,
    pub e3_e0_prompt_bpe_ratio: Option<f64>,
    pub item_id: Option<String>,
}

/// Stable inventory for release rejection reports. A candidate records only
/// its first failing gate, so zero hits means "not a first failure in this
/// sample", not that the gate was removed or never evaluated.
pub const REJECTION_CATEGORIES: &[&str] = &[
    "size_tier_cell_quota",
    "semantic_class_quota",
    "off_idiom_rate",
    "input_selection",
    "difficulty_band",
    "trace_semantic_density",
    "per_argument_sensitivity",
    "oracle_execution",
    "worst_case_preflight",
    "cross_backend_domain",
    "duplicate_semantic_fingerprint",
    "nontriviality_enumerator_error",
    "short_expression_match_within_enumerated_layer",
    "analytical_triviality_family_match",
    "insufficient_proven_exhaustive_ast_depth",
    "reference_expression_not_found",
    "avalanche",
    "encoding_dependent_task_budgets",
];

impl CandidateOutcome {
    fn new(
        attempt: usize,
        item_seed: u64,
        grammar_shape: &str,
        requested_size_tier: u8,
        difficulty_band: DifficultyBand,
    ) -> Self {
        Self {
            attempt,
            item_seed,
            grammar_shape: grammar_shape.to_string(),
            requested_size_tier,
            difficulty_band,
            accepted: false,
            rejection_category: None,
            rejection_detail: None,
            elapsed_ms: 0,
            n_steps: None,
            text_semantic_density: None,
            trace_semantic_density: None,
            proven_exhaustive_ast_depth: None,
            avalanche_score: None,
            e2_e0_prompt_bpe_ratio: None,
            e3_e0_prompt_bpe_ratio: None,
            item_id: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildSpec {
    pub seed: u64,
    pub count: usize,
    pub difficulty: DifficultyBand,
    pub arity: u8,
    pub held_out: bool,
    /// Candidate budget. `None` keeps the historical `count * 64` allowance.
    /// A probe run sets this explicitly so the batch constraints (shape quota,
    /// size-tier rotation) stay identical while the run is bounded.
    pub max_attempts: Option<usize>,
    /// D31: items admitted per (semantic class, size tier) cell. `None` derives
    /// it from the *nominal* 8x10 grid, which under-counts capacity whenever
    /// some tiers are unreachable: with only two reachable tiers the nominal
    /// cap of `ceil(count / 64)` silently limits a 100-item request to 32.
    /// The class share ceiling is enforced separately and is the validity
    /// constraint; this cap is only a spreading heuristic and is recorded.
    pub max_items_per_cell: Option<usize>,
}

pub const PROGRAM_SIZE_TIERS: u8 = 10;
pub const PROMOTED_SEMANTIC_PROFILES: usize = 8;
const CONSTRUCTOR_VARIABLES: usize = 9;
/// D37: the ladder is geometric so that every tier token-matches a lower tier
/// under both encoding factors, making matched pairs a property of the schedule
/// rather than a lucky alignment.
///
/// The ratio is fitted to *measured* renderings, not to a nominal factor:
///
/// ```text
/// E0 = 270 + 50.97 * rounds        E2 = 5.55 * E0 - 165      E3 = 11.29 * E0 - 118
/// ```
///
/// A first attempt used `rho = 5.22^(1/4) = 1.5115`, derived from a nominal E3
/// factor of 11.2. The measured factor is 10.85 at the bottom of the ladder, and
/// the resulting `rho^6 = 11.93` pushed the E3 alignment at tier 6 to a 10.07%
/// gap - outside the 10% band by seven hundredths of a percent. Fitting `rho`
/// jointly against both affine models instead gives `rho = 1.492`, which lands
/// every alignment comfortably inside the band.
///
/// | tier | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 |
/// |---|---|---|---|---|---|---|---|---|---|---|
/// | E0 | 270 | 423 | 576 | 882 | 1340 | 2003 | 2971 | 4450 | 6641 | 9903 |
///
/// Alignments: E0 tier `i` matches E2 tier `i-4` and E3 tier `i-6`, giving six
/// E2 and four E3 tier pairs.
const SIZE_TIER_WORK_ROUNDS: [usize; PROGRAM_SIZE_TIERS as usize] =
    [0, 3, 6, 12, 21, 34, 53, 82, 125, 189];

/// D36: encodings are assigned per size tier, not uniformly.
///
/// Rendering every tier in every encoding is impossible: E3 costs 11.2x its E0,
/// so the top tier's E3 prompt would exceed 80k tokens. But the matched-pair
/// design never needs it — pairs draw the E0 side from the *upper* tiers and the
/// E2/E3 side from the *lower* ones, which is exactly what the pair table
/// already shows. Capping each encoding at the tier where its prompt stops being
/// usable therefore costs nothing the analysis wanted, and it lets the ladder
/// grow far enough for E3 pairs to exist at all.
///
/// This creates a deliberate dependency between size tier and available
/// encoding. It must be declared in `VALIDITY.md` and carried into the analysis:
/// an encoding contrast is only ever estimated inside the tier band where both
/// sides are rendered.
/// E2 is rendered while its prompt stays comparable to the largest E0 prompt
/// (tier 5 renders at ~11.2k tokens); E3 while it stays under the same ceiling
/// (tier 3 renders at ~10.6k). Both caps sit at the highest tier the matched-pair
/// chains actually draw from, so capping costs no pair.
pub const HIGHEST_TIER_RENDERED_AS_E2: u8 = 5;
pub const HIGHEST_TIER_RENDERED_AS_E3: u8 = 3;

/// Whether `encoding` is rendered as a task at `size_tier`. E0 and E1 are always
/// rendered; they are the reference arm and its permutation control.
pub fn tier_renders_encoding(size_tier: u8, encoding: EncodingId) -> bool {
    match encoding {
        EncodingId::E0 | EncodingId::E1 => true,
        EncodingId::E2 => size_tier <= HIGHEST_TIER_RENDERED_AS_E2,
        EncodingId::E3 | EncodingId::E4 => size_tier <= HIGHEST_TIER_RENDERED_AS_E3,
    }
}

#[derive(Debug, Error)]
pub enum GenerateError {
    #[error("arity {requested} is outside the configured range 1..={maximum}")]
    BadArity { requested: u8, maximum: u8 },
    #[error("compile failed: {0}")]
    Compile(#[from] crate::compiler::CompileError),
    #[error("explicit backend failed: {0}")]
    Backend(#[from] crate::backend::BackendError),
    #[error("oracle failed: {0}")]
    Oracle(#[from] crate::oracle::OracleError),
    #[error("tokenizer failed: {0}")]
    Tokenizer(String),
    #[error("constructor provider contract failed: {0}")]
    ConstructorProvider(String),
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
        "batch semantic-profile coverage is incomplete: provider declared {required}, observed {observed}"
    )]
    BatchSemanticProfiles { required: usize, observed: usize },
    #[error(
        "batch has too few disjoint token-matched T2 pairs for {encoding}: required {required}, observed {observed}"
    )]
    BatchMatchedPairs {
        encoding: String,
        required: usize,
        observed: usize,
    },
}

/// One provider-produced candidate before obfuscation and acceptance checks.
///
/// Private implementations should use an opaque epoch-local `semantic_class`
/// label; constructor names or source must never be copied into public output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstructorCase {
    pub program: Program,
    pub semantic_class: String,
    pub size_tier: u8,
}

/// Candidate source boundary for public development constructors or an
/// ignored, separately compiled private constructor crate.
///
/// Implementations provide typed IR only. Every acceptance and evidence gate
/// remains in this module's single generation pipeline.
pub trait ConstructorProvider {
    /// Number of semantic-class labels the provider intends to populate.
    fn semantic_profile_count(&self) -> usize;

    /// Deterministically build one candidate for the supplied schedule cell.
    fn build(&self, seed: u64, arity: u8, size_tier: u8) -> Result<ConstructorCase, String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PublicConstructorProvider;

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
fn generated_program(seed: u64, arity: u8, size_tier: u8) -> ConstructorCase {
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
    // D35: work rounds reuse two fixed scratch cells instead of allocating a
    // fresh pair per round. The rounds are semantically neutral (+17*src then
    // -17*src), so which cells they use cannot change the computed function.
    // Allocating a fresh pair per round grew the live tape span linearly with
    // the tier - 41 rounds meant 82 extra cells - and every statement then paid
    // that span in pointer travel. That one choice produced four separate
    // walls: falling source-text density, falling trace density, worst-case
    // preflight failures, and step-cap aborts. With fixed scratch the program
    // text still grows linearly with the tier while span, movement fraction and
    // step count stay flat.
    const WORK_SCRATCH_A: usize = CONSTRUCTOR_VARIABLES;
    const WORK_SCRATCH_B: usize = CONSTRUCTOR_VARIABLES + 1;
    let work_rounds = SIZE_TIER_WORK_ROUNDS[size_tier as usize];
    for round in 0..work_rounds {
        let src = round % arity as usize;
        let factor = 17;
        body.extend([
            Statement::Copy {
                dst: WORK_SCRATCH_A,
                src,
            },
            Statement::Copy {
                dst: WORK_SCRATCH_B,
                src,
            },
            Statement::DrainScaled {
                dst: 2,
                src: WORK_SCRATCH_A,
                factor,
                subtract: false,
            },
            Statement::DrainScaled {
                dst: 2,
                src: WORK_SCRATCH_B,
                factor,
                subtract: true,
            },
        ]);
    }
    let declared_variables = CONSTRUCTOR_VARIABLES + if work_rounds > 0 { 2 } else { 0 };
    if arity == 2 {
        add_scaled(&mut body, 2, 1, 197 + (seed as usize % 3) * 6, false);
    }
    body.push(Statement::Out { src: 2 });
    ConstructorCase {
        program: Program {
            arity,
            output_arity: 1,
            variables: (0..declared_variables).map(|i| format!("v{i}")).collect(),
            body,
        },
        semantic_class: shape.into(),
        size_tier,
    }
}

impl ConstructorProvider for PublicConstructorProvider {
    fn semantic_profile_count(&self) -> usize {
        PROMOTED_SEMANTIC_PROFILES
    }

    fn build(&self, seed: u64, arity: u8, size_tier: u8) -> Result<ConstructorCase, String> {
        Ok(generated_program(seed, arity, size_tier))
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

fn validate_constructor_case(
    case: &ConstructorCase,
    requested_arity: u8,
    requested_size_tier: u8,
) -> Result<(), GenerateError> {
    let label = case.semantic_class.as_bytes();
    if label.is_empty()
        || label.len() > 64
        || !label[0].is_ascii_alphanumeric()
        || !label
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(byte))
    {
        return Err(GenerateError::ConstructorProvider(
            "semantic_class must be a 1..=64 character lowercase opaque label using [a-z0-9._-]"
                .into(),
        ));
    }
    if case.size_tier != requested_size_tier {
        return Err(GenerateError::ConstructorProvider(format!(
            "provider returned size tier {} for requested tier {requested_size_tier}",
            case.size_tier
        )));
    }
    if case.program.arity != requested_arity {
        return Err(GenerateError::ConstructorProvider(format!(
            "provider returned arity {} for requested arity {requested_arity}",
            case.program.arity
        )));
    }
    if case.program.output_arity != 1 {
        return Err(GenerateError::ConstructorProvider(format!(
            "provider returned output arity {}; benchfck v0.4 requires exactly one output",
            case.program.output_arity
        )));
    }
    Ok(())
}

pub fn generate(spec: &BuildSpec, defaults: &Defaults) -> Result<Vec<BaseItem>, GenerateError> {
    generate_with_provider(spec, defaults, &PublicConstructorProvider)
}

/// Run the exact production acceptance pipeline with an injected candidate
/// provider. Private provider source can live in an ignored external crate;
/// no alternate verifier or acceptance path is introduced.
pub fn generate_with_provider(
    spec: &BuildSpec,
    defaults: &Defaults,
    provider: &dyn ConstructorProvider,
) -> Result<Vec<BaseItem>, GenerateError> {
    generate_traced_with_provider(spec, defaults, provider).0
}

/// Same acceptance pipeline as [`generate`], additionally returning one
/// [`CandidateOutcome`] per evaluated candidate. The trace is returned even
/// when generation fails, which is the whole point of a probe run.
pub fn generate_traced(
    spec: &BuildSpec,
    defaults: &Defaults,
) -> (Result<Vec<BaseItem>, GenerateError>, Vec<CandidateOutcome>) {
    generate_traced_with_provider(spec, defaults, &PublicConstructorProvider)
}

pub fn generate_traced_with_provider(
    spec: &BuildSpec,
    defaults: &Defaults,
    provider: &dyn ConstructorProvider,
) -> (Result<Vec<BaseItem>, GenerateError>, Vec<CandidateOutcome>) {
    let mut trace = Vec::new();
    let result = generate_inner(spec, defaults, provider, &mut trace);
    (result, trace)
}

fn generate_inner(
    spec: &BuildSpec,
    defaults: &Defaults,
    provider: &dyn ConstructorProvider,
    trace: &mut Vec<CandidateOutcome>,
) -> Result<Vec<BaseItem>, GenerateError> {
    if spec.arity == 0 || spec.arity > defaults.max_arity {
        return Err(GenerateError::BadArity {
            requested: spec.arity,
            maximum: defaults.max_arity,
        });
    }
    let semantic_profile_count = provider.semantic_profile_count();
    let cells = semantic_profile_count
        .checked_mul(PROGRAM_SIZE_TIERS as usize)
        .filter(|cells| *cells > 0)
        .ok_or_else(|| {
            GenerateError::ConstructorProvider(
                "semantic_profile_count must be positive and must not overflow the tier grid"
                    .into(),
            )
        })?;
    let mut accepted = vec![];
    let mut attempt = 0;
    let max_attempts = spec.max_attempts.unwrap_or(spec.count.max(1) * 64);
    let mut last = "none".to_string();
    // D30: stratification is keyed on the (semantic class, size tier) cell, not
    // on the class alone. Keying on the class made the size ladder unusable:
    // once a class was accepted at one tier it could never be accepted at
    // another, which is exactly what the ladder needs. A per-cell cap also
    // avoids the "cover every cell before repeating" rule, which deadlocks
    // whenever a cell is unreachable — the same failure mode as D29.
    let mut cell_counts = std::collections::BTreeMap::<(String, u8), usize>::new();
    let mut shape_counts = std::collections::BTreeMap::<String, usize>::new();
    let mut accepted_fingerprints = std::collections::BTreeSet::<(u64, String)>::new();
    let max_per_cell = spec
        .max_items_per_cell
        .unwrap_or_else(|| spec.count.div_ceil(cells))
        .max(1);
    let max_per_shape = spec.count.div_ceil(4).max(1);
    let mut rejection_histogram = std::collections::BTreeMap::<String, usize>::new();
    // Declared before `reject!` so the macro body can see them: `macro_rules!`
    // bodies resolve local identifiers at the definition site, not at the
    // invocation site. Both are assigned at the top of every iteration.
    let mut probe: CandidateOutcome;
    let mut candidate_started: std::time::Instant;
    macro_rules! reject {
        ($category:expr, $reason:expr) => {{
            last = $reason;
            probe.rejection_category = Some($category.to_string());
            probe.rejection_detail = Some(last.clone());
            probe.elapsed_ms = candidate_started.elapsed().as_millis() as u64;
            trace.push(probe.clone());
            *rejection_histogram.entry($category.into()).or_default() += 1;
            attempt += 1;
            continue;
        }};
    }
    while accepted.len() < spec.count && attempt < max_attempts {
        candidate_started = std::time::Instant::now();
        let item_seed = spec.seed.wrapping_add(attempt as u64 * 0x9E37_79B9);
        // D29: the size tier must not depend on how many items were already
        // accepted. Deriving it from `accepted.len()` deadlocks the ladder — a
        // tier that cannot be accepted is never even generated, so tiers above
        // the first blocked one are never sampled. Deriving both the shape and
        // the tier from the attempt index sweeps the full shape x tier grid.
        let requested_size_tier =
            ((attempt / PROGRAM_SIZE_TIERS as usize) % PROGRAM_SIZE_TIERS as usize) as u8;
        let generated = provider
            .build(item_seed, spec.arity, requested_size_tier)
            .map_err(GenerateError::ConstructorProvider)?;
        validate_constructor_case(&generated, spec.arity, requested_size_tier)?;
        probe = CandidateOutcome::new(
            attempt,
            item_seed,
            &generated.semantic_class,
            requested_size_tier,
            spec.difficulty,
        );
        let cell = (generated.semantic_class.clone(), requested_size_tier);
        if cell_counts.get(&cell).copied().unwrap_or(0) >= max_per_cell {
            reject!(
                "size_tier_cell_quota",
                format!(
                    "cell ({}, tier {requested_size_tier}) already holds {max_per_cell}",
                    generated.semantic_class
                )
            );
        }
        if shape_counts
            .get(&generated.semantic_class)
            .copied()
            .unwrap_or(0)
            >= max_per_shape
        {
            reject!(
                "semantic_class_quota",
                format!(
                    "semantic class {} already holds {max_per_shape} of {}",
                    generated.semantic_class, spec.count
                )
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
        // D32: source-text semantic density is a covariate, not a gate. Measured
        // decay is monotone in program size (0.46 at tier 0 down to 0.076 at
        // tier 7), so any floor selects on length. A floor at 0.35 admitted only
        // tiers 0-1, whose E0 token range [226, 466] is disjoint from the E2
        // range [1102, 2182]. With no overlap the encoding effect is not
        // identified by matched pairs *or* by regression, so the floor did not
        // make H1 stricter — it made H1 unanswerable. Density is recorded per
        // item and must be carried as a control in the size-tier analysis,
        // because tier and movement fraction are now collinear by construction.
        probe.text_semantic_density = Some(text_semantic_density);
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
        probe.n_steps = Some(run.state.steps);
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
        probe.trace_semantic_density = Some(trace_semantic_density);
        if trace_semantic_density < defaults.minimum_trace_semantic_density {
            reject!(
                "trace_semantic_density",
                format!("trace semantic density {trace_semantic_density:.3}")
            );
        }
        // D34: an oracle that cannot finish within the step cap is a property of
        // the candidate, not a failure of the run. Propagating it with `?`
        // aborted a 800-candidate probe at candidate 545 and would abort any
        // batch the same way. Oracle execution failures are rejections.
        match oracle::each_argument_sensitive(&compiled.e0, &input, defaults.step_cap) {
            Ok(true) => {}
            Ok(false) => reject!(
                "per_argument_sensitivity",
                "per-argument input sensitivity".into()
            ),
            Err(error) => reject!("oracle_execution", format!("argument sensitivity: {error}")),
        }
        let worst_case = vec![255; ir.arity as usize];
        if let Err(error) = BfProgram::parse(&compiled.e0)
            .expect("compiler emits syntactically valid E0")
            .execute(&worst_case, defaults.step_cap, false)
        {
            reject!(
                "worst_case_preflight",
                format!(
                    "worst-case preflight for {}: {error}",
                    generated.semantic_class
                )
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
        // D46: exact complete-domain semantic duplicates are never admitted.
        // IR diversity is insufficient because two different tiers/layouts can
        // still implement the same total function and overweight one item in
        // downstream statistics. The fingerprint is already paid for by the
        // cross-backend gate, so this adds no parallel oracle or approximation.
        let fingerprint_key = (fingerprint.domain_size, fingerprint.digest_hex.clone());
        if accepted_fingerprints.contains(&fingerprint_key) {
            reject!(
                "duplicate_semantic_fingerprint",
                format!(
                    "complete-domain semantic function already accepted: {}",
                    fingerprint.digest_hex
                )
            );
        }
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
        probe.proven_exhaustive_ast_depth = Some(nontriviality_witness.proven_exhaustive_ast_depth);
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
        let avalanche = match oracle::avalanche_map(
            &compiled.e0,
            &input,
            defaults.step_cap,
            defaults.minimum_avalanche_positions,
            item_seed,
        ) {
            Ok(map) => map,
            Err(error) => reject!("oracle_execution", format!("avalanche: {error}")),
        };
        probe.avalanche_score = Some(avalanche.score);
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
                grammar_shape: generated.semantic_class.clone(),
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
                // D36: `None` where the tier does not render that encoding.
                e2_e0_prompt_bpe_ratio: None,
                e3_e0_prompt_bpe_ratio: None,
                e2_e0_program_bpe_ratio: tier_renders_encoding(generated.size_tier, EncodingId::E2)
                    .then(|| e2_program_bpe_tokens as f64 / e0_program_bpe_tokens.max(1) as f64),
                e3_e0_program_bpe_ratio: tier_renders_encoding(generated.size_tier, EncodingId::E3)
                    .then(|| e3_program_bpe_tokens as f64 / e0_program_bpe_tokens.max(1) as f64),
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
        // D28: neither E2/E0 nor E3/E0 is an acceptance gate. The ratio gate was
        // designed when H1 was to be tested by raw comparison; D15 replaced that
        // with token-matched pairs, which *control* the confound instead of
        // shrinking it. Keeping a gate on E2 while exempting E3 (D26) was also
        // inconsistent. Both ratios are recorded as mandatory covariates and are
        // reported, never enforced.
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
        probe.accepted = true;
        probe.item_id = Some(item.item_id.clone());
        probe.e2_e0_prompt_bpe_ratio = item.annotations.e2_e0_prompt_bpe_ratio;
        probe.e3_e0_prompt_bpe_ratio = item.annotations.e3_e0_prompt_bpe_ratio;
        probe.elapsed_ms = candidate_started.elapsed().as_millis() as u64;
        trace.push(probe.clone());
        accepted_fingerprints.insert(fingerprint_key);
        accepted.push(item);
        *cell_counts.entry(cell).or_default() += 1;
        *shape_counts
            .entry(generated.semantic_class.clone())
            .or_default() += 1;
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
        if shape_counts.len() != semantic_profile_count {
            return Err(GenerateError::BatchSemanticProfiles {
                required: semantic_profile_count,
                observed: shape_counts.len(),
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
        available_encodings: [
            EncodingId::E0,
            EncodingId::E1,
            EncodingId::E2,
            EncodingId::E3,
        ]
        .into_iter()
        .filter(|encoding| tier_renders_encoding(a.program_size_tier, *encoding))
        .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ContractProvider {
        profiles: usize,
        label: &'static str,
    }

    impl ConstructorProvider for ContractProvider {
        fn semantic_profile_count(&self) -> usize {
            self.profiles
        }

        fn build(&self, _seed: u64, arity: u8, size_tier: u8) -> Result<ConstructorCase, String> {
            Ok(ConstructorCase {
                program: Program {
                    arity,
                    output_arity: 1,
                    variables: vec!["input".into()],
                    body: vec![Statement::In { dst: 0 }, Statement::Out { src: 0 }],
                },
                semantic_class: self.label.into(),
                size_tier,
            })
        }
    }

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
    fn public_provider_is_the_existing_deterministic_constructor_source() {
        assert_eq!(
            PublicConstructorProvider.build(42, 1, 7).unwrap(),
            generated_program(42, 1, 7)
        );
    }

    #[test]
    fn provider_contract_fails_before_acceptance_work() {
        let defaults = Defaults::load("config/defaults.toml").expect("defaults load");
        let spec = BuildSpec {
            seed: 42,
            count: 1,
            difficulty: DifficultyBand::Easy,
            arity: 1,
            held_out: false,
            max_attempts: Some(1),
            max_items_per_cell: None,
        };
        let (result, trace) = generate_traced_with_provider(
            &spec,
            &defaults,
            &ContractProvider {
                profiles: 0,
                label: "private-01",
            },
        );
        assert!(matches!(result, Err(GenerateError::ConstructorProvider(_))));
        assert!(trace.is_empty());

        let (result, trace) = generate_traced_with_provider(
            &spec,
            &defaults,
            &ContractProvider {
                profiles: 1,
                label: "private constructor name",
            },
        );
        assert!(matches!(result, Err(GenerateError::ConstructorProvider(_))));
        assert!(trace.is_empty());
    }

    #[test]
    fn provider_case_must_match_requested_arity_tier_and_output_contract() {
        let mut case = ContractProvider {
            profiles: 1,
            label: "private-01",
        }
        .build(42, 1, 3)
        .unwrap();
        assert!(validate_constructor_case(&case, 1, 3).is_ok());
        case.size_tier = 4;
        assert!(validate_constructor_case(&case, 1, 3).is_err());
        case.size_tier = 3;
        case.program.arity = 2;
        assert!(validate_constructor_case(&case, 1, 3).is_err());
        case.program.arity = 1;
        case.program.output_arity = 2;
        assert!(validate_constructor_case(&case, 1, 3).is_err());
    }

    /// D29 regression. The size tier must be a function of the attempt index,
    /// never of how many items were already accepted: an acceptance-driven tier
    /// deadlocks the ladder, because a tier that cannot pass the gates is never
    /// generated again and every tier above it is never generated at all.
    #[test]
    fn size_tier_sweeps_the_grid_independently_of_acceptance() {
        let tiers: Vec<u8> = (0..(PROGRAM_SIZE_TIERS as usize * PROGRAM_SIZE_TIERS as usize))
            .map(|attempt| {
                ((attempt / PROGRAM_SIZE_TIERS as usize) % PROGRAM_SIZE_TIERS as usize) as u8
            })
            .collect();
        let distinct: std::collections::BTreeSet<_> = tiers.iter().copied().collect();
        assert_eq!(
            distinct.len(),
            PROGRAM_SIZE_TIERS as usize,
            "one full shape x tier sweep must visit every size tier"
        );
        for tier in 0..PROGRAM_SIZE_TIERS {
            assert_eq!(
                tiers.iter().filter(|value| **value == tier).count(),
                PROGRAM_SIZE_TIERS as usize,
                "tier {tier} is not sampled uniformly across the sweep"
            );
        }
    }

    #[test]
    fn probe_records_every_candidate_and_respects_its_budget() {
        let defaults = Defaults::load("config/defaults.toml").expect("defaults load");
        let (result, trace) = generate_traced(
            &BuildSpec {
                seed: 42,
                count: 100,
                difficulty: DifficultyBand::Easy,
                arity: 1,
                held_out: false,
                max_attempts: Some(6),
                max_items_per_cell: None,
            },
            &defaults,
        );
        assert!(
            trace.len() <= 6,
            "probe exceeded its candidate budget: {}",
            trace.len()
        );
        assert!(!trace.is_empty(), "probe recorded no candidates");
        for outcome in &trace {
            assert_eq!(
                outcome.accepted,
                outcome.rejection_category.is_none(),
                "every candidate is either accepted or categorised, never both or neither"
            );
            assert!(outcome.accepted == outcome.item_id.is_some());
        }
        assert!(
            result.is_err(),
            "a 6-candidate budget cannot complete a 100-item batch"
        );
    }

    #[test]
    fn release_defaults_reject_arity_two_before_candidate_work() {
        let defaults = Defaults::load("config/defaults.toml").expect("defaults load");
        let (result, trace) = generate_traced(
            &BuildSpec {
                seed: 42,
                count: 1,
                difficulty: DifficultyBand::Easy,
                arity: 2,
                held_out: false,
                max_attempts: Some(1),
                max_items_per_cell: None,
            },
            &defaults,
        );
        assert!(matches!(
            result,
            Err(GenerateError::BadArity {
                requested: 2,
                maximum: 1
            })
        ));
        assert!(trace.is_empty());
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
            .map(|seed| generated_program(seed, 1, 0).semantic_class)
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
                assert_eq!(
                    actual, expected,
                    "shape={} input={input}",
                    generated.semantic_class
                );
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
                generated.semantic_class
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
                        panic!(
                            "shape={} input={input} BF error={error}",
                            generated.semantic_class
                        )
                    })
                    .state
                    .output;
                assert_eq!(
                    actual, expected,
                    "shape={} input={input}",
                    generated.semantic_class
                );
            }
            let reference =
                tasks::search_g2_reference_expression(&compiled.e0, 1, &target, 1_000_000);
            assert!(
                reference.is_some(),
                "shape={} had no private reference witness",
                generated.semantic_class
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
            println!("shape={} analytical={analytical:?}", raw.semantic_class);
            rejected += usize::from(!analytical.named_families_excluded);
        }
        println!("analytically_rejected={rejected}/8");
        assert!(rejected > 0);
    }

    #[test]
    /// D32 + D35 regression, and the falsifiable prediction D35 was approved on.
    ///
    /// D32 retired the source-text density floor after measuring a monotone
    /// collapse with program size (0.463 at tier 0 to 0.076 at tier 7). D35
    /// diagnosed that collapse as a consequence of the ladder allocating two
    /// fresh cells per work round, which grew the live tape span and made every
    /// statement pay it in pointer travel. With fixed scratch cells the density
    /// must stay flat across the ladder. If this assertion ever fails, the D35
    /// diagnosis was wrong and the size/movement confound is intrinsic rather
    /// than an artefact of variable allocation.
    fn fixed_work_scratch_keeps_text_density_flat_across_the_ladder() {
        let mut by_tier = Vec::new();
        for tier in 0..PROGRAM_SIZE_TIERS {
            let mut sum = 0.0;
            for seed in 0..4u64 {
                let raw = generated_program(seed, 1, tier);
                let compiled = compile(&raw.program, seed, LayoutDiscipline::Contiguous).unwrap();
                let ops = compiled
                    .e0
                    .chars()
                    .filter(|ch| "+-<>[],.".contains(*ch))
                    .collect::<Vec<_>>();
                sum += ops.iter().filter(|ch| !matches!(ch, '>' | '<')).count() as f64
                    / ops.len().max(1) as f64;
            }
            by_tier.push(sum / 4.0);
        }
        let first = by_tier[0];
        let last = by_tier[PROGRAM_SIZE_TIERS as usize - 1];
        assert!(
            last >= first * 0.5,
            "density must not collapse across the ladder; tier 0 = {first:.3}, \
             tier 7 = {last:.3}, observed {by_tier:?}"
        );
        assert!(
            by_tier.iter().all(|d| *d > 0.0),
            "every tier must contain semantic operations; observed {by_tier:?}"
        );
    }

    #[test]
    /// D37 regression. The ladder's whole purpose is that E0 token counts chain
    /// under the measured encoding factors, so that token-matched pairs exist by
    /// construction rather than by coincidence. This asserts the chain directly
    /// on measured BPE counts: at least four tiers must match an E2 rendering
    /// and at least three must match an E3 rendering, within the same 10% band
    /// the pair table uses. If the compiler, the renderers or the schedule ever
    /// change the factors, this fails before a batch is ever generated.
    fn ladder_tiers_chain_under_the_measured_encoding_factors() {
        let defaults = Defaults::load("config/defaults.toml").expect("defaults load");
        let mut e0 = Vec::new();
        let mut e2 = Vec::new();
        let mut e3 = Vec::new();
        for tier in 0..PROGRAM_SIZE_TIERS {
            let raw = generated_program(0, 1, tier);
            let compiled = compile(&raw.program, 0, LayoutDiscipline::Contiguous).unwrap();
            let bytecode =
                Bytecode::from_e0_with_carrier(&compiled.e0, defaults.move_carrier).unwrap();
            e0.push(tasks::bpe_token_count(&compiled.e0, &defaults.prompt_tokenizer).unwrap());
            e2.push(
                tasks::bpe_token_count(&bytecode.e2_source(), &defaults.prompt_tokenizer).unwrap(),
            );
            e3.push(
                tasks::bpe_token_count(&bytecode.e3_source(), &defaults.prompt_tokenizer).unwrap(),
            );
        }
        let alignments = |encoded: &[u64], highest_rendered: u8| -> Vec<(usize, usize)> {
            let mut hits = Vec::new();
            for (high, e0_tokens) in e0.iter().enumerate() {
                for (low, encoded_tokens) in encoded
                    .iter()
                    .enumerate()
                    .take(highest_rendered as usize + 1)
                {
                    if low >= high {
                        continue;
                    }
                    let denominator = (*e0_tokens).max(*encoded_tokens) as f64;
                    if e0_tokens.abs_diff(*encoded_tokens) as f64 / denominator <= 0.10 {
                        hits.push((high, low));
                    }
                }
            }
            hits
        };
        let e2_hits = alignments(&e2, HIGHEST_TIER_RENDERED_AS_E2);
        let e3_hits = alignments(&e3, HIGHEST_TIER_RENDERED_AS_E3);
        assert!(
            e2_hits.len() >= 4,
            "E0<->E2 tier alignments collapsed to {e2_hits:?}; E0={e0:?} E2={e2:?}"
        );
        assert!(
            e3_hits.len() >= 3,
            "E0<->E3 tier alignments collapsed to {e3_hits:?}; E0={e0:?} E3={e3:?}"
        );
    }

    #[test]
    #[ignore = "diagnostic: prints the ladder token profile used to calibrate SIZE_TIER_WORK_ROUNDS"]
    /// D36 calibration probe. D35 removed the span growth that used to inflate
    /// the ladder, so the ladder now grows text linearly in work rounds instead
    /// of superlinearly. That shrank the E0 token spread below what the E2 and
    /// E3 contrasts need for token-matched pairs. This prints the profile the
    /// schedule must be recalibrated against; it asserts nothing.
    fn ladder_token_profile() {
        let defaults = Defaults::load("config/defaults.toml").expect("defaults load");
        println!("tier rounds  e0_bpe  e2_bpe  e3_bpe   e0/min");
        let mut e0s = Vec::new();
        let mut rows = Vec::new();
        for tier in 0..PROGRAM_SIZE_TIERS {
            let raw = generated_program(0, 1, tier);
            let compiled = compile(&raw.program, 0, LayoutDiscipline::Contiguous).unwrap();
            let bytecode =
                Bytecode::from_e0_with_carrier(&compiled.e0, defaults.move_carrier).unwrap();
            let e0 = tasks::bpe_token_count(&compiled.e0, &defaults.prompt_tokenizer).unwrap();
            let e2 =
                tasks::bpe_token_count(&bytecode.e2_source(), &defaults.prompt_tokenizer).unwrap();
            let e3 =
                tasks::bpe_token_count(&bytecode.e3_source(), &defaults.prompt_tokenizer).unwrap();
            e0s.push(e0);
            rows.push((tier, SIZE_TIER_WORK_ROUNDS[tier as usize], e0, e2, e3));
        }
        let min = *e0s.iter().min().unwrap() as f64;
        for (tier, rounds, e0, e2, e3) in rows {
            println!(
                "{tier:>4} {rounds:>6} {e0:>7} {e2:>7} {e3:>7}   {:>5.2}x",
                e0 as f64 / min
            );
        }
        println!(
            "E0 spread across the ladder: {:.2}x",
            *e0s.iter().max().unwrap() as f64 / min
        );
    }

    #[test]
    #[ignore = "diagnostic: measures 255-input E0 steps for the production seed/tier sweep"]
    /// F16 measurement. Replays the same 600-candidate seed and tier schedule as
    /// the D37 probe, including structural obfuscation, compiler template choice,
    /// and all three production layouts. The high diagnostic ceiling is only a
    /// measuring instrument; the configured cap is chosen from the printed
    /// maximum plus an explicit margin.
    fn ladder_worst_case_step_profile() {
        const SAMPLE_SEED: u64 = 42;
        const SAMPLE_CANDIDATES: usize = 600;
        const MEASUREMENT_CAP: u64 = 100_000_000;

        let started = std::time::Instant::now();
        let mut steps_by_tier = vec![Vec::<u64>::new(); PROGRAM_SIZE_TIERS as usize];
        for attempt in 0..SAMPLE_CANDIDATES {
            let item_seed = SAMPLE_SEED.wrapping_add(attempt as u64 * 0x9E37_79B9);
            let tier =
                ((attempt / PROGRAM_SIZE_TIERS as usize) % PROGRAM_SIZE_TIERS as usize) as u8;
            let generated = generated_program(item_seed, 1, tier);
            let (ir, _) = structural_obfuscate(&generated.program, item_seed);
            let discipline = match item_seed % 3 {
                0 => LayoutDiscipline::Contiguous,
                1 => LayoutDiscipline::Interleaved,
                _ => LayoutDiscipline::Strided,
            };
            let compiled = compile(&ir, item_seed, discipline).unwrap();
            let run = BfProgram::parse(&compiled.e0)
                .unwrap()
                .execute(&[255], MEASUREMENT_CAP, false)
                .unwrap_or_else(|error| {
                    panic!(
                        "measurement cap failed: attempt={attempt} tier={tier} seed={item_seed} shape={} error={error}",
                        generated.semantic_class
                    )
                });
            steps_by_tier[tier as usize].push(run.state.steps);
        }

        println!("tier samples min_steps p50_steps max_steps");
        for (tier, mut steps) in steps_by_tier.into_iter().enumerate() {
            steps.sort_unstable();
            println!(
                "{tier:>4} {:>7} {:>9} {:>9} {:>9}",
                steps.len(),
                steps[0],
                steps[steps.len() / 2],
                steps[steps.len() - 1]
            );
        }
        println!("elapsed_s={:.3}", started.elapsed().as_secs_f64());
    }

    #[test]
    /// D35: the ladder must still grow the *text* even though it no longer
    /// grows the live tape span. If this fails, D35 fixed the density collapse
    /// by destroying the size ladder itself, which is the whole point of it.
    fn fixed_work_scratch_still_grows_program_text() {
        let mut lengths = Vec::new();
        for tier in 0..PROGRAM_SIZE_TIERS {
            let raw = generated_program(0, 1, tier);
            let compiled = compile(&raw.program, 0, LayoutDiscipline::Contiguous).unwrap();
            lengths.push(
                compiled
                    .e0
                    .chars()
                    .filter(|c| "+-<>[],.".contains(*c))
                    .count(),
            );
        }
        for pair in lengths.windows(2) {
            assert!(
                pair[1] > pair[0],
                "program text must grow monotonically with the tier; observed {lengths:?}"
            );
        }
    }
}
