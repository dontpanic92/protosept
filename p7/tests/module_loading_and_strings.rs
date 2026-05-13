use p7::{InMemoryModuleProvider, interpreter::context::Data};

#[test]
fn shared_import_module_is_loaded_once_for_runtime_state() {
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module(
        "bench.shared".to_string(),
        r#"
pub let mut counter: int = 0;

pub fn next() -> int {
    counter = counter + 1;
    counter
}
"#
        .to_string(),
    );
    provider.add_module(
        "bench.leaf0".to_string(),
        r#"
import bench.shared;

pub fn value() -> int {
    shared.next()
}
"#
        .to_string(),
    );
    provider.add_module(
        "bench.leaf1".to_string(),
        r#"
import bench.shared;

pub fn value() -> int {
    shared.next()
}
"#
        .to_string(),
    );

    let module = p7::compile_with_provider(
        r#"
import bench.leaf0;
import bench.leaf1;

fn main() -> int {
    leaf0.value() + leaf1.value()
}
"#
        .to_string(),
        Box::new(provider),
    )
    .expect("shared import graph should compile");

    assert_eq!(
        p7::run(module, "main").expect("shared import graph should run"),
        Data::Int(3)
    );
}

#[test]
fn cyclic_imports_are_reported_across_nested_generators() {
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module(
        "cycle.a".to_string(),
        r#"
import cycle.b;

pub fn value() -> int {
    1
}
"#
        .to_string(),
    );
    provider.add_module(
        "cycle.b".to_string(),
        r#"
import cycle.a;

pub fn value() -> int {
    2
}
"#
        .to_string(),
    );

    let result = p7::compile_with_provider("import cycle.a;".to_string(), Box::new(provider));
    assert!(result.is_err());
}

#[test]
fn compiled_module_cache_is_scoped_to_one_compile_provider() {
    fn compile_and_run(value: i64) -> Data {
        let mut provider = InMemoryModuleProvider::new();
        provider.add_module(
            "same.path".to_string(),
            format!(
                r#"
pub fn value() -> int {{
    {value}
}}
"#
            ),
        );
        let module = p7::compile_with_provider(
            r#"
import same.path;

fn main() -> int {
    path.value()
}
"#
            .to_string(),
            Box::new(provider),
        )
        .expect("provider-specific module should compile");

        p7::run(module, "main").expect("provider-specific module should run")
    }

    assert_eq!(compile_and_run(1), Data::Int(1));
    assert_eq!(compile_and_run(2), Data::Int(2));
}

#[test]
fn unicode_substring_and_char_at_keep_char_index_semantics() {
    let module = p7::compile(
        r#"
fn main() -> int {
    let text = "aβ😄z";
    text.substring(1, 3).len_bytes()
        + (text.char_at(2) ?? "").len_bytes()
        + text.substring(-5, 2).len_chars()
        + text.substring(99, 100).len_bytes()
        + (text.char_at(-1) ?? "").len_bytes()
}
"#
        .to_string(),
    )
    .expect("unicode string test should compile");

    assert_eq!(
        p7::run(module, "main").expect("unicode string test should run"),
        Data::Int(12)
    );
}

#[test]
fn string_concat_preserves_order_and_unicode_bytes() {
    let module = p7::compile(
        r#"
fn main() -> int {
    let text = "α" + "β" + "z";
    text.len_bytes() + if text == "αβz" { 1 } else { 0 }
}
"#
        .to_string(),
    )
    .expect("concat string test should compile");

    assert_eq!(
        p7::run(module, "main").expect("concat string test should run"),
        Data::Int(6)
    );
}
