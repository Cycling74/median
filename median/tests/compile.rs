//! Behavioral compilation tests, without rustc diagnostic snapshots.
//!
//! Each `compile/*.rs` is a valid program. Exactly one `// REJECT: ...` comment replaces
//! the region following `// ACCEPT` for the invalid variant. Build every control first,
//! then require a source-associated compiler error from every invalid variant. Build (not check) is
//! essential: some API restrictions are const assertions evaluated during monomorphisation.

use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

fn cargo() -> Command {
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    // Set only child-process variables; don't mutate the test process's environment.
    command
        .env("RUSTC_WRAPPER", "")
        .env("RUSTC_WORKSPACE_WRAPPER", "");
    if std::env::var_os("CARGO_BUILD_JOBS").is_none() {
        let jobs = std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).max(1));
        command.env("CARGO_BUILD_JOBS", jobs.to_string());
    }
    command
}

fn fixtures(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures in {}", dir.display());
    paths
}

fn rejected(source: &str) -> String {
    let mut lines = Vec::new();
    let mut replacements = 0;
    let mut start = None;
    for line in source.lines() {
        if line.trim() == "// ACCEPT" {
            assert!(start.replace(lines.len()).is_none(), "nested ACCEPT region");
        } else if let Some(replacement) = line.trim().strip_prefix("// REJECT: ") {
            let start = start.take().expect("REJECT needs an ACCEPT region");
            assert!(lines.len() > start, "empty ACCEPT region");
            lines.truncate(start);
            lines.push(replacement);
            replacements += 1;
        } else {
            lines.push(line);
        }
    }
    assert!(start.is_none(), "unclosed ACCEPT region");
    assert_eq!(replacements, 1, "each fixture must isolate one rejection");
    lines.join("\n")
}

#[test]
fn rejection_replaces_the_whole_region_after_formatting() {
    let source = "impl T {\n// ACCEPT\nfn f(&self) {\n}\n// REJECT: fn f(&mut self) {}\n}";
    assert_eq!(rejected(source), "impl T {\nfn f(&mut self) {}\n}");
}

fn build(dir: &Path, target: &Path, bins: &[String]) -> Output {
    let mut command = cargo();
    command
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .args([
            "build",
            "--offline",
            "--keep-going",
            "--message-format=json",
        ]);
    for bin in bins {
        command.args(["--bin", bin]);
    }
    command
        .output()
        .expect("run Cargo for compilation fixtures")
}

fn diagnostics(output: &Output) -> Vec<Value> {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("Cargo must emit JSON"))
        .collect()
}

fn report(output: &Output) -> String {
    let mut report = String::from_utf8_lossy(&output.stderr).into_owned();
    for value in diagnostics(output) {
        if let Some(rendered) = value["message"]["rendered"].as_str() {
            report.push_str(rendered);
        }
    }
    report
}

// Const-evaluation failures can point into median or core, with the fixture appearing in
// an instantiation note or expansion. Inspect structured locations, never rendered text.
fn refers_to(value: &Value, source: &Path) -> bool {
    match value {
        Value::Object(fields) => {
            fields
                .get("file_name")
                .and_then(Value::as_str)
                .is_some_and(|name| {
                    Path::new(name) == source
                        || source.parent().is_some_and(|dir| dir.join(name) == source)
                })
                || fields.values().any(|value| refers_to(value, source))
        }
        Value::Array(values) => values.iter().any(|value| refers_to(value, source)),
        _ => false,
    }
}

fn check_rejections(output: &Output, expected: &BTreeMap<String, PathBuf>) -> Result<(), String> {
    if output.status.code() != Some(101) {
        return Err(format!(
            "expected Cargo compilation failure, got {}",
            output.status
        ));
    }
    check_messages(&diagnostics(output), expected)
}

fn check_messages(messages: &[Value], expected: &BTreeMap<String, PathBuf>) -> Result<(), String> {
    let mut rejected = BTreeSet::new();
    let mut located = BTreeSet::new();
    for value in messages {
        if value["reason"] != "compiler-message" {
            continue;
        }
        let message = &value["message"];
        let level = message["level"].as_str().unwrap_or("");
        if level.contains("internal compiler error") {
            return Err("compiler crashed".into());
        }
        let target = value["target"]["name"].as_str().unwrap_or("");
        if matches!(level, "error" | "note") {
            if let Some(source) = expected.get(target) {
                if refers_to(message, source) {
                    located.insert(target.to_owned());
                }
            }
        }
        if level != "error" {
            continue;
        }
        if !expected.contains_key(target) {
            return Err(format!(
                "error from a dependency or unexpected target: {target}"
            ));
        }
        if !message["spans"]
            .as_array()
            .is_some_and(|spans| !spans.is_empty())
        {
            return Err(format!(
                "{target}: error without a source location (possibly a linker/infrastructure error)"
            ));
        }
        rejected.insert(target.to_owned());
    }
    let missing: Vec<_> = expected
        .keys()
        .filter(|name| !rejected.contains(*name) || !located.contains(*name))
        .collect();
    if !missing.is_empty() {
        return Err(format!("no source-associated rejection for {missing:?}"));
    }
    Ok(())
}

#[test]
fn harness_distinguishes_rejections_from_build_failures() {
    let expected = BTreeMap::from([("reject_case".into(), PathBuf::from("fixtures/case.rs"))]);
    let diagnostic = |target: &str, level: &str, spans: Value| {
        json!({
            "reason": "compiler-message", "target": {"name": target},
            "message": {"level": level, "spans": spans}
        })
    };
    let local = json!([{"file_name": "case.rs"}]);
    let error = diagnostic("reject_case", "error", local.clone());
    assert!(check_messages(&[error], &expected).is_ok());
    // A const assertion may emit its instantiation location as a separate note.
    let external = diagnostic(
        "reject_case",
        "error",
        json!([{"file_name": "median/src/class.rs"}]),
    );
    let note = diagnostic("reject_case", "note", local.clone());
    assert!(check_messages(&[external.clone(), note], &expected).is_ok());
    assert!(check_messages(&[external], &expected).is_err());
    assert!(check_messages(&[], &expected).is_err());
    assert!(
        check_messages(
            &[diagnostic("dependency", "error", local.clone())],
            &expected
        )
        .is_err()
    );
    assert!(check_messages(&[diagnostic("reject_case", "error", json!([]))], &expected).is_err());
    assert!(
        check_messages(
            &[diagnostic(
                "reject_case",
                "error: internal compiler error",
                local
            )],
            &expected
        )
        .is_err()
    );
}

#[test]
fn api_rejections() {
    let median = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = median.parent().unwrap();
    let metadata = cargo()
        .current_dir(workspace)
        .args(["metadata", "--offline", "--no-deps", "--format-version=1"])
        .output()
        .unwrap();
    assert!(
        metadata.status.success(),
        "{}",
        String::from_utf8_lossy(&metadata.stderr)
    );
    let metadata: Value = serde_json::from_slice(&metadata.stdout).unwrap();
    let target = Path::new(metadata["target_directory"].as_str().unwrap()).join("compile-tests");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = target.join(format!("cases-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let mut manifest = format!(
        "[package]\nname = \"median-compile-tests\"\nversion = \"0.0.0\"\nedition = \"2024\"\nautobins = false\n\n[workspace]\n\n[dependencies]\nmedian = {{ path = {} }}\nmedian-mock = {{ path = {} }}\n",
        json!(median),
        json!(workspace.join("median-mock")),
    );
    let mut passing = Vec::new();
    let mut failing = BTreeMap::new();
    for fixture in fixtures(&median.join("tests/compile")) {
        let source = fs::read_to_string(&fixture).unwrap();
        let name = fixture.file_stem().unwrap().to_str().unwrap();
        for (prefix, source) in [("pass", source.clone()), ("reject", rejected(&source))] {
            let bin = format!("{prefix}_{name}");
            let path = dir.join(format!("{bin}.rs"));
            fs::write(
                &path,
                format!("#![allow(dead_code, unused_imports)]\n{source}\n"),
            )
            .unwrap();
            manifest.push_str(&format!(
                "\n[[bin]]\nname = {}\npath = {}\n",
                json!(bin),
                json!(path)
            ));
            if prefix == "pass" {
                passing.push(bin);
            } else {
                failing.insert(bin, path);
            }
        }
    }
    for fixture in fixtures(&median.join("tests/ui_pass")) {
        let bin = format!("pass_{}", fixture.file_stem().unwrap().to_str().unwrap());
        manifest.push_str(&format!(
            "\n[[bin]]\nname = {}\npath = {}\n",
            json!(bin),
            json!(fixture)
        ));
        passing.push(bin);
    }
    fs::write(dir.join("Cargo.toml"), manifest).unwrap();
    // Keep workspace dependency versions; Cargo only needs to add this generated package.
    fs::copy(workspace.join("Cargo.lock"), dir.join("Cargo.lock")).unwrap();
    let output = build(&dir, &target.join("target"), &passing);
    assert!(
        output.status.success(),
        "valid controls failed ({}):\n{}",
        dir.display(),
        report(&output)
    );
    let output = build(
        &dir,
        &target.join("target"),
        &failing.keys().cloned().collect::<Vec<_>>(),
    );
    if let Err(error) = check_rejections(&output, &failing) {
        panic!("{error} ({}):\n{}", dir.display(), report(&output));
    }
    eprintln!(
        "{} valid controls and {} isolated rejections passed",
        passing.len(),
        failing.len()
    );
    fs::remove_dir_all(dir).unwrap();
}
