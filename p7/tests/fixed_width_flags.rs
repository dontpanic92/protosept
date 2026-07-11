use p7::interpreter::context::{Context, Data};
use p7::interpreter::native::{NativeSignature, NativeType};
use p7::{InMemoryModuleProvider, ModuleProvider};

fn compile(source: &str) -> p7::bytecode::Module {
    p7::compile(source.to_string()).expect("source should compile")
}

fn run(source: &str) -> Data {
    let module = compile(source);
    let mut context = Context::new();
    context.load_module(module);
    context.push_function("run", Vec::new());
    context.resume().expect("program should run");
    context.stack[0].stack.pop().expect("run result")
}

#[test]
fn fixed_width_literals_casts_and_arithmetic_are_checked() {
    assert_eq!(
        run(r#"
import std.ffi;

fn run() -> int {
    let i8_min: ffi.i8 = -128;
    let i8_max: ffi.i8 = 127;
    let u8_max: ffi.u8 = 255;
    let i16_min: ffi.i16 = -32768;
    let i16_max: ffi.i16 = 32767;
    let u16_max: ffi.u16 = 65535;
    let i32_min: ffi.i32 = -2147483648;
    let i32_max: ffi.i32 = 2147483647;
    let u32_max: ffi.u32 = 4294967295;
    let i64_min: ffi.i64 = -9223372036854775808;
    let i64_max: ffi.i64 = 9223372036854775807;
    let u64_max: ffi.u64 = 9223372036854775807;
    1
}
"#),
        Data::Int(1)
    );

    assert_eq!(
        run(r#"
import std.ffi;

fn run() -> int {
    let lo: ffi.i8 = -128;
    let hi: ffi.u32 = 4294967295;
    (lo as int) + (hi as int)
}
"#),
        Data::Int(4_294_967_167)
    );

    assert_eq!(
        run(r#"
import std.ffi;

fn run() -> ffi.i8 {
    let value: ffi.i8 = 5;
    -value
}
"#),
        Data::Int(-5)
    );

    assert!(p7::compile("fn run() -> i8 { 1 }".to_string()).is_err());

    let error = p7::compile("import std.ffi; fn run() -> ffi.i8 { 128 }".to_string()).unwrap_err();
    assert!(error.to_string().contains("outside range of i8"), "{error}");
    assert!(
        p7::compile("import std.ffi; fn run() -> ffi.i8 { 128 as ffi.i8 }".to_string()).is_err()
    );
    for (ty, below, above) in [
        ("i8", "-129", "128"),
        ("u8", "-1", "256"),
        ("i16", "-32769", "32768"),
        ("u16", "-1", "65536"),
        ("i32", "-2147483649", "2147483648"),
        ("u32", "-1", "4294967296"),
        ("i64", "-9223372036854775809", "9223372036854775808"),
        ("u64", "-1", "9223372036854775808"),
    ] {
        for value in [below, above] {
            assert!(
                p7::compile(format!(
                    "import std.ffi; fn run() -> ffi.{ty} {{ {value} }}"
                ))
                .is_err(),
                "{value} should not fit {ty}"
            );
        }
    }

    let module = compile("import std.ffi; fn run(value: int) -> ffi.i8 { value as ffi.i8 }");
    let mut context = Context::new();
    context.load_module(module);
    context.push_function("run", vec![Data::Int(128)]);
    let error = context.resume().unwrap_err();
    assert!(error.to_string().contains("outside range"));

    let module = compile(
        r#"
import std.ffi;

fn run() -> ffi.i8 {
    let a: ffi.i8 = 127;
    let b: ffi.i8 = 1;
    a + b
}
"#,
    );
    let mut context = Context::new();
    context.load_module(module);
    context.push_function("run", Vec::new());
    let error = context.resume().unwrap_err();
    assert!(error.to_string().contains("outside range"));
}

#[test]
fn newtype_flags_support_associated_values_bitwise_ops_and_contains() {
    assert_eq!(
        run(r#"
import std.ffi;

struct[BitOr, BitAnd, BitXor] Access(ffi.u32) {
    pub None = Self(0);
    pub Read = Self(1);
    pub Write = Access(2);
    pub Execute = Self(4);
    pub ReadWrite = Self(3);

    pub fn bitor(ref self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }

    pub fn bitand(ref self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }

    pub fn bitxor(ref self, rhs: Self) -> Self {
        Self(self.0 ^ rhs.0)
    }

    pub fn contains(ref self, rhs: Self) -> bool {
        (self.0 & rhs.0) == rhs.0
    }

    pub fn bits(ref self) -> ffi.u32 {
        self.0
    }
}

fn run() -> int {
    let value = Access.Read | Access.Write;
    let toggled = value ^ Access.Write;
    let masked = value & Access.Read;
    if toggled.contains(Access.Read) && masked.contains(Access.Read) && !masked.contains(Access.Execute) {
        value.bits() as int
    } else {
        0
    }
}
"#),
        Data::Int(3)
    );

    for source in [
        "struct A(int) { pub X = Self(1); } fn run() -> A { A.X | A.X }",
        "struct[BitOr] A(int) { pub X = Self(1); pub fn bitor(ref self, rhs: Self) -> Self { Self(self.0 | rhs.0) } } struct[BitOr] B(int) { pub X = Self(1); pub fn bitor(ref self, rhs: Self) -> Self { Self(self.0 | rhs.0) } } fn run() -> A { A.X | B.X }",
    ] {
        assert!(p7::compile(source.to_string()).is_err());
    }
}

#[test]
fn associated_values_work_for_any_struct_and_are_not_calls() {
    assert_eq!(
        run(r#"
struct Color(r: int, g: int, b: int) {
    pub Red = Self(r = 255, g = 0, b = 0);
}

fn run() -> int {
    Color.Red.r
}
"#,),
        Data::Int(255)
    );

    assert!(
        p7::compile(
            "struct Access(int) { pub Read = Self(1); } fn run() -> Access { Access.Read() }"
                .to_string()
        )
        .is_err()
    );
    for source in [
        "struct Access(int) { pub Read = Self(1); pub Read = Self(2); }",
        "struct Access(int) { pub Read = Self(1); pub fn Read() -> Self { Self(2) } }",
    ] {
        assert!(p7::compile(source.to_string()).is_err(), "{source}");
    }
    assert!(p7::compile("flags Access: int { Read = 1 }".to_string()).is_err());
}

#[test]
fn imported_newtype_flags_survive_module_serialization() {
    let mut provider = InMemoryModuleProvider::new();
    provider.add_module(
        "pkg.perms".to_string(),
        r#"
import std.ffi;

pub struct[BitOr] Access(ffi.u16) {
    pub Read = Self(1);
    pub Write = Self(2);

    pub fn bitor(ref self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }

    pub fn bits(ref self) -> ffi.u16 {
        self.0
    }
}
"#
        .to_string(),
    );
    let module = p7::compile_module_with_provider(
        r#"
import pkg.perms;
pub fn run() -> int {
    let value: perms.Access = perms.Access.Read | perms.Access.Write;
    value.bits() as int
}
"#
        .to_string(),
        "pkg.main",
        provider.clone_boxed(),
    )
    .expect("compile imported newtype flags");
    let module = p7::bytecode::Module::from_bytes(&module.to_bytes()).expect("deserialize module");
    let mut context = Context::new();
    context.load_module(module);
    context.push_function("run", Vec::new());
    context.resume().expect("run imported newtype flags");
    assert_eq!(context.stack[0].stack.pop(), Some(Data::Int(3)));
}

#[test]
fn native_stack_adapter_checks_fixed_width_arguments_and_results() {
    use p7::embedding::{CallOutcome, Runtime};

    let module = compile(
        r#"
@intrinsic(name="width.echo")
fn echo(value: int) -> int;
@intrinsic(name="width.bad_result")
fn bad_result() -> int;

fn incoming() -> int { echo(256) }
fn outgoing() -> int { bad_result() }
"#,
    );
    let mut runtime = Runtime::new();
    runtime.register_native_function(
        "width.echo",
        NativeSignature::new(vec![NativeType::U8], Some(NativeType::U8)),
        |_context, args| Ok(Some(args[0].clone())),
    );
    runtime.register_native_function(
        "width.bad_result",
        NativeSignature::new(Vec::new(), Some(NativeType::I8)),
        |_context, _args| Ok(Some(Data::Int(128))),
    );
    runtime.load_module(module);

    match runtime.call("incoming", Vec::new()).expect("incoming call") {
        CallOutcome::Trapped(error) => {
            assert!(error.to_string().contains("argument 0 expected U8"));
        }
        other => panic!("unexpected incoming result: {other:?}"),
    }
    match runtime.call("outgoing", Vec::new()).expect("outgoing call") {
        CallOutcome::Trapped(error) => {
            assert!(error.to_string().contains("expected return type I8"));
        }
        other => panic!("unexpected outgoing result: {other:?}"),
    }
}

#[test]
fn native_abi_width_kinds_are_append_only() {
    use p7::native_abi::P7NativeType;

    assert_eq!(P7NativeType::Foreign as u32, 9);
    assert_eq!(P7NativeType::I8 as u32, 10);
    assert_eq!(P7NativeType::U8 as u32, 11);
    assert_eq!(P7NativeType::I16 as u32, 12);
    assert_eq!(P7NativeType::U16 as u32, 13);
    assert_eq!(P7NativeType::I32 as u32, 14);
    assert_eq!(P7NativeType::U32 as u32, 15);
    assert_eq!(P7NativeType::I64 as u32, 16);
    assert_eq!(P7NativeType::U64 as u32, 17);
}
