use p7::embedding::{CallOutcome, Runtime};
use p7::interpreter::context::Data;
use std::fs;
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static SHUTDOWN_TEST_LOCK: Mutex<()> = Mutex::new(());

fn compile_fixture(source: &std::path::Path, library: &std::path::Path, cfg: Option<&str>) {
    let mut command = Command::new("rustc");
    command
        .arg("--edition=2021")
        .arg("--crate-type=cdylib")
        .arg(source)
        .arg("-o")
        .arg(library);
    if let Some(cfg) = cfg {
        command.arg("--cfg").arg(cfg);
    }
    let output = command.output().expect("run rustc");
    assert!(
        output.status.success(),
        "fixture compilation failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

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
    compile_fixture(&fixture, &library, None);

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
        runtime.shutdown().expect("shutdown native extension");
    }

    fs::remove_dir_all(directory).expect("remove unloaded extension");
}

#[test]
fn shuts_down_extensions_in_reverse_order_before_dropping_registrations() {
    let _guard = SHUTDOWN_TEST_LOCK.lock().expect("shutdown test lock");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "protosept-native-shutdown-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).expect("create temp directory");
    let first = directory.join(format!(
        "{}first{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    let second = directory.join(format!(
        "{}second{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    let log = directory.join("shutdown.log");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native_extension_shutdown.rs");
    compile_fixture(&fixture, &first, Some("fixture_first"));
    compile_fixture(&fixture, &second, Some("fixture_second"));

    unsafe { std::env::set_var("P7_SHUTDOWN_LOG", &log) };
    let mut runtime = Runtime::new();
    runtime
        .load_native_extension(&first)
        .expect("load first extension");
    runtime
        .load_native_extension(&second)
        .expect("load second extension");
    runtime.shutdown().expect("shutdown extensions");
    runtime.shutdown().expect("repeat shutdown is idempotent");
    unsafe { std::env::remove_var("P7_SHUTDOWN_LOG") };

    assert_eq!(
        fs::read_to_string(&log).expect("read shutdown log"),
        "shutdown:second\ndrop:second\nshutdown:first\ndrop:first\n"
    );
    fs::remove_dir_all(directory).expect("remove unloaded extensions");
}

#[test]
fn failed_shutdown_is_reported_once_and_retains_the_library() {
    let _guard = SHUTDOWN_TEST_LOCK.lock().expect("shutdown test lock");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "protosept-native-shutdown-failure-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).expect("create temp directory");
    let library = directory.join(format!(
        "{}failure{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    let log = directory.join("shutdown.log");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native_extension_shutdown.rs");
    compile_fixture(&fixture, &library, Some("fixture_failure"));

    unsafe { std::env::set_var("P7_SHUTDOWN_LOG", &log) };
    let mut runtime = Runtime::new();
    runtime
        .load_native_extension(&library)
        .expect("load failing extension");
    let first = runtime.shutdown().expect_err("shutdown must fail");
    let second = runtime.shutdown().expect_err("shutdown remains failed");

    assert!(first
        .to_string()
        .contains(library.to_string_lossy().as_ref()));
    assert!(first.to_string().contains("remains loaded"));
    assert_eq!(first.to_string(), second.to_string());
    assert_eq!(
        fs::read_to_string(&log).expect("read shutdown log"),
        "shutdown:failure\n"
    );
    drop(runtime);
    unsafe { std::env::remove_var("P7_SHUTDOWN_LOG") };
    fs::remove_dir_all(directory).ok();
}
