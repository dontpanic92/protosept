use p7::errors::RuntimeError;
use p7::interpreter::context::Context;

fn context_with_foreign_carrier() -> Context {
    let module = p7::compile(
        r#"
@foreign(type_tag="widget.Widget", dispatcher="widget.invoke")
proto Widget {
    fn value(ref self) -> int;
}
"#
        .to_string(),
    )
    .expect("compile foreign proto");
    let mut context = Context::new();
    context.register_foreign_type("widget.Widget", None);
    context.load_module(module);
    context
}

#[test]
fn persistent_foreign_handles_are_invalidated_by_identity() {
    let mut context = context_with_foreign_carrier();
    let first = context
        .alloc_foreign_handle("widget.Widget", 42)
        .expect("first handle");
    let second = context
        .alloc_foreign_handle("widget.Widget", 42)
        .expect("second handle");

    context
        .invalidate_foreign_handle("widget.Widget", 42)
        .expect("invalidate");

    for value in [first, second] {
        context.stack[0].stack.push(value);
        let error = context
            .pop_foreign("widget.Widget")
            .expect_err("invalidated handle must be stale");
        assert!(matches!(
            error,
            RuntimeError::StaleForeignHandle {
                ref type_tag,
                handle: 42
            } if type_tag == "widget.Widget"
        ));
    }
}

#[test]
fn owned_and_borrowed_foreign_values_are_not_invalidatable_handles() {
    let mut context = context_with_foreign_carrier();
    let owned = context
        .alloc_foreign("widget.Widget", 7)
        .expect("owned foreign");
    let borrowed = context
        .alloc_foreign_ref("widget.Widget", 8)
        .expect("borrowed foreign");

    context.stack[0].stack.push(owned);
    assert_eq!(context.pop_foreign("widget.Widget").expect("owned"), 7);
    context.stack[0].stack.push(borrowed);
    assert_eq!(context.pop_foreign("widget.Widget").expect("borrowed"), 8);

    let error = context
        .invalidate_foreign_handle("widget.Widget", 7)
        .expect_err("owned value has no handle identity");
    assert!(matches!(error, RuntimeError::StaleForeignHandle { .. }));
}

#[test]
fn reused_host_token_gets_a_new_generation() {
    let mut context = context_with_foreign_carrier();
    let stale = context
        .alloc_foreign_handle("widget.Widget", 9)
        .expect("handle");
    context
        .invalidate_foreign_handle("widget.Widget", 9)
        .expect("invalidate");

    let current = context
        .alloc_foreign_handle("widget.Widget", 9)
        .expect("host token may be reused for a new object");

    context.stack[0].stack.push(stale);
    let error = context
        .pop_foreign("widget.Widget")
        .expect_err("old generation remains stale");
    assert!(matches!(error, RuntimeError::StaleForeignHandle { .. }));
    context.stack[0].stack.push(current);
    assert_eq!(
        context
            .pop_foreign("widget.Widget")
            .expect("new generation"),
        9
    );
}
