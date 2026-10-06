//! §34/§35's local end: `wp host serve` and `wp host manifest`.
//!
//! Two commands because the channel has two very different moments. Installing it is a
//! one-time act with a filesystem side effect (a record in Chrome's
//! `NativeMessagingHosts`, and the wrapper that record launches); using it is a long-lived
//! loop that must never print anything but frames. Keeping them apart is what lets
//! `serve` be honest about stdout — a stray `println!` there is a corrupt frame, which is
//! why this file's only human-facing output lives in the manifest path.

use std::io::BufReader;
use std::path::{Path, PathBuf};

use clap::Subcommand;
use vibe_core::error::{CoreError, Result};
use vibe_ingestion::IngestPipeline;
use vibe_ingestion::native_messaging::{HOST_NAME, HostManifest, HostPolicy, NativeHost};

use crate::app::App;

#[derive(Subcommand)]
pub enum HostSubcommand {
    /// Speak `codlas-nm/1` on stdin/stdout until the browser closes the channel. Chrome
    /// starts this through the wrapper below; running it by hand is a way to test the
    /// protocol with a pipe.
    Serve {
        /// Trust one more page origin (§38.2 "Generic Web"). Repeatable.
        #[arg(long)]
        allow_origin: Vec<String>,
    },
    /// Write the Chrome host record and the wrapper it launches, then say where Chrome
    /// expects the file to live.
    Manifest {
        /// The extension's id from `chrome://extensions`. Repeatable.
        #[arg(long)]
        extension_id: Vec<String>,
        /// Where to write the two files. Defaults to `<db dir>/host`.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Show both files instead of writing them.
        #[arg(long)]
        print: bool,
    },
}

pub fn dispatch(app: &App, project: Option<&str>, command: HostSubcommand) -> Result<()> {
    match command {
        HostSubcommand::Serve { allow_origin } => serve(app, project, allow_origin),
        HostSubcommand::Manifest {
            extension_id,
            out,
            print,
        } => manifest(app, project, extension_id, out, print),
    }
}

/// The whole of §35's five checks runs inside [`NativeHost::serve`]; this function only
/// decides *which* workbench and *which* project the channel is scoped to, and then hands
/// over. Materialisation is not needed here — capture writes rows, it reads none.
fn serve(app: &App, project: Option<&str>, allow_origin: Vec<String>) -> Result<()> {
    let scope = app.project(project)?;
    let mut policy = HostPolicy::default();
    for origin in &allow_origin {
        policy = policy.allow_origin(origin);
    }
    let pipeline = IngestPipeline::with_rules(app.repos.clone());
    let host = NativeHost::new(&pipeline, policy).for_project(Some(scope));

    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    let handled = host.serve(&mut reader, &mut writer)?;
    // stderr, always: stdout owns the wire.
    eprintln!("wp host serve: channel closed after {handled} frame(s)");
    Ok(())
}

fn manifest(
    app: &App,
    project: Option<&str>,
    extension_ids: Vec<String>,
    out: Option<PathBuf>,
    print: bool,
) -> Result<()> {
    if extension_ids.iter().all(|id| id.trim().is_empty()) {
        return Err(CoreError::Validation(
            "no --extension-id: Chrome only lets an extension open a native host if its id \
             is listed. Load apps/extension unpacked in chrome://extensions, copy the ID, \
             and pass it here.".into(),
        ));
    }
    let scope = app.project(project)?;
    let db = absolute(&app.db)?;
    let binary = std::env::current_exe()
        .map_err(|error| CoreError::Io(format!("cannot locate this wp binary: {error}")))?;
    let binary = absolute(&binary)?;
    let dir = out.unwrap_or_else(|| db.parent().unwrap_or(Path::new(".")).join("host"));

    let wrapper = wrapper_script(&binary, &db, &scope);
    let record = HostManifest::new(dir.join(format!("{HOST_NAME}.sh")).display().to_string(), &extension_ids).render();

    if print {
        println!("{record}\n\n-- 8< -- {HOST_NAME}.sh -- 8< --\n{wrapper}");
        return Ok(());
    }

    std::fs::create_dir_all(&dir)
        .map_err(|error| CoreError::Io(format!("cannot create {}: {error}", dir.display())))?;
    let script_path = dir.join(format!("{HOST_NAME}.sh"));
    std::fs::write(&script_path, wrapper)
        .map_err(|error| CoreError::Io(format!("cannot write {}: {error}", script_path.display())))?;
    set_executable(&script_path)?;
    let record_path = dir.join(format!("{HOST_NAME}.json"));
    std::fs::write(&record_path, format!("{record}\n")).map_err(|error| {
        CoreError::Io(format!(
            "cannot write {}: {error}",
            record_path.display()
        ))
    })?;

    println!("native host record: {}", record_path.display());
    println!("launcher wrapper:   {}", script_path.display());
    println!("scoped to project:  {scope}");
    println!();
    println!("Finish the install by putting the record where Chrome looks for it:");
    let target_dir = chrome_dir();
    let target = target_dir.join(format!("{HOST_NAME}.json"));
    println!("  mkdir -p \"{}\"", target_dir.display());
    println!("  cp \"{}\" \"{}\"", record_path.display(), target.display());
    println!("The wrapper's absolute path is what Chrome launches, so move the record without moving the script.");
    if let Some(dir) = guarded_folder(&binary) {
        println!();
        println!("warning: this wp binary sits under ~/{dir}, a folder macOS protects. A browser");
        println!("launching it has no {dir} grant of its own, and the host then blocks inside dyld");
        println!("before main(), which reaches the popup as a silent timeout, not an error. Copy wp");
        println!("outside ~/{dir} (e.g. ~/.local/bin), rerun `wp host manifest` from the copy, or");
        println!("grant the browser access under System Settings > Privacy & Security > Files and Folders.");
    }
    Ok(())
}

/// The folder macOS guards behind an access grant, when `path` sits inside one of the three
/// that need no full-disk grant. A browser launching a native host from inside them has no
/// grant of its own, and the host blocks inside dyld before `main` — a silent hang that
/// `manifest` should name rather than let a person debug for an evening.
#[cfg(target_os = "macos")]
fn guarded_folder(path: &Path) -> Option<&'static str> {
    let home = std::env::var("HOME").ok().filter(|home| !home.is_empty())?;
    guarded_folder_in(path, Path::new(&home))
}

#[cfg(not(target_os = "macos"))]
fn guarded_folder(_path: &Path) -> Option<&'static str> {
    None
}

fn guarded_folder_in(path: &Path, home: &Path) -> Option<&'static str> {
    ["Documents", "Desktop", "Downloads"]
        .iter()
        .find(|dir| path.starts_with(home.join(*dir)))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::guarded_folder_in;
    use std::path::Path;

    #[test]
    fn guarded_folders_are_recognised_by_path_component_not_text_prefix() {
        let home = Path::new("/Users/someone");
        assert_eq!(
            guarded_folder_in(Path::new("/Users/someone/Documents/x/wp"), home),
            Some("Documents")
        );
        assert_eq!(
            guarded_folder_in(Path::new("/Users/someone/Desktop/wp"), home),
            Some("Desktop")
        );
        assert_eq!(
            guarded_folder_in(Path::new("/Users/someone/Downloads/wp"), home),
            Some("Downloads")
        );
        assert_eq!(
            guarded_folder_in(Path::new("/Users/someone/.local/bin/wp"), home),
            None
        );
        assert_eq!(
            guarded_folder_in(Path::new("/Users/someone/Documents-old/wp"), home),
            None
        );
    }
}

/// Where a macOS Chrome looks for host records. This is the only platform-shaped constant
/// in the CLI, and it is printed rather than written to — §7.6 keeps the core from needing
/// to know where a product stores anything, and a path in a `println!` is not a dependency.
fn chrome_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "~".to_string());
    PathBuf::from(home)
        .join("Library/Application Support/Google/Chrome/NativeMessagingHosts")
}

/// Chrome passes no arguments to a native host, so everything the loop needs to know —
/// which database, which project — has to be baked into what it launches. Each value is
/// single-quoted with the quotes escaped, because a path with a space is normal on macOS
/// and a path with a quote is not worth a subtle shell bug. Both flags are global, so they
/// precede the verb.
fn wrapper_script(binary: &Path, db: &Path, project: &str) -> String {
    format!(
        "#!/bin/sh\n# Generated by `wp host manifest`. Chrome launches native hosts with no\n# arguments, so the workbench's database and project scope live here.\nset -e\n\nexec {} --db {} --project {} host serve\n",
        shell_quote(&binary.display().to_string()),
        shell_quote(&db.display().to_string()),
        shell_quote(project),
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\'"))
}

/// A path Chrome can launch without a working directory to interpret it.
fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::fs::canonicalize(path)
        .map_err(|error| CoreError::Io(format!("cannot resolve {}: {error}", path.display())))
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| CoreError::Io(format!("cannot chmod {}: {error}", path.display())))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}
