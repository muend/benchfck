use benchfck::{
    backend::Bytecode,
    compiler::{LayoutDiscipline, compile},
    ir::{LoopClass, Program, Statement},
    oracle::exhaustive_validate,
};

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

/// Central M3 property suite. It is ignored in the fast developer test set
/// because it performs 2.56 million exhaustive input bindings.
fn validate_range(limit: u64) {
    for n in 0..limit {
        let p = program(n);
        let d = match n % 3 {
            0 => LayoutDiscipline::Contiguous,
            1 => LayoutDiscipline::Interleaved,
            _ => LayoutDiscipline::Strided,
        };
        let c = compile(&p, n, d).unwrap();
        let b = Bytecode::from_e0(&c.e0).unwrap();
        exhaustive_validate(&p, &c.e0, &b.e2_source(), &b.e3_source(), 1_000_000)
            .unwrap_or_else(|error| panic!("n={n}, layout={d:?}, error={error}, IR={p:?}"));
    }
}

#[test]
fn five_hundred_program_fast_property_shard() {
    validate_range(500);
}

#[test]
#[ignore = "extended 10k-program exhaustive compiler validation"]
fn ten_thousand_ir_programs_match_every_backend_over_the_full_domain() {
    validate_range(10_000);
}
