use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use p7::{
    InMemoryModuleProvider,
    bytecode::Module,
    interpreter::context::{Context, Data},
};

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

const STRING_CHAR_ACCESS: &str = r#"
fn main() -> int {
    let text = "αβγδεζηθικλμνξοπρστυφχψωabcdefghijklmnopqrstuvwxyz";
    let mut total = 0;
    let mut i = 0;
    loop {
        if i >= 40 {
            break;
        }
        total = total + (text.char_at(i) ?? "").len_bytes();
        total = total + text.substring(i, i + 1).len_bytes();
        i = i + 1;
    }
    total
}
"#;

const STRING_SPLIT_TRIM_SEARCH: &str = r#"
fn main() -> int {
    let text = "  alpha,beta,gamma,delta,epsilon,zeta,eta,theta  ";
    let mut total = 0;
    let mut i = 0;
    loop {
        if i >= 120 {
            break;
        }
        let trimmed = text.trim();
        total = total + trimmed.index_of("gamma");
        total = total + trimmed.split(",").len();
        total = total + if trimmed.contains("theta") { 1 } else { 0 };
        total = total + if trimmed.starts_with("alpha") { 1 } else { 0 };
        total = total + if trimmed.ends_with("theta") { 1 } else { 0 };
        i = i + 1;
    }
    total
}
"#;

const MODULE_LOAD_SHARED: &str = r#"
import bench.leaf0;
import bench.leaf1;
import bench.leaf2;
import bench.leaf3;
import bench.leaf4;
import bench.leaf5;

fn main() -> int {
    leaf0.value() + leaf1.value() + leaf2.value() + leaf3.value() + leaf4.value() + leaf5.value()
}
"#;

const MODULE_LOAD_SHARED_DEP: &str = r#"
pub let seed: int = 11;

pub fn value() -> int {
    seed
}
"#;

fn compile_fixture(source: &str) -> Module {
    p7::compile(source.to_string()).expect("runtime benchmark fixture should compile")
}

fn compile_module_load_fixture() -> Module {
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module(
        "bench.shared".to_string(),
        MODULE_LOAD_SHARED_DEP.to_string(),
    );
    for idx in 0..6 {
        provider.add_module(
            format!("bench.leaf{idx}"),
            format!(
                r#"
import bench.shared;

pub fn value() -> int {{
    shared.value() + {idx}
}}
"#
            ),
        );
    }
    p7::compile_with_provider(MODULE_LOAD_SHARED.to_string(), Box::new(provider))
        .expect("module loading benchmark fixture should compile")
}

fn run_main(module: &Module) -> Data {
    p7::run(module.clone(), "main").expect("runtime benchmark fixture should run")
}

fn runtime_benches(c: &mut Criterion) {
    // Focused baselines for module-loading and string runtime work. Compare
    // run_strings_char_access, run_strings_split_trim_search, and
    // load_module_shared_import_graph before/after related optimizations.
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
        ("strings_char_access", STRING_CHAR_ACCESS),
        ("strings_split_trim_search", STRING_SPLIT_TRIM_SEARCH),
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

    let mut module_loading = c.benchmark_group("module_loading");
    module_loading.bench_function("load_module_shared_import_graph", |b| {
        b.iter_batched(
            compile_module_load_fixture,
            |module| {
                let mut ctx = Context::new();
                ctx.load_module(black_box(module));
                black_box(ctx);
            },
            BatchSize::SmallInput,
        )
    });
    module_loading.finish();
}

criterion_group!(benches, runtime_benches);
criterion_main!(benches);
