use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use p7::{
    lexer::{Lexer, Token, TokenType},
    parser::Parser,
    InMemoryModuleProvider,
};

const FRONTEND_LARGE: &str = include_str!("fixtures/frontend_large.p7");
const IMPORT_MAIN: &str = include_str!("fixtures/import_main.p7");
const IMPORT_MATH: &str = include_str!("fixtures/bench_math.p7");
const IMPORT_TYPES: &str = include_str!("fixtures/bench_types.p7");

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

fn compiler_benches(c: &mut Criterion) {
    let mut frontend = c.benchmark_group("frontend");
    frontend.throughput(Throughput::Bytes(FRONTEND_LARGE.len() as u64));
    frontend.bench_function("lexer_large_source", |b| {
        b.iter(|| black_box(tokenize(black_box(FRONTEND_LARGE)).len()))
    });
    frontend.bench_function("parser_large_source", |b| {
        b.iter(|| black_box(parse(black_box(FRONTEND_LARGE))))
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
    compile.finish();
}

criterion_group!(benches, compiler_benches);
criterion_main!(benches);
