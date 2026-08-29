//! `plannotator-tui last` through the real binary against the hosts crate's fixtures.

#![allow(clippy::expect_used, reason = "tests assert by panicking")]

use std::path::PathBuf;
use std::process::Command;

use plannotator_tui_hosts::{Role, claude, codex, copilot, droid, pi};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_plannotator-tui"))
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plannotator-tui-hosts/tests/fixtures")
}

#[test]
fn print_writes_the_newest_assistant_message_of_a_claude_transcript() {
    let transcript = fixtures().join("claude-code.jsonl");
    let text = std::fs::read_to_string(&transcript).expect("fixture");
    let expected = claude::parse_messages(&text, 25)
        .into_iter()
        .find(|m| m.role == Role::Assistant)
        .expect("fixture has an assistant message")
        .text;
    let out = bin()
        .args(["last", "--host", "claude", "--session"])
        .arg(&transcript)
        .arg("--print")
        .output()
        .expect("runs");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim_end(), expected.trim_end());
}

#[test]
fn print_skips_a_newer_human_prompt() {
    let transcript =
        std::env::temp_dir().join(format!("plannotator-tui-last-print-{}.jsonl", std::process::id()));
    std::fs::write(
        &transcript,
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"message":{"role":"user","content":"older prompt"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","message":{"id":"a1","role":"assistant","content":[{"type":"text","text":"assistant reply"}]}}"#,
            "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"a1","message":{"role":"user","content":"newer prompt"}}"#,
        ),
    )
    .expect("writes transcript");

    let out = bin()
        .args(["last", "--host", "claude", "--session"])
        .arg(&transcript)
        .arg("--print")
        .output()
        .expect("runs");
    std::fs::remove_file(&transcript).expect("removes transcript");

    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim_end(), "assistant reply");
    assert!(out.stderr.is_empty());
}

#[test]
fn print_keeps_the_harmless_failure_contract_for_a_human_only_transcript() {
    let transcript =
        std::env::temp_dir().join(format!("plannotator-tui-last-human-only-{}.jsonl", std::process::id()));
    std::fs::write(
        &transcript,
        r#"{"type":"user","uuid":"u1","parentUuid":null,"message":{"role":"user","content":"a human prompt"}}"#,
    )
    .expect("writes transcript");

    let out = bin()
        .args(["last", "--host", "claude", "--session"])
        .arg(&transcript)
        .arg("--print")
        .output()
        .expect("runs");
    std::fs::remove_file(&transcript).expect("removes transcript");

    assert!(out.status.success(), "exit 0 is the contract");
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no assistant messages yet"));
}

#[test]
fn print_reads_a_codex_thread_from_a_sessions_root() {
    let root = fixtures().join("codex");
    let files = codex::find_transcripts(&root, None);
    let contents: Vec<String> = files.iter().map(|p| std::fs::read_to_string(p).expect("file")).collect();
    let expected = codex::parse_messages(&contents, 25)
        .into_iter()
        .find(|m| m.role == Role::Assistant)
        .expect("fixture has an assistant message")
        .text;
    let out = bin()
        .args(["last", "--host", "codex", "--session"])
        .arg(&root)
        .arg("--print")
        .output()
        .expect("runs");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim_end(), expected.trim_end());
}

#[test]
fn print_reads_a_copilot_session_directory() {
    let dir = fixtures().join("copilot/session-state/aaaa1111-0000-4000-8000-000000000001");
    let events = std::fs::read_to_string(dir.join("events.jsonl")).expect("fixture");
    let expected = copilot::parse_messages(&events, 25)
        .into_iter()
        .find(|m| m.role == Role::Assistant)
        .expect("fixture has an assistant message")
        .text;
    let out = bin()
        .args(["last", "--host", "copilot", "--session"])
        .arg(&dir)
        .arg("--print")
        .output()
        .expect("runs");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim_end(), expected.trim_end());
}

#[test]
fn print_reads_a_droid_log() {
    let log = fixtures().join("droid/sessions/-Users-me-repo/be4202cc-4266-4e3b-b0f1-9324af19e4be.jsonl");
    let text = std::fs::read_to_string(&log).expect("fixture");
    let expected = droid::parse_messages(&text, 25)
        .into_iter()
        .find(|m| m.role == Role::Assistant)
        .expect("fixture has an assistant message")
        .text;
    let out =
        bin().args(["last", "--host", "droid", "--session"]).arg(&log).arg("--print").output().expect("runs");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim_end(), expected.trim_end());
}

#[test]
fn print_never_fails_the_caller_when_nothing_is_found() {
    let missing = fixtures().join("does-not-exist.jsonl");
    let out = bin().args(["last", "--session"]).arg(&missing).arg("--print").output().expect("runs");
    assert!(out.status.success(), "exit 0 is the contract");
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("does-not-exist.jsonl"));
}

#[test]
fn stdin_is_printed_back_verbatim() {
    use std::io::Write as _;
    let mut child = bin()
        .args(["last", "--stdin", "--print"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawns");
    child.stdin.take().expect("stdin").write_all(b"# hi\n\nfrom stdin\n").expect("write");
    let out = child.wait_with_output().expect("runs");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "# hi\n\nfrom stdin\n");
}

#[test]
fn print_writes_the_newest_assistant_message_of_a_pi_session() {
    let transcript = fixtures().join("pi.jsonl");
    let text = std::fs::read_to_string(&transcript).expect("fixture");
    let expected = pi::parse_messages(&text, 25)
        .into_iter()
        .find(|m| m.role == Role::Assistant)
        .expect("fixture has an assistant message")
        .text;
    let out = bin()
        .args(["last", "--host", "pi", "--session"])
        .arg(&transcript)
        .arg("--print")
        .output()
        .expect("runs");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim_end(), expected.trim_end());
}

#[test]
fn print_reads_an_explicit_omp_session_with_the_pi_parser() {
    let transcript = fixtures().join("pi.jsonl");
    let text = std::fs::read_to_string(&transcript).expect("fixture");
    let expected = pi::parse_messages(&text, 25)
        .into_iter()
        .find(|m| m.role == Role::Assistant)
        .expect("fixture has an assistant message")
        .text;
    let out = bin()
        .args(["last", "--host", "omp", "--session"])
        .arg(&transcript)
        .arg("--print")
        .output()
        .expect("runs");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim_end(), expected.trim_end());
}

#[test]
fn print_reports_that_omp_requires_an_explicit_session() {
    let out = bin().args(["last", "--host", "omp", "--print"]).output().expect("runs");
    assert!(out.status.success(), "exit 0 is the contract");
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("OMP requires an explicit session path"));
}
