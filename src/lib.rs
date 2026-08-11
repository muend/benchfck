//! benchfck is a Brainfuck-based benchmark generator and exact execution harness.
//! It measures machine-state tracking, structure extraction, causal reasoning,
//! and computation compression. It does not use model-based grading.

pub mod backend;
pub mod bf;
pub mod compiler;
pub mod config;
pub mod generator;
pub mod ir;
pub mod metrics;
pub mod oracle;
pub mod schema;
pub mod tasks;

pub const PINNED_SEMANTICS: &str = "cell = 8-bit unsigned, wraps at 255→0 and 0→255\ntape = 30,000 cells, pointer starts at 0, moving left of 0 is a hard error\n`,` on exhausted input sets the cell to 0\nstep cap = 1,000,000; exceeding it classifies the run as `NON_TERMINATING`";
