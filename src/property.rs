//! Deterministic cross-backend compiler property population.
//!
//! The fast 500-program shard, the ignored 10k test, and the release evidence
//! command all call this module so their populations cannot drift apart.

use crate::{
    backend::Bytecode,
    compiler::{LayoutDiscipline, compile},
    ir::{LoopClass, Program, Statement},
    oracle::exhaustive_validate,
};

pub const FAST_PROGRAMS: u64 = 500;
pub const RELEASE_PROGRAMS: u64 = 10_000;
pub const INPUTS_PER_PROGRAM: u64 = 256;
pub const PROPERTY_STEP_CAP: u64 = 1_000_000;
pub const PROTOCOL_VERSION: &str = "benchfck.property-suite.v1";
pub const PROTOCOL: &str = "deterministic n=0..limit; arity=1; complete 256-byte domain; layouts selected by n mod 3; compare typed IR, canonical Brainfuck E0, compact explicit E2, and verbose explicit E3; step cap 1000000; any compile, parse, execution, output, or weighted-step mismatch fails the run";

fn program(n: u64) -> Program {
    let mut body = vec![
        Statement::In { dst: 0 },
        Statement::Set {
            dst: 1,
            value: (n as u8).wrapping_mul(17),
        },
        Statement::Set { dst: 2, value: 1 },
        Statement::Set {
            dst: 3,
            value: (n.rotate_left(11) as u8).wrapping_add(3),
        },
    ];
    for slot in 0..=n as usize % 6 {
        match (n.rotate_right((slot * 7) as u32) + slot as u64) % 6 {
            0 => body.push(Statement::Add { dst: 1, src: 0 }),
            1 => body.push(Statement::Sub { dst: 1, src: 3 }),
            2 => body.push(Statement::Copy { dst: 3, src: 1 }),
            3 => body.push(Statement::Set {
                dst: 3,
                value: (n as u8).wrapping_add(slot as u8),
            }),
            4 => body.push(Statement::If {
                cond: 0,
                body: vec![Statement::Add { dst: 1, src: 2 }],
            }),
            _ => body.push(Statement::If {
                cond: 3,
                body: vec![
                    Statement::Sub { dst: 1, src: 2 },
                    Statement::Add { dst: 3, src: 0 },
                ],
            }),
        }
    }
    if n.is_multiple_of(3) {
        let mut loop_body = vec![Statement::Sub { dst: 4, src: 2 }];
        if n.is_multiple_of(2) {
            loop_body.push(Statement::Add { dst: 1, src: 2 });
        } else {
            loop_body.push(Statement::Sub { dst: 1, src: 2 });
            loop_body.push(Statement::Add { dst: 3, src: 2 });
        }
        body.extend([
            Statement::Set {
                dst: 4,
                value: (n % 5 + 1) as u8,
            },
            Statement::While {
                cond: 4,
                class: LoopClass::S1,
                body: loop_body,
            },
        ]);
    }
    body.push(Statement::Out { src: 1 });
    Program {
        arity: 1,
        output_arity: 1,
        variables: (0..5).map(|i| format!("v{i}")).collect(),
        body,
    }
}

/// Return one balanced, contiguous shard of `0..total`.
///
/// The first `total % shard_count` shards receive one extra program. Adjacent
/// shards meet exactly, so their union covers the population without overlap.
pub fn shard_bounds(
    total: u64,
    shard_index: u64,
    shard_count: u64,
) -> Result<std::ops::Range<u64>, String> {
    if shard_count == 0 {
        return Err("property shard count must be greater than zero".into());
    }
    if shard_index >= shard_count {
        return Err(format!(
            "property shard index {shard_index} is outside 0..{shard_count}"
        ));
    }
    let base = total / shard_count;
    let remainder = total % shard_count;
    let start = shard_index * base + shard_index.min(remainder);
    let length = base + u64::from(shard_index < remainder);
    Ok(start..start + length)
}

/// Validate an exact half-open slice of the deterministic population.
pub fn validate_slice(start: u64, end: u64) -> Result<(), String> {
    if start > end {
        return Err(format!("property slice start {start} exceeds end {end}"));
    }
    for n in start..end {
        let program = program(n);
        let layout = match n % 3 {
            0 => LayoutDiscipline::Contiguous,
            1 => LayoutDiscipline::Interleaved,
            _ => LayoutDiscipline::Strided,
        };
        let compiled = compile(&program, n, layout)
            .map_err(|error| format!("n={n}, layout={layout:?}, compile error={error}"))?;
        let bytecode = Bytecode::from_e0(&compiled.e0)
            .map_err(|error| format!("n={n}, layout={layout:?}, parse error={error}"))?;
        exhaustive_validate(
            &program,
            &compiled.e0,
            &bytecode.e2_source(),
            &bytecode.e3_source(),
            PROPERTY_STEP_CAP,
        )
        .map_err(|error| {
            format!("n={n}, layout={layout:?}, validation error={error}, IR={program:?}")
        })?;
    }
    Ok(())
}

pub fn validate_range(limit: u64) -> Result<(), String> {
    validate_slice(0, limit)
}

/// Validate one balanced shard of a fixed population.
pub fn validate_shard(total: u64, shard_index: u64, shard_count: u64) -> Result<(), String> {
    let bounds = shard_bounds(total, shard_index, shard_count)?;
    validate_slice(bounds.start, bounds.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn population_is_deterministic_at_boundaries() {
        assert_eq!(program(0), program(0));
        assert_eq!(program(RELEASE_PROGRAMS - 1), program(RELEASE_PROGRAMS - 1));
        assert_ne!(program(0), program(1));
    }

    #[test]
    fn balanced_shards_cover_population_once() {
        let shards = (0..4)
            .map(|index| shard_bounds(RELEASE_PROGRAMS, index, 4).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(shards[0].start, 0);
        assert_eq!(shards.last().unwrap().end, RELEASE_PROGRAMS);
        assert!(shards.windows(2).all(|pair| pair[0].end == pair[1].start));
        assert!(shards.iter().all(|shard| shard.end - shard.start == 2_500));
    }

    #[test]
    fn uneven_shards_are_balanced_and_invalid_layouts_fail_closed() {
        assert_eq!(shard_bounds(10, 0, 3).unwrap(), 0..4);
        assert_eq!(shard_bounds(10, 1, 3).unwrap(), 4..7);
        assert_eq!(shard_bounds(10, 2, 3).unwrap(), 7..10);
        assert!(shard_bounds(10, 0, 0).is_err());
        assert!(shard_bounds(10, 3, 3).is_err());
    }
}
