use crate::ir::{Program, Statement};
use rand::{Rng, SeedableRng, seq::SliceRandom};
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LayoutDiscipline {
    Contiguous,
    Interleaved,
    Strided,
    HeldOut,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Layout {
    pub discipline: LayoutDiscipline,
    pub variables: Vec<usize>,
    pub temporaries: Vec<usize>,
}

impl Layout {
    pub fn build(variable_count: usize, discipline: LayoutDiscipline) -> Self {
        // The first entries are the stable high-frequency access graph used by
        // the semantic constructors (one, counter, remainder, difference,
        // modulus, gate, quotient, output, inputs). Remaining variables retain
        // logical order. Disciplines permute the same compact physical span;
        // they no longer manufacture difficulty by inserting empty tape gaps.
        let preferred = [4usize, 3, 5, 8, 7, 9, 6, 2, 0, 1, 10, 11, 12, 13, 14];
        let mut logical_order = preferred
            .into_iter()
            .filter(|logical| *logical < variable_count)
            .collect::<Vec<_>>();
        let missing = (0..variable_count)
            .filter(|logical| !logical_order.contains(logical))
            .collect::<Vec<_>>();
        logical_order.extend(missing);
        let mut physical = (1..=variable_count).collect::<Vec<_>>();
        match discipline {
            LayoutDiscipline::Contiguous => {}
            LayoutDiscipline::Interleaved => {
                for pair in physical.chunks_mut(2) {
                    if pair.len() == 2 {
                        pair.swap(0, 1);
                    }
                }
            }
            LayoutDiscipline::Strided => physical.rotate_left(1),
            LayoutDiscipline::HeldOut => physical.reverse(),
        }
        let mut variables = vec![0usize; variable_count];
        for (logical, cell) in logical_order.into_iter().zip(physical) {
            variables[logical] = cell;
        }
        let used: BTreeSet<_> = variables.iter().copied().collect();
        let temporaries = (0..=(variable_count * 2 + 16))
            .filter(|x| !used.contains(x))
            .take(16)
            .collect();
        Self {
            discipline,
            variables,
            temporaries,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompilerMetadata {
    pub seed: u64,
    pub layout: Layout,
    pub statement_templates: Vec<u8>,
    pub obfuscation_passes: Vec<String>,
    pub final_pointer: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompiledProgram {
    pub e0: String,
    pub e1: String,
    pub permutation: Vec<(char, char)>,
    pub metadata: CompilerMetadata,
}

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("variable index {0} is out of range")]
    BadVariable(usize),
    #[error("temporary-cell budget exhausted")]
    NoTemporary,
    #[error("pointer invariant failed after statement: expected 0, got {0}")]
    PointerInvariant(usize),
}

pub fn structural_obfuscate(program: &Program, seed: u64) -> (Program, Vec<String>) {
    let mut rng = ChaCha20Rng::seed_from_u64(seed ^ 0xD3D3_D3D3);
    fn rw(s: &Statement) -> (BTreeSet<usize>, BTreeSet<usize>, bool) {
        use Statement::*;
        let mut r = BTreeSet::new();
        let mut w = BTreeSet::new();
        let mut effect = false;
        match s {
            Set { dst, .. } | In { dst } => {
                w.insert(*dst);
            }
            Copy { dst, src } => {
                r.insert(*src);
                w.insert(*dst);
            }
            Add { dst, src } | Sub { dst, src } => {
                r.insert(*dst);
                r.insert(*src);
                w.insert(*dst);
            }
            DrainScaled { dst, src, .. } => {
                r.insert(*dst);
                r.insert(*src);
                w.insert(*dst);
                w.insert(*src);
            }
            Out { src } => {
                r.insert(*src);
                effect = true;
            }
            While { cond, .. } | If { cond, .. } | IfNonZeroDrain { cond, .. } => {
                r.insert(*cond);
                effect = true;
            }
        }
        (r, w, effect)
    }
    fn independent(a: &Statement, b: &Statement) -> bool {
        let (ar, aw, ae) = rw(a);
        let (br, bw, be) = rw(b);
        !ae && !be && aw.is_disjoint(&br) && aw.is_disjoint(&bw) && bw.is_disjoint(&ar)
    }
    let mut out = program.clone();
    let mut changed = false;
    let mut i = 0;
    while i + 1 < out.body.len() {
        if independent(&out.body[i], &out.body[i + 1]) && rng.random_bool(0.5) {
            out.body.swap(i, i + 1);
            changed = true;
            i += 2;
        } else {
            i += 1;
        }
    }
    (
        out,
        if changed {
            vec!["reorder_independent_statements".into()]
        } else {
            vec![]
        },
    )
}

pub fn compile(
    program: &Program,
    seed: u64,
    discipline: LayoutDiscipline,
) -> Result<CompiledProgram, CompileError> {
    let layout = Layout::build(program.variables.len(), discipline);
    let mut c = Compiler {
        code: String::new(),
        pointer: 0,
        layout: layout.clone(),
        free: layout.temporaries.clone(),
        rng: ChaCha20Rng::seed_from_u64(seed),
        templates: vec![],
    };
    c.compile_block(&program.body)?;
    c.move_to(0);
    if c.pointer != 0 {
        return Err(CompileError::PointerInvariant(c.pointer));
    }
    let e0 = c.code;
    let symbols = ['+', '-', '>', '<', '[', ']', ',', '.'];
    let mut perm = symbols;
    perm.shuffle(&mut c.rng);
    let permutation: Vec<_> = symbols.into_iter().zip(perm).collect();
    let e1 = e0
        .chars()
        .map(|x| {
            permutation
                .iter()
                .find(|(a, _)| *a == x)
                .map(|(_, b)| *b)
                .unwrap_or(x)
        })
        .collect();
    Ok(CompiledProgram {
        e0,
        e1,
        permutation,
        metadata: CompilerMetadata {
            seed,
            layout,
            statement_templates: c.templates,
            obfuscation_passes: vec![],
            final_pointer: c.pointer,
        },
    })
}

struct Compiler {
    code: String,
    pointer: usize,
    layout: Layout,
    free: Vec<usize>,
    rng: ChaCha20Rng,
    templates: Vec<u8>,
}

impl Compiler {
    fn emit(&mut self, c: char) {
        self.code.push(c);
    }
    fn move_to(&mut self, dst: usize) {
        while self.pointer < dst {
            self.emit('>');
            self.pointer += 1;
        }
        while self.pointer > dst {
            self.emit('<');
            self.pointer -= 1;
        }
    }
    fn cell(&self, v: usize) -> Result<usize, CompileError> {
        self.layout
            .variables
            .get(v)
            .copied()
            .ok_or(CompileError::BadVariable(v))
    }
    fn alloc(&mut self) -> Result<usize, CompileError> {
        let index = self
            .free
            .iter()
            .enumerate()
            .min_by_key(|(_, cell)| (self.pointer.abs_diff(**cell), **cell))
            .map(|(index, _)| index)
            .ok_or(CompileError::NoTemporary)?;
        Ok(self.free.remove(index))
    }
    fn alloc_variant(&mut self, variant: u8) -> Result<usize, CompileError> {
        if self.free.is_empty() {
            return Err(CompileError::NoTemporary);
        }
        let mut choices = self
            .free
            .iter()
            .enumerate()
            .map(|(index, cell)| (index, self.pointer.abs_diff(*cell), *cell))
            .collect::<Vec<_>>();
        choices.sort_by_key(|(_, distance, cell)| (*distance, *cell));
        let index = choices[usize::from(variant) % choices.len().min(3)].0;
        Ok(self.free.remove(index))
    }
    fn release(&mut self, x: usize) {
        self.free.push(x);
    }
    fn clear(&mut self, cell: usize, variant: u8) {
        self.move_to(cell);
        self.emit('[');
        self.emit(if variant.is_multiple_of(2) { '-' } else { '+' });
        self.emit(']');
    }
    fn delta(&mut self, cell: usize, value: u8, variant: u8) {
        self.move_to(cell);
        if variant.is_multiple_of(2) || value <= 128 {
            for _ in 0..value {
                self.emit('+');
            }
        } else {
            for _ in 0..(256u16 - value as u16) {
                self.emit('-');
            }
        }
    }
    fn copy_core(&mut self, dst: usize, src: usize, tmp: usize, subtract: bool) {
        self.clear(dst, 0);
        self.clear(tmp, 0);
        self.move_to(src);
        self.emit('[');
        self.emit('-');
        self.move_to(dst);
        self.emit(if subtract { '-' } else { '+' });
        self.move_to(tmp);
        self.emit('+');
        self.move_to(src);
        self.emit(']');
        self.move_to(tmp);
        self.emit('[');
        self.emit('-');
        self.move_to(src);
        self.emit('+');
        self.move_to(tmp);
        self.emit(']');
    }
    fn copy_core_alt(&mut self, dst: usize, src: usize, tmp: usize, subtract: bool) {
        self.clear(dst, 1);
        self.clear(tmp, 1);
        self.move_to(src);
        self.emit('[');
        self.emit('-');
        self.move_to(tmp);
        self.emit('+');
        self.move_to(dst);
        self.emit(if subtract { '-' } else { '+' });
        self.move_to(src);
        self.emit(']');
        self.move_to(tmp);
        self.emit('[');
        self.emit('-');
        self.move_to(src);
        self.emit('+');
        self.move_to(tmp);
        self.emit(']');
    }
    fn add_core(&mut self, dst: usize, src: usize, tmp: usize, subtract: bool) {
        self.clear(tmp, 0);
        self.move_to(src);
        self.emit('[');
        self.emit('-');
        self.move_to(dst);
        self.emit(if subtract { '-' } else { '+' });
        self.move_to(tmp);
        self.emit('+');
        self.move_to(src);
        self.emit(']');
        self.move_to(tmp);
        self.emit('[');
        self.emit('-');
        self.move_to(src);
        self.emit('+');
        self.move_to(tmp);
        self.emit(']');
    }
    fn booleanize(&mut self, cond: usize, scan: usize, gate: usize, variant: u8) {
        if variant == 2 {
            self.copy_core_alt(scan, cond, gate, false);
        } else {
            self.copy_core(scan, cond, gate, false);
        }
        self.move_to(scan);
        self.emit('[');
        self.clear(scan, variant);
        self.move_to(gate);
        self.emit('+');
        self.move_to(scan);
        self.emit(']');
    }
    fn compile_block(&mut self, xs: &[Statement]) -> Result<(), CompileError> {
        for s in xs {
            self.compile_stmt(s)?;
        }
        Ok(())
    }
    fn compile_stmt(&mut self, s: &Statement) -> Result<(), CompileError> {
        use Statement::*;
        let variant = self.rng.random_range(0..3);
        self.templates.push(variant);
        match s {
            Set { dst, value } => {
                let d = self.cell(*dst)?;
                if variant < 2 {
                    self.clear(d, variant);
                    self.delta(d, *value, variant);
                } else {
                    let t = self.alloc_variant(variant)?;
                    self.clear(d, variant);
                    self.clear(t, variant);
                    self.delta(t, *value, variant);
                    self.move_to(t);
                    self.emit('[');
                    self.emit('-');
                    self.move_to(d);
                    self.emit('+');
                    self.move_to(t);
                    self.emit(']');
                    self.release(t);
                }
            }
            Copy { dst, src } => {
                let d = self.cell(*dst)?;
                let x = self.cell(*src)?;
                if d != x {
                    let t = self.alloc_variant(variant)?;
                    if variant == 0 {
                        self.copy_core(d, x, t, false);
                        self.release(t);
                    } else if variant == 1 {
                        self.copy_core_alt(d, x, t, false);
                        self.release(t);
                    } else {
                        let r = self.alloc_variant(variant)?;
                        self.copy_core(t, x, r, false);
                        self.clear(d, variant);
                        self.move_to(t);
                        self.emit('[');
                        self.emit('-');
                        self.move_to(d);
                        self.emit('+');
                        self.move_to(t);
                        self.emit(']');
                        self.release(r);
                        self.release(t);
                    }
                }
            }
            Add { dst, src } | Sub { dst, src } => {
                let d = self.cell(*dst)?;
                let x = self.cell(*src)?;
                let sub = matches!(s, Sub { .. });
                if d == x {
                    if sub {
                        self.clear(d, variant);
                    } else {
                        let t = self.alloc()?;
                        let r = self.alloc()?;
                        self.copy_core(t, x, r, false);
                        self.add_core(d, t, r, false);
                        self.release(r);
                        self.release(t);
                    }
                } else if variant == 0 {
                    let t = self.alloc_variant(variant)?;
                    self.add_core(d, x, t, sub);
                    self.release(t);
                } else if variant == 1 {
                    let t = self.alloc_variant(variant)?;
                    let r = self.alloc_variant(variant)?;
                    self.copy_core(t, x, r, false);
                    self.add_core(d, t, r, sub);
                    self.release(r);
                    self.release(t);
                } else {
                    let t = self.alloc_variant(variant)?;
                    self.clear(t, variant);
                    self.move_to(x);
                    self.emit('[');
                    self.emit('-');
                    self.move_to(t);
                    self.emit('+');
                    self.move_to(d);
                    self.emit(if sub { '-' } else { '+' });
                    self.move_to(x);
                    self.emit(']');
                    self.move_to(t);
                    self.emit('[');
                    self.emit('-');
                    self.move_to(x);
                    self.emit('+');
                    self.move_to(t);
                    self.emit(']');
                    self.release(t);
                }
            }
            DrainScaled {
                dst,
                src,
                factor,
                subtract,
            } => {
                let d = self.cell(*dst)?;
                let x = self.cell(*src)?;
                if d == x {
                    self.clear(d, variant);
                } else {
                    self.move_to(x);
                    self.emit('[');
                    self.emit('-');
                    self.move_to(d);
                    for _ in 0..*factor {
                        self.emit(if *subtract { '-' } else { '+' });
                    }
                    self.move_to(x);
                    self.emit(']');
                }
            }
            In { dst } => {
                let d = self.cell(*dst)?;
                if variant == 0 {
                    self.move_to(d);
                    self.emit(',');
                } else if variant == 1 {
                    let t = self.alloc_variant(variant)?;
                    self.move_to(t);
                    self.emit(',');
                    let r = self.alloc_variant(variant)?;
                    self.copy_core(d, t, r, false);
                    self.clear(t, variant);
                    self.release(r);
                    self.release(t);
                } else {
                    let t = self.alloc_variant(variant)?;
                    self.move_to(t);
                    self.emit(',');
                    self.clear(d, variant);
                    self.move_to(t);
                    self.emit('[');
                    self.emit('-');
                    self.move_to(d);
                    self.emit('+');
                    self.move_to(t);
                    self.emit(']');
                    self.release(t);
                }
            }
            Out { src } => {
                let x = self.cell(*src)?;
                if variant == 0 {
                    self.move_to(x);
                    self.emit('.');
                } else {
                    let t = self.alloc_variant(variant)?;
                    let r = self.alloc_variant(variant)?;
                    if variant == 1 {
                        self.copy_core(t, x, r, false);
                    } else {
                        self.copy_core_alt(t, x, r, false);
                    }
                    self.move_to(t);
                    self.emit('.');
                    self.clear(t, variant);
                    self.release(r);
                    self.release(t);
                }
            }
            While { cond, body, .. } => {
                let c = self.cell(*cond)?;
                // Brainfuck's native loop has exactly the IR while semantics;
                // the former gate/scan lowering duplicated state traffic on
                // every iteration and turned template choice into a movement
                // confound rather than a semantic-preserving surface variant.
                self.move_to(c);
                self.emit('[');
                self.compile_block(body)?;
                self.move_to(c);
                self.emit(']');
            }
            If { cond, body } => {
                let c = self.cell(*cond)?;
                let scan = self.alloc_variant(variant)?;
                let gate = self.alloc_variant(variant)?;
                if variant == 0 {
                    self.copy_core(scan, c, gate, false);
                    self.move_to(scan);
                    self.emit('[');
                    self.clear(scan, variant);
                    self.compile_block(body)?;
                    self.move_to(scan);
                    self.emit(']');
                } else {
                    self.booleanize(c, scan, gate, variant);
                    self.move_to(gate);
                    self.emit('[');
                    self.emit('-');
                    self.compile_block(body)?;
                    self.move_to(gate);
                    self.emit(']');
                }
                self.release(gate);
                self.release(scan);
            }
            IfNonZeroDrain { cond, body } => {
                let c = self.cell(*cond)?;
                self.move_to(c);
                self.emit('[');
                self.clear(c, variant);
                self.compile_block(body)?;
                self.move_to(c);
                self.emit(']');
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bf, ir};
    fn p(stmt: Statement) -> Program {
        Program {
            arity: 1,
            output_arity: 1,
            variables: vec!["v0".into(), "v1".into()],
            body: vec![
                Statement::In { dst: 0 },
                Statement::Set { dst: 1, value: 3 },
                stmt,
                Statement::Out { src: 0 },
            ],
        }
    }
    #[test]
    fn golden_statement_semantics_across_seeds() {
        for seed in 0..12 {
            for program in [
                p(Statement::Add { dst: 0, src: 1 }),
                p(Statement::Sub { dst: 0, src: 1 }),
                p(Statement::Copy { dst: 0, src: 1 }),
            ] {
                let c = compile(&program, seed, LayoutDiscipline::Interleaved).unwrap();
                let expected = ir::execute(&program, &[254], 1000).unwrap().output;
                assert_eq!(
                    bf::execute(&c.e0, &[254], 1_000_000, false)
                        .unwrap()
                        .state
                        .output,
                    expected
                );
            }
        }
    }
    #[test]
    fn layouts_change_code_not_meaning() {
        let program = p(Statement::Add { dst: 0, src: 1 });
        let a = compile(&program, 1, LayoutDiscipline::Contiguous).unwrap();
        let b = compile(&program, 2, LayoutDiscipline::Strided).unwrap();
        assert_ne!(a.e0, b.e0);
        assert_eq!(
            bf::execute(&a.e0, &[9], 1_000_000, false)
                .unwrap()
                .state
                .output,
            bf::execute(&b.e0, &[9], 1_000_000, false)
                .unwrap()
                .state
                .output
        );
    }
}
