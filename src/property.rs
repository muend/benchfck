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

pub fn validate_range(limit: u64) -> Result<(), String> {
    for n in 0..limit {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn population_is_deterministic_at_boundaries() {
        assert_eq!(program(0), program(0));
        assert_eq!(program(RELEASE_PROGRAMS - 1), program(RELEASE_PROGRAMS - 1));
        assert_ne!(program(0), program(1));
    }
}
