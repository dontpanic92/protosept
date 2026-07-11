use p7::ast::{Statement, Type as AstType};
use p7::interpreter::context::{Context, Data};
use p7::lexer::{Lexer, TokenType};
use p7::parser::Parser;
use p7::{InMemoryModuleProvider, ModuleProvider};

const INHERITANCE_SOURCE: &str = r#"
@foreign(dispatcher="inherit.invoke", type_tag="inherit.Base")
pub proto Base {
    fn base_value(ref self) -> int;
}

@foreign(dispatcher="inherit.invoke", type_tag="inherit.Derived")
pub proto[Base] Derived {
    fn derived_value(ref self) -> int;
}

@foreign(dispatcher="inherit.invoke", type_tag="inherit.Other")
pub proto Other {}

@intrinsic(name="inherit.make_derived")
fn make_derived() -> handle<Derived>;

@intrinsic(name="inherit.make_base")
fn make_base() -> handle<Base>;
"#;

fn host_make_derived(ctx: &mut Context) -> Result<(), p7::errors::RuntimeError> {
    ctx.push_foreign_handle("inherit.Derived", 42)
}

fn host_make_base(ctx: &mut Context) -> Result<(), p7::errors::RuntimeError> {
    ctx.push_foreign_handle("inherit.Base", 7)
}

fn host_invoke(ctx: &mut Context) -> Result<(), p7::errors::RuntimeError> {
    let frame = ctx.stack_frame_mut()?;
    let type_tag = match frame.stack.pop() {
        Some(Data::String(value)) => value,
        other => panic!("expected type tag, got {other:?}"),
    };
    let method = match frame.stack.pop() {
        Some(Data::String(value)) => value,
        other => panic!("expected method, got {other:?}"),
    };
    assert!(matches!(frame.stack.pop(), Some(Data::Int(_))));
    assert!(matches!(frame.stack.pop(), Some(Data::Array(_))));
    assert_eq!(type_tag.as_ref(), "inherit.Derived");

    let handle = ctx.pop_foreign("inherit.Base")?;
    let result = match method.as_ref() {
        "base_value" => 10,
        "derived_value" => handle,
        other => panic!("unexpected method {other}"),
    };
    ctx.stack_frame_mut()?.stack.push(Data::Int(result));
    Ok(())
}

fn run_module(source: &str) -> Result<Data, p7::errors::RuntimeError> {
    let module = p7::compile(source.to_string()).expect("compile");
    let mut context = Context::new();
    context.register_host_function("inherit.make_derived".to_string(), host_make_derived);
    context.register_host_function("inherit.make_base".to_string(), host_make_base);
    context.register_host_function("inherit.invoke".to_string(), host_invoke);
    context.load_module(module);
    context.push_function("run", Vec::new());
    context.resume()?;
    context.stack[0]
        .stack
        .pop()
        .ok_or(p7::errors::RuntimeError::StackUnderflow)
}

#[test]
fn parser_records_multiple_proto_bases() {
    let mut lexer = Lexer::new("proto[First, second.Second] Derived {}".to_string());
    let mut tokens = Vec::new();
    loop {
        let token = lexer.next_token();
        if token.token_type == TokenType::EOF {
            break;
        }
        tokens.push(token);
    }
    let statements = Parser::new(tokens).parse().expect("parse");
    let Statement::ProtoDeclaration { name, bases, .. } = &statements[0] else {
        panic!("expected proto declaration");
    };
    assert_eq!(name.name.as_str(), "Derived");
    assert_eq!(bases.len(), 2);
    assert!(matches!(&bases[0], AstType::Identifier(id) if id.name == "First"));
    assert!(matches!(&bases[1], AstType::Identifier(id) if id.name == "second.Second"));
}

#[test]
fn implicit_upcast_inherited_call_and_checked_downcast_succeed() {
    let source = format!(
        r#"{INHERITANCE_SOURCE}
fn read_base(value: handle<Base>) -> int {{
    value.base_value()
}}

pub fn run() -> int {{
    let derived = make_derived();
    let inherited = derived.base_value();
    let base: handle<Base> = derived;
    let downcast = base as handle<Derived>;
    inherited + read_base(downcast) + downcast.derived_value()
}}
"#
    );
    assert_eq!(run_module(&source).expect("run"), Data::Int(62));
}

#[test]
fn script_implementation_of_derived_proto_inherits_base_requirements() {
    let source = format!(
        r#"{INHERITANCE_SOURCE}
struct[Derived] Implementation {{
    fn base_value(ref self) -> int {{ 20 }}
    fn derived_value(ref self) -> int {{ 22 }}
}}

fn read_base(value: box<Base>) -> int {{
    value.base_value()
}}

pub fn run() -> int {{
    let derived: box<Derived> = Implementation();
    read_base(derived) + derived.derived_value()
}}
"#
    );
    assert_eq!(
        p7::compile_and_run(source, "run").expect("compile and run"),
        Data::Int(42)
    );
}

#[test]
fn checked_downcast_traps_on_dynamic_type_mismatch() {
    let source = format!(
        r#"{INHERITANCE_SOURCE}
pub fn run() -> int {{
    let base = make_base();
    let _derived = base as handle<Derived>;
    0
}}
"#
    );
    let error = run_module(&source).expect_err("downcast must trap");
    assert!(
        error
            .to_string()
            .contains("dynamic type_tag 'inherit.Base' is not a 'inherit.Derived'"),
        "unexpected error: {error}"
    );
}

#[test]
fn context_foreign_is_a_is_transitive() {
    let source = format!(
        r#"{INHERITANCE_SOURCE}
@foreign(dispatcher="inherit.invoke", type_tag="inherit.MostDerived")
pub proto[Derived] MostDerived {{}}

fn transitive_upcast(value: handle<MostDerived>) -> handle<Base> {{
    value
}}
"#
    );
    let module = p7::compile(source).expect("compile");
    let mut context = Context::new();
    context.load_module(module);
    let value = context
        .alloc_foreign_handle("inherit.MostDerived", 1)
        .expect("foreign value");
    assert!(context.foreign_is_a(&value, "inherit.MostDerived"));
    assert!(context.foreign_is_a(&value, "inherit.Derived"));
    assert!(context.foreign_is_a(&value, "inherit.Base"));
    assert!(!context.foreign_is_a(&value, "inherit.Other"));
    assert!(
        context
            .invalidate_foreign_handle("inherit.Base", 1)
            .is_err(),
        "invalidation remains keyed by the dynamic tag"
    );
    context
        .invalidate_foreign_handle("inherit.MostDerived", 1)
        .expect("invalidate dynamic tag");
    assert!(matches!(
        context.foreign_handle(&value, "inherit.Base"),
        Err(p7::errors::RuntimeError::StaleForeignHandle { .. })
    ));
}

#[test]
fn imported_inheritance_survives_module_serialization() {
    let provider_source = r#"
@foreign(dispatcher="inherit.invoke", type_tag="inherit.Base")
pub proto Base {
    fn base_value(ref self) -> int;
}

@foreign(dispatcher="inherit.invoke", type_tag="inherit.Derived")
pub proto[Base] Derived {}

@intrinsic(name="inherit.make_derived")
pub fn make_derived() -> handle<Derived>;
"#;
    let main_source = r#"
import interfaces;

pub fn run() -> int {
    let derived = interfaces.make_derived();
    let base: handle<interfaces.Base> = derived;
    base.base_value()
}
"#;
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module("interfaces".to_string(), provider_source.to_string());
    let module = p7::compile_with_provider(main_source.to_string(), provider.clone_boxed())
        .expect("compile");
    let module = p7::bytecode::Module::from_bytes(&module.to_bytes()).expect("deserialize");

    let mut context = Context::new();
    context.register_host_function("inherit.make_derived".to_string(), host_make_derived);
    context.register_host_function("inherit.invoke".to_string(), host_invoke);
    context.load_module(module);
    context.push_function("run", Vec::new());
    context.resume().expect("run");
    assert_eq!(context.stack[0].stack.pop(), Some(Data::Int(10)));
}

#[test]
fn invalid_proto_inheritance_has_clear_diagnostics() {
    for (source, needle) in [
        ("proto[B] A; proto[A] B;", "proto inheritance cycle"),
        ("proto Base; proto[Base, Base] Derived;", "duplicate base"),
        (
            "struct NotProto; proto[NotProto] Derived;",
            "Expected protocol name",
        ),
        (
            "proto Base<T>; proto[Base<int>] Derived;",
            "generic proto bases are not supported",
        ),
        (
            "proto A { fn value(ref self) -> int; } proto B { fn value(ref self) -> float; } proto[A, B] C;",
            "conflicting signatures",
        ),
    ] {
        let error = p7::compile(source.to_string()).expect_err("compile must fail");
        assert!(
            error.to_string().contains(needle),
            "expected '{needle}', got {error}"
        );
    }
}
