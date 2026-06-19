use media_manager::ingest::IngestEngine;
use media_manager::provider::ExampleProvider;
use media_manager::scanner::Scanner;
use media_manager::storage::Repository;
use std::env;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let db_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("usage: scan_to_sqlite <library.sqlite> <source_root>"))?;
    let source_root = env::args()
        .nth(2)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("usage: scan_to_sqlite <library.sqlite> <source_root>"))?;
    let repo = Repository::open(&db_path)?;
    repo.migrate()?;
    let engine = IngestEngine::new(ExampleProvider);
    let items = Scanner::scan_sources(&[source_root.clone()])?
        .into_iter()
        .map(|item| engine.decide(item))
        .collect::<Vec<_>>();
    let job_id = repo.create_ingest_job(&[source_root], &items)?;
    println!("created ingest job {job_id} with {} items", items.len());
    Ok(())
}
