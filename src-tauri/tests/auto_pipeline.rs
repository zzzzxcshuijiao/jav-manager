use media_manager::domain::{
    ArchiveAsset, CodeKind, CompletedFile, ExceptionKind, HoldingReason, PipelineOutcome,
    ScrapeStatus, ScrapedWorkMetadata, WatchStatus, Work,
};
use media_manager::nfo::render_scraped_nfo;
use media_manager::pipeline::{
    execute_archive_layout, persist_pipeline_success, plan_archive_layout, ExecutedArchive,
};
use media_manager::pipeline::{identify_completed_file, PipelineIdentification};
use media_manager::pipeline::{
    is_aria2_complete, is_heuristically_complete, Aria2TaskSnapshot, CompletionSnapshot,
};
use media_manager::pipeline::{
    AutoPipeline, PipelineScrapeError, ScrapeContext, ScrapeCoordinator, ScraperSource,
};
use media_manager::storage::Repository;

#[test]
fn aria2_snapshot_is_complete_only_when_lengths_match() {
    assert!(is_aria2_complete(&Aria2TaskSnapshot {
        status: "complete".to_string(),
        completed_length: 100,
        total_length: 100,
    }));
    assert!(!is_aria2_complete(&Aria2TaskSnapshot {
        status: "active".to_string(),
        completed_length: 100,
        total_length: 100,
    }));
    assert!(!is_aria2_complete(&Aria2TaskSnapshot {
        status: "complete".to_string(),
        completed_length: 99,
        total_length: 100,
    }));
}

#[test]
fn completed_file_uses_temp_file_metadata_and_fingerprint() {
    let tmp = tempfile::tempdir().unwrap();
    let video = tmp.path().join("ABP-100.mp4");
    std::fs::write(&video, b"fake video bytes").unwrap();

    let completed = CompletedFile::from_path(&video).unwrap();

    assert_eq!(completed.path, video);
    assert_eq!(completed.file_name, "ABP-100.mp4");
    assert_eq!(completed.size_bytes, 16);
    assert!(completed.file_hash.unwrap().starts_with("fp:"));
}

#[test]
fn heuristic_completion_rejects_aria2_control_files_and_accepts_stable_video() {
    let tmp = tempfile::tempdir().unwrap();
    let video = tmp.path().join("ABP-100.mp4");
    std::fs::write(&video, b"video").unwrap();

    let first = CompletionSnapshot::capture(&video).unwrap();
    let second = CompletionSnapshot::capture(&video).unwrap();

    assert!(is_heuristically_complete(&first, &second));

    std::fs::write(tmp.path().join("ABP-100.mp4.aria2"), b"partial").unwrap();
    let blocked = CompletionSnapshot::capture(&video).unwrap();
    assert!(!is_heuristically_complete(&blocked, &blocked));
}

#[test]
fn heuristic_completion_rejects_files_that_change_between_samples() {
    let tmp = tempfile::tempdir().unwrap();
    let video = tmp.path().join("ABP-101.mp4");
    std::fs::write(&video, b"video").unwrap();
    let first = CompletionSnapshot::capture(&video).unwrap();
    std::fs::write(&video, b"video still growing").unwrap();
    let second = CompletionSnapshot::capture(&video).unwrap();

    assert!(!is_heuristically_complete(&first, &second));
}

#[test]
fn identifies_standard_code_from_completed_file_name() {
    let tmp = tempfile::tempdir().unwrap();
    let video = tmp.path().join("[site] abp_100 1080p.mp4");
    std::fs::write(&video, b"video").unwrap();
    let completed = CompletedFile::from_path(&video).unwrap();

    let identified = identify_completed_file(&completed);

    assert_eq!(
        identified,
        PipelineIdentification::Identified {
            normalized_code: "ABP-100".to_string(),
        }
    );
}

#[test]
fn missing_code_is_sent_to_holding_not_exception_queue() {
    let tmp = tempfile::tempdir().unwrap();
    let video = tmp.path().join("random trailer.mp4");
    std::fs::write(&video, b"video").unwrap();
    let completed = CompletedFile::from_path(&video).unwrap();

    let identified = identify_completed_file(&completed);

    assert_eq!(
        identified,
        PipelineIdentification::Holding {
            reason: HoldingReason::NoCode,
        }
    );
}

struct FakeScraper {
    name: &'static str,
    result: anyhow::Result<Option<ScrapedWorkMetadata>>,
}

impl ScraperSource for FakeScraper {
    fn name(&self) -> &str {
        self.name
    }

    fn lookup(&self, _normalized_code: &str) -> anyhow::Result<Option<ScrapedWorkMetadata>> {
        match &self.result {
            Ok(value) => Ok(value.clone()),
            Err(error) => Err(anyhow::anyhow!(error.to_string())),
        }
    }
}

fn scraped(code: &str, source: &str) -> ScrapedWorkMetadata {
    ScrapedWorkMetadata {
        source: source.to_string(),
        normalized_code: code.to_string(),
        title: format!("{code} title"),
        original_title: Some(format!("{code} original")),
        summary: Some("summary".to_string()),
        actors: vec!["Actor A".to_string()],
        genres: vec!["Genre A".to_string()],
        studio: Some("Studio A".to_string()),
        director: Some("Director A".to_string()),
        release_date: Some("2026-06-25".to_string()),
        cover_path: None,
    }
}

fn persisted_work(code: &str) -> Work {
    Work {
        id: None,
        normalized_code: Some(code.to_string()),
        source_code: Some(code.to_string()),
        code_kind: CodeKind::Standard,
        title_zh: None,
        original_title: None,
        aliases: Vec::new(),
        summary: None,
        outline: None,
        cover_path: None,
        poster_path: None,
        thumb_path: None,
        fanart_path: None,
        screenshot_path: None,
        gif_path: None,
        tags: Vec::new(),
        sets: Vec::new(),
        lists: Vec::new(),
        rating: None,
        rating_value: None,
        rating_max: None,
        rating_votes: None,
        criticrating: None,
        watch_status: WatchStatus::Unwatched,
        genres: Vec::new(),
        studio: None,
        label: None,
        director: None,
        release_date: None,
        runtime_minutes: None,
        year: None,
        website: None,
        mpaa: None,
        has_video: true,
        ratings: Vec::new(),
        watch_progress_seconds: None,
        last_played_at: None,
    }
}

#[test]
fn scraper_coordinator_falls_back_and_records_attempts() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();

    let first = FakeScraper {
        name: "FANZA",
        result: Ok(None),
    };
    let second = FakeScraper {
        name: "JavBus",
        result: Ok(Some(scraped("ABP-200", "JavBus"))),
    };
    let coordinator = ScrapeCoordinator {
        sources: vec![&first, &second],
    };

    let context = ScrapeContext {
        work_id: None,
        normalized_code: "ABP-200".to_string(),
        object_path: tmp.path().join("ABP-200.mp4"),
        pipeline_run_id: None,
    };
    let metadata = coordinator.scrape(&repo, &context).unwrap();

    assert_eq!(metadata.source, "JavBus");
    let jobs = repo.list_scrape_jobs().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0].source, "JavBus");
    assert_eq!(jobs[0].status, ScrapeStatus::Success);
    assert_eq!(jobs[0].work_id, None);
    assert_eq!(jobs[0].normalized_code.as_deref(), Some("ABP-200"));
    assert_eq!(jobs[1].source, "FANZA");
    assert_eq!(jobs[1].status, ScrapeStatus::Failed);
}

#[test]
fn scraper_coordinator_returns_all_failed_error() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();

    let first = FakeScraper {
        name: "FANZA",
        result: Ok(None),
    };
    let coordinator = ScrapeCoordinator {
        sources: vec![&first],
    };

    let context = ScrapeContext {
        work_id: None,
        normalized_code: "ABP-201".to_string(),
        object_path: tmp.path().join("ABP-201.mp4"),
        pipeline_run_id: None,
    };
    let err = coordinator.scrape(&repo, &context).unwrap_err();

    assert_eq!(err, PipelineScrapeError::AllSourcesFailed);
    assert!(repo.get_work_by_code("ABP-201").unwrap().is_none());
}

#[test]
fn nfo_writer_renders_scraped_metadata_as_kodi_movie_xml() {
    let xml = render_scraped_nfo(&scraped("ABP-300", "FANZA"));

    assert!(xml.contains("<movie>"));
    assert!(xml.contains("<num>ABP-300</num>"));
    assert!(xml.contains("<title>ABP-300 title</title>"));
    assert!(xml.contains("<originaltitle>ABP-300 original</originaltitle>"));
    assert!(xml.contains("<actor><name>Actor A</name></actor>"));
    assert!(xml.contains("<genre>Genre A</genre>"));
    assert!(xml.contains("<studio>Studio A</studio>"));
    assert!(xml.contains("<director>Director A</director>"));
    assert!(xml.contains("<premiered>2026-06-25</premiered>"));
}

#[test]
fn archive_layout_uses_flat_code_directory_and_version_suffixes() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("archive");
    let source = tmp.path().join("ABP-400.mp4");
    std::fs::create_dir_all(archive.join("ABP-400")).unwrap();
    std::fs::write(archive.join("ABP-400").join("ABP-400.mp4"), b"existing").unwrap();
    std::fs::write(&source, b"video").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();

    let plan = plan_archive_layout(&archive, &file, &scraped("ABP-400", "FANZA"), &[]).unwrap();

    assert_eq!(plan.work_dir, archive.join("ABP-400"));
    assert_eq!(
        plan.video_target,
        archive.join("ABP-400").join("ABP-400-v2.mp4")
    );
    assert_eq!(plan.nfo_target, archive.join("ABP-400").join("ABP-400.nfo"));
}

#[test]
fn archive_layout_discovers_code_named_assets_from_asset_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("archive");
    let assets = tmp.path().join("assets");
    let source = tmp.path().join("ABP-401.mp4");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(&source, b"video").unwrap();
    std::fs::write(assets.join("ABP-401.jpg"), b"poster").unwrap();
    std::fs::write(assets.join("ABP-401-shot.jpg"), b"shot").unwrap();
    std::fs::write(assets.join("ABP-401.gif"), b"gif").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();

    let plan = plan_archive_layout(
        &archive,
        &file,
        &scraped("ABP-401", "FANZA"),
        &[assets.clone()],
    )
    .unwrap();

    assert!(plan
        .assets
        .iter()
        .any(|asset| asset.relative_target == std::path::PathBuf::from("poster.jpg")));
    assert!(plan
        .assets
        .iter()
        .any(|asset| asset.relative_target
            == std::path::PathBuf::from("screenshot/ABP-401-shot.jpg")));
    assert!(plan
        .assets
        .iter()
        .any(|asset| asset.relative_target == std::path::PathBuf::from("ABP-401.gif")));
}

#[test]
fn archive_executor_moves_video_writes_nfo_and_copies_assets() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("archive");
    let assets = tmp.path().join("assets");
    let source = tmp.path().join("ABP-500.mp4");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(&source, b"video").unwrap();
    std::fs::write(assets.join("ABP-500.jpg"), b"poster").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();
    let metadata = scraped("ABP-500", "FANZA");
    let plan = plan_archive_layout(&archive, &file, &metadata, &[assets]).unwrap();

    let executed = execute_archive_layout(&plan, &file, &metadata).unwrap();

    assert!(!source.exists());
    assert!(executed.video_path.exists());
    assert!(executed.nfo_path.exists());
    assert!(archive.join("ABP-500").join("poster.jpg").exists());
    let nfo = std::fs::read_to_string(executed.nfo_path).unwrap();
    assert!(nfo.contains("<num>ABP-500</num>"));
}

#[test]
fn archive_executor_rolls_video_back_when_asset_copy_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("archive");
    let source = tmp.path().join("ABP-501.mp4");
    let missing_asset = tmp.path().join("missing").join("ABP-501.jpg");
    std::fs::write(&source, b"video").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();
    let metadata = scraped("ABP-501", "FANZA");
    let mut plan = plan_archive_layout(&archive, &file, &metadata, &[]).unwrap();
    plan.assets.push(ArchiveAsset {
        source_path: missing_asset,
        relative_target: std::path::PathBuf::from("poster.jpg"),
    });

    let result = execute_archive_layout(&plan, &file, &metadata);

    assert!(result.is_err());
    assert!(source.exists(), "video should be restored to original path");
    assert!(
        !plan.video_target.exists(),
        "failed archive target should not retain moved video"
    );
}

#[test]
fn pipeline_success_persists_work_relations_and_file_version() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();
    let source = tmp.path().join("ABP-600.mp4");
    let archived = tmp
        .path()
        .join("archive")
        .join("ABP-600")
        .join("ABP-600.mp4");
    let nfo = tmp
        .path()
        .join("archive")
        .join("ABP-600")
        .join("ABP-600.nfo");
    std::fs::create_dir_all(archived.parent().unwrap()).unwrap();
    std::fs::write(&source, b"video").unwrap();
    std::fs::write(&archived, b"video").unwrap();
    std::fs::write(&nfo, b"nfo").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();

    let work_id = persist_pipeline_success(
        &repo,
        &file,
        &scraped("ABP-600", "FANZA"),
        &ExecutedArchive {
            video_path: archived.clone(),
            nfo_path: nfo,
        },
    )
    .unwrap();

    let work = repo.get_work_by_id(work_id).unwrap().unwrap();
    assert_eq!(work.normalized_code.as_deref(), Some("ABP-600"));
    assert_eq!(work.title_zh.as_deref(), Some("ABP-600 title"));
    assert_eq!(work.studio.as_deref(), Some("Studio A"));
    assert_eq!(
        repo.list_work_actors(work_id).unwrap()[0].primary_name,
        "Actor A"
    );
    assert_eq!(repo.list_work_tags(work_id).unwrap()[0].name, "Genre A");
    let versions = repo.list_file_versions_for_work(work_id).unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].archived_path.as_ref(), Some(&archived));
    assert_eq!(
        versions[0].normalized_file_name.as_deref(),
        Some("ABP-600.mp4")
    );
}

#[test]
fn repository_can_find_existing_versions_by_file_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();
    let work_id = repo.upsert_work(&persisted_work("ABP-601")).unwrap();

    repo.upsert_file_version_for_work(
        work_id,
        tmp.path(),
        &tmp.path().join("ABP-601.mp4"),
        Some(&tmp.path().join("archive/ABP-601/ABP-601.mp4")),
        "ABP-601.mp4",
        Some("ABP-601.mp4"),
        10,
        Some("fp:same"),
    )
    .unwrap();

    let matches = repo.find_file_versions_by_hash("fp:same").unwrap();

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].work_id, Some(work_id));
}

#[test]
fn auto_pipeline_success_archives_and_records_run() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();
    let source = tmp.path().join("inbox").join("ABP-700.mp4");
    let assets = tmp.path().join("assets");
    let archive = tmp.path().join("archive");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(&source, b"video").unwrap();
    std::fs::write(assets.join("ABP-700.jpg"), b"poster").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();
    let scraper = FakeScraper {
        name: "FANZA",
        result: Ok(Some(scraped("ABP-700", "FANZA"))),
    };
    let pipeline = AutoPipeline {
        repo: &repo,
        archive_root: archive.clone(),
        asset_roots: vec![assets],
        scrapers: ScrapeCoordinator {
            sources: vec![&scraper],
        },
    };

    let outcome: PipelineOutcome = pipeline.process_completed_file(file).unwrap();

    assert_eq!(outcome.status, "archived");
    assert!(outcome.work_id.is_some());
    assert!(archive.join("ABP-700").join("ABP-700.mp4").exists());
    assert!(archive.join("ABP-700").join("ABP-700.nfo").exists());
    assert_eq!(repo.list_pipeline_runs().unwrap()[0].status, "archived");
}

#[test]
fn auto_pipeline_routes_missing_code_to_holding() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();
    let source = tmp.path().join("random.mp4");
    std::fs::write(&source, b"video").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();
    let scraper = FakeScraper {
        name: "FANZA",
        result: Ok(None),
    };
    let pipeline = AutoPipeline {
        repo: &repo,
        archive_root: tmp.path().join("archive"),
        asset_roots: vec![],
        scrapers: ScrapeCoordinator {
            sources: vec![&scraper],
        },
    };

    let outcome = pipeline.process_completed_file(file).unwrap();

    assert_eq!(outcome.status, "holding");
    assert!(outcome.holding_id.is_some());
    assert_eq!(
        repo.list_holding().unwrap()[0].reason,
        HoldingReason::NoCode
    );
    assert!(repo.list_exceptions().unwrap().is_empty());
}

#[test]
fn auto_pipeline_routes_scrape_failure_to_exception_queue() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();
    let source = tmp.path().join("ABP-701.mp4");
    std::fs::write(&source, b"video").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();
    let scraper = FakeScraper {
        name: "FANZA",
        result: Ok(None),
    };
    let pipeline = AutoPipeline {
        repo: &repo,
        archive_root: tmp.path().join("archive"),
        asset_roots: vec![],
        scrapers: ScrapeCoordinator {
            sources: vec![&scraper],
        },
    };

    let outcome = pipeline.process_completed_file(file).unwrap();

    assert_eq!(outcome.status, "exception");
    let exceptions = repo.list_exceptions().unwrap();
    assert_eq!(exceptions[0].kind, ExceptionKind::ScrapeFailed);
    assert!(exceptions[0].evidence_json.contains("ABP-701"));
}

#[test]
fn auto_pipeline_routes_duplicate_fingerprint_to_exception_queue() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();
    let first = tmp.path().join("ABP-702.mp4");
    let second = tmp.path().join("ABP-703.mp4");
    std::fs::write(&first, b"same bytes").unwrap();
    std::fs::write(&second, b"same bytes").unwrap();
    let existing = CompletedFile::from_path(&first).unwrap();
    let work_id = repo.upsert_work(&persisted_work("ABP-702")).unwrap();
    repo.upsert_file_version_for_work(
        work_id,
        first.parent().unwrap(),
        &first,
        None,
        "ABP-702.mp4",
        Some("ABP-702.mp4"),
        existing.size_bytes,
        existing.file_hash.as_deref(),
    )
    .unwrap();
    let file = CompletedFile::from_path(&second).unwrap();
    let scraper = FakeScraper {
        name: "FANZA",
        result: Ok(Some(scraped("ABP-703", "FANZA"))),
    };
    let pipeline = AutoPipeline {
        repo: &repo,
        archive_root: tmp.path().join("archive"),
        asset_roots: vec![],
        scrapers: ScrapeCoordinator {
            sources: vec![&scraper],
        },
    };

    let outcome = pipeline.process_completed_file(file).unwrap();

    assert_eq!(outcome.status, "exception");
    assert_eq!(
        repo.list_exceptions().unwrap()[0].kind,
        ExceptionKind::DuplicateCandidate
    );
}

#[test]
fn auto_pipeline_marks_operational_archive_failure_without_exception() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();
    let source = tmp.path().join("ABP-704.mp4");
    std::fs::write(&source, b"video").unwrap();
    let file = CompletedFile::from_path(&source).unwrap();
    let mut metadata = scraped("ABP-704", "FANZA");
    metadata.cover_path = Some(tmp.path().join("missing").join("ABP-704.jpg"));
    let scraper = FakeScraper {
        name: "FANZA",
        result: Ok(Some(metadata)),
    };
    let pipeline = AutoPipeline {
        repo: &repo,
        archive_root: tmp.path().join("archive"),
        asset_roots: vec![],
        scrapers: ScrapeCoordinator {
            sources: vec![&scraper],
        },
    };

    let result = pipeline.process_completed_file(file);

    assert!(result.is_err());
    assert_eq!(repo.list_pipeline_runs().unwrap()[0].status, "failed");
    assert!(repo.list_exceptions().unwrap().is_empty());
    assert!(source.exists(), "failed archive should roll video back");
}
