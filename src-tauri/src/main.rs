// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `chronos mcp`: the Claude Code connection (docs/CLAUDE-CODE.md), no window.
    if std::env::args().nth(1).as_deref() == Some("mcp") {
        std::process::exit(chronos_lib::mcp::serve());
    }
    chronos_lib::run()
}
