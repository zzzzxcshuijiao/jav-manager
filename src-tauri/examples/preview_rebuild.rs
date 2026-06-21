//! Dry-run an NFO rebuild against one or more source roots.
//!
//! Usage:
//!     preview_rebuild <library.sqlite> <source_root> [<source_root> ...]
//!
//! Opens (or creates) the database, runs the schema migration, then scans the
//! given roots for NFO/video pairs and groups them exactly as a real rebuild
//! would, but WITHOUT clearing any tables or writing any rows. Prints the
//! resulting RebuildReport as pretty JSON so the counts can be inspected before
//! committing to a destructive full rebuild from the UI.

use media_manager::storage::Repository;
use std::env;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = env::args().skip(1);
    let db_path = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("usage: preview_rebuild <library.sqlite> <source_root> [<source_root> ...]"))?;
    let roots: Vec<PathBuf> = args.map(PathBuf::from).collect();
    if roots.is_empty() {
        anyhow::bail!("usage: preview_rebuild <library.sqlite> <source_root> [<source_root> ...]");
    }

    let repo = Repository::open(&db_path)?;
    repo.migrate()?;
    let report = repo.preview_rebuild(&roots)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
