//! Recent-first lookup, cache invalidation, and names outside the tail window. -- PI/OpenAI

use std::fs::{File, FileTimes};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn run(home: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_asf"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn names_are_cached_recent_first_and_refreshed_after_renaming() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("asf-name-search-{unique}"));
    let directory = home.join(".pi/agent/sessions/project");
    std::fs::create_dir_all(&directory).unwrap();
    let recent = directory.join("2026-10-03_01a10007-446e-7029-9d46-254776d94b86.jsonl");
    let old = directory.join("2026-10-02_01a10007-446e-7029-9d46-254776d94b87.jsonl");
    let session = |id: &str, name: &str| {
        format!(
            "{{\"type\":\"session\",\"id\":\"{id}\",\"cwd\":\"/tmp\"}}\n{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"start\"}}}}\n{{\"type\":\"session_info\",\"name\":\"{name}\"}}\n"
        )
    };
    std::fs::write(
        &recent,
        session("01a10007-446e-7029-9d46-254776d94b86", "recent name"),
    )
    .unwrap();
    std::fs::write(
        &old,
        session("01a10007-446e-7029-9d46-254776d94b87", "older name"),
    )
    .unwrap();
    File::open(&old)
        .unwrap()
        .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(100)))
        .unwrap();

    assert_eq!(
        run(&home, &["--paths", "-a", "pi", "-n", "1"]).trim(),
        recent.to_str().unwrap()
    );
    let cache_path = home.join("cache/asf/metadata-v2/index.json");
    let cache: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&cache_path).unwrap()).unwrap();
    assert!(cache.get(recent.to_str().unwrap()).is_some());
    assert!(
        cache.get(old.to_str().unwrap()).is_some(),
        "the first lookup must populate all metadata"
    );

    assert_eq!(
        run(&home, &["recent name", "--paths"]).trim(),
        recent.to_str().unwrap()
    );
    let cache: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&cache_path).unwrap()).unwrap();
    assert!(cache.get(old.to_str().unwrap()).is_some());
    let first = run(&home, &["recent name", "-n", "1"]);
    assert!(first.contains("recent name"));
    assert_eq!(run(&home, &["recent name", "-n", "1"]), first);

    // Put the new name beyond both windows; head/tail alone must not keep the stale name. -- PI/OpenAI
    let filler = format!(
        "{{\"type\":\"custom\",\"data\":\"{}\"}}\n",
        "x".repeat(70_000)
    );
    std::fs::write(
        &recent,
        format!(
            "{}{}{{\"type\":\"session_info\",\"name\":\"renamed in middle\"}}\n{}",
            session("01a10007-446e-7029-9d46-254776d94b86", "recent name"),
            filler,
            filler
        ),
    )
    .unwrap();
    let renamed = run(&home, &["renamed in middle", "-n", "1"]);
    assert!(renamed.contains("renamed in middle"), "{renamed}");
    assert_eq!(run(&home, &["recent name"]), "nothing matched\n");
    assert_eq!(run(&home, &["renamed in middle", "-n", "1"]), renamed);

    use std::io::Write;
    let mut writer = std::fs::OpenOptions::new()
        .append(true)
        .open(&recent)
        .unwrap();
    writer
        .write_all(b"{\"type\":\"session_info\",\"name\":\"split")
        .unwrap();
    assert!(run(&home, &["renamed in middle", "-a", "pi"]).contains("renamed in middle"));
    writer.write_all(b" rename\"}\n").unwrap();
    assert!(run(&home, &["split rename", "-a", "pi"]).contains("split rename"));
    assert_eq!(
        run(&home, &["renamed in middle", "-a", "pi"]),
        "nothing matched\n"
    );
    assert_eq!(
        run(&home, &["/tmp", "-a", "pi", "--paths", "-n", "1"]).trim(),
        recent.to_str().unwrap()
    );
    assert!(
        run(&home, &["-u", "01a10007-446e-7029-9d46-254776d94b86"])
            .contains("pi --session 01a10007-446e-7029-9d46-254776d94b86")
    );
    std::fs::write(
        &recent,
        session("01a10007-446e-7029-9d46-254776d94b86", "after truncate"),
    )
    .unwrap();
    assert!(run(&home, &["after truncate", "-a", "pi"]).contains("after truncate"));
    assert_eq!(
        run(&home, &["split rename", "-a", "pi"]),
        "nothing matched\n"
    );
    let mut writer = std::fs::OpenOptions::new()
        .append(true)
        .open(&recent)
        .unwrap();
    writer
        .write_all(br#"{"type":"session_info","name":"before newline"}"#)
        .unwrap();
    assert!(run(&home, &["before newline", "-a", "pi"]).contains("before newline"));
    let cached: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&cache_path).unwrap()).unwrap();
    assert_eq!(
        cached[recent.to_str().unwrap()]["offset"].as_u64().unwrap(),
        recent.metadata().unwrap().len()
    );
    writer.write_all(b"\n").unwrap();
    writeln!(
        writer,
        "{}",
        serde_json::json!({"type":"session_info", "name":r"ΟΣ metadata \n"})
    )
    .unwrap();
    assert!(run(&home, &[r"οσ METADATA \n", "-a", "pi"]).contains(r"ΟΣ metadata \n"));
    std::fs::remove_file(&recent).unwrap();
    assert_eq!(run(&home, &["renamed in middle"]), "nothing matched\n");
    assert_eq!(
        run(&home, &["--paths", "-n", "1"]).trim(),
        old.to_str().unwrap()
    );
    let codex = home.join(".codex/sessions/rollout-01a10007-446e-7029-9d46-254776d94b88.jsonl");
    std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
    std::fs::write(&codex, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"01a10007-446e-7029-9d46-254776d94b88\",\"cwd\":\"/tmp\"}}\n").unwrap();
    let index = home.join(".codex/session_index.jsonl");
    let index_record = |name| {
        format!("{{\"id\":\"01a10007-446e-7029-9d46-254776d94b88\",\"thread_name\":\"{name}\"}}\n")
    };
    std::fs::write(&index, index_record("indexed name")).unwrap();
    assert!(run(&home, &["indexed name", "-a", "codex"]).contains("indexed name"));
    std::fs::write(&index, index_record("indexed rename")).unwrap();
    assert!(run(&home, &["indexed rename", "-a", "codex"]).contains("indexed rename"));
    assert_eq!(
        run(&home, &["indexed name", "-a", "codex"]),
        "nothing matched\n"
    );

    let claude = home.join(".claude/projects/project/01a10007-446e-7029-9d46-254776d94b89.jsonl");
    std::fs::create_dir_all(claude.parent().unwrap()).unwrap();
    std::fs::write(
        &claude,
        concat!(
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"start\"}}\n",
            "{\"type\":\"custom-title\",\"customTitle\":\"custom wins\"}\n",
            "{\"type\":\"agent-name\",\"agentName\":\"previous name\"}\n",
            "{\"type\":\"agent-name\",\"agentName\":\"\"}\n",
        ),
    )
    .unwrap();
    assert!(run(&home, &["custom wins", "-a", "claude"]).contains("custom wins"));
    assert_eq!(
        run(&home, &["previous name", "-a", "claude"]),
        "nothing matched\n"
    );
    std::fs::remove_dir_all(home).unwrap();
}
