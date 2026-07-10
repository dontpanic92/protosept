use p7::interpreter::context::{Context, Data};
use p7::{InMemoryModuleProvider, ModuleProvider};

fn run_root(source: &str, provider: InMemoryModuleProvider) -> Data {
    let module = p7::compile_module_with_provider(
        source.to_string(),
        "app.features.main",
        provider.clone_boxed(),
    )
    .expect("compile package root");
    let mut context = Context::new();
    context.load_module(module);
    context.push_function("run", Vec::new());
    context.resume().expect("run package root");
    context.stack[0].stack.pop().expect("result")
}

#[test]
fn relative_import_resolves_from_current_module_parent() {
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module(
        "app.features.helper".to_string(),
        "pub fn value() -> int { 21 }".to_string(),
    );

    let result = run_root(
        r#"
import .helper;

pub fn run() -> int {
    helper.value() * 2
}
"#,
        provider,
    );

    assert_eq!(result, Data::Int(42));
}

#[test]
fn package_root_import_resolves_from_package_name() {
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module(
        "app.shared.answer".to_string(),
        "pub fn value() -> int { 42 }".to_string(),
    );

    let result = run_root(
        r#"
import _.shared.answer;

pub fn run() -> int {
    answer.value()
}
"#,
        provider,
    );

    assert_eq!(result, Data::Int(42));
}

#[test]
fn absolute_dependency_import_keeps_qualified_path() {
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module(
        "math.answer".to_string(),
        "pub fn value() -> int { 42 }".to_string(),
    );

    let result = run_root(
        r#"
import math.answer;

pub fn run() -> int {
    answer.value()
}
"#,
        provider,
    );

    assert_eq!(result, Data::Int(42));
}

#[test]
fn relative_import_from_directory_module_uses_module_as_parent() {
    let mut provider = InMemoryModuleProvider::new();
    provider.add_directory_module(
        "app.net".to_string(),
        "import .socket; pub fn value() -> int { socket.value() }".to_string(),
    );
    provider.add_module(
        "app.net.socket".to_string(),
        "pub fn value() -> int { 42 }".to_string(),
    );

    let module = p7::compile_module_with_provider(
        "import app.net; pub fn run() -> int { net.value() }".to_string(),
        "app.main",
        provider.clone_boxed(),
    )
    .expect("compile directory module import");
    let mut context = Context::new();
    context.load_module(module);
    context.push_function("run", Vec::new());
    context.resume().expect("run");
    assert_eq!(context.stack[0].stack.pop(), Some(Data::Int(42)));
}
