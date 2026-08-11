use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const TAPE_LEN: usize = 30_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MachineState {
    pub ip: usize,
    pub pointer: usize,
    pub tape: Vec<u8>,
    pub input_pos: usize,
    pub output: Vec<u8>,
    pub steps: u64,
}

impl Default for MachineState {
    fn default() -> Self {
        Self {
            ip: 0,
            pointer: 0,
            tape: vec![0; TAPE_LEN],
            input_pos: 0,
            output: vec![],
            steps: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TracePoint {
    pub step: u64,
    pub ip: usize,
    pub instruction: char,
    pub pointer: usize,
    pub touched_cell: usize,
    pub cell_value: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BfRun {
    pub state: MachineState,
    pub trace: Vec<TracePoint>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Error)]
#[serde(tag = "kind", content = "detail", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BfError {
    #[error("unmatched bracket at instruction {0}")]
    UnmatchedBracket(usize),
    #[error("pointer moved left of zero")]
    PointerUnderflow,
    #[error("pointer moved beyond the 30,000-cell tape")]
    PointerOverflow,
    #[error("step cap exceeded")]
    NonTerminating,
    #[error("supplied state is invalid: {0}")]
    InvalidState(String),
}

#[derive(Clone, Debug)]
pub struct BfProgram {
    code: Vec<u8>,
    jumps: Vec<Option<usize>>,
}

impl BfProgram {
    pub fn parse(source: &str) -> Result<Self, BfError> {
        let code: Vec<u8> = source.bytes().filter(|b| b"+-<>[],.".contains(b)).collect();
        let mut jumps = vec![None; code.len()];
        let mut stack = vec![];
        for (i, b) in code.iter().enumerate() {
            match b {
                b'[' => stack.push(i),
                b']' => {
                    let open = stack.pop().ok_or(BfError::UnmatchedBracket(i))?;
                    jumps[open] = Some(i);
                    jumps[i] = Some(open);
                }
                _ => {}
            }
        }
        if let Some(i) = stack.pop() {
            return Err(BfError::UnmatchedBracket(i));
        }
        Ok(Self { code, jumps })
    }

    pub fn len(&self) -> usize {
        self.code.len()
    }
    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }
    pub fn code(&self) -> String {
        String::from_utf8(self.code.clone()).expect("BF is ASCII")
    }

    pub fn execute(&self, input: &[u8], step_cap: u64, trace: bool) -> Result<BfRun, BfError> {
        self.run(MachineState::default(), input, step_cap, trace, None)
    }

    pub fn continue_from(
        &self,
        s: MachineState,
        input: &[u8],
        step_cap: u64,
        trace: bool,
    ) -> Result<BfRun, BfError> {
        self.run(s, input, step_cap, trace, None)
    }

    pub fn state_after(
        &self,
        input: &[u8],
        completed_steps: u64,
        step_cap: u64,
    ) -> Result<MachineState, BfError> {
        Ok(self
            .run(
                MachineState::default(),
                input,
                step_cap,
                false,
                Some(completed_steps),
            )?
            .state)
    }

    fn run(
        &self,
        mut s: MachineState,
        input: &[u8],
        step_cap: u64,
        trace: bool,
        stop_after: Option<u64>,
    ) -> Result<BfRun, BfError> {
        if s.tape.len() != TAPE_LEN {
            return Err(BfError::InvalidState("tape length must be 30,000".into()));
        }
        if s.pointer >= TAPE_LEN || s.ip > self.code.len() {
            return Err(BfError::InvalidState(
                "pointer or instruction index out of range".into(),
            ));
        }
        let mut points = vec![];
        while s.ip < self.code.len() && stop_after.is_none_or(|target| s.steps < target) {
            if s.steps >= step_cap {
                return Err(BfError::NonTerminating);
            }
            let ip = s.ip;
            let op = self.code[ip];
            let touched = s.pointer;
            match op {
                b'+' => {
                    s.tape[s.pointer] = s.tape[s.pointer].wrapping_add(1);
                    s.ip += 1;
                }
                b'-' => {
                    s.tape[s.pointer] = s.tape[s.pointer].wrapping_sub(1);
                    s.ip += 1;
                }
                b'>' => {
                    if s.pointer + 1 >= TAPE_LEN {
                        return Err(BfError::PointerOverflow);
                    }
                    s.pointer += 1;
                    s.ip += 1;
                }
                b'<' => {
                    if s.pointer == 0 {
                        return Err(BfError::PointerUnderflow);
                    }
                    s.pointer -= 1;
                    s.ip += 1;
                }
                b',' => {
                    s.tape[s.pointer] = input.get(s.input_pos).copied().unwrap_or(0);
                    s.input_pos += 1;
                    s.ip += 1;
                }
                b'.' => {
                    s.output.push(s.tape[s.pointer]);
                    s.ip += 1;
                }
                b'[' => {
                    if s.tape[s.pointer] == 0 {
                        s.ip = self.jumps[ip].unwrap() + 1;
                    } else {
                        s.ip += 1;
                    }
                }
                b']' => {
                    if s.tape[s.pointer] != 0 {
                        s.ip = self.jumps[ip].unwrap();
                    } else {
                        s.ip += 1;
                    }
                }
                _ => unreachable!(),
            }
            s.steps += 1;
            if trace {
                points.push(TracePoint {
                    step: s.steps,
                    ip,
                    instruction: op as char,
                    pointer: s.pointer,
                    touched_cell: touched,
                    cell_value: s.tape[touched],
                });
            }
        }
        Ok(BfRun {
            state: s,
            trace: points,
        })
    }
}

pub fn execute(source: &str, input: &[u8], step_cap: u64, trace: bool) -> Result<BfRun, BfError> {
    BfProgram::parse(source)?.execute(input, step_cap, trace)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrapping_and_exhausted_input() {
        assert_eq!(
            execute("-.,.", &[], 50, false).unwrap().state.output,
            vec![255, 0]
        );
    }
    #[test]
    fn left_edge_is_hard_error() {
        assert_eq!(
            execute("<", &[], 5, false).unwrap_err(),
            BfError::PointerUnderflow
        );
    }
    #[test]
    fn cap_is_exact() {
        assert_eq!(
            execute("+[]", &[], 20, false).unwrap_err(),
            BfError::NonTerminating
        );
    }
    #[test]
    fn fork_and_continue_matches_full_run() {
        let p = BfProgram::parse(",+.+.").unwrap();
        let full = p.execute(&[7], 50, false).unwrap();
        let mut partial = MachineState::default();
        partial.tape[0] = 8;
        partial.input_pos = 1;
        partial.ip = 2;
        partial.steps = 2;
        assert_eq!(
            p.continue_from(partial, &[7], 50, false)
                .unwrap()
                .state
                .output,
            full.state.output
        );
    }
}
