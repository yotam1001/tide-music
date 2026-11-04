#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    tide_provider::native_login::main();
}
