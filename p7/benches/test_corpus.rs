use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use p7::InMemoryModuleProvider;

const TEST_MODULE_SOURCE: &str = r#"
pub struct test(
    pub expected_type: string,
    pub expected_value: string,
);
"#;

const FOO_MODULE_SOURCE: &str = r#"
pub fn bar() -> int {
    return 7;
}
"#;

const FOO_BAR_MOD_SOURCE: &str = r#"
pub fn value() -> int {
    return 5;
}
"#;

const CORPUS: &[(&str, &str)] = &[
    (
        "arrays",
        include_str!("../../tests/test_array_operations.p7"),
    ),
    ("closures", include_str!("../../tests/test_closures.p7")),
    (
        "dynamic_dispatch",
        include_str!("../../tests/test_dynamic_dispatch.p7"),
    ),
    ("gc", include_str!("../../tests/test_gc.p7")),
    ("generics", include_str!("../../tests/test_generics.p7")),
    ("hashmap", include_str!("../../tests/test_hashmap.p7")),
    ("imports", include_str!("../../tests/test_imports.p7")),
    (
        "string_methods",
        include_str!("../../tests/test_string_methods.p7"),
    ),
];

fn test_provider() -> InMemoryModuleProvider {
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module("test".to_string(), TEST_MODULE_SOURCE.to_string());
    provider.add_module("foo".to_string(), FOO_MODULE_SOURCE.to_string());
    provider.add_module("foo.bar_mod".to_string(), FOO_BAR_MOD_SOURCE.to_string());
    provider
}

fn test_corpus_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("test_corpus_compile");

    for (name, source) in CORPUS {
        group.bench_function(*name, |b| {
            b.iter_batched(
                test_provider,
                |provider| {
                    black_box(
                        p7::compile_with_provider(
                            black_box((*source).to_string()),
                            Box::new(provider),
                        )
                        .unwrap(),
                    )
                },
                BatchSize::SmallInput,
            )
        });
    }

    group.bench_function("curated_subset_all", |b| {
        b.iter_batched(
            test_provider,
            |provider| {
                for (_, source) in CORPUS {
                    black_box(
                        p7::compile_with_provider(
                            black_box((*source).to_string()),
                            Box::new(provider.clone()),
                        )
                        .unwrap(),
                    );
                }
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(benches, test_corpus_benches);
criterion_main!(benches);
