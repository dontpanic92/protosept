use p7::embedding::{CallOutcome, Runtime};
use p7::interpreter::context::Data;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn loads_a_versioned_dynamic_extension() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "protosept-native-extension-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).expect("create temp directory");
    let library = directory.join(format!(
        "{}fixture{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native_extension.rs");
    let output = Command::new("rustc")
        .arg("--edition=2021")
        .arg("--crate-type=cdylib")
        .arg(&fixture)
        .arg("-o")
        .arg(&library)
        .output()
        .expect("run rustc");
    assert!(
        output.status.success(),
        "fixture compilation failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    {
        let mut runtime = Runtime::new();
        runtime
            .load_native_extension(&library)
            .expect("load native extension");
        runtime.load_module(
            p7::compile(
                r#"
@intrinsic(name="dynamic.answer")
fn answer() -> int;

fn run() -> int {
    answer()
}
"#
                .to_string(),
            )
            .expect("compile script"),
        );

        assert!(matches!(
            runtime.call("run", Vec::new()).expect("run"),
            CallOutcome::Returned(Some(Data::Int(42)))
        ));
    }

    fs::remove_dir_all(directory).ok();
}
