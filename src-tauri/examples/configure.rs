use media_manager::storage::Repository;
use std::env;
use std::path::PathBuf;

// Sets persisted source roots and archive root for a media-manager library.
fn main() -> anyhow::Result<()> {
    let mut args = env::args().skip(1);
    let db_path = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("usage: configure <library.sqlite> --sources <...> --archive <path>"))?;

    let mut sources: Vec<PathBuf> = Vec::new();
    let mut archive: Option<PathBuf> = None;
    let mut collecting_sources = false;
    for arg in args {
        match arg.as_str() {
            "--sources" => collecting_sources = true,
            "--archive" => collecting_sources = false,
            other if other.starts_with("--") => collecting_sources = false,
            other => {
                if collecting_sources {
                    sources.push(PathBuf::from(other));
                } else {
                    archive = Some(PathBuf::from(other));
                }
            }
        }
    }

    if sources.is_empty() && archive.is_none() {
        anyhow::bail!("nothing to configure: pass --sources and/or --archive");
    }

    let repo = Repository::open(&db_path)?;
    repo.migrate()?;
    if !sources.is_empty() {
        repo.set_source_roots(&sources)?;
    }
    if let Some(archive) = archive {
        repo.set_archive_root(&archive)?;
    }
    drop(repo);

    let repo = Repository::open(&db_path)?;
    println!("database={}", db_path.display());
    println!(
        "source_roots={}",
        repo.get_source_roots()?
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("|")
    );
    println!(
        "archive_root={}",
        repo.get_archive_root()?
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<none>".to_string())
    );
    Ok(())
}
