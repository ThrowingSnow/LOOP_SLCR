//! Runs the Java bridge test against the real shared library.
//!
//! Compiles `tests/java` with `javac`, builds the `cdylib`, and runs the class
//! with the library on `java.library.path`. Skipped with a message when there is
//! no JDK, so the suite still passes on a machine that only builds the CLI.
//!
//! This is the only place a few properties can be checked at all: a direct
//! `ByteBuffer` arriving intact, a `float[]` coming back with the right length,
//! and — the one that matters — a Rust panic becoming a Java exception rather
//! than a corrupted runtime. `catch_unwind` returning `Err` in a Rust test
//! proves nothing about what the JVM does with an unwind that escapes.

use std::path::{Path, PathBuf};
use std::process::Command;

use loopslcr_core::wav::{write, BitDepth, Metadata, WriteSpec};
use loopslcr_core::AudioBuffer;

const RATE: u32 = 8_000;
/// One bar of 4/4 at 200 BPM at [`RATE`]. Exact, deliberately.
const BAR: usize = 9_600;

fn tool(name: &str) -> Option<PathBuf> {
    // A JDK the tests installed takes precedence over whatever is on PATH: the
    // Android build needs 21, and the system may well have something else.
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        let candidate = PathBuf::from(home).join("bin").join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let found = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .ok()?;
    found
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&found.stdout).trim()))
}

/// Where cargo put the `cdylib` for this test run.
fn library_dir() -> PathBuf {
    // The test binary sits in `target/<profile>/deps`; the library is one up.
    let mut path = std::env::current_exe().expect("no test binary path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path
}

fn wav_file(dir: &Path, bars: usize) -> PathBuf {
    let frames = bars * BAR;
    let data: Vec<f64> = (0..frames).map(|i| 0.25 * (i as f64 * 0.05).sin()).collect();
    let buffer = AudioBuffer::new(vec![data.clone(), data], RATE);
    let bytes = write::write(
        &buffer,
        &WriteSpec {
            depth: BitDepth::Int24,
            metadata: Metadata::default(),
        },
    )
    .expect("could not encode the test loop");
    let path = dir.join("200 loop.wav");
    std::fs::write(&path, bytes).expect("could not write the test loop");
    path
}

#[test]
fn the_bridge_works_from_a_real_jvm() {
    let (Some(javac), Some(java)) = (tool("javac"), tool("java")) else {
        eprintln!("no JDK found — skipping the JVM bridge test");
        return;
    };

    let lib_dir = library_dir();
    let library = lib_dir.join("libloopslcr_jni.so");
    if !library.is_file() {
        // `cargo test` builds the test binary but not necessarily the cdylib, so
        // say what to run rather than failing on something that is not a defect.
        eprintln!(
            "{} not built — run `cargo build -p loopslcr-jni` first; skipping",
            library.display()
        );
        return;
    }

    let scratch = lib_dir.join("jvm-bridge-test");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("could not make a scratch directory");
    let wav = wav_file(&scratch, 8);

    let sources = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/java");
    let compile = Command::new(&javac)
        .arg("-d")
        .arg(&scratch)
        .arg(sources.join("org/loopslcr/Native.java"))
        .arg(sources.join("org/loopslcr/BridgeTest.java"))
        .output()
        .expect("javac did not run");
    assert!(
        compile.status.success(),
        "javac failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&java)
        .arg(format!("-Djava.library.path={}", lib_dir.display()))
        .arg("-cp")
        .arg(&scratch)
        .arg("org.loopslcr.BridgeTest")
        .arg(&wav)
        .output()
        .expect("java did not run");

    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        run.status.success(),
        "the bridge test failed:\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("all bridge checks passed"),
        "unexpected output:\n{stdout}\n{stderr}"
    );

    let _ = std::fs::remove_dir_all(&scratch);
}
