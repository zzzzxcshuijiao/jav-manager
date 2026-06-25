use media_manager::daemon::{CompletionPolicy, DaemonConfig, DaemonState, HeadlessDaemon};
use media_manager::domain::ScrapedWorkMetadata;
use media_manager::pipeline::{ScrapeCoordinator, ScraperSource};
use media_manager::storage::Repository;
use std::time::Duration;

struct FakeScraper;

impl ScraperSource for FakeScraper {
    fn name(&self) -> &str {
        "fake"
    }

    fn lookup(&self, normalized_code: &str) -> anyhow::Result<Option<ScrapedWorkMetadata>> {
        if normalized_code == "ABP-300" {
            Ok(Some(scraped(normalized_code)))
        } else {
            Ok(None)
        }
    }
}

fn scraped(code: &str) -> ScrapedWorkMetadata {
    ScrapedWorkMetadata {
        source: "fake".to_string(),
        normalized_code: code.to_string(),
        title: format!("{code} title"),
        original_title: Some(format!("{code} original")),
        summary: Some("summary".to_string()),
        actors: vec!["Actor A".to_string()],
        genres: vec!["Genre A".to_string()],
        studio: Some("Studio A".to_string()),
        director: None,
        release_date: Some("2026-06-25".to_string()),
        cover_path: None,
    }
}

fn open_repo(db: &std::path::Path) -> Repository {
    let repo = Repository::open(db).unwrap();
    repo.migrate().unwrap();
    repo
}

fn configured_repo(
    tmp: &tempfile::TempDir,
) -> (
    Repository,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    let inbox = tmp.path().join("inbox");
    let archive = tmp.path().join("archive");
    let assets = tmp.path().join("assets");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::create_dir_all(&archive).unwrap();
    std::fs::create_dir_all(&assets).unwrap();
    repo.set_source_roots(&[inbox.clone()]).unwrap();
    repo.set_archive_root(&archive).unwrap();
    repo.set_resource_pool_dirs(&[assets.clone()]).unwrap();
    (repo, inbox, archive, assets)
}

fn daemon<'a>(repo: &'a Repository, scraper: &'a FakeScraper) -> HeadlessDaemon<'a> {
    let config = DaemonConfig::load(repo).unwrap();
    HeadlessDaemon::with_completion_policy(
        repo,
        config,
        ScrapeCoordinator {
            sources: vec![scraper],
        },
        CompletionPolicy {
            sample_delay: Duration::ZERO,
        },
    )
}

#[test]
fn daemon_config_loads_roots_from_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, inbox, archive, assets) = configured_repo(&tmp);

    let config = DaemonConfig::load(&repo).unwrap();

    assert_eq!(config.source_roots, vec![inbox]);
    assert_eq!(config.archive_root, archive);
    assert_eq!(config.asset_roots, vec![assets]);
}

#[test]
fn daemon_config_requires_archive_root() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = open_repo(&tmp.path().join("library.sqlite"));
    repo.set_source_roots(&[tmp.path().join("inbox")]).unwrap();

    let error = DaemonConfig::load(&repo).unwrap_err();

    assert!(error.to_string().contains("archive_root"));
}

#[test]
fn daemon_status_starts_idle_with_empty_queue() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, _, _, _) = configured_repo(&tmp);
    let scraper = FakeScraper;
    let daemon = daemon(&repo, &scraper);

    let status = daemon.status();

    assert_eq!(status.state, DaemonState::Idle);
    assert_eq!(status.queued, 0);
    assert_eq!(status.processed, 0);
    assert_eq!(status.last_error, None);
}
