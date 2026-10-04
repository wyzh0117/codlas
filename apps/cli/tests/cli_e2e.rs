//! §58 from the other side: the same closed loop, driven through the real `wp`
//! binary instead of the libraries. The unit tests prove each engine is right; this
//! proves a user can actually reach those engines and that the §41-§44, §53-§55
//! promises hold in the composed thing — a refusal that only exists in a library is
//! not a promise the workbench keeps.
//!
//! Each test gets its own database. The fixture transcript contains a secret, so a
//! shared one would let a redaction test decide what a policy test sees.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .join("tests")
        .join("fixtures")
}

/// One workbench in one temp directory.
struct Workbench {
    _dir: tempfile::TempDir,
    db: PathBuf,
}

impl Workbench {
    /// Charter, transcript, saved map — the state every downstream command reads.
    fn ready() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = dir.path().join("workbench.db");
        let workbench = Workbench {
            _dir: dir,
            db,
        };
        workbench.ok(&["init", "--file", &Self::fixture("charter.json")]);
        workbench.ok(&["import", "transcript", &Self::fixture("transcript.json")]);
        workbench.ok(&["map", "save"]);
        workbench
    }

    fn fixture(name: &str) -> String {
        fixtures().join(name).to_string_lossy().into_owned()
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wp"));
        command.arg("--db").arg(&self.db);
        // A caller's WP_DB must not point these tests at a real workbench.
        command.env_remove("WP_DB");
        command.env_remove("VIBE_SECURITY_ALLOW_KEYCHAIN");
        command.current_dir(self._dir.path());
        command.args(args);
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("wp runs")
    }

    /// Success, stdout as text. A failed run panics with stderr attached, because a
    /// bare exit code says nothing about why §53 refused.
    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "`wp {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("utf-8 stdout")
    }

    /// Success, stdout parsed as JSON.
    fn json(&self, args: &[&str]) -> serde_json::Value {
        let mut with_flag = args.to_vec();
        with_flag.push("--json");
        let text = self.ok(&with_flag);
        serde_json::from_str(&text).unwrap_or_else(|error| {
            panic!("`wp {}` printed no JSON: {error}\n{text}", args.join(" "))
        })
    }

    fn secret_value(&self) -> String {
        self.ok(&[
            "secret",
            "put",
            "deploy_token",
            "--value",
            "sk-e2e-should-never-reach-sqlite",
        ])
    }

    /// Every byte in the database file, so a test can prove a value is absent.
    fn database_bytes(&self) -> Vec<u8> {
        std::fs::read(&self.db).expect("the database exists")
    }
}

#[test]
fn a_pack_carries_the_facts_a_question_needs_and_records_what_it_dropped() {
    let work = Workbench::ready();
    let pack = work.json(&[
        "context",
        "build",
        "--question",
        "why does the ingest path duplicate messages?",
        "--tokens",
        "600",
    ]);

    assert_eq!(
        pack["request"].as_str(),
        Some("why does the ingest path duplicate messages?"),
        "§41: the user's own words travel verbatim"
    );

    let sections: Vec<&str> = pack["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["section"].as_str().unwrap())
        .collect();
    for expected in [
        "original_intent",
        "relevant_scope",
        "current_phase",
        "active_decisions",
        "architecture_constraints",
        "request",
    ] {
        assert!(
            sections.contains(&expected),
            "§42 puts {expected} in every pack, saw {sections:?}"
        );
    }

    // The confirmed decision is a project fact, so it must be in the pack as itself —
    // not as a model's paraphrase of it.
    let text = pack["markdown"].as_str().unwrap();
    assert!(
        text.contains("drift 规则保持词法实现"),
        "the user's decision is missing from the pack:\n{text}"
    );
    assert!(
        text.contains("user-confirmed, locked"),
        "§7.4: the pack has to say who decided this\n{text}"
    );

    // §43 never clips an item mid-sentence; it drops whole items and says so.
    let big = work.json(&["context", "build", "--question", "why lexical", "--tokens", "4000"]);
    let small = work.json(&["context", "build", "--question", "why lexical", "--tokens", "150"]);
    let (big_n, small_n) = (
        big["items"].as_array().unwrap().len(),
        small["items"].as_array().unwrap().len(),
    );
    assert!(
        small_n < big_n,
        "a tighter budget must carry less, saw {small_n} vs {big_n}"
    );
    assert!(
        small["markdown"]
            .as_str()
            .unwrap()
            .contains("omitted to stay inside"),
        "§43: what was dropped stays visible in the pack"
    );

    // The plan is the same decision made before the pack is built, item by item.
    let plan = work.json(&["context", "plan", "--question", "why lexical", "--tokens", "600"]);
    let held: Vec<&serde_json::Value> = plan
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kept"] == serde_json::json!(false))
        .collect();
    assert!(
        !held.is_empty(),
        "a 600-token plan over this project must hold something back"
    );
    assert!(
        held.iter().all(|item| item["reason"]
            .as_str()
            .map(|reason| reason.contains("budget") || reason.contains("window"))
            .unwrap_or(false)),
        "every held-back item owes the plan a reason: {held:?}"
    );
}

#[test]
fn policy_refuses_a_cloud_target_instead_of_quietly_downgrading_it() {
    let work = Workbench::ready();
    work.ok(&["policy", "set", "--set", "local-only=true"]);

    let refused = work.run(&[
        "context",
        "prepare",
        "--question",
        "why lexical drift",
        "--to",
        "chatgpt-web",
        "--tokens",
        "500",
    ]);
    assert!(
        !refused.status.success(),
        "a §53 refusal must reach the shell, not print a pack anyway"
    );
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("local_only") && stderr.contains("§53"),
        "the refusal should name the rule, saw: {stderr}"
    );

    // The same request to a local target is allowed, and §53 still gets a vote on
    // its contents.
    let shown = work.ok(&["policy", "show"]);
    assert!(shown.contains("local-only"), "{shown}");
    let check = work.run(&["policy", "check", "--to", "chatgpt-web"]);
    assert!(!check.status.success(), "`policy check` predicts the refusal");
}

#[test]
fn the_policy_filter_redacts_the_secret_the_transcript_leaked() {
    let work = Workbench::ready();
    let pack = work.json(&["context", "build", "--question", "deploy token", "--tokens", "4000"]);
    let text = pack["markdown"].as_str().unwrap();

    // The fixture writes a real-looking token into a task summary and a message.
    assert!(
        !text.contains("ghp_") && !text.contains("github_token: "),
        "§53 redact-secrets leaked a value:\n{text}"
    );
    assert!(
        text.contains("[REDACTED:github_token]"),
        "the redaction has to stay visible, or the pack reads like the secret was never there"
    );
    assert!(
        pack["redactedCount"].as_u64().unwrap() > 0,
        "§54 counts the redactions"
    );
    assert_eq!(
        pack["policyApplied"]["redactSecrets"],
        serde_json::json!(true),
        "the pack says which policy compiled it"
    );
}

#[test]
fn prepare_hands_the_payload_to_a_human_and_an_audit_row_to_the_record() {
    let work = Workbench::ready();
    let out = work
        ._dir
        .path()
        .join("to-agent.md")
        .to_string_lossy()
        .into_owned();

    let before = work.json(&["audit", "--limit", "100"]).as_array().unwrap().len();
    let prepared = work
        .ok(&[
            "context",
            "prepare",
            "--question",
            "why lexical drift",
            "--to",
            "local-coding-agent",
            "--tokens",
            "500",
            "--out",
            &out,
        ]);
    let after = work.json(&["audit", "--limit", "100"]).as_array().unwrap().len();

    let written = std::fs::read_to_string(&out).expect("the pack reached the file");
    assert!(written.starts_with("# Project Context"), "{written}");
    // §7.8: the workbench compiles, the user sends. Nothing here claims to have
    // contacted an agent.
    assert!(
        prepared.contains("sends nothing") && prepared.contains("copy it in"),
        "prepare must be honest about who does the sending:\n{prepared}"
    );

    assert!(after > before, "preparing a pack must leave a trace (§54)");
    let newest = &work.json(&["audit", "--limit", "1"])[0];
    assert_eq!(newest["component"].as_str(), Some("context-engine"));
    assert_eq!(newest["action"].as_str(), Some("prepare_for_agent"));
    assert_eq!(
        newest["producedRef"].as_str(),
        Some("prepared:local-coding-agent"),
        "§54: which agent this was prepared for"
    );
    assert!(
        newest["readRefs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reference| reference.as_str().unwrap().starts_with("charter:")),
        "§54: the audit says what was read"
    );

    // A refusal is audited too — the point is the trace, not the success.
    work.ok(&["policy", "set", "--set", "local-only=true"]);
    work.run(&[
        "context",
        "prepare",
        "--question",
        "why lexical drift",
        "--to",
        "chatgpt-web",
    ]);
    let actions: Vec<String> = work
        .json(&["audit", "--limit", "20"])
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["action"].as_str().unwrap().to_string())
        .collect();
    assert!(
        actions.iter().any(|action| action.contains("refuse")),
        "the §53 refusal vanished from the audit log: {actions:?}"
    );
}

#[test]
fn a_secret_keeps_its_value_out_of_the_database() {
    let work = Workbench::ready();

    let put = work.secret_value();
    let reference = put
        .split('→')
        .nth(1)
        .map(str::trim)
        .unwrap_or_else(|| panic!("`secret put` should print the ref: {put}"))
        .to_string();
    assert!(
        reference.starts_with("local:"),
        "§53: the ref is what the workbench keeps, saw {reference}"
    );

    let listed = work.ok(&["secret", "list"]);
    assert!(listed.contains("deploy_token") && listed.contains(&reference));
    assert!(
        !listed.contains("sk-e2e-should-never-reach-sqlite"),
        "list must not print values"
    );

    let read_back = work.ok(&["secret", "get", "deploy_token"]);
    assert!(
        read_back.contains("sk-e2e-should-never-reach-sqlite"),
        "`get` is the one place a value comes back"
    );

    let raw = String::from_utf8_lossy(&work.database_bytes()).into_owned();
    assert!(
        !raw.contains("sk-e2e-should-never-reach-sqlite"),
        "§53: no secret in SQLite plaintext"
    );
    assert!(
        raw.contains(&reference),
        "…but the reference belongs there, and without it the value is unreachable"
    );

    // The keychain is an opt-in: no consent, no attempt to touch the system store.
    let refused = work.run(&["secret", "put", "kc", "--value", "x", "--keychain"]);
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("opt-in"),
        "the refusal should explain how to opt in"
    );

    work.ok(&["secret", "rm", "deploy_token"]);
    assert!(!work.ok(&["secret", "list"]).contains("deploy_token"));
    let gone = work.run(&["secret", "get", "deploy_token"]);
    assert!(
        !gone.status.success(),
        "a removed secret must stop resolving, or the index entry only looked gone"
    );
    assert!(
        !std::path::Path::new(&format!(
            "{}/secrets/{}.value",
            work._dir.path().to_string_lossy(),
            reference.trim_start_matches("local:")
        ))
        .exists(),
        "§53: the value itself has to leave the machine too, not just the pointer"
    );
}

#[test]
fn the_six_exports_and_one_pack_carry_the_same_facts() {
    let work = Workbench::ready();
    let dir = work._dir.path().to_string_lossy().into_owned();

    let mut files = Vec::new();
    for (command, expected) in [
        ("state", "\"tasks\""),
        ("map", "## Original Intent"),
        ("charter", "\"charter\""),
        ("decisions", "drift"),
        ("timeline", "\"type\""),
    ] {
        let path = work
            .ok(&["export", command, &format!("{dir}/export-{command}.out")]);
        let text = std::fs::read_to_string(format!("{dir}/export-{command}.out")).unwrap();
        assert!(
            text.contains(expected),
            "§55 export {command} lost {expected}:\n{text}"
        );
        assert!(!path.trim().is_empty(), "{command} should say where it wrote");
        files.push((command, text));
    }

    let pack = work.ok(&["export", "pack", &format!("{dir}/project.projectpack")]);
    assert!(pack.contains("members"), "{pack}");

    let archive = zip::ZipArchive::new(std::fs::File::open(format!("{dir}/project.projectpack")).unwrap())
        .expect("a readable zip — §55 says zip/json/md is enough for V1");
    let names: Vec<String> = archive.file_names().map(|name| name.to_string()).collect();
    for expected in [
        "manifest.json",
        "project-state.json",
        "project-map.md",
        "charter.json",
        "decisions.json",
        "aep.json",
        "timeline.json",
    ] {
        assert!(names.contains(&expected.to_string()), "§55: {expected} missing — the pack holds {names:?}");
    }

    let member = |name: &str| -> String {
        let mut reader = zip::ZipArchive::new(
            std::fs::File::open(format!("{dir}/project.projectpack")).unwrap(),
        )
        .unwrap();
        let mut buffer = String::new();
        std::io::Read::read_to_string(&mut reader.by_name(name).unwrap(), &mut buffer).unwrap();
        buffer
    };

    let manifest: serde_json::Value =
        serde_json::from_str(&member("manifest.json")).expect("manifest is JSON");
    assert_eq!(manifest["format"].as_str(), Some("projectpack"));
    assert!(
        manifest["note"]
            .as_str()
            .unwrap()
            .contains("not a second source of truth"),
        "禁止 4: the pack is a projection of the log, never a state of its own"
    );

    // Exported twice from the same state, the members must agree byte for byte —
    // including with the standalone exports.
    let exported = |name: &str| -> String {
        files
            .iter()
            .find(|(command, _)| *command == name)
            .map(|(_, text)| text.clone())
            .expect("the export ran")
    };
    assert_eq!(
        member("project-map.md"),
        exported("map"),
        "the packed map is the exported map, not a second compile"
    );
    let state: serde_json::Value = serde_json::from_str(&member("project-state.json")).unwrap();
    let standalone: serde_json::Value = serde_json::from_str(&exported("state")).unwrap();
    assert_eq!(state["stateHash"], standalone["stateHash"]);
    assert_eq!(
        manifest["stateHash"].as_str(),
        state["stateHash"].as_str(),
        "the manifest names the state the pack was cut from"
    );
}

/// §44 gives advice, never a decision: the ranked list names a capability and its
/// §53 verdict, and the manual route is always there because the send stays human.
#[test]
fn routing_is_advice_that_reports_the_policy_instead_of_obeying_it() {
    let work = Workbench::ready();
    let routed = work.ok(&[
        "context",
        "route",
        "--question",
        "修复 storage adapter 里失败的测试",
    ]);
    assert!(
        routed.contains("local-coding-agent"),
        "code wording points at the repo agent:\n{routed}"
    );
    assert!(routed.contains("manual-copy"), "§7.8: the copy route is never off the list");

    work.ok(&["policy", "set", "--set", "local-only=true"]);
    let gated = work.json(&["context", "route", "--question", "review 这个架构权衡"]);
    let cloud = gated
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["target"] == serde_json::json!("Web"))
        .expect("the cloud route is still listed");
    assert_eq!(
        cloud["allowedByPolicy"],
        serde_json::json!(false),
        "§53 says no to the route without deleting it from the advice"
    );
}

/// §40 through the workbench, not through the port. An L1 hit whose record has no body
/// used to answer with an empty column, and a `--kind` filter used to be obeyed by the
/// full-text layer only — both are only visible when the command is actually run.
#[test]
fn every_search_hit_names_its_layer_and_says_something() {
    let work = Workbench::ready();
    let hits = work.json(&["search", "drift", "--limit", "10"]);
    assert!(
        hits.as_array().map(Vec::len).unwrap_or(0) > 0,
        "the fixture project talks about drift constantly"
    );
    for hit in hits.as_array().unwrap() {
        assert!(
            !hit["snippet"].as_str().unwrap().trim().is_empty(),
            "a hit with nothing to show is not an answer: {hit}"
        );
        assert!(
            ["structured", "full_text", "relation"].contains(&hit["layer"].as_str().unwrap()),
            "§40: the result has to say which layer produced it: {hit}"
        );
    }

    let scoped = work.json(&["search", "drift", "--limit", "10", "--kind", "decision"]);
    assert!(
        scoped
            .as_array()
            .unwrap()
            .iter()
            .all(|hit| hit["kind"] == serde_json::json!("decision")),
        "a --kind filter the other layers ignore is not a filter: {scoped}"
    );
}

/// A decision captured from a one-line summary has a title and no separate body. The
/// pack used to render that as a dangling " — ", which reads as a corrupted record.
#[test]
fn a_title_only_decision_renders_as_a_decision_not_a_broken_line() {
    let work = Workbench::ready();
    let pack = work.json(&["context", "build", "--question", "drift", "--tokens", "1200"]);
    let decisions: Vec<&str> = pack["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["section"] == serde_json::json!("active_decisions"))
        .map(|item| item["content"].as_str().unwrap())
        .collect();
    assert!(!decisions.is_empty(), "the fixture confirms a decision");
    for line in decisions {
        assert!(
            !line.contains("—  [") && !line.ends_with("— "),
            "an empty clause joined with ' — ':\n{line}"
        );
    }
}

/// §14's items 2 and 3: the project is bound to a directory, and that directory is a
/// source like any other. Until `wp import files` existed the file scan was reachable
/// only from the golden test's library calls, so a user could not capture the code the
/// drift rules are supposed to judge.
#[test]
fn a_bound_directory_is_a_source_and_capturing_it_twice_writes_nothing_twice() {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).expect("repo dir");
    std::fs::write(repo.join("src/auth.rs"), "pub fn login() {}\n").expect("file");

    let spec = serde_json::json!({
        "project": {
            "name": "Captured",
            "description": null,
            "rootPath": repo.to_string_lossy(),
            "currentPhase": "V1-D Capture",
        },
        "charter": serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(fixtures().join("charter.json")).unwrap()
        )
        .unwrap()["charter"],
        "scope": serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(fixtures().join("charter.json")).unwrap()
        )
        .unwrap()["scope"],
        "baseline": serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(fixtures().join("charter.json")).unwrap()
        )
        .unwrap()["baseline"],
    });
    let spec_path = dir.path().join("spec.json");
    std::fs::write(&spec_path, serde_json::to_vec(&spec).unwrap()).expect("spec");

    let db = dir.path().join("workbench.db");
    let run = |args: &[&str]| -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wp"));
        command
            .arg("--db")
            .arg(&db)
            .env_remove("WP_DB")
            .args(args);
        command.output().expect("wp runs")
    };
    let ok = |args: &[&str]| -> String {
        let out = run(args);
        assert!(
            out.status.success(),
            "`wp {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    };

    ok(&["init", "--file", &spec_path.to_string_lossy()]);
    let first = ok(&["import", "files"]);
    assert!(first.contains("1 changed"), "the tree has one file: {first}");

    let again = ok(&["import", "files"]);
    assert!(
        again.contains("1 unchanged") && again.contains("0 changed"),
        "an unchanged file is not a second fact: {again}"
    );

    let state = serde_json::from_str::<serde_json::Value>(&ok(&["export", "state"])).unwrap();
    let artifacts = state["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 1, "the scan recorded one artifact");
    assert_eq!(artifacts[0]["path"], serde_json::json!("src/auth.rs"));

    // §39: capture is evidence. A commit or a file never closes a task by itself, so
    // the log grew events but no project fact changed status.
    let timeline = run(&["--json", "export", "timeline"]);
    let events = serde_json::from_str::<serde_json::Value>(&String::from_utf8_lossy(&timeline.stdout))
            .expect("timeline json");
    assert!(
        events
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["type"] == serde_json::json!("FILE_CHANGED")),
        "the scan left a FILE_CHANGED event in the log"
    );
}

/// A capture with nothing bound has to say so instead of scanning the current
/// directory, which would quietly record whichever project the shell happened to be in.
#[test]
fn a_capture_without_a_bound_directory_refuses_to_guess() {
    let work = Workbench::ready();
    let out = work.run(&["import", "files", "--dir", "/nonexistent-place"]);
    assert!(!out.status.success(), "a missing directory is an error");
    let unbound = {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("w.db");
        let spec = dir.path().join("spec.json");
        let mut value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(fixtures().join("charter.json")).unwrap())
                .unwrap();
        value["project"]["rootPath"] = serde_json::Value::Null;
        std::fs::write(&spec, serde_json::to_vec(&value).unwrap()).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_wp"));
        command
            .arg("--db")
            .arg(&db)
            .arg("init")
            .arg("--file")
            .arg(&spec);
        command.output().expect("init");
        let mut command = Command::new(env!("CARGO_BIN_EXE_wp"));
        command
            .arg("--db")
            .arg(&db)
            .env_remove("WP_DB")
            .arg("import")
            .arg("files");
        command.output().expect("wp runs")
    };
    assert!(!unbound.status.success());
    assert!(
        String::from_utf8_lossy(&unbound.stderr).contains("root_path"),
        "the error should name what is missing"
    );
}
