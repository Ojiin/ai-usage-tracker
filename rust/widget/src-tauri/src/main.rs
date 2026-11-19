// No console window on Windows.
#![windows_subsystem = "windows"]
// The exe keeps the Python build's name, AIUsageTracker.exe.
#![allow(non_snake_case)]

fn main() {
    ai_usage_tracker_lib::run()
}
