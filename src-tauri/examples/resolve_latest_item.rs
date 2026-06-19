use media_manager::storage::Repository;
use std::env;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let db_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("usage: resolve_latest_item <library.sqlite> <item_id>"))?;
    let item_id = env::args()
        .nth(2)
        .ok_or_else(|| anyhow::anyhow!("usage: resolve_latest_item <library.sqlite> <item_id>"))?
        .parse::<i64>()?;
    let repo = Repository::open(&db_path)?;
    repo.migrate()?;
    let work_id = repo.resolve_ingest_item(item_id, None)?;
    println!("resolved item {item_id} into work {work_id}");
    Ok(())
}
