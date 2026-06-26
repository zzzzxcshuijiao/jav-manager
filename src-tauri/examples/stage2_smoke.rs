use media_manager::domain::{CompletedFile, ScrapedWorkMetadata};
use media_manager::pipeline::{AutoPipeline, ScrapeCoordinator, ScraperSource};
use media_manager::storage::Repository;

/// Deterministic scraper used by the smoke example; it returns one success and
/// lets all other codes exercise the scrape-failure route.
struct SmokeScraper;

impl ScraperSource for SmokeScraper {
    fn name(&self) -> &str {
        "smoke"
    }

    fn lookup(&self, normalized_code: &str) -> anyhow::Result<Option<ScrapedWorkMetadata>> {
        if normalized_code == "ABP-900" {
            Ok(Some(ScrapedWorkMetadata {
                source: self.name().to_string(),
                normalized_code: normalized_code.to_string(),
                title: "ABP-900 smoke title".to_string(),
                original_title: None,
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
    std::fs::write(inbox.join("ABP-900.mp4"), b"video-900")?;
    std::fs::write(inbox.join("random.mp4"), b"random-video")?;
    std::fs::write(inbox.join("ABP-901.mp4"), b"video-901")?;
    std::fs::write(assets.join("ABP-900.jpg"), b"poster")?;

    let repo = Repository::open(&db)?;
    repo.migrate()?;
    let scraper = SmokeScraper;
    let pipeline = AutoPipeline {
        repo: &repo,
        archive_root: archive.clone(),
        asset_roots: vec![assets],
        scrapers: ScrapeCoordinator {
            sources: vec![&scraper],
        },
    };

    for name in ["ABP-900.mp4", "random.mp4", "ABP-901.mp4"] {
        let file = CompletedFile::from_path(&inbox.join(name))?;
        let _ = pipeline.process_completed_file(file)?;
    }

    println!("stage2_smoke=completed");
    let archived = repo
        .list_pipeline_runs()?
        .iter()
        .filter(|run| run.status == "archived")
        .count();
    println!("archived={archived}");
    println!("holding={}", repo.list_holding()?.len());
    println!("exceptions={}", repo.list_exceptions()?.len());
    println!(
        "archive_has_video={}",
        archive.join("ABP-900/ABP-900.mp4").exists()
    );
    println!(
        "archive_has_nfo={}",
        archive.join("ABP-900/ABP-900.nfo").exists()
    );
    println!("no_real_resources_required=true");
    Ok(())
}
