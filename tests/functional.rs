//! asf against a fake HOME of scrubbed real sessions, one per agent. `cargo test` builds the
//! binary itself. Rebuild the fixtures with tests/harvest.sh when an agent changes format.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const HOME: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/home");

struct Output {
    stdout: String,
    stderr: String,
    success: bool,
}

fn run(home: &str, args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_asf"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CACHE_HOME", std::env::temp_dir().join(format!("asf-test-cache-{}", std::process::id())))
        .output()
        .unwrap();
    Output {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        success: out.status.success(),
    }
}

fn asf(args: &[&str]) -> String {
    // the sessions record this as their working directory; resume says so when it is gone
    std::fs::create_dir_all("/tmp/asf-fixture-repo").unwrap();
    let out = run(HOME, args);
    assert!(out.success, "asf {args:?} failed: {}", out.stderr);
    out.stdout
}

fn path_of(agent: &str) -> String {
    asf(&["--paths", "-a", agent, "-n", "1"]).trim().to_string()
}

#[test]
fn every_agent_is_read() {
    let out = asf(&["-n", "20"]);
    // the agent column, padded, so that "pi" cannot be matched by "copilot"
    let agents = [
        "claude", "codex", "pi", "opencode", "copilot", "gemini", "hermes",
    ];
    for agent in agents {
        assert!(
            out.contains(&format!("| {agent} ")),
            "{agent} is missing from\n{out}"
        );
    }
    // six files-on-disk agents with one session each, and hermes with two
    assert!(out.contains("8 sessions matched"), "{out}");
}

/// claude rewrites its name records and keeps the old copies, so the last agent-name wins.
#[test]
fn the_name_is_the_one_the_agent_ended_with() {
    let out = asf(&["-n", "20"]);
    assert!(out.contains("fixture claude agentName"), "{out}");
    assert!(
        !out.contains("fixture claude aiTitle"),
        "a lesser name record won:\n{out}"
    );
    // the file holds four earlier copies of each record, all labelled stale
    assert!(
        !out.contains("fixture claude stale"),
        "an older name record won:\n{out}"
    );
    assert!(out.contains("fixture opencode title"), "{out}");
    // pi keeps its name in a session_info record, which beats the opening message
    assert!(out.contains("fixture pi name"), "{out}");
}

#[test]
fn streaming_names_match_the_complete_scan() {
    let ordered = asf(&["--stream-rows"]);
    let dates: Vec<_> = ordered.lines().map(|line| &line[..16]).collect();
    assert!(dates.windows(2).all(|pair| pair[0] >= pair[1]), "not newest first:\n{ordered}");
    for args in [vec![], vec!["fixture"], vec!["nothing said this"], vec!["--sub"], vec!["-a", "pi"]] {
        let mut complete = asf(&["--rows", "-n", "20"].into_iter().chain(args.iter().copied()).collect::<Vec<_>>())
            .lines().filter(|line| !line.is_empty()).map(str::to_owned).collect::<Vec<_>>();
        let mut streamed = asf(&["--stream-rows"].into_iter().chain(args.iter().copied()).collect::<Vec<_>>())
            .lines().map(str::to_owned).collect::<Vec<_>>();
        complete.sort();
        streamed.sort();
        assert_eq!(streamed, complete, "{args:?}");
    }
}

#[test]
fn a_name_search_matches_the_name_and_the_opening_message() {
    assert!(asf(&["fixture", "claude"]).contains("1 sessions matched"));
    assert!(asf(&["gemini widget"]).contains("1 sessions matched"));
    assert!(asf(&["nothing said this"]).contains("nothing matched"));
}

#[test]
fn a_content_search_reads_the_whole_transcript() {
    let out = asf(&["-c", "the pi widget"]);
    assert!(out.contains("| pi "), "{out}");
    assert!(out.contains("1 sessions matched"), "{out}");
    assert!(asf(&["-c", "widget"]).contains("8 sessions matched"));
}

/// a run an agent started for itself cannot be resumed, so it is hidden unless asked for
#[test]
fn subagent_runs_are_hidden() {
    assert!(asf(&["-n", "20"]).contains("8 sessions matched"));
    assert!(asf(&["--sub", "-n", "20"]).contains("12 sessions matched"));
    // pi names the ones a tool started; claude keeps its own in a directory of their own
    assert!(!asf(&["--paths", "-n", "20"]).contains("rev-fixture-1"));
    assert!(asf(&["--sub", "--paths", "-n", "20"]).contains("rev-fixture-1"));
    assert!(!asf(&["--paths", "-n", "20"]).contains("/subagents/"));
    assert!(asf(&["--sub", "--paths", "-n", "20"]).contains("/subagents/"));
    // content mode learns it from the header separately
    assert!(asf(&["-c", "widget"]).contains("8 sessions matched"));
    assert!(asf(&["--sub", "-c", "widget"]).contains("12 sessions matched"));
}

#[test]
fn each_agent_gets_its_own_resume_command() {
    for (agent, wanted) in [
        ("claude", "claude --resume "),
        ("codex", "codex resume "),
        ("pi", "pi --session "),
        ("opencode", "opencode --session ses_"),
        ("copilot", "copilot --resume="),
        ("hermes", "hermes --resume "),
    ] {
        let out = asf(&["--resume", &path_of(agent)]);
        assert!(
            out.contains(wanted),
            "{agent}: wanted {wanted:?}, got {out}"
        );
    }
    // gemini resumes by list index, never by id
    assert!(asf(&["--resume", &path_of("gemini")]).contains("no resume command"));
}

#[test]
fn read_exports_markdown() {
    let plain = asf(&["--read", &path_of("claude")]);
    assert!(plain.starts_with("## "), "{plain}");
    assert!(plain.contains("## assistant"), "{plain}");
    assert!(
        !plain.contains("- `Bash`"),
        "tool calls are out unless asked for:\n{plain}"
    );

    let tools = asf(&["--read", &path_of("claude"), "--tools"]);
    assert!(tools.contains("- `"), "{tools}");
    assert!(
        tools.contains("## tool"),
        "a tool result is not the user talking:\n{tools}"
    );

    let think = asf(&["--read", &path_of("codex"), "--think"]);
    assert!(think.contains("\n> "), "{think}");
}

/// hermes keeps its sessions in a sqlite database, so none of the file reading applies
#[test]
fn hermes_is_read_out_of_its_database() {
    let path = path_of("hermes");
    assert!(
        path.contains("state.db#"),
        "a hermes session is a row, not a file: {path}"
    );
    let out = asf(&["--read", &path, "--tools", "--think"]);
    assert!(out.contains("## user"), "{out}");
    assert!(out.contains("- `bash`"), "{out}");
    assert!(
        out.contains("> the hermes widget factory counts its boxes"),
        "{out}"
    );
    // and the flags have to be asked for, or they are not flags
    let plain = asf(&["--read", &path]);
    assert!(
        !plain.contains("- `bash`"),
        "tool calls are out unless asked for:\n{plain}"
    );
    assert!(
        !plain.contains("counts its boxes"),
        "reasoning is out unless asked for:\n{plain}"
    );
    // the child session is a run it started for itself, and hermes records the parent
    assert!(!asf(&["--paths", "-a", "hermes", "-n", "9"]).contains("20260802_213338_6dbf13"));
    assert!(
        asf(&["--sub", "--paths", "-a", "hermes", "-n", "9"]).contains("20260802_213338_6dbf13")
    );
}

#[test]
fn head_and_tail_cut_the_middle_out() {
    let path = path_of("codex");
    let all = asf(&["--read", &path]);
    let ends = asf(&["--read", &path, "--head", "1", "--tail", "1"]);
    assert!(ends.contains("\n---\n\n... "), "{ends}");
    assert!(ends.contains(" messages omitted ...\n\n---\n"), "{ends}");
    assert!(ends.len() < all.len(), "cutting the middle made it longer");
}

/// Every client, every field the preview reads out of it. One stub session per agent, so an
/// agent that changes its format fails a row here instead of quietly showing a blank column.
///
/// The values are what the scrubber writes: a name it knows becomes `fixture <agent> <key>`,
/// every other free text becomes the filler. "" is a field that store does not hold at all.
#[test]
fn each_agent_gives_up_its_name_directory_and_model() {
    let filler = |agent: &str| format!("the {agent} widget factory ships on tuesday");
    let want = [
        // agent, name, dir, model
        (
            "claude",
            "fixture claude agentName",
            "/tmp/asf-fixture-repo",
            "filler",
        ),
        ("codex", "filler", "/tmp/asf-fixture-repo", "filler"),
        ("pi", "fixture pi name", "/tmp/asf-fixture-repo", "filler"),
        // gemini keeps one logs.json of prompts per project: no directory in it, no model
        ("gemini", "filler", "fixture-project", ""),
        (
            "opencode",
            "fixture opencode title",
            "/tmp/asf-fixture-repo",
            "filler",
        ),
        ("copilot", "filler", "/tmp/asf-fixture-repo", "filler"),
        (
            "hermes",
            "fixture hermes 2",
            "/tmp/asf-fixture-repo",
            "fixture hermes model",
        ),
    ];
    for (agent, name, dir, model) in want {
        let out = asf(&["--preview", &path_of(agent)])
            .replace("\u{1b}[2m", "")
            .replace("\u{1b}[0m", "");
        let expect = |value: &str| {
            if value == "filler" {
                filler(agent)
            } else {
                value.to_string()
            }
        };
        let client = if model.is_empty() {
            agent.to_string()
        } else {
            format!("{agent}  {}", expect(model))
        };
        for line in [
            format!("client {client}"),
            format!("name   {}", expect(name)),
            format!("dir    {dir}"),
        ] {
            assert!(out.contains(&line), "{agent}: no `{line}` in\n{out}");
        }
    }
}

/// The transcript path is what the picker and --paths hand you, but the resume command shows
/// a session id, so that has to find the session too: every store keeps the id in the path.
#[test]
fn a_session_id_finds_its_session() {
    for agent in ["claude", "codex", "pi", "opencode", "copilot", "hermes"] {
        let path = path_of(agent);
        let id = asf(&["--resume", &path]);
        // cd 'dir' && claude --resume <id>, or copilot --resume=<id>
        let id = id.split_whitespace().last().expect("no resume command");
        let id = id.rsplit('=').next().unwrap().trim_end_matches('\'');
        assert_eq!(
            asf(&["--paths", id]).trim(),
            path,
            "{agent}: id {id} is not a query"
        );
        let direct = asf(&["-r", &path]);
        for query in [id, &id[..id.len() - 4]] {
            let out = run(HOME, &["-r", query]);
            assert!(out.success, "{agent}: {query}: {}", out.stderr);
            assert_eq!(out.stdout, direct, "{agent}: {query}");
            assert_eq!(out.stderr, format!("asf: session {path}\n"));
        }
    }
}

/// A path that is not there fails at once, instead of searching every transcript
/// for its text.
#[test]
fn a_missing_session_file_fails_fast() {
    for missing in ["/nonexistent/s.jsonl", "gone.jsonl"] {
        let start = std::time::Instant::now();
        let out = Command::new(env!("CARGO_BIN_EXE_asf")).args(["-r", missing]).env("HOME", HOME).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{missing}: should fail");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("no such session file"), "{missing}: stderr was {err:?}");
        assert!(start.elapsed().as_secs() < 2, "{missing}: took {:?}", start.elapsed());
    }
}

#[test]
fn short_and_long_action_flags_agree() {
    let path = path_of("claude");
    assert_eq!(asf(&["-r", &path]), asf(&["--read", &path]));
    assert_eq!(asf(&["-p", &path]), asf(&["--preview", &path]));
    assert_eq!(asf(&["-u", &path]), asf(&["--resume", &path]));
}

fn pi_session_with_tool_call() -> (std::path::PathBuf, std::path::PathBuf) {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("asf-functional-{stamp}"));
    let path = home.join(".pi/agent/sessions/test.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"session","id":"01a00251-ccf4-7d29-9971-60e6334e483d"}"#,
            "\n",
            r#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"assistant speech"},{"type":"toolCall","name":"write","arguments":{"content":"tool argument"}}]}}"#,
            "\n",
        ),
    )
    .unwrap();
    (home, path)
}

#[test]
fn pi_ids_come_from_headers_not_artifacts() {
    let (home, path) = pi_session_with_tool_call();
    let h = home.to_str().unwrap();
    let id = "01a00251-ccf4-7d29-9971-60e6334e483d";
    let twin = "01a00251-ccf4-7d29-9971-60e6334e483e";
    let stale = "01a00252-ccf4-7d29-9971-60e6334e483d";
    let dir = path.parent().unwrap().join("project");
    let artifacts = dir.join("subagent-artifacts/outputs");
    std::fs::create_dir_all(&artifacts).unwrap();
    let renamed = dir.join(format!("2026-10-02_{stale}.jsonl"));
    std::fs::rename(&path, &renamed).unwrap();
    std::fs::copy(&renamed, artifacts.join(format!("{id}.jsonl"))).unwrap();
    std::fs::write(
        dir.join(format!("2026-10-02_{twin}.jsonl")),
        format!("{}\n", serde_json::json!({"type": "session", "id": twin})),
    ).unwrap();
    let out = run(h, &["-r", id]);
    assert!(out.success, "{}", out.stderr);
    assert_eq!(out.stderr, format!("asf: session {}\n", renamed.display()));
    assert!(out.stdout.contains("assistant speech"));
    assert!(run(h, &["-r", stale]).stderr.contains("no such session id"));
    assert!(run(h, &["-r", &id[..7]]).stderr.contains("ambiguous session id"));
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn opencode_child_ids_need_sub() {
    let (home, _) = pi_session_with_tool_call();
    let path = home.join(".local/share/opencode/storage/session/project/ses_CHILD.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, r#"{"id":"ses_CHILD","parentID":"ses_PARENT"}"#).unwrap();
    let h = home.to_str().unwrap();
    assert!(!run(h, &["-u", "ses_CHILD"]).success);
    let child = run(h, &["-u", "ses_CHILD", "--sub"]);
    assert!(child.success, "{}", child.stderr);
    assert!(child.stdout.contains("opencode --session ses_CHILD"));
    std::fs::remove_dir_all(home).unwrap();
}

fn only_role(text: &str, role: &str) {
    let headings: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("## "))
        .collect();
    assert!(!headings.is_empty(), "no headings in {text}");
    assert!(
        headings.iter().all(|line| *line == format!("## {role}")),
        "{text}"
    );
}

#[test]
fn role_filtering_keeps_roles_before_tail_across_formats() {
    for agent in [
        "claude", "codex", "pi", "opencode", "copilot", "gemini", "hermes",
    ] {
        let path = path_of(agent);
        let users = asf(&["-r", &path, "--role", "user", "--tail", "1"]);
        only_role(&users, "user");
    }
    for agent in ["claude", "codex", "pi", "opencode", "copilot", "hermes"] {
        let path = path_of(agent);
        let assistants = asf(&["-r", &path, "--role", "assistant", "--tail", "1"]);
        only_role(&assistants, "assistant");
    }
}

#[test]
fn tool_role_enables_tool_records() {
    let path = path_of("pi");
    let tools = asf(&["-r", &path, "--role", "tool", "--tail", "2"]);
    only_role(&tools, "tool");
    assert!(
        tools.contains("- ->"),
        "--role tool must retain tool details:\n{tools}"
    );
}

#[test]
fn tool_call_arguments_need_tools() {
    let (home, path) = pi_session_with_tool_call();
    let home = home.to_string_lossy().into_owned();
    let path = path.to_string_lossy().into_owned();
    let plain = run(&home, &["-r", &path]);
    assert!(plain.success, "{}", plain.stderr);
    assert!(plain.stdout.contains("assistant speech"));
    assert!(!plain.stdout.contains("tool argument"), "{}", plain.stdout);

    let tools = run(&home, &["-r", &path, "--tools"]);
    assert!(tools.success, "{}", tools.stderr);
    assert!(tools.stdout.contains("tool argument"), "{}", tools.stdout);
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn query_actions_identify_selection_on_stderr_only() {
    let one = run(HOME, &["fixture", "pi", "name", "-r"]);
    assert!(one.success, "{}", one.stderr);
    assert!(one.stdout.starts_with("## "), "{}", one.stdout);
    assert!(
        one.stderr.contains("asf: reading the newest match (older sessions not searched):"),
        "{}",
        one.stderr
    );
    assert!(one.stderr.contains("| when"), "{}", one.stderr);

    let resume = run(HOME, &["fixture", "pi", "name", "--resume"]);
    assert!(resume.success, "{}", resume.stderr);
    assert!(resume.stdout.contains("pi --session"), "{}", resume.stdout);
    assert!(
        resume.stderr.contains("asf: resuming the newest match (older sessions not searched):"),
        "{}",
        resume.stderr
    );

    let many = run(HOME, &["fixture", "-p"]);
    assert!(many.success, "{}", many.stderr);
    assert!(many.stdout.contains("client "), "{}", many.stdout);
    assert!(
        many.stderr
            .contains("asf: previewing the newest match (older sessions not searched):"),
        "{}",
        many.stderr
    );
    assert!(!many.stdout.contains("asf: previewing"), "{}", many.stdout);

    let none = run(HOME, &["no fixture matches this", "-u"]);
    assert!(!none.success);
    assert!(none.stdout.is_empty(), "{}", none.stdout);
    assert_eq!(
        none.stderr,
        "asf: nothing matched \"no fixture matches this\"\n"
    );
}

#[test]
fn absent_stores_are_quiet() {
    let empty_home = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/empty-home");
    let out = run(empty_home, &["-n", "1"]);
    assert!(out.success, "{}", out.stderr);
    assert_eq!(out.stdout, "nothing matched\n");
    assert!(out.stderr.is_empty(), "{}", out.stderr);
}
