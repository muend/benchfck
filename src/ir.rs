use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub type Var = usize;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LoopClass {
    S0,
    S1,
    S2,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Statement {
    Set {
        dst: Var,
        value: u8,
    },
    Copy {
        dst: Var,
        src: Var,
    },
    Add {
        dst: Var,
        src: Var,
    },
    Sub {
        dst: Var,
        src: Var,
    },
    DrainScaled {
        dst: Var,
        src: Var,
        factor: u8,
        subtract: bool,
    },
    In {
        dst: Var,
    },
    Out {
        src: Var,
    },
    While {
        cond: Var,
        class: LoopClass,
        body: Vec<Statement>,
    },
    If {
        cond: Var,
        body: Vec<Statement>,
    },
    /// Execute once when nonzero and consume the condition to zero.
    IfNonZeroDrain {
        cond: Var,
        body: Vec<Statement>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Program {
    pub arity: u8,
    pub output_arity: u8,
    pub variables: Vec<String>,
    pub body: Vec<Statement>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct IrRun {
    pub output: Vec<u8>,
    pub variables: Vec<u8>,
    pub steps: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionProfile {
    pub statements_total: usize,
    pub statements_executed: usize,
    pub loops_total: usize,
    pub loops_entered: usize,
    pub minimum_loop_iterations: u64,
}

impl ExecutionProfile {
    pub fn is_fully_exercised(&self) -> bool {
        self.statements_total == self.statements_executed
            && self.loops_total == self.loops_entered
            && (self.loops_total == 0 || self.minimum_loop_iterations > 0)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IrError {
    #[error("variable index {0} is out of range")]
    BadVariable(usize),
    #[error("step cap exceeded")]
    NonTerminating,
}

pub fn execute(program: &Program, input: &[u8], step_cap: u64) -> Result<IrRun, IrError> {
    let mut state = IrState {
        vars: vec![0; program.variables.len()],
        input,
        input_pos: 0,
        output: vec![],
        steps: 0,
        step_cap,
    };
    exec_block(&program.body, &mut state)?;
    Ok(IrRun {
        output: state.output,
        variables: state.vars,
        steps: state.steps,
    })
}

/// Executes the IR while recording path-based statement and loop coverage.
/// This is an acceptance oracle for a concrete (program, input) item, not a
/// substitute execution backend.
pub fn execution_profile(
    program: &Program,
    input: &[u8],
    step_cap: u64,
) -> Result<ExecutionProfile, IrError> {
    let mut state = IrState {
        vars: vec![0; program.variables.len()],
        input,
        input_pos: 0,
        output: vec![],
        steps: 0,
        step_cap,
    };
    let mut hits = BTreeSet::new();
    let mut loops = BTreeMap::new();
    exec_profiled_block(
        &program.body,
        &mut state,
        &mut Vec::new(),
        &mut hits,
        &mut loops,
    )?;
    let (statements_total, loops_total) = static_counts(&program.body);
    Ok(ExecutionProfile {
        statements_total,
        statements_executed: hits.len(),
        loops_total,
        loops_entered: loops.values().filter(|n| **n > 0).count(),
        minimum_loop_iterations: loops.values().copied().min().unwrap_or(0),
    })
}

fn static_counts(block: &[Statement]) -> (usize, usize) {
    block.iter().fold((0, 0), |(statements, loops), stmt| {
        let (nested_statements, nested_loops) = match stmt {
            Statement::While { body, .. }
            | Statement::If { body, .. }
            | Statement::IfNonZeroDrain { body, .. } => static_counts(body),
            _ => (0, 0),
        };
        (
            statements + 1 + nested_statements,
            loops + usize::from(matches!(stmt, Statement::While { .. })) + nested_loops,
        )
    })
}

fn exec_profiled_block(
    block: &[Statement],
    s: &mut IrState<'_>,
    path: &mut Vec<usize>,
    hits: &mut BTreeSet<Vec<usize>>,
    loops: &mut BTreeMap<Vec<usize>, u64>,
) -> Result<(), IrError> {
    for (index, stmt) in block.iter().enumerate() {
        path.push(index);
        hits.insert(path.clone());
        tick(s)?;
        match stmt {
            Statement::Set { dst, value } => set(s, *dst, *value)?,
            Statement::Copy { dst, src } => set(s, *dst, get(s, *src)?)?,
            Statement::Add { dst, src } => set(s, *dst, get(s, *dst)?.wrapping_add(get(s, *src)?))?,
            Statement::Sub { dst, src } => set(s, *dst, get(s, *dst)?.wrapping_sub(get(s, *src)?))?,
            Statement::DrainScaled {
                dst,
                src,
                factor,
                subtract,
            } => {
                let scaled = get(s, *src)?.wrapping_mul(*factor);
                let next = if *subtract {
                    get(s, *dst)?.wrapping_sub(scaled)
                } else {
                    get(s, *dst)?.wrapping_add(scaled)
                };
                set(s, *dst, next)?;
                set(s, *src, 0)?;
            }
            Statement::In { dst } => {
                let x = s.input.get(s.input_pos).copied().unwrap_or(0);
                s.input_pos += 1;
                set(s, *dst, x)?;
            }
            Statement::Out { src } => s.output.push(get(s, *src)?),
            Statement::While { cond, body, .. } => {
                loops.entry(path.clone()).or_insert(0);
                while get(s, *cond)? != 0 {
                    tick(s)?;
                    *loops.get_mut(path).expect("loop was registered") += 1;
                    exec_profiled_block(body, s, path, hits, loops)?;
                }
            }
            Statement::If { cond, body } => {
                if get(s, *cond)? != 0 {
                    exec_profiled_block(body, s, path, hits, loops)?;
                }
            }
            Statement::IfNonZeroDrain { cond, body } => {
                if get(s, *cond)? != 0 {
                    set(s, *cond, 0)?;
                    exec_profiled_block(body, s, path, hits, loops)?;
                }
            }
        }
        path.pop();
    }
    Ok(())
}

struct IrState<'a> {
    vars: Vec<u8>,
    input: &'a [u8],
    input_pos: usize,
    output: Vec<u8>,
    steps: u64,
    step_cap: u64,
}

fn tick(s: &mut IrState<'_>) -> Result<(), IrError> {
    s.steps += 1;
    if s.steps > s.step_cap {
        Err(IrError::NonTerminating)
    } else {
        Ok(())
    }
}

fn get(s: &IrState<'_>, v: Var) -> Result<u8, IrError> {
    s.vars.get(v).copied().ok_or(IrError::BadVariable(v))
}

fn set(s: &mut IrState<'_>, v: Var, x: u8) -> Result<(), IrError> {
    *s.vars.get_mut(v).ok_or(IrError::BadVariable(v))? = x;
    Ok(())
}

fn exec_block(block: &[Statement], s: &mut IrState<'_>) -> Result<(), IrError> {
    for stmt in block {
        tick(s)?;
        match stmt {
            Statement::Set { dst, value } => set(s, *dst, *value)?,
            Statement::Copy { dst, src } => set(s, *dst, get(s, *src)?)?,
            Statement::Add { dst, src } => set(s, *dst, get(s, *dst)?.wrapping_add(get(s, *src)?))?,
            Statement::Sub { dst, src } => set(s, *dst, get(s, *dst)?.wrapping_sub(get(s, *src)?))?,
            Statement::DrainScaled {
                dst,
                src,
                factor,
                subtract,
            } => {
                let scaled = get(s, *src)?.wrapping_mul(*factor);
                let next = if *subtract {
                    get(s, *dst)?.wrapping_sub(scaled)
                } else {
                    get(s, *dst)?.wrapping_add(scaled)
                };
                set(s, *dst, next)?;
                set(s, *src, 0)?;
            }
            Statement::In { dst } => {
                let x = s.input.get(s.input_pos).copied().unwrap_or(0);
                s.input_pos += 1;
                set(s, *dst, x)?;
            }
            Statement::Out { src } => s.output.push(get(s, *src)?),
            Statement::While { cond, body, .. } => {
                while get(s, *cond)? != 0 {
                    tick(s)?;
                    exec_block(body, s)?;
                }
            }
            Statement::If { cond, body } => {
                if get(s, *cond)? != 0 {
                    exec_block(body, s)?;
                }
            }
            Statement::IfNonZeroDrain { cond, body } => {
                if get(s, *cond)? != 0 {
                    set(s, *cond, 0)?;
                    exec_block(body, s)?;
                }
            }
        }
    }
    Ok(())
}

impl Program {
    pub fn nesting_depth(&self) -> usize {
        fn depth(xs: &[Statement]) -> usize {
            xs.iter()
                .map(|s| match s {
                    Statement::While { body, .. }
                    | Statement::If { body, .. }
                    | Statement::IfNonZeroDrain { body, .. } => 1 + depth(body),
                    _ => 0,
                })
                .max()
                .unwrap_or(0)
        }
        depth(&self.body)
    }
    pub fn loop_counts(&self) -> (usize, usize, usize) {
        fn visit(xs: &[Statement], n: &mut [usize; 3]) {
            for s in xs {
                match s {
                    Statement::While { class, body, .. } => {
                        n[match class {
                            LoopClass::S0 => 0,
                            LoopClass::S1 => 1,
                            LoopClass::S2 => 2,
                        }] += 1;
                        visit(body, n);
                    }
                    Statement::If { body, .. } | Statement::IfNonZeroDrain { body, .. } => {
                        visit(body, n)
                    }
                    _ => {}
                }
            }
        }
        let mut n = [0; 3];
        visit(&self.body, &mut n);
        (n[0], n[1], n[2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handwritten_ir_wraps_and_reads_exhausted_as_zero() {
        let p = Program {
            arity: 1,
            output_arity: 2,
            variables: vec!["v0".into(), "v1".into()],
            body: vec![
                Statement::In { dst: 0 },
                Statement::Set { dst: 1, value: 255 },
                Statement::Add { dst: 1, src: 0 },
                Statement::Out { src: 1 },
                Statement::In { dst: 0 },
                Statement::Out { src: 0 },
            ],
        };
        assert_eq!(execute(&p, &[1], 100).unwrap().output, vec![0, 0]);
    }
}
