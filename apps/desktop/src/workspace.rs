//! Which database the shell opens, and which project it shows.
//!
//! Both answers come from `vibe_cli::app`, the CLI's own runtime, so the window and `wp`
//! cannot end up reading two different files or picking two different "current" projects.
//! A double-clicked app bundle has `/` as its working directory, which is why `--db` is
//! accepted here as well as `$WP_DB`.

use std::path::PathBuf;

use vibe_cli::app::{default_db, App};
use vibe_core::error::{CoreError, Result};
use vibe_domain::Project;

pub struct Shell {
    pub app: App,
}

impl Shell {
    pub fn open() -> Result<Self> {
        Self::from_args(std::env::args())
    }

    /// `--db <path>` or `--db=<path>`, then whatever [`default_db`] decides.
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let args: Vec<String> = args.into_iter().collect();
        let path = flag_value(&args, "--db").map(PathBuf::from).unwrap_or_else(default_db);
        Ok(Self { app: App::open(&path)? })
    }

    /// The project the shell is showing: explicit pointer, then the lone project —
    /// [`App::project`], not a fourth restatement of the same order.
    pub fn project(&self) -> Result<Project> {
        let id = self.app.project(None)?;
        self.app
            .repos
            .projects
            .get_project(&id)?
            .ok_or_else(|| CoreError::NotFound {
                what: format!("project {id}"),
            })
    }
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    let exact = format!("{flag}=");
    if let Some(pair) = args.iter().find(|arg| arg.starts_with(&exact)) {
        return pair.split_once('=').map(|(_, value)| value.to_string());
    }
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::flag_value;

    #[test]
    fn the_db_flag_accepts_both_spellings_and_absence() {
        let spaced = vec!["wp-desktop".to_string(), "--db".into(), "/tmp/a.db".into()];
        let joined = vec!["wp-desktop".to_string(), "--db=/tmp/b.db".into()];
        assert_eq!(flag_value(&spaced, "--db").as_deref(), Some("/tmp/a.db"));
        assert_eq!(flag_value(&joined, "--db").as_deref(), Some("/tmp/b.db"));
        assert_eq!(flag_value(&["wp-desktop".into()], "--db"), None);
    }
}
