//! Cross-format JSONL, message-time bounds, and exact project selection. -- PI/OpenAI

use serde_json::{Value, json};
use std::fs::{self, File, FileTimes};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Home(PathBuf);

impl Home {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = Self(std::env::temp_dir().join(format!("asf-filters-{stamp}")));
        fs::create_dir_all(&home.0).unwrap();
        home
    }

    fn write(&self, path: &str, entries: &[Value]) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = entries
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, format!("{text}\n")).unwrap();
        path
    }

    fn pi(&self, name: &str, id: &str, cwd: &str, entries: &[Value]) -> PathBuf {
        let mut records =
            vec![json!({"type":"session", "id":id, "cwd":cwd, "timestamp":"2026-01-01T00:00:00Z"})];
        records.extend_from_slice(entries);
        self.write(
            &format!(".pi/agent/sessions/project/{name}.jsonl"),
            &records,
        )
    }

    fn run(&self, args: &[&str]) -> Output {
        run(&self.0, args)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_asf"))
        .args(args)
        .env("HOME", home)
        .env(
            "XDG_CACHE_HOME",
            std::env::temp_dir().join(format!(
                "asf-filter-cache-{}-{}",
                std::process::id(),
                home.file_name().unwrap().to_string_lossy()
            )),
        )
        .env("TZ", "Australia/Perth")
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn records(output: &Output) -> Vec<Value> {
    stdout(output)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn user(id: &str, time: Option<&str>, text: &str) -> Value {
    let mut entry = json!({"type":"message", "id":id, "message":{"role":"user", "content":text}});
    if let Some(time) = time {
        entry["timestamp"] = json!(time);
    }
    entry
}

const ID: &str = "01a10007-446e-7029-9d46-254776d94b86";

#[test]
fn antigravity_reads_native_sqlite_and_keeps_step_numbers() {
    let home = Home::new();
    let build = |relative: String| {
        let path = home.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        stdout(
            &Command::new("sqlite3")
                .args(["-init", "/dev/null"])
                .arg(&path)
                .arg(format!("\n{}", include_str!("../fixtures/agy.sql")))
                .output()
                .unwrap(),
        );
        path
    };
    let path = build(format!(".gemini/antigravity-cli/conversations/{ID}.db"));
    let child = "01a10007-446e-7029-9d46-254776d94b87";
    let child_path = build(format!(".gemini/antigravity/conversations/{child}.db"));
    stdout(
        &Command::new("sqlite3")
            .args(["-init", "/dev/null"])
            .arg(&child_path)
            .arg("UPDATE trajectory_metadata_blob SET data = X'2a06706172656e74';")
            .output()
            .unwrap(),
    );
    let path = path.to_str().unwrap();
    assert_eq!(
        stdout(&home.run(&["-a", "agy", "--paths", "rust changes"])).trim(),
        path
    );
    let all = stdout(&home.run(&["-a", "agy", "--paths", "--sub"]));
    assert!(all.lines().any(|line| line == child_path.to_str().unwrap()));
    assert_eq!(
        stdout(&home.run(&["-a", "agy", "-c", "--paths", "duplicate entry"])).trim(),
        path
    );
    // A scoped search must not decode the unrelated conversation. -- PI/OpenAI
    stdout(
        &Command::new("sqlite3")
            .arg(&child_path)
            .arg("UPDATE steps SET step_format = 99;")
            .output()
            .unwrap(),
    );
    let scoped = home.run(&[
        "-a", "agy", "-c", "--paths", "--sub", "--cwd", "/tmp/asf project", "duplicate entry",
    ]);
    assert_eq!(stdout(&scoped).trim(), path);
    assert!(scoped.stderr.is_empty(), "{}", String::from_utf8_lossy(&scoped.stderr));
    let rows = records(&home.run(&["-a", "agy", "-r", &ID[..8], "--jsonl"]));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1]["source"]["index"], 3);
    assert_eq!(rows[1]["timestamp"], "2026-01-01T01:01:00Z");
    assert_eq!(rows[1]["text"], "found the duplicate entry");
    assert_eq!(rows[1]["cwd"], "/tmp/asf project");
    assert_eq!(rows[1]["source"]["record_id"], "fixture-response");
    let extra = stdout(&home.run(&["-r", path, "--line", "3", "--tools", "--think"]));
    assert!(extra.contains("private reasoning") && extra.contains("`read_file`"));
    assert!(stdout(&home.run(&["-p", path, "--line", "3"])).contains("## assistant #3"));
    let resume = stdout(&home.run(&["-u", path]));
    assert!(
        resume.trim().ends_with(&format!("agy --conversation {ID}")),
        "{resume}"
    );
}

#[test]
fn copilot_search_ignores_bookkeeping_and_preview_reports_read_errors() {
    let home = Home::new();
    let label = format!(".copilot/session-state/{ID}");
    home.write(
        &format!("{label}/metadata.json"),
        &[json!({"text":"copilot needle"})],
    );
    let found = home.run(&["-a", "copilot", "-c", "--paths", "copilot needle"]);
    assert!(stdout(&found).trim().is_empty());
    assert!(found.stderr.is_empty());
    let events = home.write(
        &format!("{label}/events.jsonl"),
        &[json!({"type":"user.message", "data":{"content":"copilot needle"}})],
    );
    assert_eq!(
        stdout(&home.run(&["-a", "copilot", "-c", "--paths", "copilot needle"])).trim(),
        events.to_str().unwrap()
    );
    use std::io::Write;
    fs::OpenOptions::new()
        .append(true)
        .open(&events)
        .unwrap()
        .write_all(&[0xff, b'\n'])
        .unwrap();
    let preview = home.run(&["-p", events.to_str().unwrap()]);
    assert!(!preview.status.success());
    let error = String::from_utf8_lossy(&preview.stderr);
    assert!(
        error.contains("events.jsonl:2:") && error.contains("UTF-8") && !error.contains("panicked"),
        "{error}"
    );
}

#[test]
fn fixture_formats_share_a_jsonl_schema_with_traceable_sources() {
    let home = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/home"));
    for agent in [
        "claude", "codex", "pi", "opencode", "gemini", "copilot", "hermes",
    ] {
        let path = stdout(&run(home, &["--paths", "-a", agent, "-n", "1"]))
            .trim()
            .to_string();
        let output = run(
            home,
            &["-r", &path, "--jsonl", "--role", "user", "--tail", "1"],
        );
        let rows = records(&output);
        assert_eq!(rows.len(), 1, "{agent}");
        let row = &rows[0];
        assert_eq!(row["agent"], agent);
        assert_eq!(row["role"], "user");
        assert!(!row["text"].as_str().unwrap().is_empty());
        assert!(!row["session_id"].as_str().unwrap().is_empty());
        row["timestamp"]
            .as_str()
            .unwrap()
            .parse::<jiff::Timestamp>()
            .unwrap();
        assert!(!row["source"]["path"].as_str().unwrap().is_empty());
        if agent == "hermes" {
            assert!(
                row["source"]["record_id"]
                    .as_str()
                    .unwrap()
                    .parse::<u64>()
                    .is_ok()
            );
        } else if agent == "gemini" {
            assert!(row["source"]["index"].as_u64().unwrap() > 0);
            assert_ne!(row["session_id"], "logs");
        } else if agent == "opencode" {
            assert!(!row["source"]["parts"].as_array().unwrap().is_empty());
            let original: Value =
                serde_json::from_slice(&fs::read(row["source"]["path"].as_str().unwrap()).unwrap())
                    .unwrap();
            let time =
                jiff::Timestamp::from_millisecond(original["time"]["created"].as_i64().unwrap())
                    .unwrap();
            assert_eq!(row["timestamp"], time.to_string());
        } else {
            let original = fs::read_to_string(row["source"]["path"].as_str().unwrap()).unwrap();
            let original: Value = serde_json::from_str(
                original
                    .lines()
                    .nth(row["source"]["line"].as_u64().unwrap() as usize - 1)
                    .unwrap(),
            )
            .unwrap();
            let time: jiff::Timestamp = original["timestamp"].as_str().unwrap().parse().unwrap();
            assert_eq!(row["timestamp"], time.to_string());
        }
        let time = row["timestamp"].as_str().unwrap();
        let bounded = records(&run(
            home,
            &[
                "-r", &path, "--jsonl", "--role", "user", "--since", time, "--until", time,
            ],
        ));
        assert!(
            !bounded.is_empty(),
            "{agent}: an exact-time window lost its source record"
        );
        assert!(bounded.iter().all(|row| row["timestamp"] == time));
    }
}

#[test]
fn date_and_role_filters_precede_head_and_tail_without_jsonl_markers() {
    let home = Home::new();
    let path = home.pi("dated", ID, "/tmp/project", &[
        user("old", Some("2026-10-23T00:00:00Z"), "old instruction"),
        user("first", Some("2026-10-24T00:00:00Z"), "first instruction"),
        json!({"type":"message", "timestamp":"2026-10-24T00:30:00Z", "message":{"role":"assistant", "content":[{"type":"text","text":"assistant answer"}]}}),
        user("middle", Some("2026-10-24T01:00:00Z"), "middle instruction"),
        user("last", Some("2026-10-24T02:00:00Z"), "last instruction"),
        user("new", Some("2026-10-25T00:00:00Z"), "new instruction"),
    ]);
    let args = [
        "-r",
        path.to_str().unwrap(),
        "--role",
        "user",
        "--since",
        "2026-10-24T00:00:00Z",
        "--until",
        "2026-10-24T10:00:00+08:00",
        "--head",
        "1",
        "--tail",
        "1",
    ];
    let rows = records(&home.run(&[args.as_slice(), &["--jsonl"]].concat()));
    assert_eq!(
        rows.iter()
            .map(|row| row["text"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["first instruction", "last instruction"]
    );
    assert_eq!(rows[0]["source"]["line"], 3);
    assert_eq!(rows[1]["source"]["record_id"], "last");
    let markdown = stdout(&home.run(&args));
    assert!(markdown.contains("1 messages omitted"), "{markdown}");
    assert!(!markdown.contains("assistant answer"));
    assert!(markdown.contains("## user #3\nfirst instruction"));
    assert!(markdown.contains("## user #6\nlast instruction"));
    let around = stdout(&home.run(&["-r", path.to_str().unwrap(), "--around", "5"]));
    assert_eq!(
        around.trim(),
        "## assistant #4\nassistant answer\n\n## user #5\nmiddle instruction\n\n## user #6\nlast instruction"
    );
}

#[test]
fn unknown_or_invalid_times_are_null_and_never_filled_from_headers_or_nested_tools() {
    let home = Home::new();
    let mut nested = user("nested", None, "nested-time instruction");
    nested["message"]["tool"] = json!({"timestamp":"2026-10-24T01:00:00Z"});
    let path = home.pi(
        "unknown",
        ID,
        "/tmp/project",
        &[
            user("missing", None, "missing timestamp"),
            user("invalid", Some("not a timestamp"), "invalid timestamp"),
            nested,
            user("dated", Some("2026-10-24T00:00:00Z"), "dated instruction"),
        ],
    );
    let all = records(&home.run(&["-r", path.to_str().unwrap(), "--jsonl"]));
    assert_eq!(all.len(), 4);
    assert!(all[..3].iter().all(|row| row["timestamp"].is_null()));
    let output = home.run(&[
        "-r",
        path.to_str().unwrap(),
        "--jsonl",
        "--since",
        "2026-01-01T00:00:00Z",
    ]);
    assert_eq!(records(&output).len(), 1);
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("excluded 3 messages with unknown timestamps")
    );
}

#[test]
fn pi_message_milliseconds_and_local_dates_are_supported() {
    let home = Home::new();
    let mut entry = user("milliseconds", None, "millisecond timestamp");
    let time: jiff::Timestamp = "2026-10-23T16:00:00Z".parse().unwrap();
    entry["message"]["timestamp"] = json!(time.as_millisecond());
    let path = home.pi("milliseconds", ID, "/tmp/project", &[entry]);
    let output = home.run(&[
        "-r",
        path.to_str().unwrap(),
        "--jsonl",
        "--since",
        "2026-10-24",
        "--until",
        "2026-10-24",
    ]);
    assert_eq!(records(&output)[0]["timestamp"], "2026-10-23T16:00:00Z");
}

#[test]
fn relative_windows_use_message_time_and_empty_jsonl_is_successful() {
    let home = Home::new();
    let recent = jiff::Timestamp::now()
        .checked_sub(Duration::from_secs(3600))
        .unwrap()
        .to_string();
    let path = home.pi(
        "relative",
        ID,
        "/tmp/project",
        &[
            user("old", Some("2026-01-01T00:00:00Z"), "old message"),
            user("recent", Some(&recent), "recent message"),
        ],
    );
    let rows = records(&home.run(&[
        "-r",
        path.to_str().unwrap(),
        "--jsonl",
        "--since",
        "24h",
        "--until",
        "now",
    ]));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["text"], "recent message");
    assert!(
        records(&home.run(&["-r", path.to_str().unwrap(), "--jsonl", "--since", "1s"])).is_empty()
    );
}

#[test]
fn jsonl_preserves_distinct_records_even_when_text_is_repeated_or_shortened() {
    let home = Home::new();
    let path = home.pi(
        "resends",
        ID,
        "/tmp/project",
        &[
            user(
                "first",
                Some("2026-10-23T00:00:00Z"),
                "yes, run that experiment",
            ),
            user("second", Some("2026-10-24T00:00:00Z"), "yes"),
        ],
    );
    let rows = records(&home.run(&["-r", path.to_str().unwrap(), "--jsonl"]));
    assert_eq!(rows.len(), 2);
    let markdown = stdout(&home.run(&[
        "-r",
        path.to_str().unwrap(),
        "--since",
        "2026-10-24T00:00:00Z",
    ]));
    assert_eq!(markdown.trim(), "## user #3\nyes");
}

#[test]
fn cwd_filtering_happens_before_limits_exact_name_stops_and_newest_selection() {
    let home = Home::new();
    let wanted = home.0.join("wanted");
    let elsewhere = home.0.join("wanted-other");
    fs::create_dir_all(&wanted).unwrap();
    fs::create_dir_all(&elsewhere).unwrap();
    let old = home.pi(
        "old",
        ID,
        wanted.to_str().unwrap(),
        &[user("a", None, "shared topic")],
    );
    File::open(&old)
        .unwrap()
        .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(100)))
        .unwrap();
    home.pi(
        "new",
        "01a10007-446e-7029-9d46-254776d94b87",
        elsewhere.to_str().unwrap(),
        &[user("b", None, "shared topic")],
    );
    for flags in [vec![], vec!["-c"], vec!["--stream-rows"]] {
        let args = [
            vec![
                "--paths",
                "-n",
                "1",
                "--cwd",
                wanted.to_str().unwrap(),
                "shared topic",
            ],
            flags,
        ]
        .concat();
        let output = stdout(&home.run(&args));
        assert!(output.contains(old.to_str().unwrap()), "{output}");
        assert!(!output.contains("new.jsonl"), "{output}");
    }
    let picked = records(&home.run(&[
        "-r",
        "--jsonl",
        "--cwd",
        wanted.to_str().unwrap(),
        "shared topic",
    ]));
    assert_eq!(picked[0]["session_id"], ID);
    assert!(
        !home
            .run(&[
                "-r",
                old.to_str().unwrap(),
                "--cwd",
                elsewhere.to_str().unwrap()
            ])
            .status
            .success()
    );
    assert!(
        !home
            .run(&["-r", ID, "--cwd", elsewhere.to_str().unwrap()])
            .status
            .success()
    );
    let relative = Command::new(env!("CARGO_BIN_EXE_asf"))
        .args(["--cwd", "./wanted/../wanted", "--paths", "-n", "1"])
        .current_dir(&home.0)
        .env("HOME", &home.0)
        .env("XDG_CACHE_HOME", home.0.join("cache"))
        .output()
        .unwrap();
    assert_eq!(stdout(&relative).trim(), old.to_str().unwrap());
}

#[test]
fn cwd_matches_symlinks_but_not_project_path_prefixes() {
    let home = Home::new();
    let project = home.0.join("project");
    let alias = home.0.join("alias");
    fs::create_dir_all(&project).unwrap();
    std::os::unix::fs::symlink(&project, &alias).unwrap();
    let path = home.pi(
        "aliased",
        ID,
        alias.to_str().unwrap(),
        &[user("a", None, "aliased message")],
    );
    assert_eq!(
        stdout(&home.run(&["--paths", "--cwd", project.to_str().unwrap()])).trim(),
        path.to_str().unwrap()
    );
}

#[test]
fn role_selection_uses_records_not_headings_inside_message_text() {
    let home = Home::new();
    let path = home.pi(
        "heading",
        ID,
        "/tmp/project",
        &[user(
            "a",
            Some("2026-10-24T00:00:00Z"),
            "a quote:\n## assistant\nnot a separate message",
        )],
    );
    let rows = records(&home.run(&["-r", path.to_str().unwrap(), "--jsonl", "--role", "user"]));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["role"], "user");
}

#[test]
fn markdown_role_filter_does_not_merge_user_turns_separated_by_an_assistant() {
    let home = Home::new();
    let path = home.pi(
        "separate",
        ID,
        "/tmp/project",
        &[
            user("a", None, "yes"),
            json!({"type":"message", "message":{"role":"assistant", "content":"another question"}}),
            user("b", None, "yes"),
        ],
    );
    let markdown = stdout(&home.run(&["-r", path.to_str().unwrap(), "--role", "user"]));
    assert_eq!(markdown.matches("## user").count(), 2);
}

#[test]
fn invalid_or_reversed_dates_fail_before_search_and_read_flags_require_read() {
    let home = Home::new();
    for args in [
        vec!["-r", "--since", "yesterday-ish"],
        vec!["-r", "--since", "99999999999999999999999h"],
        vec!["-r", "--since", "2026-10-25", "--until", "2026-10-24"],
        vec!["--since", "24h"],
        vec!["--until", "now"],
        vec!["--jsonl"],
    ] {
        let output = home.run(&args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
}
