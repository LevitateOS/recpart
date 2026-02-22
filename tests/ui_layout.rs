use std::process::Command;

fn run_recpart_with_size(cols: &str, rows: &str) -> std::process::Output {
    Command::new("cargo")
        .args(["run", "-q", "-p", "recpart", "--", "tui"])
        .env("COLUMNS", cols)
        .env("LINES", rows)
        .output()
        .expect("recpart command should execute through cargo run")
}

#[test]
fn tui_rejects_terminal_below_minimum_size() {
    let output = run_recpart_with_size("79", "23");

    assert_eq!(output.status.code(), Some(12));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Resize terminal to at least 80 columns and 24 rows"),
        "unexpected stderr: {stderr}"
    );
}
