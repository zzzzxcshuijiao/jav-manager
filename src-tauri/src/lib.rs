pub mod acceptance;
pub mod archive;
pub mod commands;
pub mod domain;
pub mod identifier;
pub mod ingest;
pub mod matcher;
pub mod provider;
pub mod scanner;
pub mod storage;
pub mod thumbnail;
pub mod nfo;

pub fn run() {
    commands::build_app()
        .run(tauri::generate_context!())
        .expect("failed to run media manager app");
}
