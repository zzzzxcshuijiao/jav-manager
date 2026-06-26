pub mod acceptance;
pub mod archive;
pub mod commands;
pub mod control_service;
pub mod control_service_host;
pub mod daemon_control;
pub mod daemon;
pub mod domain;
pub mod identifier;
pub mod ingest;
pub mod matcher;
pub mod migration;
pub mod nfo;
pub mod pipeline;
pub mod provider;
pub mod resource_pool;
pub mod scanner;
pub mod storage;
pub mod thumbnail;

pub mod library_rebuild;
pub mod poster_index;

pub fn run() {
    commands::build_app()
        .run(tauri::generate_context!())
        .expect("failed to run media manager app");
}
