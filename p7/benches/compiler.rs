use std::hint::black_box;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use p7::{
    InMemoryModuleProvider,
    lexer::{Lexer, Token, TokenType},
    parser::Parser,
};

const FRONTEND_LARGE: &str = include_str!("fixtures/frontend_large.p7");
const IMPORT_MAIN: &str = include_str!("fixtures/import_main.p7");
const IMPORT_MATH: &str = include_str!("fixtures/bench_math.p7");
const IMPORT_TYPES: &str = include_str!("fixtures/bench_types.p7");

const SHARED_IMPORT: &str = r#"
pub fn seed() -> int {
    7
}

pub struct SharedValue(
    pub value: int,
);
"#;

const BUILTIN_HEAVY: &str = r#"
fn main() -> int {
    let base = "  alpha,beta,gamma,delta  ";
    let trimmed = base.trim();
    trimmed.len_bytes()
        + trimmed.len_chars()
        + trimmed.substring(0, 5).len_bytes()
        + trimmed.index_of("gamma")
        + if trimmed.contains("beta") { 1 } else { 0 }
}
"#;

fn tokenize(source: &str) -> Vec<Token> {
    let mut lexer = Lexer::new(source.to_string());
    let mut tokens = Vec::new();

    loop {
        let token = lexer.next_token();
        if token.token_type == TokenType::EOF {
            break;
        }
        tokens.push(token);
    }

    tokens
}

fn parse(source: &str) -> usize {
    let tokens = tokenize(source);
    let mut parser = Parser::new(tokens);
    parser.parse().expect("benchmark source should parse").len()
}

fn import_provider() -> InMemoryModuleProvider {
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module("bench.math".to_string(), IMPORT_MATH.to_string());
    provider.add_module("bench.types".to_string(), IMPORT_TYPES.to_string());
    provider
}

fn wide_import_graph(width: usize) -> (String, InMemoryModuleProvider) {
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module("bench.shared".to_string(), SHARED_IMPORT.to_string());

    let mut main = String::new();
    for idx in 0..width {
        let path = format!("bench.leaf{idx}");
        provider.add_module(
            path.clone(),
            format!(
                r#"
import bench.shared;

pub fn value() -> int {{
    shared.seed() + {idx}
}}
"#
            ),
        );
        main.push_str(&format!("import {path};\n"));
    }

    main.push_str("\nfn main() -> int {\n    0");
    for idx in 0..width {
        main.push_str(&format!(" + leaf{idx}.value()"));
    }
    main.push_str("\n}\n");

    (main, provider)
}

fn repeated_identifier_source(repetitions: usize) -> String {
    let mut source = String::from(
        r#"
fn repeated_identifier_workload() -> int {
    let repeated_name = "same-literal-value";
    let mut total = repeated_name.len_bytes();
"#,
    );

    for idx in 0..repetitions {
        source.push_str(&format!(
            r#"    let repeated_binding_{idx} = "same-literal-value";
    total = total + repeated_binding_{idx}.len_bytes() + repeated_name.len_chars();
"#
        ));
    }

    source.push_str("    total\n}\n");
    source
}

fn compiler_benches(c: &mut Criterion) {
    // Focused baselines for module-loading and string/frontend work. Compare
    // these names before/after each optimization in this area:
    // compile_wide_shared_import_graph, compile_builtin_string_heavy,
    // lexer_repeated_identifiers_and_strings, parser_repeated_identifiers_and_strings.
    let repeated_source = repeated_identifier_source(256);

    let mut frontend = c.benchmark_group("frontend");
    frontend.throughput(Throughput::Bytes(FRONTEND_LARGE.len() as u64));
    frontend.bench_function("lexer_large_source", |b| {
        b.iter(|| black_box(tokenize(black_box(FRONTEND_LARGE)).len()))
    });
    frontend.bench_function("parser_large_source", |b| {
        b.iter(|| black_box(parse(black_box(FRONTEND_LARGE))))
    });
    frontend.throughput(Throughput::Bytes(repeated_source.len() as u64));
    frontend.bench_function("lexer_repeated_identifiers_and_strings", |b| {
        b.iter(|| black_box(tokenize(black_box(&repeated_source)).len()))
    });
    frontend.bench_function("parser_repeated_identifiers_and_strings", |b| {
        b.iter(|| black_box(parse(black_box(&repeated_source))))
    });
    frontend.finish();

    let mut compile = c.benchmark_group("compile");
    compile.throughput(Throughput::Bytes(FRONTEND_LARGE.len() as u64));
    compile.bench_function("compile_basic_module", |b| {
        b.iter(|| black_box(p7::compile(black_box(FRONTEND_LARGE.to_string())).unwrap()))
    });

    compile.throughput(Throughput::Bytes(IMPORT_MAIN.len() as u64));
    compile.bench_function("compile_import_graph", |b| {
        b.iter_batched(
            import_provider,
            |provider| {
                black_box(
                    p7::compile_with_provider(
                        black_box(IMPORT_MAIN.to_string()),
                        Box::new(provider),
                    )
                    .unwrap(),
                )
            },
            BatchSize::SmallInput,
        )
    });
    compile.bench_function("compile_wide_shared_import_graph", |b| {
        b.iter_batched(
            || wide_import_graph(24),
            |(source, provider)| {
                black_box(p7::compile_with_provider(black_box(source), Box::new(provider)).unwrap())
            },
            BatchSize::SmallInput,
        )
    });
    compile.throughput(Throughput::Bytes(BUILTIN_HEAVY.len() as u64));
    compile.bench_function("compile_builtin_string_heavy", |b| {
        b.iter(|| black_box(p7::compile(black_box(BUILTIN_HEAVY.to_string())).unwrap()))
    });
    compile.finish();
}

criterion_group!(benches, compiler_benches);
criterion_main!(benches);
