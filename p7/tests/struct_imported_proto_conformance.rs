//! Regression: a struct conforming to a proto declared in an imported
//! module must compile without any companion top-level function whose
//! signature references the proto.
//!
//! Pre-fix, the parser produces an `Identifier { name: "radiance.IDirector" }`
//! for the dotted conformance, but `resolve_proto_identifier` only consults
//! `find_type_in_scope`, which performs a literal child-name lookup and
//! therefore fails with `SemanticError::TypeNotFound`. Ordinary typed
//! contexts go through `resolve_qualified_type_name`, which walks the
//! module alias and imports the type. The fix routes dotted proto names
//! through the same qualified-name path.

use p7::InMemoryModuleProvider;

const RADIANCE: &str = r#"
pub proto IDirector {
    fn activate(self: ref<IDirector>) -> int;
    fn update(self: ref<IDirector>, dt: float) -> ?box<IDirector>;
}
"#;

const USER: &str = r#"
import radiance;

struct[radiance.IDirector] Stub() {
    pub fn activate(self: ref<Self>) -> int { 0 }
    pub fn update(self: ref<Self>, dt: float) -> ?box<radiance.IDirector> {
        return null;
    }
}
"#;

#[test]
fn struct_can_conform_to_imported_proto_without_forward_ref_helper() {
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module("radiance".to_string(), RADIANCE.to_string());
    let _module = p7::compile_with_provider(USER.to_string(), Box::new(provider))
        .expect("imported-proto conformance must compile without a forward-ref helper");
}

const USER_MULTI: &str = r#"
import radiance;
import other;

struct[radiance.IDirector, other.IExtra] Stub2() {
    pub fn activate(self: ref<Self>) -> int { 1 }
    pub fn update(self: ref<Self>, dt: float) -> ?box<radiance.IDirector> {
        return null;
    }
    pub fn ping(self: ref<Self>) -> int { 7 }
}
"#;

const OTHER: &str = r#"
pub proto IExtra {
    fn ping(self: ref<IExtra>) -> int;
}
"#;

#[test]
fn struct_can_conform_to_multiple_imported_protos() {
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module("radiance".to_string(), RADIANCE.to_string());
    provider.add_module("other".to_string(), OTHER.to_string());
    let _module = p7::compile_with_provider(USER_MULTI.to_string(), Box::new(provider))
        .expect("multi-module conformance must compile");
}
