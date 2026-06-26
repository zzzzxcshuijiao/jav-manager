use media_manager::daemon::{CompletionPolicy, DaemonConfig, HeadlessDaemon};
use media_manager::domain::ScrapedWorkMetadata;
use media_manager::pipeline::{ScrapeCoordinator, ScraperSource};
use media_manager::storage::Repository;
use std::time::Duration;

/// Deterministic scraper for daemon smoke; it succeeds for ABP-300 and lets
/// every other recognized code exercise the exception path.
struct SmokeScraper;

impl ScraperSource for SmokeScraper {
    fn name(&self) -> &str {
        "smoke"
    }

    fn lookup(&self, normalized_code: &str) -> anyhow::Result<Option<ScrapedWorkMetadata>> {
        if normalized_code == "ABP-300" {
            Ok(Some(ScrapedWorkMetadata {
                source: self.name().to_string(),
                normalized_code: normalized_code.to_string(),
                title: "ABP-300 smoke title".to_string(),
                original_title: Some("ABP-300 smoke original".to_string()),
                summary: Some("smoke summary".to_string()),
                actors: vec!["Smoke Actor".to_string()],
                genres: vec!["Smoke Genre".to_string()],
                studio: Some("Smoke Studio".to_string()),
                director: None,
                release_date: Some("2026-06-25".to_string()),
                cover_path: None,
            }))
        } else {
            Ok(None)
        }
    }
}

fn main() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let db = tmp.path().join("library.sqlite");
    let inbox = tmp.path().join("inbox");
    let assets = tmp.path().join("assets");
    let archive = tmp.path().join("archive");
    std::fs::create_dir_all(&inbox)?;
    std::fs::create_dir_all(&assets)?;
    std::fs::create_dir_all(&archive)?;
    std::fs::write(inbox.join("ABP-300.mp4"), b"video-300")?;
    std::fs::write(inbox.join("random.mp4"), b"random-video")?;
    std::fs::write(inbox.join("ABP-301.mp4"), b"video-301")?;
    std::fs::write(inbox.join("notes.txt"), b"not-video")?;

    let repo = Repository::open(&db)?;
    repo.migrate()?;
    repo.set_source_roots(&[inbox])?;
    repo.set_archive_root(&archive)?;
    repo.set_resource_pool_dirs(&[assets])?;

    let scraper = SmokeScraper;
    let config = DaemonConfig::load(&repo)?;
    let mut daemon = HeadlessDaemon::with_completion_policy(
        &repo,
        config,
        ScrapeCoordinator {
            sources: vec![&scraper],
        },
        CompletionPolicy {
            sample_delay: Duration::ZERO,
        },
    );

    let report = daemon.run_once()?;

    println!("stage3_daemon_smoke=completed");
    println!("queued={}", report.scan.queued_files);
    println!("archived={}", report.process.archived);
    println!("holding={}", report.process.holding);
    println!("exceptions={}", report.process.exceptions);
    println!("failed={}", report.process.failed);
    println!("no_real_resources_required=true");
    Ok(())
}
