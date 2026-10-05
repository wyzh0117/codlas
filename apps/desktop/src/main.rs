//! The window itself: open the store, hand the shell to Tauri, register the commands.
//!
//! There is deliberately no logic here. Interpreting events belongs to `state-engine`, judging
//! them to `guard-engine`, and packing them for an agent to `context-engine` — the shell shows
//! what those compile to (§61, 禁止 4, 禁止 7, ADR-020).

mod commands;

fn main() {
    let shell = match wp_desktop::Shell::open() {
        Ok(shell) => shell,
        Err(error) => {
            // A workbench that opens onto an empty screen when the database is unreachable is
            // the §62 failure mode; say why and stop.
            eprintln!("Codlas 工作台起不来：{error}");
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .manage(shell)
        .invoke_handler(tauri::generate_handler![
            commands::project_info,
            commands::diff_view,
            commands::apply_pending,
            commands::map_view,
            commands::overview_view,
            commands::decisions_view,
            commands::evolution_view,
            commands::timeline_view,
            commands::drift_view,
            commands::search_view,
            commands::context_view,
            commands::health_view
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the Codlas workbench shell");
}
