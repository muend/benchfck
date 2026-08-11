use crate::{
    backend::Bytecode,
    bf::{self, BfError, BfProgram},
    ir::{self, Program},
};
use rand::{SeedableRng, seq::SliceRandom};
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", content = "value", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Observable {
    Output(Vec<u8>),
    Identical,
    NonTerminating,
    Error(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AvalancheRecord {
    pub position: usize,
    pub from: char,
    pub to: char,
    pub outcome: Observable,
    pub changed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticFingerprint {
    pub algorithm: String,
    pub domain_size: u64,
    pub digest_hex: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AvalancheMap {
    pub records: Vec<AvalancheRecord>,
    pub score: f64,
    pub sampling_rate: f64,
}

#[derive(Debug, Error)]
pub enum OracleError {
    #[error("reference E0 failed: {0}")]
    Reference(#[from] BfError),
    #[error("explicit encoding failed: {0}")]
    Backend(#[from] crate::backend::BackendError),
    #[error("backend mismatch on input {input:?}: {detail}")]
    Differential { input: Vec<u8>, detail: String },
    #[error("arity {0} is outside the exhaustive v1 range")]
    BadArity(u8),
}

fn domain(arity: u8) -> Result<Box<dyn Iterator<Item = Vec<u8>>>, OracleError> {
    match arity {
        1 => Ok(Box::new((0u16..=255).map(|a| vec![a as u8]))),
        2 => Ok(Box::new(
            (0u32..=65535).map(|n| vec![(n >> 8) as u8, n as u8]),
        )),
        x => Err(OracleError::BadArity(x)),
    }
}

/// INV-4 and INV-3: all item truth is read from E0; IR is validation-only.
pub fn exhaustive_validate(
    program: &Program,
    e0: &str,
    e2_source: &str,
    e3_source: &str,
    step_cap: u64,
) -> Result<SemanticFingerprint, OracleError> {
    let bf = BfProgram::parse(e0)?;
    // Parse both public renderings through independent decoders. Reusing the
    // in-memory Bytecode here would only test one executor twice.
    let e2_backend = Bytecode::parse_e2(e2_source)?;
    let e3_backend = Bytecode::parse_e3(e3_source)?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    for input in domain(program.arity)? {
        let truth = bf
            .execute(&input, step_cap, false)
            .map_err(|e| OracleError::Differential {
                input: input.clone(),
                detail: format!("E0: {e}"),
            })?
            .state
            .output;
        let ir_out = ir::execute(program, &input, step_cap)
            .map_err(|e| OracleError::Differential {
                input: input.clone(),
                detail: format!("IR: {e}"),
            })?
            .output;
        let e2 = e2_backend
            .execute(&input, step_cap)
            .map_err(|e| OracleError::Differential {
                input: input.clone(),
                detail: format!("E2: {e}"),
            })?
            .output;
        let e3 = e3_backend
            .execute(&input, step_cap)
            .map_err(|e| OracleError::Differential {
                input: input.clone(),
                detail: format!("E3: {e}"),
            })?
            .output;
        if truth != ir_out || truth != e2 || truth != e3 {
            return Err(OracleError::Differential {
                input,
                detail: format!("E0={truth:?}, IR={ir_out:?}, E2={e2:?}, E3={e3:?}"),
            });
        }
        hasher.update((input.len() as u32).to_le_bytes());
        hasher.update(&input);
        hasher.update((truth.len() as u32).to_le_bytes());
        hasher.update(&truth);
        size += 1;
    }
    Ok(SemanticFingerprint {
        algorithm: "sha256_length_delimited_domain_table".into(),
        domain_size: size,
        digest_hex: format!("{:x}", hasher.finalize()),
    })
}

fn classify(result: Result<bf::BfRun, BfError>, reference: &[u8]) -> (Observable, bool) {
    match result {
        Ok(run) if run.state.output == reference => (Observable::Identical, false),
        Ok(run) => (Observable::Output(run.state.output), true),
        Err(BfError::NonTerminating) => (Observable::NonTerminating, true),
        Err(e) => (Observable::Error(e.to_string()), true),
    }
}

pub fn avalanche_map(
    e0: &str,
    input: &[u8],
    step_cap: u64,
    min_positions: usize,
    seed: u64,
) -> Result<AvalancheMap, OracleError> {
    let parsed = BfProgram::parse(e0)?;
    let canonical = parsed.code();
    let reference = parsed.execute(input, step_cap, false)?.state.output;
    let chars: Vec<char> = canonical.chars().collect();
    // Pointer movement and bracket corruption can make a trivial layout or a
    // syntax error look causally important. Mutate only executed data/I/O
    // operations and keep mutations within that semantic alphabet.
    let mut positions: Vec<usize> = chars
        .iter()
        .enumerate()
        .filter_map(|(i, c)| matches!(c, '+' | '-' | ',' | '.').then_some(i))
        .collect();
    let eligible_positions = positions.len();
    let mut rng = ChaCha20Rng::seed_from_u64(seed ^ 0xA11A_A11A);
    if positions.len() > min_positions {
        positions.shuffle(&mut rng);
        positions.truncate(min_positions);
        positions.sort_unstable();
    }
    let alphabet = ['+', '-', ',', '.'];
    let mut records = vec![];
    for pos in positions.iter().copied() {
        for to in alphabet {
            let from = chars[pos];
            if to == from {
                continue;
            }
            let mut mutant = chars.clone();
            mutant[pos] = to;
            let source: String = mutant.into_iter().collect();
            let (outcome, changed) = match BfProgram::parse(&source) {
                Ok(p) => classify(p.execute(input, step_cap, false), &reference),
                Err(e) => (Observable::Error(e.to_string()), true),
            };
            records.push(AvalancheRecord {
                position: pos,
                from,
                to,
                outcome,
                changed,
            });
        }
    }
    let score = if records.is_empty() {
        0.0
    } else {
        records.iter().filter(|r| r.changed).count() as f64 / records.len() as f64
    };
    let sampling_rate = if eligible_positions == 0 {
        0.0
    } else {
        positions.len() as f64 / eligible_positions as f64
    };
    Ok(AvalancheMap {
        records,
        score,
        sampling_rate,
    })
}

pub fn input_sensitive(program: &Program, e0: &str, step_cap: u64) -> Result<bool, OracleError> {
    let p = BfProgram::parse(e0)?;
    let first = p
        .execute(&vec![0; program.arity as usize], step_cap, false)?
        .state
        .output;
    for x in domain(program.arity)?.take(512) {
        if p.execute(&x, step_cap, false)?.state.output != first {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The selected item binding must expose every argument: holding all other
/// bytes fixed, at least one deterministic perturbation of each argument must
/// change the observable output.
pub fn each_argument_sensitive(
    e0: &str,
    selected: &[u8],
    step_cap: u64,
) -> Result<bool, OracleError> {
    let p = BfProgram::parse(e0)?;
    let baseline = p.execute(selected, step_cap, false)?.state.output;
    for index in 0..selected.len() {
        let mut changed = false;
        for delta in [1u8, 17, 63, 127, 255] {
            let mut perturbed = selected.to_vec();
            perturbed[index] = perturbed[index].wrapping_add(delta);
            if p.execute(&perturbed, step_cap, false)?.state.output != baseline {
                changed = true;
                break;
            }
        }
        if !changed {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn off_idiom_rate(e0: &str) -> f64 {
    const IDIOMS: [&str; 5] = ["[-]", "[->+<]", "[->+>+<<]", "[<+>-]", "[->++<]"];
    if e0.is_empty() {
        return 0.0;
    }
    let matches: usize = IDIOMS
        .iter()
        .map(|p| e0.match_indices(p).count() * p.len())
        .sum();
    matches as f64 / e0.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        backend::Bytecode,
        compiler::{LayoutDiscipline, compile},
        ir::{Program, Statement},
    };
    #[test]
    fn cross_backend_domain_and_fingerprint_are_stable() {
        let p = Program {
            arity: 1,
            output_arity: 1,
            variables: vec!["v0".into(), "v1".into()],
            body: vec![
                Statement::In { dst: 0 },
                Statement::Set { dst: 1, value: 1 },
                Statement::Add { dst: 0, src: 1 },
                Statement::Out { src: 0 },
            ],
        };
        let c = compile(&p, 3, LayoutDiscipline::Contiguous).unwrap();
        let b = Bytecode::from_e0(&c.e0).unwrap();
        let a = exhaustive_validate(&p, &c.e0, &b.e2_source(), &b.e3_source(), 1_000_000).unwrap();
        let z = exhaustive_validate(&p, &c.e0, &b.e2_source(), &b.e3_source(), 1_000_000).unwrap();
        assert_eq!(a, z);
        assert_eq!(a.domain_size, 256);
    }

    #[test]
    fn every_selected_argument_must_change_the_output_independently() {
        let sensitive = Program {
            arity: 2,
            output_arity: 1,
            variables: vec!["a".into(), "b".into()],
            body: vec![
                Statement::In { dst: 0 },
                Statement::In { dst: 1 },
                Statement::Add { dst: 0, src: 1 },
                Statement::Out { src: 0 },
            ],
        };
        let insensitive = Program {
            body: vec![
                Statement::In { dst: 0 },
                Statement::In { dst: 1 },
                Statement::Out { src: 0 },
            ],
            ..sensitive.clone()
        };
        let a = compile(&sensitive, 9, LayoutDiscipline::Contiguous).unwrap();
        let b = compile(&insensitive, 9, LayoutDiscipline::Contiguous).unwrap();
        assert!(each_argument_sensitive(&a.e0, &[17, 29], 1_000_000).unwrap());
        assert!(!each_argument_sensitive(&b.e0, &[17, 29], 1_000_000).unwrap());
    }
}
