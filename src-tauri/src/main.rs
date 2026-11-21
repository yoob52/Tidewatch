#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = tidewatch_lib::maybe_run_mock_agent() {
        std::process::exit(code);
    }
    tidewatch_lib::run()
}
