use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MoveCarrier {
    #[default]
    Rle,
    Expanded,
    Omitted,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Op {
    /// Trace-preserving carrier for a monotonic E0 pointer run. `cell` is the
    /// final explicit cell and `count` is the exact number of E0 moves.
    Move(usize, u32),
    Inc(usize, u32),
    Dec(usize, u32),
    In(usize),
    Out(usize),
    JumpZero(usize, usize),
    JumpNonZero(usize, usize),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Bytecode {
    pub ops: Vec<Op>,
    #[serde(default)]
    pub move_carrier: MoveCarrier,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackendTracePoint {
    pub step: u64,
    pub ip: usize,
    pub cell: usize,
    pub value: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendRun {
    pub output: Vec<u8>,
    pub cells: Vec<u8>,
    pub steps: u64,
    pub trace: Vec<BackendTracePoint>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BackendError {
    #[error("E0 has an unmatched bracket at {0}")]
    UnmatchedBracket(usize),
    #[error("E0 moves left of cell zero while resolving operands")]
    PointerUnderflow,
    #[error("cell operand {0} is outside the 30,000-cell tape")]
    BadCell(usize),
    #[error("jump target {0} out of range")]
    BadJump(usize),
    #[error("step cap exceeded")]
    NonTerminating,
    #[error("malformed explicit encoding: {0}")]
    MalformedEncoding(String),
}

impl Bytecode {
    pub fn from_e0(source: &str) -> Result<Self, BackendError> {
        Self::from_e0_with_carrier(source, MoveCarrier::Rle)
    }

    /// Resolves compiler-known pointer positions without lifting E0 back to
    /// IR. RLE is the benchmark carrier; expanded and omitted are diagnostic
    /// variants used to quantify the movement-channel confound.
    pub fn from_e0_with_carrier(
        source: &str,
        move_carrier: MoveCarrier,
    ) -> Result<Self, BackendError> {
        let raw = parse_e0_expanded(source)?;
        let ops = match move_carrier {
            MoveCarrier::Rle => compress_runs(&raw),
            MoveCarrier::Expanded => raw,
            MoveCarrier::Omitted => omit_moves(&raw),
        };
        Ok(Self { ops, move_carrier })
    }

    pub fn parse_e2(source: &str) -> Result<Self, BackendError> {
        let mut lines = source.lines();
        let move_carrier = parse_carrier_header(
            lines
                .next()
                .ok_or_else(|| BackendError::MalformedEncoding(source.into()))?,
        )?;
        let mut ops = Vec::new();
        for line in lines {
            ops.push(parse_e2_op(line)?);
        }
        Ok(Self { ops, move_carrier })
    }

    pub fn parse_e3(source: &str) -> Result<Self, BackendError> {
        let mut lines = source.lines();
        let move_carrier = parse_carrier_header(
            lines
                .next()
                .ok_or_else(|| BackendError::MalformedEncoding(source.into()))?,
        )?;
        let mut ops = Vec::new();
        for line in lines {
            ops.push(parse_e3_op(line)?);
        }
        Ok(Self { ops, move_carrier })
    }

    pub fn execute(&self, input: &[u8], step_cap: u64) -> Result<BackendRun, BackendError> {
        self.execute_traced(input, step_cap, false)
    }

    pub fn execute_traced(
        &self,
        input: &[u8],
        step_cap: u64,
        trace: bool,
    ) -> Result<BackendRun, BackendError> {
        let mut cells = vec![0u8; 30_000];
        let mut output = vec![];
        let mut input_pos = 0;
        let mut ip = 0;
        let mut steps = 0;
        let mut trace_cursor = 0usize;
        let mut points = vec![];

        while ip < self.ops.len() {
            match self.ops[ip] {
                Op::Move(final_cell, count) => {
                    check(final_cell)?;
                    let distance = trace_cursor.abs_diff(final_cell);
                    if distance != count as usize {
                        return Err(BackendError::MalformedEncoding(format!(
                            "MOVE to {final_cell} claims {count} steps from {trace_cursor}"
                        )));
                    }
                    let right = final_cell > trace_cursor;
                    for _ in 0..count {
                        ensure_step(&mut steps, step_cap)?;
                        trace_cursor = if right {
                            trace_cursor + 1
                        } else {
                            trace_cursor - 1
                        };
                        if trace {
                            push_trace(&mut points, steps, ip, trace_cursor, cells[trace_cursor]);
                        }
                    }
                    ip += 1;
                }
                Op::Inc(cell, count) => {
                    check(cell)?;
                    for _ in 0..count {
                        ensure_step(&mut steps, step_cap)?;
                        cells[cell] = cells[cell].wrapping_add(1);
                        if trace {
                            push_trace(&mut points, steps, ip, cell, cells[cell]);
                        }
                    }
                    ip += 1;
                }
                Op::Dec(cell, count) => {
                    check(cell)?;
                    for _ in 0..count {
                        ensure_step(&mut steps, step_cap)?;
                        cells[cell] = cells[cell].wrapping_sub(1);
                        if trace {
                            push_trace(&mut points, steps, ip, cell, cells[cell]);
                        }
                    }
                    ip += 1;
                }
                Op::In(cell) => {
                    check(cell)?;
                    ensure_step(&mut steps, step_cap)?;
                    cells[cell] = input.get(input_pos).copied().unwrap_or(0);
                    input_pos += 1;
                    if trace {
                        push_trace(&mut points, steps, ip, cell, cells[cell]);
                    }
                    ip += 1;
                }
                Op::Out(cell) => {
                    check(cell)?;
                    ensure_step(&mut steps, step_cap)?;
                    output.push(cells[cell]);
                    if trace {
                        push_trace(&mut points, steps, ip, cell, cells[cell]);
                    }
                    ip += 1;
                }
                Op::JumpZero(cell, target) => {
                    check(cell)?;
                    if target > self.ops.len() {
                        return Err(BackendError::BadJump(target));
                    }
                    ensure_step(&mut steps, step_cap)?;
                    if trace {
                        push_trace(&mut points, steps, ip, cell, cells[cell]);
                    }
                    ip = if cells[cell] == 0 { target } else { ip + 1 };
                }
                Op::JumpNonZero(cell, target) => {
                    check(cell)?;
                    if target >= self.ops.len() {
                        return Err(BackendError::BadJump(target));
                    }
                    ensure_step(&mut steps, step_cap)?;
                    if trace {
                        push_trace(&mut points, steps, ip, cell, cells[cell]);
                    }
                    ip = if cells[cell] != 0 { target } else { ip + 1 };
                }
            }
        }
        Ok(BackendRun {
            output,
            cells,
            steps,
            trace: points,
        })
    }

    pub fn e2_source(&self) -> String {
        std::iter::once(carrier_header(self.move_carrier).to_string())
            .chain(self.ops.iter().map(e2))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn e3_source(&self) -> String {
        std::iter::once(carrier_header(self.move_carrier).to_string())
            .chain(self.ops.iter().map(e3))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn e4_source(&self) -> String {
        self.ops
            .iter()
            .enumerate()
            .map(|(i, o)| format!("Step {}: {}", i + 1, e4(o)))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn parse_e0_expanded(source: &str) -> Result<Vec<Op>, BackendError> {
    let code: Vec<u8> = source.bytes().filter(|b| b"+-<>[],.".contains(b)).collect();
    let mut ptr = 0usize;
    let mut ops = vec![];
    let mut stack = vec![];
    for (source_ip, op) in code.into_iter().enumerate() {
        match op {
            b'>' => {
                ptr += 1;
                ops.push(Op::Move(ptr, 1));
            }
            b'<' => {
                if ptr == 0 {
                    return Err(BackendError::PointerUnderflow);
                }
                ptr -= 1;
                ops.push(Op::Move(ptr, 1));
            }
            b'+' => ops.push(Op::Inc(ptr, 1)),
            b'-' => ops.push(Op::Dec(ptr, 1)),
            b',' => ops.push(Op::In(ptr)),
            b'.' => ops.push(Op::Out(ptr)),
            b'[' => {
                let at = ops.len();
                ops.push(Op::JumpZero(ptr, usize::MAX));
                stack.push(at);
            }
            b']' => {
                let open = stack
                    .pop()
                    .ok_or(BackendError::UnmatchedBracket(source_ip))?;
                let close = ops.len();
                ops.push(Op::JumpNonZero(ptr, open));
                ops[open] = Op::JumpZero(ptr, close + 1);
            }
            _ => unreachable!(),
        }
    }
    if let Some(i) = stack.pop() {
        return Err(BackendError::UnmatchedBracket(i));
    }
    Ok(ops)
}

fn compress_runs(raw: &[Op]) -> Vec<Op> {
    let mut out = Vec::new();
    let mut raw_to_out = vec![0usize; raw.len() + 1];
    let mut i = 0;
    while i < raw.len() {
        let out_index = out.len();
        raw_to_out[i] = out_index;
        let mut j = i + 1;
        let compressed = match raw[i] {
            Op::Move(first, 1) => {
                let mut final_cell = first;
                let mut direction = None;
                while let Some(Op::Move(next, 1)) = raw.get(j) {
                    let delta = *next as isize - final_cell as isize;
                    if delta.abs() != 1 || direction.is_some_and(|d| d != delta) {
                        break;
                    }
                    direction = Some(delta);
                    final_cell = *next;
                    raw_to_out[j] = out_index;
                    j += 1;
                }
                Op::Move(final_cell, (j - i) as u32)
            }
            Op::Inc(cell, 1) => {
                while matches!(raw.get(j), Some(Op::Inc(c, 1)) if *c == cell) {
                    raw_to_out[j] = out_index;
                    j += 1;
                }
                Op::Inc(cell, (j - i) as u32)
            }
            Op::Dec(cell, 1) => {
                while matches!(raw.get(j), Some(Op::Dec(c, 1)) if *c == cell) {
                    raw_to_out[j] = out_index;
                    j += 1;
                }
                Op::Dec(cell, (j - i) as u32)
            }
            ref op => op.clone(),
        };
        out.push(compressed);
        i = j;
    }
    raw_to_out[raw.len()] = out.len();
    remap_jumps(&mut out, &raw_to_out);
    out
}

fn omit_moves(raw: &[Op]) -> Vec<Op> {
    let mut out = Vec::new();
    let mut raw_to_out = vec![0usize; raw.len() + 1];
    for (i, op) in raw.iter().enumerate() {
        raw_to_out[i] = out.len();
        if !matches!(op, Op::Move(_, _)) {
            out.push(op.clone());
        }
    }
    raw_to_out[raw.len()] = out.len();
    remap_jumps(&mut out, &raw_to_out);
    out
}

fn remap_jumps(ops: &mut [Op], raw_to_out: &[usize]) {
    for op in ops {
        match op {
            Op::JumpZero(_, target) | Op::JumpNonZero(_, target) => {
                *target = raw_to_out[*target];
            }
            _ => {}
        }
    }
}

fn ensure_step(steps: &mut u64, cap: u64) -> Result<(), BackendError> {
    if *steps >= cap {
        return Err(BackendError::NonTerminating);
    }
    *steps += 1;
    Ok(())
}

fn push_trace(points: &mut Vec<BackendTracePoint>, step: u64, ip: usize, cell: usize, value: u8) {
    points.push(BackendTracePoint {
        step,
        ip,
        cell,
        value,
    });
}

fn check(c: usize) -> Result<(), BackendError> {
    if c >= 30_000 {
        Err(BackendError::BadCell(c))
    } else {
        Ok(())
    }
}

fn e2(o: &Op) -> String {
    match o {
        Op::Move(c, n) => format!("M{c}{}", compact_suffix(*n)),
        Op::Inc(c, n) => format!("+{c}{}", compact_suffix(*n)),
        Op::Dec(c, n) => format!("-{c}{}", compact_suffix(*n)),
        Op::In(c) => format!("I{c}"),
        Op::Out(c) => format!("O{c}"),
        Op::JumpZero(c, t) => format!("Z{c}>{t}"),
        Op::JumpNonZero(c, t) => format!("N{c}>{t}"),
    }
}

fn e3(o: &Op) -> String {
    match o {
        Op::Move(c, n) => format!("MOVE cell {c} count {n}"),
        Op::Inc(c, n) => format!("ADD cell {c} count {n}"),
        Op::Dec(c, n) => format!("SUBTRACT cell {c} count {n}"),
        Op::In(c) => format!("INPUT cell {c}"),
        Op::Out(c) => format!("OUTPUT cell {c}"),
        Op::JumpZero(c, t) => format!("JUMP_IF_ZERO cell {c} target {t}"),
        Op::JumpNonZero(c, t) => format!("JUMP_IF_NONZERO cell {c} target {t}"),
    }
}

fn e4(o: &Op) -> String {
    match o {
        Op::Move(c, n) => format!("advance the trace cursor {n} steps to explicit cell q{c}"),
        Op::Inc(c, n) => format!("increase opaque variable q{c} by {n}, wrapping at 255"),
        Op::Dec(c, n) => format!("decrease opaque variable q{c} by {n}, wrapping at 0"),
        Op::In(c) => format!("read the next input into opaque variable q{c}"),
        Op::Out(c) => format!("emit opaque variable q{c}"),
        Op::JumpZero(c, t) => format!("if q{c} is zero, continue at step {}", t + 1),
        Op::JumpNonZero(c, t) => format!("if q{c} is nonzero, continue at step {}", t + 1),
    }
}

fn compact_suffix(count: u32) -> String {
    if count > 1 {
        format!("*{count}")
    } else {
        String::new()
    }
}

fn carrier_header(carrier: MoveCarrier) -> &'static str {
    match carrier {
        MoveCarrier::Rle => "@rle",
        MoveCarrier::Expanded => "@expanded",
        MoveCarrier::Omitted => "@omitted",
    }
}

fn parse_carrier_header(source: &str) -> Result<MoveCarrier, BackendError> {
    match source {
        "@rle" => Ok(MoveCarrier::Rle),
        "@expanded" => Ok(MoveCarrier::Expanded),
        "@omitted" => Ok(MoveCarrier::Omitted),
        _ => Err(BackendError::MalformedEncoding(source.into())),
    }
}

fn parse_counted<'a>(body: &'a str, marker: &str) -> Result<(&'a str, u32), BackendError> {
    if let Some((head, count)) = body.rsplit_once(marker) {
        let count = count
            .parse::<u32>()
            .map_err(|_| BackendError::MalformedEncoding(body.into()))?;
        if count == 0 {
            return Err(BackendError::MalformedEncoding(body.into()));
        }
        Ok((head, count))
    } else {
        Ok((body, 1))
    }
}

fn parse_e2_op(body: &str) -> Result<Op, BackendError> {
    for (prefix, jump_nonzero) in [("Z", false), ("N", true)] {
        if let Some(rest) = body.strip_prefix(prefix) {
            let (cell, target) = rest
                .split_once('>')
                .ok_or_else(|| BackendError::MalformedEncoding(body.into()))?;
            return if jump_nonzero {
                Ok(Op::JumpNonZero(
                    parse_usize(cell, body)?,
                    parse_usize(target, body)?,
                ))
            } else {
                Ok(Op::JumpZero(
                    parse_usize(cell, body)?,
                    parse_usize(target, body)?,
                ))
            };
        }
    }
    let (head, count) = parse_counted(body, "*")?;
    if let Some(cell) = head.strip_prefix('M') {
        return Ok(Op::Move(parse_usize(cell, body)?, count));
    }
    if let Some(cell) = head.strip_prefix('+') {
        return Ok(Op::Inc(parse_usize(cell, body)?, count));
    }
    if let Some(cell) = head.strip_prefix('-') {
        return Ok(Op::Dec(parse_usize(cell, body)?, count));
    }
    if let Some(cell) = head.strip_prefix('I').filter(|_| count == 1) {
        return Ok(Op::In(parse_usize(cell, body)?));
    }
    if let Some(cell) = head.strip_prefix('O').filter(|_| count == 1) {
        return Ok(Op::Out(parse_usize(cell, body)?));
    }
    Err(BackendError::MalformedEncoding(body.into()))
}

fn parse_e3_op(body: &str) -> Result<Op, BackendError> {
    let words = body.split_whitespace().collect::<Vec<_>>();
    match words.as_slice() {
        ["MOVE", "cell", cell, "count", count] => {
            Ok(Op::Move(parse_usize(cell, body)?, parse_u32(count, body)?))
        }
        ["ADD", "cell", cell, "count", count] => {
            Ok(Op::Inc(parse_usize(cell, body)?, parse_u32(count, body)?))
        }
        ["SUBTRACT", "cell", cell, "count", count] => {
            Ok(Op::Dec(parse_usize(cell, body)?, parse_u32(count, body)?))
        }
        ["INPUT", "cell", cell] => Ok(Op::In(parse_usize(cell, body)?)),
        ["OUTPUT", "cell", cell] => Ok(Op::Out(parse_usize(cell, body)?)),
        ["JUMP_IF_ZERO", "cell", cell, "target", target] => Ok(Op::JumpZero(
            parse_usize(cell, body)?,
            parse_usize(target, body)?,
        )),
        ["JUMP_IF_NONZERO", "cell", cell, "target", target] => Ok(Op::JumpNonZero(
            parse_usize(cell, body)?,
            parse_usize(target, body)?,
        )),
        _ => Err(BackendError::MalformedEncoding(body.into())),
    }
}

fn parse_u32(text: &str, context: &str) -> Result<u32, BackendError> {
    let value = text
        .parse()
        .map_err(|_| BackendError::MalformedEncoding(context.into()))?;
    if value == 0 {
        return Err(BackendError::MalformedEncoding(context.into()));
    }
    Ok(value)
}

fn parse_usize(text: &str, context: &str) -> Result<usize, BackendError> {
    text.parse()
        .map_err(|_| BackendError::MalformedEncoding(context.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bf;

    #[test]
    fn explicit_backend_matches_bf_for_rle_and_expanded() {
        let s = ",[->+++++<]>.";
        for carrier in [MoveCarrier::Rle, MoveCarrier::Expanded] {
            let b = Bytecode::from_e0_with_carrier(s, carrier).unwrap();
            for x in 0..=255u8 {
                let explicit = b.execute(&[x], 1_000_000).unwrap();
                let implicit = bf::execute(s, &[x], 1_000_000, false).unwrap();
                assert_eq!(explicit.output, implicit.state.output);
                assert_eq!(explicit.steps, implicit.state.steps);
            }
        }
    }

    #[test]
    fn omitted_carrier_preserves_values_but_reports_fewer_steps() {
        let s = ",[->+<]>.";
        let b = Bytecode::from_e0_with_carrier(s, MoveCarrier::Omitted).unwrap();
        let explicit = b.execute(&[13], 1_000_000).unwrap();
        let implicit = bf::execute(s, &[13], 1_000_000, false).unwrap();
        assert_eq!(explicit.output, implicit.state.output);
        assert!(explicit.steps < implicit.state.steps);
    }

    #[test]
    fn rendered_backends_are_independently_parseable_for_all_carriers() {
        for carrier in [
            MoveCarrier::Rle,
            MoveCarrier::Expanded,
            MoveCarrier::Omitted,
        ] {
            let b = Bytecode::from_e0_with_carrier(",[->+++++<]>.", carrier).unwrap();
            assert_eq!(Bytecode::parse_e2(&b.e2_source()).unwrap(), b);
            assert_eq!(Bytecode::parse_e3(&b.e3_source()).unwrap(), b);
        }
    }

    #[test]
    fn rle_is_materially_smaller_than_expanded() {
        let source = ">>>>>>>>>>>>>>>>++++++++++++++++++++<<<<<<<<<<<<<<<<";
        let rle = Bytecode::from_e0_with_carrier(source, MoveCarrier::Rle).unwrap();
        let expanded = Bytecode::from_e0_with_carrier(source, MoveCarrier::Expanded).unwrap();
        assert!(rle.e2_source().len() * 3 < expanded.e2_source().len());
        assert!(rle.e3_source().len() * 3 < expanded.e3_source().len());
    }
}
