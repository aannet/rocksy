/// Commands of the app, the same list as `generate_handler!` in `src/main.rs`.
/// Declaring them gives each an `allow-<command>` permission: a command is
/// then callable only from a window whose capability lists it
/// (`capabilities/default.json`), instead of from every window by default.
const COMMANDS: &[&str] = &[
    "startup_info",
    "read_file",
    "save_file",
    "list_vaults",
    "add_vault",
    "remove_vault",
    "open_vault",
    "close_vault",
    "build_tags",
    "tags_list",
    "tag_fragments",
    "file_tags",
    "search",
    "backlinks",
    "outgoing_links",
    "unresolved_links",
    "set_session",
    "pick_folder",
    "pick_file",
    "log_timing",
    "open_external",
    "close_ack",
    "quit",
];

fn main() {
    let attributes =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
