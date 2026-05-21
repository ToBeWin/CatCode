use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_repo() -> std::path::PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("catcode-handoff-cli-{id}"));
    std::fs::create_dir_all(&path).unwrap();
    let output = Command::new("git")
        .arg("init")
        .current_dir(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    path
}

#[test]
fn handoff_json_accepts_unquoted_multiword_task() {
    let repo = temp_repo();
    let output = Command::new(env!("CARGO_BIN_EXE_catcode"))
        .args([
            "handoff",
            "fix",
            "login",
            "bug",
            "--project-dir",
            repo.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "catcode handoff failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["task_summary"].as_str().unwrap(), "fix login bug");
    assert!(json["ready"].as_bool().unwrap());
    assert!(
        json["changes"]["changed_files"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    std::fs::remove_dir_all(repo).unwrap();
}
