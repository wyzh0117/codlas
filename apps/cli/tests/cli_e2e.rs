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

    /// Where a test may build a checkout or a folder to watch. The database lives here too.
    fn dir(&self) -> &Path {
        self._dir.path()
    }
}

/// Drives real `git` in a scratch directory. A watch pass is the one capture that has to be
/// proven against an actual repository, since a canned runner cannot show git's own output
/// shapes. The identity flags keep the result independent of whoever runs the suite.
fn git_in(dir: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    command
        .arg("-c")
        .arg("user.name=Watch Test")
        .arg("-c")
        .arg("user.email=watch@example.com")
        .args(args)
        .current_dir(dir);
    let out = command.output().expect("git runs");
    assert!(
        out.status.success(),
        "`git {}` failed in {}: {}",
        args.join(" "),
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 git stdout")
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
        prepared.contains("sends nothing") && prepared.contains("paste the prepared pack"),
        "prepare must be honest about who does the sending:\n{prepared}"
    );
    // §58: those words are the adapter contract's `fallback` verb, and a fallback has to
    // name the payload the user is being asked to carry.
    assert!(
        prepared.contains(&format!("payload written to {out}")),
        "the fallback must point at the file it wants carried:\n{prepared}"
    );

    let handed_over = work.json(&[
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
    assert_eq!(
        handed_over["fallback"]["payloadPath"].as_str(),
        Some(out.as_str()),
        "the machine read says which file to carry"
    );
    assert!(
        handed_over["fallback"]["instruction"]
            .as_str()
            .unwrap_or_default()
            .contains("sends nothing"),
        "same words, whichever way the pack is read:\n{handed_over}"
    );
    assert_eq!(handed_over["copyFallback"].as_bool(), Some(true));

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

/// §45's traceability leg, which Stage E's acceptance names ("Decision 可打开 Evidence")
/// but the CLI could not serve: the refs were only visible inside a `--json` payload, so
/// a user could not follow one. Replay already maintains `evidence_links`; this proves
/// the read path is reachable from the surface that ships.
#[test]
fn a_decision_opens_onto_the_evidence_it_cites() {
    let work = Workbench::ready();
    let state = work.json(&["state", "show"]);
    let decision = state["decisions"][0]["id"]
        .as_str()
        .expect("the fixture confirms a decision");

    let linked = work.json(&["evidence", "for", &format!("decision:{decision}")]);
    let rows = linked.as_array().expect("a list of evidence");
    assert!(
        !rows.is_empty(),
        "the confirmed decision cites nothing — replay lost the link"
    );
    assert!(
        rows.iter()
            .all(|row| row["type"] == serde_json::json!("message")),
        "transcript evidence is recorded as messages: {linked}"
    );

    let first = rows[0]["id"].as_str().expect("an evidence id");
    let shown = work.json(&["evidence", "show", first]);
    assert_eq!(shown[0]["id"].as_str(), Some(first));
    assert!(
        shown[0]["summary"]
            .as_str()
            .unwrap_or_default()
            .contains("drift"),
        "the row should carry the message the decision was made from: {shown}"
    );

    let bounded = work.json(&["evidence", "list", "--limit", "3"]);
    assert_eq!(
        bounded.as_array().unwrap().len(),
        3,
        "--limit bounds the listing rather than printing the whole log"
    );

    // A target the store cannot name is refused, never answered with a guess.
    let no_colon = work.run(&["evidence", "for", "decision-without-a-colon"]);
    assert!(
        !no_colon.status.success(),
        "`kind:id` is the only accepted shape"
    );
    let unknown = work.run(&["evidence", "show", "no-such-evidence"]);
    assert!(
        !unknown.status.success(),
        "an unknown id is a miss, not an empty row"
    );
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("no-such-evidence"),
        "the failure should name the id it looked for"
    );
}

/// §55 / §62 Recovery from the surface a user actually stands on: the whole workbench
/// is one file, and a restore has to put every project, event and index back — while
/// refusing to be the thing that destroys the newer work by accident.
#[test]
fn a_backup_returns_the_workbench_to_an_earlier_moment() {
    let work = Workbench::ready();

    let status = work.json(&["backup", "status"]);
    assert_eq!(status["integrity"].as_str(), Some("ok"));
    assert!(status["schemaVersion"].as_i64().unwrap() > 0);

    let created = work.json(&["backup", "create"]);
    let file = PathBuf::from(created["file"].as_str().expect("a default destination"));
    assert!(file.exists(), "the backup is a real file, not a claim");
    assert_eq!(created["report"]["integrity"].as_str(), Some("ok"));
    assert_eq!(
        created["report"]["projects"].as_i64(),
        Some(1),
        "the copy carries the project the fixture built"
    );

    let verified = work.json(&["backup", "verify", &file.to_string_lossy()]);
    assert_eq!(verified["report"], created["report"]);

    let kept = work.json(&["project", "list"])[0]["name"]
        .as_str()
        .expect("the fixture's project")
        .to_string();
    work.ok(&["project", "create", "--name", "written after the backup"]);
    assert_eq!(
        work.json(&["project", "list"]).as_array().unwrap().len(),
        2,
        "and then the workbench moves on"
    );
    let unacknowledged = work.run(&["backup", "restore", &file.to_string_lossy()]);
    assert!(
        !unacknowledged.status.success(),
        "restore discards work, so it never runs on a bare path"
    );
    assert!(
        String::from_utf8_lossy(&unacknowledged.stderr).contains("--yes"),
        "and says what would make it run"
    );

    let restored = work.json(&["backup", "restore", &file.to_string_lossy(), "--yes"]);
    assert_eq!(restored["report"]["projects"].as_i64(), Some(1));

    let projects = work.json(&["project", "list"]);
    assert_eq!(projects.as_array().unwrap().len(), 1);
    assert_eq!(projects[0]["name"].as_str(), Some(kept.as_str()));

    // A restore lands inside a live workbench: every read path still works afterwards.
    let state = work.json(&["state", "show"]);
    assert!(
        !state["decisions"].as_array().unwrap().is_empty(),
        "the transcript that produced the decision came back with it"
    );
    assert!(!work.ok(&["map", "show"]).trim().is_empty());

    // The discarded state is not lost: restore snapshotted it first.
    let backups = file.parent().expect("a backups directory");
    let pre_restore = std::fs::read_dir(backups)
        .expect("backups dir")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().starts_with("workbench-pre-restore-"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    assert_eq!(pre_restore.len(), 1, "{:?}", pre_restore);
    let safety = work.json(&["backup", "verify", &pre_restore[0].to_string_lossy()]);
    assert_eq!(
        safety["report"]["projects"].as_i64(),
        Some(2),
        "the snapshot holds the two projects the restore replaced"
    );

    let missing = work.run(&["backup", "verify", "no-such-file.db"]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("not found"));
}

/// §62 Degradation on the surface a user actually stands on: a source that cannot be
/// read is named and exits non-zero, and the rest of the workbench keeps working.
#[test]
fn a_broken_adapter_is_named_and_the_workbench_keeps_working() {
    let work = Workbench::ready();

    let verdicts = work.json(&["adapters"]);
    let transcript = verdicts
        .as_array()
        .unwrap()
        .iter()
        .find(|state| state["adapterId"] == serde_json::json!("transcript"))
        .expect("the fixture import left a verdict behind");
    assert_eq!(transcript["health"].as_str(), Some("ok"));

    // A missing file is a failed capture, not a silent success.
    let missing = work._dir.path().join("never-written.txt");
    let failed = work.run(&[
        "import",
        "manual",
        &missing.to_string_lossy(),
        "--source-id",
        "ghost",
    ]);
    assert!(
        !failed.status.success(),
        "one source, nothing read, exit non-zero"
    );
    assert!(
        String::from_utf8_lossy(&failed.stderr).contains("ghost"),
        "{}",
        String::from_utf8_lossy(&failed.stderr)
    );

    let verdicts = work.json(&["adapters"]);
    let rows = verdicts.as_array().unwrap();
    let ghost = rows
        .iter()
        .find(|state| state["adapterId"] == serde_json::json!("ghost"))
        .unwrap_or_else(|| panic!("the failure should be recorded, not only printed: {rows:?}"));
    assert_eq!(ghost["health"].as_str(), Some("failed"));
    assert_eq!(
        rows[0]["adapterId"].as_str(),
        Some("ghost"),
        "the listing opens on what is wrong"
    );

    // The dead source costs the workbench nothing else.
    let pack = work.json(&[
        "context",
        "build",
        "--question",
        "why does the ingest path duplicate messages?",
        "--tokens",
        "600",
    ]);
    assert!(
        !pack["items"].as_array().unwrap().is_empty(),
        "the context pack still travels"
    );
    assert!(!work.ok(&["map", "show"]).trim().is_empty());

    // One run, one live source and one absent one: unavailable is not failed (§51).
    let live = work._dir.path().join("live-chat.txt");
    std::fs::write(&live, "user: 确认保留 上下文预算\n").expect("write");
    let manifest = work._dir.path().join("sources.json");
    std::fs::write(
        &manifest,
        serde_json::json!([
            { "type": "manual", "id": "present", "file": live },
            { "type": "manual", "id": "absent", "file": missing },
        ])
        .to_string(),
    )
    .expect("manifest");
    work.ok(&["import", "all", "--file", &manifest.to_string_lossy()]);

    let verdicts = work.json(&["adapters"]);
    let rows = verdicts.as_array().unwrap();
    let health = |id: &str| {
        rows.iter()
            .find(|state| state["adapterId"] == serde_json::json!(id))
            .unwrap_or_else(|| panic!("no verdict recorded for {id}: {rows:?}"))["health"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    assert_eq!(health("present"), "ok", "a dead source did not cost the live one its run");
    assert_eq!(health("absent"), "unavailable");
    assert_eq!(health("ghost"), "failed");
}

/// §39's watch, against a repository that exists. The library tests prove the diff rules;
/// this proves the CLI reaches them, that a pass which moves nothing writes nothing, and
/// that the whole loop stays evidence-only: the task list cannot change because a branch
/// was cut (§19 禁止 9).
#[test]
fn watching_a_checkout_reports_the_branch_and_the_dirty_set() {
    let work = Workbench::ready();
    let repo = work.dir().join("checkout");
    std::fs::create_dir_all(&repo).expect("checkout dir");
    git_in(&repo, &["init", "-q"]);
    std::fs::write(repo.join("a.txt"), "one\n").expect("file");
    git_in(&repo, &["add", "a.txt"]);
    git_in(&repo, &["commit", "-q", "-m", "first"]);
    let start_branch = git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]).trim().to_string();

    let before = work.json(&["export", "state"]);
    let tasks_before = before["tasks"].clone();

    let first = work.json(&["watch", "--dir", &repo.to_string_lossy()]);
    assert_eq!(first["git"]["compared"], serde_json::json!(false), "no earlier pass");
    assert_eq!(first["git"]["eventsAppended"], serde_json::json!(0));
    assert_eq!(first["git"]["branch"], serde_json::json!(start_branch));

    let second = work.json(&["watch", "--dir", &repo.to_string_lossy()]);
    assert_eq!(second["git"]["compared"], serde_json::json!(true));
    assert_eq!(
        second["git"]["eventsAppended"],
        serde_json::json!(0),
        "a second pass over an untouched tree is not a second fact"
    );

    std::fs::write(repo.join("a.txt"), "two\n").expect("edit");
    let third = work.json(&["watch", "--dir", &repo.to_string_lossy()]);
    assert_eq!(third["git"]["statusChanged"], serde_json::json!(true));
    assert_eq!(third["git"]["branchChanged"], serde_json::json!(false));
    assert_eq!(third["git"]["dirty"].as_array().unwrap().len(), 1);
    assert!(
        third["files"]["changed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == &serde_json::json!("a.txt")),
        "the same pass re-scanned the tree: {}",
        third["files"]
    );

    git_in(&repo, &["checkout", "-q", "-b", "feat/reconnect"]);
    let fourth = work.json(&["watch", "--dir", &repo.to_string_lossy()]);
    assert_eq!(fourth["git"]["branchChanged"], serde_json::json!(true));
    assert_eq!(fourth["git"]["previousBranch"], serde_json::json!(start_branch));
    assert_eq!(fourth["git"]["branch"], serde_json::json!("feat/reconnect"));

    let timeline = work.json(&["export", "timeline"]);
    let types: Vec<&str> = timeline
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|event| event["type"].as_str())
        .collect();
    assert!(types.contains(&"GIT_BRANCH_CHANGED"), "{types:?}");
    assert!(types.contains(&"GIT_STATUS_CHANGED"), "{types:?}");
    assert!(types.contains(&"FILE_CHANGED"), "{types:?}");

    // Evidence, not conclusion: watching changed nothing about the work itself.
    assert_eq!(
        work.json(&["export", "state"])["tasks"],
        tasks_before,
        "a branch switch must not move a task"
    );
}

/// §14 binds a plain folder as readily as a checkout, so a watch pass over one has to say
/// "no repository here" and still do the half it can — silently reporting zero files would
/// look identical to a capture that found nothing.
#[test]
fn watching_a_plain_folder_still_captures_the_files_it_can() {
    let work = Workbench::ready();
    let folder = work.dir().join("plain");
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(folder.join("notes.md"), "# notes\n").expect("file");

    let text = work.ok(&["watch", "--dir", &folder.to_string_lossy()]);
    assert!(
        text.contains("not a git checkout"),
        "the report says which half could not run: {text}"
    );
    assert!(text.contains("1 changed"), "the file scan still ran: {text}");

    let report = work.json(&["watch", "--dir", &folder.to_string_lossy()]);
    assert!(report["git"].is_null(), "no repository, so no git section");
    assert_eq!(report["files"]["scanned"], serde_json::json!(1));
}

/// §52's L3 has one rule and it is not "dedupe better": 只提示，不直接删除. A library
/// function that returns pairs cannot delete anything anyway, so what this surface has to
/// prove is the reach and the restraint — a hint a user can act on, and message rows that
/// are all still there. Re-importing the same transcript is the check: L1/L2 can only report
/// the whole batch as already present if nothing was merged or dropped on the way here.
#[test]
fn a_near_duplicate_hint_names_the_pair_and_removes_nothing() {
    use vibe_ingestion::NOTE;

    let work = Workbench::ready();
    let repeat = work.dir().join("repeat.json");
    std::fs::write(
        &repeat,
        r#"[
  {"role":"user","content":"we should keep the near duplicate pass read only so that a hint can never delete a message"},
  {"role":"assistant","content":"agreed, and we should keep the near duplicate pass read only so that a hint can never merge or delete a message"},
  {"role":"user","content":"the search layer uses an fts5 index rather than a table scan"}
]"#,
    )
    .expect("a transcript to import");
    let path = repeat.to_string_lossy().into_owned();

    let import = work.json(&["import", "transcript", &path, "--source-id", "repeat"]);
    assert_eq!(import["totalNewMessages"], serde_json::json!(3));

    let report = work.json(&["duplicates"]);
    let hints = report["hints"].as_array().expect("a list of hints");
    assert_eq!(hints.len(), 1, "one reworded turn, not a pile: {report}");
    let hint = &hints[0];
    assert_eq!(hint["note"], serde_json::json!(NOTE));
    assert_eq!(report["note"], serde_json::json!(NOTE));
    let score = hint["similarity"].as_f64().expect("a score");
    assert!(
        (0.8..0.95).contains(&score),
        "the pair clears the default bar and not a stricter one: {score}"
    );
    // Oldest side first: a hint that lists the answer before the question cannot be read.
    assert!(
        hint["firstExcerpt"]
            .as_str()
            .unwrap_or_default()
            .starts_with("we should keep"),
        "{hint}"
    );
    assert!(
        hint["secondExcerpt"]
            .as_str()
            .unwrap_or_default()
            .starts_with("agreed"),
        "{hint}"
    );
    assert_eq!(
        report["messagesScanned"],
        serde_json::json!(16),
        "the project-wide window reads the fixture's 13 turns as well as these 3"
    );

    // The thread is named, so the scoped re-run is reachable from the hint itself.
    let thread = hint["conversationId"].as_str().expect("a conversation id");
    let scoped = work.json(&["duplicates", "--conversation", thread]);
    assert_eq!(scoped["scope"], serde_json::json!(thread));
    assert_eq!(
        scoped["hints"], report["hints"],
        "scoping to the pair's own conversation changes what is read, not what is found"
    );

    // Same data, same answer (§59), and a second pass over a hint surface stays a read.
    assert_eq!(
        work.json(&["duplicates"]),
        report,
        "a hint list that moves between runs is not a hint about the data"
    );

    // The bar is honoured at the surface, not only in the library.
    assert_eq!(
        work.json(&["duplicates", "--threshold", "0.95"])["hints"],
        serde_json::json!([])
    );

    let prose = work.ok(&["duplicates"]);
    assert!(
        prose.contains("nothing was merged, deleted or rewritten"),
        "the note travels with the hint: {prose}"
    );

    // Nothing was taken out of the database.
    let again = work.json(&["import", "transcript", &path, "--source-id", "repeat"]);
    assert_eq!(again["totalNewMessages"], serde_json::json!(0));
    assert_eq!(again["totalDuplicates"], serde_json::json!(3));

    // Bad input is refused rather than answered with a guess.
    let out_of_range = work.run(&["duplicates", "--threshold", "1.7"]);
    assert!(
        !out_of_range.status.success(),
        "0.0-1.0 is the whole meaning of a threshold"
    );
    let missing = work.run(&["duplicates", "--conversation", "no-such-thread"]);
    assert!(
        !missing.status.success(),
        "an unknown conversation is a miss, not an empty scan"
    );
}
