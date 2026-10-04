//! `wp-mcp` — §38.5's MCP server as a process an agent can start.
//!
//! stdout is the protocol and nothing else: a stray log line interleaved into the JSON-RPC
//! stream breaks a client's parser, so every word of diagnosis goes to stderr. The database is
//! the same file `wp` uses, opened read-write through the same ports — this binary adds a door,
//! not a second copy of the workbench.

use std::io;
use std::path::PathBuf;

use vibe_mcp_server::McpServer;
use vibe_storage::SqliteStore;

/// `--db`, then `$WP_DB`, then the `.vibe/workbench.db` beside the working directory — the same
/// precedence `wp` uses, so an agent and a human end up reading one database by default.
fn default_db() -> PathBuf {
    if let Ok(path) = std::env::var("WP_DB") {
        if !path.trim().is_empty() {
            return PathBuf::from(path);
        }
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".vibe")
        .join("workbench.db")
}

const HELP: &str = "\
wp-mcp — Codlas workbench MCP server (stdio)

  --db <path>          workbench database (default: $WP_DB, then ./.vibe/workbench.db)
  --project <id>       pin the server to one project
  --print-config       print the client config snippet and exit
  -h, --help           this text

Each stdin line is one JSON-RPC message; each stdout line is one answer. Tools:
project_state, project_map, project_timeline, search, open_evidence, context_pack.";

fn main() {
    if let Err(message) = run() {
        eprintln!("wp-mcp: {message}");
        std::process::exit(1);
    }
}

fn run() -> std::result::Result<(), String> {
    let mut db: Option<PathBuf> = None;
    let mut project: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = Some(PathBuf::from(next(&mut args, "--db")?)),
            "--project" => project = Some(next(&mut args, "--project")?),
            "--print-config" => {
                let path = db.clone().unwrap_or_else(default_db);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "mcpServers": {
                            "codlas": {
                                "command": "wp-mcp",
                                "args": ["--db", path.to_string_lossy()],
                            }
                        }
                    }))
                    .map_err(|e| e.to_string())?
                );
                return Ok(());
            }
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            other => return Err(format!("unknown argument '{other}' — try --help")),
        }
    }

    let path = db.unwrap_or_else(default_db);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
    }
    let store = SqliteStore::open(&path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let server = McpServer::new(store.repositories(), project);
    eprintln!("wp-mcp: serving {} over stdio", path.display());

    let stdin = io::stdin();
    let stdout = io::stdout();
    let answered = server
        .serve(stdin.lock(), stdout.lock())
        .map_err(|e| format!("the stream failed: {e}"))?;
    eprintln!("wp-mcp: stdin closed after {answered} answers");
    Ok(())
}

fn next(args: &mut impl Iterator<Item = String>, flag: &str) -> std::result::Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{flag} needs a value — try --help"))
}
