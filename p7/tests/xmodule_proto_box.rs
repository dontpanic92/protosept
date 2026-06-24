//! Regression for gaps.md #1: boxing a value of a struct/enum imported from
//! another module to its (imported) script-defined proto must dispatch into the
//! type's DEFINING module, not the boxing site's module.
//!
//! Pre-fix, `BoxToProto`/`RefToProto` stamped the current frame's module index
//! and the importer's local type id onto the proto box. The importer has no
//! vtable entry for the imported type (its methods aren't copied), so dispatch
//! failed with: `Method 'paint' not found in vtable for type N (origin module
//! M) implementing proto P`.

use p7::InMemoryModuleProvider;
use p7::interpreter::context::{Context, Data};

const UI: &str = r#"
pub proto IElement {
    fn paint(self: ref<IElement>) -> int;
}

pub struct[IElement] Text(pub value: int) {
    pub fn paint(self: ref<Self>) -> int { self.value }
}

pub struct[IElement] Doubler(pub value: int) {
    pub fn paint(self: ref<Self>) -> int { self.value * 2 }
}
"#;

fn run_entry(user: &str) -> Data {
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module("ui".to_string(), UI.to_string());
    let module = p7::compile_with_provider(user.to_string(), Box::new(provider)).expect("compile");
    let mut ctx = Context::new();
    ctx.load_module(module);
    ctx.push_function("entry", Vec::new());
    ctx.resume().expect("run");
    ctx.stack[0].stack.pop().expect("result")
}

#[test]
fn box_imported_struct_to_imported_proto_dispatches() {
    // box(ui.Text(7)) coerced to box<ui.IElement> in the *screen* module must
    // dispatch ui.Text::paint correctly.
    let user = r#"
import ui;
pub fn entry() -> int {
    let e: box<ui.IElement> = box(ui.Text(7));
    e.paint()
}
"#;
    assert_eq!(run_entry(user), Data::Int(7));
}

#[test]
fn ref_imported_struct_to_imported_proto_dispatches() {
    let user = r#"
import ui;
pub fn entry() -> int {
    let mut t: ui.Text = ui.Text(20);
    let e: ref<ui.IElement> = ref(t);
    e.paint()
}
"#;
    assert_eq!(run_entry(user), Data::Int(20));
}

#[test]
fn array_of_imported_proto_boxes_dispatches() {
    // Exercises the array-literal autobox site (gaps.md #4 residual) across a
    // module boundary: each element boxes a distinct imported concrete type.
    let user = r#"
import ui;
pub fn entry() -> int {
    let elements: array<box<ui.IElement>> = [ui.Text(5), ui.Doubler(5)];
    let mut total: int = 0;
    for e in elements {
        total = total + e.paint();
    }
    total
}
"#;
    // Text -> 5, Doubler -> 10
    assert_eq!(run_entry(user), Data::Int(15));
}
