use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use p7::{bytecode::Module, interpreter::context::Data};

const NUMERIC_LOOP: &str = include_str!("fixtures/runtime_numeric_loop.p7");
const FUNCTION_CALLS: &str = include_str!("fixtures/runtime_function_calls.p7");
const RECURSIVE_CALLS: &str = include_str!("fixtures/runtime_recursive_calls.p7");
const DYNAMIC_DISPATCH: &str = include_str!("fixtures/runtime_dynamic_dispatch.p7");
const STRUCT_FIELDS: &str = include_str!("fixtures/runtime_struct_fields.p7");
const ARRAYS: &str = include_str!("fixtures/runtime_arrays.p7");
const HASHMAPS: &str = include_str!("fixtures/runtime_hashmaps.p7");
const HASHMAPS_SMALL_INT: &str = include_str!("fixtures/runtime_hashmaps_small_int.p7");
const HASHMAPS_LARGE_INT: &str = include_str!("fixtures/runtime_hashmaps_large_int.p7");
const HASHMAPS_STRING_KEYS: &str = include_str!("fixtures/runtime_hashmaps_string_keys.p7");
const HASHMAPS_CONSTRUCT: &str = include_str!("fixtures/runtime_hashmaps_construct.p7");
const HASHMAPS_REMOVE: &str = include_str!("fixtures/runtime_hashmaps_remove.p7");
const HASHMAPS_MIXED: &str = include_str!("fixtures/runtime_hashmaps_mixed.p7");
const STRINGS: &str = include_str!("fixtures/runtime_strings.p7");
const CLOSURES: &str = include_str!("fixtures/runtime_closures.p7");
const GC_BOXES: &str = include_str!("fixtures/runtime_gc_boxes.p7");

fn compile_fixture(source: &str) -> Module {
    p7::compile(source.to_string()).expect("runtime benchmark fixture should compile")
}

fn run_main(module: &Module) -> Data {
    p7::run(module.clone(), "main").expect("runtime benchmark fixture should run")
}

fn runtime_benches(c: &mut Criterion) {
    let fixtures = [
        ("numeric_loop", NUMERIC_LOOP),
        ("function_calls", FUNCTION_CALLS),
        ("recursive_calls", RECURSIVE_CALLS),
        ("dynamic_dispatch", DYNAMIC_DISPATCH),
        ("struct_fields", STRUCT_FIELDS),
        ("arrays", ARRAYS),
        ("hashmaps", HASHMAPS),
        ("hashmaps_small_int", HASHMAPS_SMALL_INT),
        ("hashmaps_large_int", HASHMAPS_LARGE_INT),
        ("hashmaps_string_keys", HASHMAPS_STRING_KEYS),
        ("hashmaps_construct", HASHMAPS_CONSTRUCT),
        ("hashmaps_remove", HASHMAPS_REMOVE),
        ("hashmaps_mixed", HASHMAPS_MIXED),
        ("strings", STRINGS),
        ("closures", CLOSURES),
        ("gc_boxes", GC_BOXES),
    ];

    let mut runtime = c.benchmark_group("runtime");
    for (name, source) in fixtures {
        let module = compile_fixture(source);
        runtime.bench_function(format!("run_{name}"), |b| {
            b.iter(|| black_box(run_main(black_box(&module))))
        });
    }
    runtime.finish();

    let mut end_to_end = c.benchmark_group("end_to_end");
    end_to_end.bench_function("compile_and_run_numeric_loop", |b| {
        b.iter(|| {
            let module = p7::compile(black_box(NUMERIC_LOOP.to_string())).unwrap();
            black_box(p7::run(module, "main").unwrap())
        })
    });
    end_to_end.finish();
}

criterion_group!(benches, runtime_benches);
criterion_main!(benches);
