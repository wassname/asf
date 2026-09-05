//! asf against a fake HOME of scrubbed real sessions, one per agent. `cargo test` builds the
//! binary itself. Rebuild the fixtures with tests/harvest.sh when an agent changes format.

use std::process::Command;

const HOME: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/home");

fn asf(args: &[&str]) -> String {
    // the sessions record this as their working directory; resume says so when it is gone
    std::fs::create_dir_all("/tmp/asf-fixture-repo").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_asf")).args(args).env("HOME", HOME).output().unwrap();
    assert!(
        out.status.success(),
        "asf {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn path_of(agent: &str) -> String {
    asf(&["--paths", "-a", agent, "-n", "1"]).trim().to_string()
}

#[test]
fn every_agent_is_read() {
    let out = asf(&["-n", "20"]);
    // the agent column, padded, so that "pi" cannot be matched by "copilot"
    let agents = ["claude", "codex", "pi", "opencode", "copilot", "gemini", "hermes"];
    for agent in agents {
        assert!(out.contains(&format!("| {agent} ")), "{agent} is missing from\n{out}");
    }
    // six files-on-disk agents with one session each, and hermes with two
    assert!(out.contains("8 sessions matched"), "{out}");
}

/// claude rewrites its name records and keeps the old copies, so the last agent-name wins.
#[test]
fn the_name_is_the_one_the_agent_ended_with() {
    let out = asf(&["-n", "20"]);
    assert!(out.contains("fixture claude agentName"), "{out}");
    assert!(!out.contains("fixture claude aiTitle"), "a lesser name record won:\n{out}");
    // the file holds four earlier copies of each record, all labelled stale
    assert!(!out.contains("fixture claude stale"), "an older name record won:\n{out}");
    assert!(out.contains("fixture opencode title"), "{out}");
    // pi keeps its name in a session_info record, which beats the opening message
    assert!(out.contains("fixture pi name"), "{out}");
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
        assert!(out.contains(wanted), "{agent}: wanted {wanted:?}, got {out}");
    }
    // gemini resumes by list index, never by id
    assert!(asf(&["--resume", &path_of("gemini")]).contains("no resume command"));
}

#[test]
fn read_exports_markdown() {
    let plain = asf(&["--read", &path_of("claude")]);
    assert!(plain.starts_with("## "), "{plain}");
    assert!(plain.contains("## assistant"), "{plain}");
    assert!(!plain.contains("- `Bash`"), "tool calls are out unless asked for:\n{plain}");

    let tools = asf(&["--read", &path_of("claude"), "--tools"]);
    assert!(tools.contains("- `"), "{tools}");
    assert!(tools.contains("## tool"), "a tool result is not the user talking:\n{tools}");

    let think = asf(&["--read", &path_of("codex"), "--think"]);
    assert!(think.contains("\n> "), "{think}");
}

/// hermes keeps its sessions in a sqlite database, so none of the file reading applies
#[test]
fn hermes_is_read_out_of_its_database() {
    let path = path_of("hermes");
    assert!(path.contains("state.db#"), "a hermes session is a row, not a file: {path}");
    let out = asf(&["--read", &path, "--tools", "--think"]);
    assert!(out.contains("## user"), "{out}");
    assert!(out.contains("- `bash`"), "{out}");
    assert!(out.contains("> the hermes widget factory counts its boxes"), "{out}");
    // and the flags have to be asked for, or they are not flags
    let plain = asf(&["--read", &path]);
    assert!(!plain.contains("- `bash`"), "tool calls are out unless asked for:\n{plain}");
    assert!(!plain.contains("counts its boxes"), "reasoning is out unless asked for:\n{plain}");
    // the child session is a run it started for itself, and hermes records the parent
    assert!(!asf(&["--paths", "-a", "hermes", "-n", "9"]).contains("20260802_213338_6dbf13"));
    assert!(asf(&["--sub", "--paths", "-a", "hermes", "-n", "9"]).contains("20260802_213338_6dbf13"));
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
        ("claude", "fixture claude agentName", "/tmp/asf-fixture-repo", "filler"),
        ("codex", "filler", "/tmp/asf-fixture-repo", "filler"),
        ("pi", "fixture pi name", "/tmp/asf-fixture-repo", "filler"),
        // gemini keeps one logs.json of prompts per project: no directory in it, no model
        ("gemini", "filler", "fixture-project", ""),
        ("opencode", "fixture opencode title", "/tmp/asf-fixture-repo", "filler"),
        ("copilot", "filler", "/tmp/asf-fixture-repo", "filler"),
        ("hermes", "fixture hermes 2", "/tmp/asf-fixture-repo", "fixture hermes model"),
    ];
    for (agent, name, dir, model) in want {
        let out = asf(&["--preview", &path_of(agent)]).replace("\u{1b}[2m", "").replace("\u{1b}[0m", "");
        let expect = |value: &str| if value == "filler" { filler(agent) } else { value.to_string() };
        let client =
            if model.is_empty() { agent.to_string() } else { format!("{agent}  {}", expect(model)) };
        for line in [format!("client {client}"), format!("name   {}", expect(name)), format!("dir    {dir}")] {
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
        assert_eq!(asf(&["--paths", id]).trim(), path, "{agent}: id {id} is not a query");
        // and the same id where a path is asked for
        assert!(asf(&["--read", id]).contains("## "), "{agent}: --read {id} read nothing");
    }
}
