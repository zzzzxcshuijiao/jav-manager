use media_manager::acceptance::{
    build_phase_f_dry_run_report, format_count_pairs, summarize_phase_f_dry_run,
};
use media_manager::archive::{normalized_file_name, ArchivePlanner};
use media_manager::commands::{
    decide_items_with_provider_enabled,
    execute_archive_actions, existing_file_parent_directory, require_existing_path,
    validate_archive_action_paths,
};
use media_manager::domain::{
    ArchiveAction, ArchiveActionLog, ArchiveConflict, ArchivePlan, FileVersion, IngestDecision, IngestItem,
    IngestItemFilters, IngestJobSummary, ProviderMetadata, ReviewReason, WatchStatus, Work, CodeKind,
};
use media_manager::identifier::{extract_code_from_text, normalize_code};
use media_manager::ingest::IngestEngine;
use media_manager::matcher::attach_version_to_work;
use media_manager::provider::{DisabledProvider, ExampleProvider, MetadataProvider};
use media_manager::scanner::{is_video_file, parse_ffprobe_media_info, should_probe_media_file, Scanner};
use media_manager::storage::Repository;
use media_manager::thumbnail::{
    clear_thumbnail_cache, enforce_cache_size, get_or_create_thumbnail, thumbnail_cache_path,
    thumbnail_cache_summary,
};
use std::path::PathBuf;
use std::process::Command;
use std::fs;

#[test]
fn normalizes_codes_with_case_spacing_and_separators() {
    assert_eq!(normalize_code(" abp  001 "), Some("ABP-001".to_string()));
    assert_eq!(normalize_code("abp_001"), Some("ABP-001".to_string()));
    assert_eq!(normalize_code("ABP-1"), Some("ABP-001".to_string()));
}

#[test]
fn extracts_code_from_noisy_directory_or_file_name() {
    let text = r"H:\downloads\[site] ABP_001 1080p uncensored\ABP001-source.mp4";
    assert_eq!(extract_code_from_text(text), Some("ABP-001".to_string()));
}

#[test]
fn recognizes_common_video_extensions_only() {
    assert!(is_video_file(PathBuf::from("movie.mkv").as_path()));
    assert!(is_video_file(PathBuf::from("clip.MP4").as_path()));
    assert!(!is_video_file(PathBuf::from("cover.jpg").as_path()));
    assert!(!is_video_file(PathBuf::from("notes.nfo").as_path()));
}

#[test]
fn scanner_recursively_collects_video_files_with_extracted_codes() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("inbox");
    std::fs::create_dir_all(source.join("[source] ABP-001 1080p")).unwrap();
    std::fs::write(source.join("[source] ABP-001 1080p").join("ABP_001.mp4"), b"video").unwrap();
    std::fs::write(source.join("cover.jpg"), b"image").unwrap();

    let items = Scanner::scan_sources(&[source.clone()]).unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].source_root, source);
    assert_eq!(items[0].normalized_code, Some("ABP-001".to_string()));
    assert_eq!(items[0].decision, IngestDecision::NeedsReview);
}

#[test]
fn scanner_prefers_file_name_code_over_source_directory_version_numbers() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp
        .path()
        .join("CineMingle-1.3.0")
        .join("JAV_output")
        .join("Actress");
    std::fs::create_dir_all(source.join("ABP-525")).unwrap();
    std::fs::write(source.join("ABP-525").join("ABP-525.mp4"), b"video").unwrap();

    let items = Scanner::scan_sources(&[source.clone()]).unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].normalized_code, Some("ABP-525".to_string()));
}

#[test]
fn scanner_uses_local_nfo_and_cover_as_item_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("inbox").join("ABP-525");
    std::fs::create_dir_all(&source).unwrap();
    let video = source.join("ABP-525.mp4");
    let cover = source.join("cover.jpg");
    std::fs::write(&video, b"video").unwrap();
    std::fs::write(
        source.join("ABP-525.nfo"),
        r#"
        <movie>
          <title>本地中文标题</title>
          <originaltitle>Local Original Title</originaltitle>
          <plot>本地简介</plot>
          <premiered>2024-01-02</premiered>
        </movie>
        "#,
    )
    .unwrap();
    std::fs::write(&cover, b"image").unwrap();

    let items = Scanner::scan_sources(&[temp.path().join("inbox")]).unwrap();
    let metadata = items[0].metadata.as_ref().unwrap();

    assert_eq!(metadata.provider, "local");
    assert_eq!(metadata.title_zh, Some("本地中文标题".to_string()));
    assert_eq!(metadata.original_title, Some("Local Original Title".to_string()));
    assert_eq!(metadata.summary, Some("本地简介".to_string()));
    assert_eq!(metadata.cover_url, Some(cover.to_string_lossy().to_string()));
    assert_eq!(metadata.release_date, Some("2024-01-02".to_string()));
    assert_eq!(metadata.confidence, 0.95);
}

#[test]
fn scanner_uses_uniqueid_or_num_from_local_nfo_when_path_has_no_code() {
    let temp = tempfile::tempdir().unwrap();
    let uniqueid_source = temp.path().join("uniqueid-inbox").join("movie-a");
    let num_source = temp.path().join("num-inbox").join("movie-b");
    std::fs::create_dir_all(&uniqueid_source).unwrap();
    std::fs::create_dir_all(&num_source).unwrap();
    std::fs::write(uniqueid_source.join("video.mp4"), b"video").unwrap();
    std::fs::write(
        uniqueid_source.join("movie.nfo"),
        r#"<movie><uniqueid type="num">abp525</uniqueid></movie>"#,
    )
    .unwrap();
    std::fs::write(num_source.join("main.mkv"), b"video").unwrap();
    std::fs::write(num_source.join("info.nfo"), r#"<movie><num>abs204</num></movie>"#).unwrap();

    let mut items = Scanner::scan_sources(&[temp.path().to_path_buf()]).unwrap();
    items.sort_by(|left, right| left.file_name.cmp(&right.file_name));

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].normalized_code, Some("ABS-204".to_string()));
    assert_eq!(items[1].normalized_code, Some("ABP-525".to_string()));
    assert!(items.iter().all(|item| !item.review_reasons.contains(&ReviewReason::MissingCode)));
}

#[test]
fn scanner_marks_conflict_when_path_code_and_local_nfo_code_disagree() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("inbox").join("ABP-525");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("ABP-525.mp4"), b"video").unwrap();
    std::fs::write(source.join("ABP-525.nfo"), r#"<movie><num>ABS-204</num></movie>"#).unwrap();

    let items = Scanner::scan_sources(&[temp.path().join("inbox")]).unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].normalized_code, Some("ABP-525".to_string()));
    assert_eq!(items[0].decision, IngestDecision::NeedsReview);
    assert!(items[0].review_reasons.contains(&ReviewReason::CodeConflict));
    let conflict = items[0].code_conflict.as_ref().unwrap();
    assert_eq!(conflict.path_code, "ABP-525");
    assert_eq!(conflict.nfo_code, "ABS-204");
    assert_eq!(conflict.nfo_path, source.join("ABP-525.nfo"));
}

#[test]
fn scanner_prefers_scraper_style_ps_poster_images_for_local_cover() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("inbox").join("ABP-525");
    std::fs::create_dir_all(&source).unwrap();
    let video = source.join("ABP-525.mp4");
    let keyword_poster = source.join("ABP-525-poster.jpg");
    let ps_poster = source.join("ABP-525-ps.jpg");
    std::fs::write(&video, b"video").unwrap();
    std::fs::write(&keyword_poster, b"poster").unwrap();
    std::fs::write(&ps_poster, b"ps").unwrap();

    let items = Scanner::scan_sources(&[temp.path().join("inbox")]).unwrap();

    assert_eq!(
        items[0].metadata.as_ref().unwrap().cover_url,
        Some(ps_poster.to_string_lossy().to_string())
    );
}
#[test]
fn scanner_falls_back_to_code_named_cover_for_multi_cd_video_stem() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("inbox").join("IDBD-815");
    std::fs::create_dir_all(&source).unwrap();
    let video = source.join("IDBD-815-cd2.mp4");
    let cover = source.join("IDBD-815.jpg");
    std::fs::write(&video, b"video").unwrap();
    std::fs::write(&cover, b"poster").unwrap();
    let items = Scanner::scan_sources(&[temp.path().join("inbox")]).unwrap();
    assert_eq!(
        items[0].metadata.as_ref().unwrap().cover_url,
        Some(cover.to_string_lossy().to_string())
    );
}

#[test]
fn scanner_marks_exact_same_file_content_as_duplicate_candidates() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("inbox");
    std::fs::create_dir_all(source.join("ABP-525")).unwrap();
    std::fs::create_dir_all(source.join("ABP-525-copy")).unwrap();
    std::fs::write(source.join("ABP-525").join("ABP-525.mp4"), b"same video bytes").unwrap();
    std::fs::write(
        source.join("ABP-525-copy").join("ABP-525-copy.mp4"),
        b"same video bytes",
    )
    .unwrap();

    let items = Scanner::scan_sources(&[source]).unwrap();

    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|item| item.file_hash.is_some()));
    assert_eq!(items[0].file_hash, items[1].file_hash);
    assert!(items.iter().all(|item| item.decision == IngestDecision::DuplicateCandidate));
    assert!(items
        .iter()
        .all(|item| item.review_reasons.contains(&ReviewReason::DuplicateFile)));
}

#[test]
fn parses_ffprobe_json_media_info_from_video_stream() {
    let output = r#"
    {
      "streams": [
        {
          "codec_type": "audio",
          "codec_name": "aac"
        },
        {
          "codec_type": "video",
          "codec_name": "h264",
          "width": 1920,
          "height": 1080,
          "duration": "125.72"
        }
      ],
      "format": {
        "duration": "126.34"
      }
    }
    "#;

    let info = parse_ffprobe_media_info(output).unwrap();

    assert_eq!(info.duration_seconds, Some(126));
    assert_eq!(info.width, Some(1920));
    assert_eq!(info.height, Some(1080));
    assert_eq!(info.codec, Some("h264".to_string()));
}

#[test]
fn tiny_files_are_not_sent_to_ffprobe() {
    assert!(!should_probe_media_file(5));
    assert!(should_probe_media_file(1024));
}

#[test]
fn sample_file_fingerprint_hashes_large_files_without_loading_whole_file() {
    let temp = tempfile::tempdir().unwrap();
    let big = temp.path().join("big.mp4");
    let big_copy = temp.path().join("big-copy.mp4");
    let big_variant = temp.path().join("big-variant.mp4");
    // 4 MB: larger than the 3 MB head/mid/tail threshold so the large-file path runs.
    let content = vec![0xAB_u8; 4 * 1024 * 1024];
    std::fs::write(&big, &content).unwrap();
    std::fs::write(&big_copy, &content).unwrap();
    let mut variant = content.clone();
    variant[0] = 0xCD; // change the head region so the fingerprint must differ
    std::fs::write(&big_variant, &variant).unwrap();

    let fp = media_manager::scanner::sample_file_fingerprint(&big).unwrap();
    let fp_copy = media_manager::scanner::sample_file_fingerprint(&big_copy).unwrap();
    let fp_variant = media_manager::scanner::sample_file_fingerprint(&big_variant).unwrap();

    assert!(fp.starts_with("fp:"), "fingerprint should carry the fp: prefix");
    assert_eq!(fp, fp_copy, "identical large files share a fingerprint");
    assert_ne!(fp, fp_variant, "differing head bytes change the fingerprint");
}

#[test]
fn thumbnail_cache_paths_are_deterministic_and_unique_per_video_path() {
    let temp = tempfile::tempdir().unwrap();
    let first = thumbnail_cache_path(temp.path(), PathBuf::from("H:/inbox/ABP-525.mp4").as_path());
    let first_again = thumbnail_cache_path(temp.path(), PathBuf::from("H:/inbox/ABP-525.mp4").as_path());
    let second = thumbnail_cache_path(temp.path(), PathBuf::from("H:/inbox/ABS-204.mkv").as_path());

    assert_eq!(first, first_again);
    assert_ne!(first, second);
    assert_eq!(first.parent(), Some(temp.path()));
    assert_eq!(first.extension().and_then(|value| value.to_str()), Some("jpg"));
}

#[test]
fn thumbnail_cache_size_limit_can_remove_generated_images() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first.jpg");
    let second = temp.path().join("second.jpg");
    std::fs::write(&first, b"1234").unwrap();
    std::fs::write(&second, b"5678").unwrap();

    enforce_cache_size(temp.path(), 0).unwrap();

    assert!(!first.exists());
    assert!(!second.exists());
}

#[test]
fn thumbnail_cache_summary_counts_files_and_clear_removes_them() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("nested")).unwrap();
    let first = temp.path().join("first.jpg");
    let second = temp.path().join("second.jpg");
    std::fs::write(&first, b"1234").unwrap();
    std::fs::write(&second, b"567890").unwrap();

    let summary = thumbnail_cache_summary(temp.path()).unwrap();
    let cleared = clear_thumbnail_cache(temp.path()).unwrap();
    let after = thumbnail_cache_summary(temp.path()).unwrap();

    assert_eq!(summary.file_count, 2);
    assert_eq!(summary.total_bytes, 10);
    assert_eq!(cleared.file_count, 2);
    assert_eq!(cleared.total_bytes, 10);
    assert_eq!(after.file_count, 0);
    assert_eq!(after.total_bytes, 0);
    assert!(temp.path().join("nested").exists());
}

#[test]
fn thumbnail_generation_creates_cached_jpg_when_ffmpeg_is_available() {
    let ffmpeg_available = Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !ffmpeg_available {
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let video = temp.path().join("sample.mp4");
    let cache = temp.path().join("thumbs");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=320x180:d=1",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&video)
        .status()
        .unwrap();
    assert!(status.success());

    let thumbnail = get_or_create_thumbnail(&video, &cache, 1024 * 1024)
        .unwrap()
        .unwrap();
    let thumbnail_again = get_or_create_thumbnail(&video, &cache, 1024 * 1024)
        .unwrap()
        .unwrap();

    assert_eq!(thumbnail, thumbnail_again);
    assert!(thumbnail.exists());
    assert!(std::fs::metadata(&thumbnail).unwrap().len() > 0);
    assert_eq!(thumbnail.extension().and_then(|value| value.to_str()), Some("jpg"));
}

#[test]
fn same_code_files_attach_as_versions_of_one_work() {
    let work = Work {
        id: Some(7),
        normalized_code: Some("ABP-001".to_string()),
        source_code: None,
        code_kind: CodeKind::Standard,
        outline: None,
        poster_path: None,
        thumb_path: None,
        fanart_path: None,
        screenshot_path: None,
        gif_path: None,
        sets: vec![],
        rating_value: None,
        rating_max: None,
        rating_votes: None,
        criticrating: None,
        label: None,
        runtime_minutes: None,
        year: None,
        website: None,
        mpaa: None,
        has_video: true,
        ratings: vec![],
        title_zh: Some("中文标题".to_string()),
        original_title: None,
        aliases: vec![],
        summary: None,
        cover_path: None,
        tags: vec![],
        lists: vec![],
        rating: None,
        watch_status: WatchStatus::Unwatched,
        genres: vec![],
        studio: None,
        director: None,
        release_date: None,
    };
    let mut version = FileVersion {
        id: None,
        work_id: None,
        source_root: PathBuf::from("H:/inbox"),
        original_path: PathBuf::from("H:/inbox/ABP001.mp4"),
        archived_path: None,
        original_file_name: "ABP001.mp4".to_string(),
        normalized_file_name: None,
        size_bytes: 10,
        duration_seconds: None,
        width: None,
        height: None,
        codec: None,
        file_hash: Some("hash".to_string()),
    };

    attach_version_to_work(&work, &mut version);

    assert_eq!(version.work_id, Some(7));
    assert_eq!(version.normalized_file_name, Some("ABP-001.mp4".to_string()));
}

struct FailingProvider;

impl MetadataProvider for FailingProvider {
    fn name(&self) -> &str {
        "failing"
    }

    fn lookup(&self, _normalized_code: &str, _original_file_name: &str) -> anyhow::Result<Option<ProviderMetadata>> {
        Err(anyhow::anyhow!("provider offline"))
    }
}

struct LowConfidenceProvider;

impl MetadataProvider for LowConfidenceProvider {
    fn name(&self) -> &str {
        "low-confidence"
    }

    fn lookup(&self, _normalized_code: &str, _original_file_name: &str) -> anyhow::Result<Option<ProviderMetadata>> {
        Ok(Some(ProviderMetadata {
            provider: "low-confidence".to_string(),
            title_zh: None,
            original_title: None,
            aliases: vec![],
            summary: None,
            cover_url: None,
            release_date: None,
            confidence: 0.5,
            actors: vec![],
            genres: vec![],
            studio: None,
            director: None,
        }))
    }
}

#[test]
fn provider_failure_marks_item_for_review_without_auto_archive() {
    let engine = IngestEngine::new(FailingProvider);
    let item = ingest_item("ABP-001", 0.95);

    let decided = engine.decide(item);

    assert_eq!(decided.decision, IngestDecision::NeedsReview);
    assert!(decided.review_reasons.contains(&ReviewReason::ProviderFailed));
}

#[test]
fn disabled_provider_does_not_return_example_metadata() {
    let provider = DisabledProvider;

    let result = provider.lookup("ABP-525", "ABP-525.mp4").unwrap();

    assert!(result.is_none());
}

#[test]
fn provider_toggle_controls_example_metadata_lookup_for_ingest_decisions() {
    let disabled = decide_items_with_provider_enabled(vec![ingest_item("ABP-525", 0.95)], false)
        .remove(0);
    let enabled = decide_items_with_provider_enabled(vec![ingest_item("ABP-525", 0.95)], true)
        .remove(0);

    assert_eq!(disabled.decision, IngestDecision::NeedsReview);
    assert!(disabled.review_reasons.contains(&ReviewReason::ProviderFailed));
    assert!(disabled.metadata.is_none());
    assert_eq!(enabled.decision, IngestDecision::AutoArchive);
    assert_eq!(enabled.metadata.unwrap().provider, "example");
}

#[test]
fn low_confidence_item_stays_in_review_queue() {
    let engine = IngestEngine::new(FailingProvider);
    let item = ingest_item("ABP-001", 0.62);

    let decided = engine.decide(item);

    assert_eq!(decided.decision, IngestDecision::NeedsReview);
    assert!(decided.review_reasons.contains(&ReviewReason::LowConfidence));
}

#[test]
fn low_confidence_provider_metadata_stays_in_review_queue() {
    let engine = IngestEngine::new(LowConfidenceProvider);
    let item = ingest_item("ABP-001", 0.95);

    let decided = engine.decide(item);

    assert_eq!(decided.decision, IngestDecision::NeedsReview);
    assert!(decided.review_reasons.contains(&ReviewReason::LowConfidence));
}

#[test]
fn local_metadata_promotes_item_to_auto_archive_without_network_provider() {
    let engine = IngestEngine::new(FailingProvider);
    let mut item = ingest_item("ABP-525", 0.82);
    item.metadata = Some(ProviderMetadata {
        provider: "local".to_string(),
        title_zh: Some("本地中文标题".to_string()),
        original_title: None,
        aliases: vec![],
        summary: Some("本地简介".to_string()),
        cover_url: Some("H:/inbox/ABP-525/cover.jpg".to_string()),
        release_date: None,
        confidence: 0.95,
            actors: vec![],
            genres: vec![],
            studio: None,
            director: None,
    });

    let decided = engine.decide(item);

    assert_eq!(decided.decision, IngestDecision::AutoArchive);
    assert!(decided.review_reasons.is_empty());
    assert_eq!(decided.confidence, 0.95);
    assert_eq!(decided.metadata.unwrap().provider, "local");
}

#[test]
fn code_conflict_stays_in_review_even_with_high_confidence_local_metadata() {
    let engine = IngestEngine::new(FailingProvider);
    let mut item = ingest_item("ABP-525", 0.82);
    item.review_reasons = vec![ReviewReason::CodeConflict];
    item.metadata = Some(ProviderMetadata {
        provider: "local".to_string(),
        title_zh: Some("本地标题".to_string()),
        original_title: None,
        aliases: vec![],
        summary: None,
        cover_url: None,
        release_date: None,
        confidence: 0.95,
            actors: vec![],
            genres: vec![],
            studio: None,
            director: None,
    });

    let decided = engine.decide(item);

    assert_eq!(decided.decision, IngestDecision::NeedsReview);
    assert!(decided.review_reasons.contains(&ReviewReason::CodeConflict));
}

#[test]
fn conflicted_local_nfo_item_can_be_manually_resolved_and_planned() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("inbox");
    let archive_root = temp.path().join("archive");
    let movie_dir = source.join("ABP-525");
    std::fs::create_dir_all(&movie_dir).unwrap();
    std::fs::create_dir_all(&archive_root).unwrap();
    let video_path = movie_dir.join("ABP-525.mp4");
    std::fs::write(&video_path, vec![1_u8; 2048]).unwrap();
    std::fs::write(
        movie_dir.join("ABP-525.nfo"),
        r#"
        <movie>
          <title>Local wrong nfo</title>
          <num>ABS-204</num>
          <plot>metadata belongs to another code</plot>
        </movie>
        "#,
    )
    .unwrap();

    let engine = IngestEngine::new(FailingProvider);
    let scanned: Vec<IngestItem> = Scanner::scan_sources(&[source.clone()])
        .unwrap()
        .into_iter()
        .map(|item| engine.decide(item))
        .collect();

    assert_eq!(scanned.len(), 1);
    assert_eq!(scanned[0].normalized_code, Some("ABP-525".to_string()));
    assert_eq!(scanned[0].decision, IngestDecision::NeedsReview);
    assert!(scanned[0].review_reasons.contains(&ReviewReason::CodeConflict));

    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let job_id = repo.create_ingest_job(&[source.clone()], &scanned).unwrap();
    let item_id = repo.list_ingest_items(job_id).unwrap()[0].id.unwrap();
    let persisted_conflict_item = repo.list_ingest_items(job_id).unwrap().remove(0);
    let persisted_conflict = persisted_conflict_item.code_conflict.as_ref().unwrap();
    assert_eq!(persisted_conflict.path_code, "ABP-525");
    assert_eq!(persisted_conflict.nfo_code, "ABS-204");

    let work_id = repo
        .resolve_ingest_item(item_id, Some("ABP-525".to_string()))
        .unwrap();
    let stored_item = repo.list_ingest_items(job_id).unwrap().remove(0);
    let stored_job = repo.get_ingest_job(job_id).unwrap().unwrap();
    let versions = repo.list_file_versions_for_work(work_id).unwrap();
    let plan = ArchivePlanner::new(archive_root.clone())
        .preview(&repo.list_archive_candidate_items_by_ids(&[item_id]).unwrap())
        .unwrap();

    assert_eq!(stored_item.decision, IngestDecision::AutoArchive);
    assert!(stored_item.review_reasons.is_empty());
    assert!(stored_item.code_conflict.is_none());
    assert_eq!(stored_item.candidate_work_id, Some(work_id));
    assert_eq!(stored_job.auto_count, 1);
    assert_eq!(stored_job.review_count, 0);
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].normalized_file_name, Some("ABP-525.mp4".to_string()));
    assert_eq!(plan.conflicts.len(), 0);
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].from_path, video_path);
    assert_eq!(plan.actions[0].to_path, archive_root.join("ABP-525").join("ABP-525.mp4"));
}

#[test]
fn ingest_engine_preserves_duplicate_candidates_without_auto_archive() {
    let engine = IngestEngine::new(ExampleProvider);
    let mut item = ingest_item("ABP-525", 0.95);
    item.decision = IngestDecision::DuplicateCandidate;
    item.review_reasons = vec![ReviewReason::DuplicateFile];
    item.file_hash = Some("same-hash".to_string());

    let decided = engine.decide(item);

    assert_eq!(decided.decision, IngestDecision::DuplicateCandidate);
    assert!(decided.review_reasons.contains(&ReviewReason::DuplicateFile));
    assert!(decided.metadata.is_none());
}

#[test]
fn archive_plan_uses_code_directory_and_normalized_file_name() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let archive = temp.path().join("archive");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&archive).unwrap();
    let source_file = source.join("ABP001.mp4");
    std::fs::write(&source_file, b"video").unwrap();
    let mut item = ingest_item("ABP-001", 0.98);
    item.decision = IngestDecision::AutoArchive;
    item.path = source_file;
    let planner = ArchivePlanner::new(archive.clone());

    let plan = planner.preview(&[item]).unwrap();

    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].to_path, archive.join("ABP-001").join("ABP-001.mp4"));
    assert_eq!(plan.actions[0].original_file_name, "ABP001.mp4");
    assert!(plan.conflicts.is_empty());
}

#[test]
fn archive_plan_omits_review_items() {
    let item = ingest_item("ABP-001", 0.62);
    let planner = ArchivePlanner::new(PathBuf::from("H:/Archive"));

    let plan = planner.preview(&[item]).unwrap();

    assert!(plan.actions.is_empty());
}

#[test]
fn archive_plan_reports_missing_source_file_before_execution() {
    let temp = tempfile::tempdir().unwrap();
    let missing_source = temp.path().join("source").join("ABP-001.mp4");
    let mut item = ingest_item("ABP-001", 0.98);
    item.decision = IngestDecision::AutoArchive;
    item.path = missing_source.clone();
    item.file_name = "ABP-001.mp4".to_string();

    let plan = ArchivePlanner::new(temp.path().join("archive"))
        .preview(&[item])
        .unwrap();

    assert!(plan.actions.is_empty());
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].path, missing_source);
    assert_eq!(plan.conflicts[0].reason, ReviewReason::MoveFailed);
    assert!(plan.conflicts[0].message.contains("source file does not exist"));
}

#[test]
fn phase_f_dry_run_summary_counts_ingest_and_preview_state() {
    let job = IngestJobSummary {
        id: 42,
        status: "completed".to_string(),
        total_items: 3,
        auto_count: 1,
        review_count: 2,
        failed_count: 0,
    };
    let mut auto = ingest_item("ABP-001", 0.98);
    auto.decision = IngestDecision::AutoArchive;
    auto.duration_seconds = Some(120);
    let mut duplicate = ingest_item("ABP-001", 0.98);
    duplicate.decision = IngestDecision::DuplicateCandidate;
    duplicate.review_reasons = vec![ReviewReason::DuplicateFile];
    let mut missing_code = ingest_item("", 0.2);
    missing_code.normalized_code = None;
    missing_code.review_reasons = vec![ReviewReason::MissingCode, ReviewReason::LowConfidence];
    let plan = ArchivePlan {
        id: None,
        actions: vec![ArchiveAction {
            item_id: Some(1),
            work_code: "ABP-001".to_string(),
            from_path: PathBuf::from("H:/inbox/ABP-001.mp4"),
            to_path: PathBuf::from("H:/archive/ABP-001/ABP-001.mp4"),
            original_file_name: "ABP-001.mp4".to_string(),
            normalized_file_name: "ABP-001.mp4".to_string(),
        }],
        conflicts: vec![ArchiveConflict {
            item_id: Some(2),
            path: PathBuf::from("H:/inbox/ABP-001-copy.mp4"),
            reason: ReviewReason::MoveFailed,
            message: "source file does not exist".to_string(),
        }],
    };

    let summary = summarize_phase_f_dry_run(&job, &[auto, duplicate, missing_code], &plan);

    assert_eq!(summary.job_id, 42);
    assert_eq!(summary.items_with_code, 2);
    assert_eq!(summary.items_with_media_info, 1);
    assert_eq!(summary.duplicate_count, 1);
    assert_eq!(summary.archive_actions, 1);
    assert_eq!(summary.archive_conflicts, 1);
    assert_eq!(
        format_count_pairs(&summary.decisions),
        "AutoArchive:1,DuplicateCandidate:1,NeedsReview:1"
    );
    assert_eq!(
        format_count_pairs(&summary.review_reasons),
        "DuplicateFile:1,LowConfidence:1,MissingCode:1"
    );
}

#[test]
fn phase_f_dry_run_report_preserves_item_and_plan_detail() {
    let job = IngestJobSummary {
        id: 7,
        status: "completed".to_string(),
        total_items: 1,
        auto_count: 1,
        review_count: 0,
        failed_count: 0,
    };
    let mut item = ingest_item("ABP-123", 0.98);
    item.decision = IngestDecision::AutoArchive;
    item.duration_seconds = Some(90);
    item.codec = Some("h264".to_string());
    item.metadata = Some(ProviderMetadata {
        provider: "local".to_string(),
        title_zh: Some("sample".to_string()),
        original_title: None,
        aliases: vec![],
        summary: Some("summary".to_string()),
        cover_url: Some("H:/covers/ABP-123.jpg".to_string()),
        release_date: None,
        confidence: 0.98,
            actors: vec![],
            genres: vec![],
            studio: None,
            director: None,
    });
    let plan = ArchivePlan {
        id: Some(1),
        actions: vec![ArchiveAction {
            item_id: Some(3),
            work_code: "ABP-123".to_string(),
            from_path: PathBuf::from("H:/inbox/ABP-123.mp4"),
            to_path: PathBuf::from("H:/archive/ABP-123/ABP-123.mp4"),
            original_file_name: "ABP-123.mp4".to_string(),
            normalized_file_name: "ABP-123.mp4".to_string(),
        }],
        conflicts: vec![],
    };

    let report = build_phase_f_dry_run_report(
        PathBuf::from("C:/temp/library.sqlite"),
        "disabled",
        1,
        &job,
        &[item.clone()],
        &plan,
    );

    assert_eq!(report.provider, "disabled");
    assert_eq!(report.source_root_count, 1);
    assert!(report.no_files_moved);
    assert_eq!(report.summary.job_id, 7);
    assert_eq!(report.items.len(), 1);
    assert_eq!(report.items[0].normalized_code.as_deref(), Some("ABP-123"));
    assert_eq!(report.items[0].codec.as_deref(), Some("h264"));
    assert_eq!(
        report.items[0].metadata.as_ref().and_then(|metadata| metadata.title_zh.as_deref()),
        Some("sample")
    );
    assert_eq!(report.archive_plan.actions.len(), 1);
    assert_eq!(report.archive_plan.actions[0].work_code, "ABP-123");
}

#[test]
fn archive_plan_uses_next_version_suffix_when_destination_exists() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let archive_root = temp.path().join("archive");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(archive_root.join("ABP-001")).unwrap();
    let source_file = source.join("ABP-001.mp4");
    std::fs::write(&source_file, b"video").unwrap();
    std::fs::write(archive_root.join("ABP-001").join("ABP-001.mp4"), b"existing").unwrap();
    let mut item = ingest_item("ABP-001", 0.98);
    item.decision = IngestDecision::AutoArchive;
    item.path = source_file;

    let plan = ArchivePlanner::new(archive_root).preview(&[item]).unwrap();

    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].normalized_file_name, "ABP-001-v2.mp4");
    assert!(plan.actions[0].to_path.ends_with("ABP-001-v2.mp4"));
    assert!(plan.conflicts.is_empty());
}

#[test]
fn archive_action_validation_rejects_paths_outside_configured_roots() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let archive = temp.path().join("archive");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&archive).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let outside_file = outside.join("ABP001.mp4");
    std::fs::write(&outside_file, b"video").unwrap();
    let action = ArchiveAction {
        item_id: Some(1),
        work_code: "ABP-001".to_string(),
        from_path: outside_file,
        to_path: archive.join("ABP-001").join("ABP-001.mp4"),
        original_file_name: "ABP001.mp4".to_string(),
        normalized_file_name: "ABP-001.mp4".to_string(),
    };

    let result = validate_archive_action_paths(&action, &[source], &archive);

    assert!(result.is_err());
}

#[test]
fn system_open_helpers_require_existing_paths_and_resolve_parent_directory() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("ABP-525.mp4");
    std::fs::write(&source, b"video").unwrap();

    assert_eq!(require_existing_path(&source).unwrap(), source);
    assert_eq!(existing_file_parent_directory(&source).unwrap(), temp.path().to_path_buf());
    assert!(require_existing_path(&temp.path().join("missing.mp4")).is_err());
    assert!(existing_file_parent_directory(&temp.path().join("missing.mp4")).is_err());
}

#[test]
fn normalized_file_name_adds_version_index_for_multiple_versions() {
    assert_eq!(
        normalized_file_name("ABP-001", PathBuf::from("H:/inbox/source.mkv").as_path(), 2),
        "ABP-001-v2.mkv"
    );
}

#[test]
fn repository_migrates_and_persists_work_profile() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();

    let work = Work {
        id: None,
        normalized_code: Some("ABP-001".to_string()),
        source_code: None,
        code_kind: CodeKind::Standard,
        outline: None,
        poster_path: None,
        thumb_path: None,
        fanart_path: None,
        screenshot_path: None,
        gif_path: None,
        sets: vec![],
        rating_value: None,
        rating_max: None,
        rating_votes: None,
        criticrating: None,
        label: None,
        runtime_minutes: None,
        year: None,
        website: None,
        mpaa: None,
        has_video: true,
        ratings: vec![],
        title_zh: Some("中文标题".to_string()),
        original_title: Some("Original".to_string()),
        aliases: vec!["ABP001".to_string()],
        summary: Some("summary".to_string()),
        cover_path: None,
        tags: vec!["tag-a".to_string()],
        lists: vec!["favorites".to_string()],
        rating: Some(8),
        watch_status: WatchStatus::Favorite,
        genres: vec![],
        studio: None,
        director: None,
        release_date: None,
    };

    let id = repo.upsert_work(&work).unwrap();
    let stored = repo.get_work_by_code("ABP-001").unwrap().unwrap();

    assert!(id > 0);
    assert_eq!(stored.title_zh, Some("中文标题".to_string()));
    assert_eq!(stored.tags, vec!["tag-a".to_string()]);
    assert_eq!(stored.watch_status, WatchStatus::Favorite);
}

#[test]
fn repository_lists_works_for_review_queue_merge_selection() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    repo.upsert_work(&Work {
        id: None,
        normalized_code: Some("ABP-525".to_string()),
        source_code: None,
        code_kind: CodeKind::Standard,
        outline: None,
        poster_path: None,
        thumb_path: None,
        fanart_path: None,
        screenshot_path: None,
        gif_path: None,
        sets: vec![],
        rating_value: None,
        rating_max: None,
        rating_votes: None,
        criticrating: None,
        label: None,
        runtime_minutes: None,
        year: None,
        website: None,
        mpaa: None,
        has_video: true,
        ratings: vec![],
        title_zh: Some("已有作品 A".to_string()),
        original_title: None,
        aliases: vec![],
        summary: None,
        cover_path: None,
        tags: vec![],
        lists: vec![],
        rating: None,
        watch_status: WatchStatus::Unwatched,
        genres: vec![],
        studio: None,
        director: None,
        release_date: None,
    })
    .unwrap();
    repo.upsert_work(&Work {
        id: None,
        normalized_code: Some("ABS-204".to_string()),
        source_code: None,
        code_kind: CodeKind::Standard,
        outline: None,
        poster_path: None,
        thumb_path: None,
        fanart_path: None,
        screenshot_path: None,
        gif_path: None,
        sets: vec![],
        rating_value: None,
        rating_max: None,
        rating_votes: None,
        criticrating: None,
        label: None,
        runtime_minutes: None,
        year: None,
        website: None,
        mpaa: None,
        has_video: true,
        ratings: vec![],
        title_zh: Some("已有作品 B".to_string()),
        original_title: None,
        aliases: vec![],
        summary: None,
        cover_path: None,
        tags: vec![],
        lists: vec![],
        rating: None,
        watch_status: WatchStatus::Favorite,
        genres: vec![],
        studio: None,
        director: None,
        release_date: None,
    })
    .unwrap();

    let works = repo.list_works().unwrap();

    assert_eq!(works.len(), 2);
    assert_eq!(works[0].normalized_code.as_deref(), Some("ABP-525"));
    assert_eq!(works[0].title_zh, Some("已有作品 A".to_string()));
    assert_eq!(works[1].normalized_code.as_deref(), Some("ABS-204"));
}

#[test]
fn repository_updates_work_profile_fields_without_replacing_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let work_id = repo
        .upsert_work(&Work {
            id: None,
            normalized_code: Some("ABP-525".to_string()),
            title_zh: Some("Existing Title".to_string()),
            original_title: Some("Original".to_string()),
            aliases: vec!["ABP525".to_string()],
            summary: Some("summary".to_string()),
            source_code: None,
            code_kind: CodeKind::Standard,
            outline: None,
            poster_path: None,
            thumb_path: None,
            fanart_path: None,
        screenshot_path: None,
        gif_path: None,
            sets: vec![],
            rating_value: None,
            rating_max: None,
            rating_votes: None,
            criticrating: None,
            label: None,
            runtime_minutes: None,
            year: None,
            website: None,
            mpaa: None,
            has_video: true,
            ratings: vec![],
            cover_path: None,
            tags: vec![],
            lists: vec![],
            rating: None,
            watch_status: WatchStatus::Unwatched,
        genres: vec![],
        studio: None,
        director: None,
        release_date: None,
        })
        .unwrap();

    let updated = repo
        .update_work_profile(
            work_id,
            vec!["收藏".to_string(), "高清".to_string()],
            vec!["待看".to_string()],
            Some(9),
            WatchStatus::Favorite,
        )
        .unwrap();
    let stored = repo.get_work_by_code("ABP-525").unwrap().unwrap();

    assert_eq!(updated.tags, vec!["收藏", "高清"]);
    assert_eq!(updated.lists, vec!["待看"]);
    assert_eq!(updated.rating, Some(9));
    assert_eq!(updated.watch_status, WatchStatus::Favorite);
    assert_eq!(stored.title_zh, Some("Existing Title".to_string()));
    assert_eq!(stored.original_title, Some("Original".to_string()));
    assert_eq!(stored.tags, vec!["收藏", "高清"]);
    assert_eq!(stored.lists, vec!["待看"]);
}

#[test]
fn repository_upsert_work_does_not_replace_user_profile_fields() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let work_id = repo
        .upsert_work(&Work {
            id: None,
            normalized_code: Some("ABP-525".to_string()),
            title_zh: Some("Existing Title".to_string()),
            original_title: Some("Original".to_string()),
            aliases: vec!["ABP525".to_string()],
            summary: Some("old summary".to_string()),
            source_code: None,
            code_kind: CodeKind::Standard,
            outline: None,
            poster_path: None,
            thumb_path: None,
            fanart_path: None,
        screenshot_path: None,
        gif_path: None,
            sets: vec![],
            rating_value: None,
            rating_max: None,
            rating_votes: None,
            criticrating: None,
            label: None,
            runtime_minutes: None,
            year: None,
            website: None,
            mpaa: None,
            has_video: true,
            ratings: vec![],
            cover_path: None,
            tags: vec!["收藏".to_string(), "高清".to_string()],
            lists: vec!["待看".to_string()],
            rating: Some(9),
            watch_status: WatchStatus::Favorite,
        genres: vec![],
        studio: None,
        director: None,
        release_date: None,
        })
        .unwrap();

    let second_work_id = repo
        .upsert_work(&Work {
            id: None,
            normalized_code: Some("ABP-525".to_string()),
            title_zh: Some("Provider Title".to_string()),
            source_code: None,
            code_kind: CodeKind::Standard,
            outline: None,
            poster_path: None,
            thumb_path: None,
            fanart_path: None,
        screenshot_path: None,
        gif_path: None,
            sets: vec![],
            rating_value: None,
            rating_max: None,
            rating_votes: None,
            criticrating: None,
            label: None,
            runtime_minutes: None,
            year: None,
            website: None,
            mpaa: None,
            has_video: true,
            ratings: vec![],
            original_title: Some("Provider Original".to_string()),
            aliases: vec!["Provider Alias".to_string()],
            summary: Some("new summary".to_string()),
            cover_path: Some(PathBuf::from("H:/covers/ABP-525.jpg")),
            tags: vec![],
            lists: vec![],
            rating: None,
            watch_status: WatchStatus::Unwatched,
        genres: vec![],
        studio: None,
        director: None,
        release_date: None,
        })
        .unwrap();
    let stored = repo.get_work_by_code("ABP-525").unwrap().unwrap();

    assert_eq!(second_work_id, work_id);
    assert_eq!(stored.title_zh, Some("Existing Title".to_string()));
    assert_eq!(stored.original_title, Some("Original".to_string()));
    assert_eq!(stored.aliases, vec!["ABP525".to_string()]);
    assert_eq!(stored.summary, Some("old summary".to_string()));
    assert_eq!(stored.cover_path, Some(PathBuf::from("H:/covers/ABP-525.jpg")));
    assert_eq!(stored.tags, vec!["收藏", "高清"]);
    assert_eq!(stored.lists, vec!["待看"]);
    assert_eq!(stored.rating, Some(9));
    assert_eq!(stored.watch_status, WatchStatus::Favorite);
}

#[test]
fn repository_persists_source_roots_and_archive_root_settings() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    {
        let repo = Repository::open(&db_path).unwrap();
        repo.migrate().unwrap();
        repo.set_source_roots(&[
            PathBuf::from("H:/Inbox"),
            PathBuf::from("H:/Downloads/A"),
        ])
        .unwrap();
        repo.set_archive_root(&PathBuf::from("H:/Archive")).unwrap();
    }

    let reopened = Repository::open(&db_path).unwrap();
    reopened.migrate().unwrap();

    assert_eq!(
        reopened.get_source_roots().unwrap(),
        vec![PathBuf::from("H:/Inbox"), PathBuf::from("H:/Downloads/A")]
    );
    assert_eq!(
        reopened.get_archive_root().unwrap(),
        Some(PathBuf::from("H:/Archive"))
    );
}

#[test]
fn repository_defaults_metadata_provider_to_disabled_and_persists_toggle() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();

    assert!(!repo.get_metadata_provider_enabled().unwrap());

    repo.set_metadata_provider_enabled(true).unwrap();
    assert!(repo.get_metadata_provider_enabled().unwrap());

    repo.set_metadata_provider_enabled(false).unwrap();
    assert!(!repo.get_metadata_provider_enabled().unwrap());
}

#[test]
fn repository_persists_ingest_job_and_items() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut item = ingest_item("ABP-001", 0.95);
    item.decision = IngestDecision::AutoArchive;

    let job_id = repo
        .create_ingest_job(&[PathBuf::from("H:/inbox")], &[item.clone()])
        .unwrap();
    let stored_job = repo.get_ingest_job(job_id).unwrap().unwrap();
    let stored_items = repo.list_ingest_items(job_id).unwrap();

    assert_eq!(stored_job.id, job_id);
    assert_eq!(stored_job.total_items, 1);
    assert_eq!(stored_job.auto_count, 1);
    assert_eq!(stored_items.len(), 1);
    assert_eq!(stored_items[0].job_id, Some(job_id));
    assert_eq!(stored_items[0].normalized_code, Some("ABP-001".to_string()));
    assert_eq!(stored_items[0].decision, IngestDecision::AutoArchive);
}

#[test]
fn repository_auto_promotes_auto_archive_items_into_work_versions() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut first = ingest_item("ABP-525", 0.95);
    first.decision = IngestDecision::AutoArchive;
    first.review_reasons = vec![];
    first.path = PathBuf::from("H:/inbox/ABP-525.mp4");
    first.file_name = "ABP-525.mp4".to_string();
    let mut second = ingest_item("ABP-525", 0.96);
    second.decision = IngestDecision::AutoArchive;
    second.review_reasons = vec![];
    second.path = PathBuf::from("H:/inbox/ABP-525-CD2.mp4");
    second.file_name = "ABP-525-CD2.mp4".to_string();

    let job_id = repo
        .create_ingest_job(&[PathBuf::from("H:/inbox")], &[first, second])
        .unwrap();
    let stored_items = repo.list_ingest_items(job_id).unwrap();
    let work = repo.get_work_by_code("ABP-525").unwrap().unwrap();
    let versions = repo.list_file_versions_for_work(work.id.unwrap()).unwrap();

    assert!(stored_items.iter().all(|item| item.candidate_work_id == work.id));
    assert!(stored_items.iter().all(|item| item.decision == IngestDecision::AutoArchive));
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].normalized_file_name, Some("ABP-525.mp4".to_string()));
    assert_eq!(versions[1].normalized_file_name, Some("ABP-525-v2.mp4".to_string()));
}

#[test]
fn repository_returns_latest_ingest_job_for_startup_restore() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut first = ingest_item("ABP-525", 0.95);
    first.file_name = "ABP-525.mp4".to_string();
    first.path = PathBuf::from("H:/inbox/ABP-525.mp4");
    let mut second = ingest_item("ABS-204", 0.95);
    second.file_name = "ABS-204.mp4".to_string();
    second.path = PathBuf::from("H:/inbox/ABS-204.mp4");
    let first_job_id = repo
        .create_ingest_job(&[PathBuf::from("H:/inbox")], &[first])
        .unwrap();
    let second_job_id = repo
        .create_ingest_job(&[PathBuf::from("H:/inbox")], &[second])
        .unwrap();

    let latest = repo.get_latest_ingest_job().unwrap().unwrap();

    assert!(second_job_id > first_job_id);
    assert_eq!(latest.id, second_job_id);
    assert_eq!(latest.total_items, 1);
}

#[test]
fn repository_prefers_latest_non_empty_ingest_job_for_startup_restore() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut item = ingest_item("ABP-525", 0.95);
    item.file_name = "ABP-525.mp4".to_string();
    item.path = PathBuf::from("H:/inbox/ABP-525.mp4");
    let non_empty_job_id = repo
        .create_ingest_job(&[PathBuf::from("H:/inbox")], &[item])
        .unwrap();
    let empty_job_id = repo
        .create_ingest_job(&[PathBuf::from("H:/empty")], &[])
        .unwrap();

    let latest = repo.get_latest_ingest_job().unwrap().unwrap();

    assert!(empty_job_id > non_empty_job_id);
    assert_eq!(latest.id, non_empty_job_id);
    assert_eq!(latest.total_items, 1);
}

#[test]
fn repository_persists_ingest_item_local_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut item = ingest_item("ABP-525", 0.95);
    item.metadata = Some(ProviderMetadata {
        provider: "local".to_string(),
        title_zh: Some("本地中文标题".to_string()),
        original_title: Some("Local Original Title".to_string()),
        aliases: vec![],
        summary: Some("本地简介".to_string()),
        cover_url: Some("H:/inbox/ABP-525/cover.jpg".to_string()),
        release_date: Some("2024-01-02".to_string()),
        confidence: 0.95,
            actors: vec![],
            genres: vec![],
            studio: None,
            director: None,
    });

    let job_id = repo.create_ingest_job(&[PathBuf::from("H:/inbox")], &[item]).unwrap();
    let stored = repo.list_ingest_items(job_id).unwrap().remove(0);
    let metadata = stored.metadata.unwrap();

    assert_eq!(metadata.provider, "local");
    assert_eq!(metadata.title_zh, Some("本地中文标题".to_string()));
    assert_eq!(metadata.cover_url, Some("H:/inbox/ABP-525/cover.jpg".to_string()));
    assert_eq!(metadata.release_date, Some("2024-01-02".to_string()));
}

#[test]
fn scanned_items_persist_with_file_name_codes_in_repository() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp
        .path()
        .join("CineMingle-1.3.0")
        .join("JAV_output")
        .join("Actress");
    for relative_path in [
        PathBuf::from("ABP-525").join("ABP-525.mp4"),
        PathBuf::from("ABP-627").join("ABP-627.mp4"),
        PathBuf::from("ABS-204").join("ABS-204.mp4"),
    ] {
        let file_path = source.join(relative_path);
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, file_path.to_string_lossy().as_bytes()).unwrap();
    }
    std::fs::write(source.join("cover.jpg"), b"image").unwrap();
    let repo = Repository::open(&temp.path().join("library.sqlite")).unwrap();
    repo.migrate().unwrap();
    let engine = IngestEngine::new(ExampleProvider);

    let items: Vec<IngestItem> = Scanner::scan_sources(&[source.clone()])
        .unwrap()
        .into_iter()
        .map(|item| engine.decide(item))
        .collect();
    let job_id = repo.create_ingest_job(&[source], &items).unwrap();
    let stored_job = repo.get_ingest_job(job_id).unwrap().unwrap();
    let mut stored_codes: Vec<String> = repo
        .list_ingest_items(job_id)
        .unwrap()
        .into_iter()
        .map(|item| {
            assert_eq!(item.job_id, Some(job_id));
            assert_eq!(item.decision, IngestDecision::NeedsReview);
            assert!(item.review_reasons.contains(&ReviewReason::LowConfidence));
            item.normalized_code.unwrap()
        })
        .collect();
    stored_codes.sort();

    assert_eq!(stored_job.total_items, 3);
    assert_eq!(stored_job.review_count, 3);
    assert_eq!(stored_codes, vec!["ABP-525", "ABP-627", "ABS-204"]);
}

#[test]
fn repository_persists_duplicate_candidate_file_hashes() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("inbox");
    std::fs::create_dir_all(source.join("ABP-525")).unwrap();
    std::fs::create_dir_all(source.join("ABP-525-copy")).unwrap();
    std::fs::write(source.join("ABP-525").join("ABP-525.mp4"), b"same video bytes").unwrap();
    std::fs::write(
        source.join("ABP-525-copy").join("ABP-525-copy.mp4"),
        b"same video bytes",
    )
    .unwrap();
    let repo = Repository::open(&temp.path().join("library.sqlite")).unwrap();
    repo.migrate().unwrap();

    let items = Scanner::scan_sources(&[source.clone()]).unwrap();
    let job_id = repo.create_ingest_job(&[source], &items).unwrap();
    let stored = repo.list_ingest_items(job_id).unwrap();

    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].file_hash, stored[1].file_hash);
    assert!(stored.iter().all(|item| item.decision == IngestDecision::DuplicateCandidate));
    assert!(stored
        .iter()
        .all(|item| item.review_reasons.contains(&ReviewReason::DuplicateFile)));
    assert_eq!(repo.get_ingest_job(job_id).unwrap().unwrap().review_count, 2);
}

#[test]
fn repository_marks_duplicate_candidates_as_ignored_after_review() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut duplicate = ingest_item("ABP-525", 0.8);
    duplicate.decision = IngestDecision::DuplicateCandidate;
    duplicate.review_reasons = vec![ReviewReason::DuplicateFile];
    duplicate.file_hash = Some("same-hash".to_string());
    let job_id = repo
        .create_ingest_job(&[duplicate.source_root.clone()], &[duplicate])
        .unwrap();
    let item_id = repo.list_ingest_items(job_id).unwrap()[0].id.unwrap();

    let updated = repo.ignore_duplicate_items(&[item_id]).unwrap();
    let stored_item = repo.list_ingest_items(job_id).unwrap().remove(0);
    let stored_job = repo.get_ingest_job(job_id).unwrap().unwrap();

    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].decision, IngestDecision::Ignored);
    assert!(updated[0].review_reasons.is_empty());
    assert_eq!(stored_item.decision, IngestDecision::Ignored);
    assert!(stored_item.review_reasons.is_empty());
    assert_eq!(stored_job.review_count, 0);
    assert_eq!(stored_job.auto_count, 0);
    assert_eq!(stored_job.failed_count, 0);
}

#[test]
fn repository_filters_ingest_items_by_decision_reason_and_code_presence() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut auto = ingest_item("ABP-525", 0.95);
    auto.decision = IngestDecision::AutoArchive;
    auto.review_reasons = vec![];
    let mut missing_code = ingest_item("", 0.2);
    missing_code.normalized_code = None;
    missing_code.file_name = "unknown.mp4".to_string();
    missing_code.path = PathBuf::from("H:/inbox/unknown.mp4");
    missing_code.review_reasons = vec![ReviewReason::MissingCode];
    let mut low_confidence = ingest_item("ABS-204", 0.5);
    low_confidence.file_name = "ABS-204.mp4".to_string();
    low_confidence.path = PathBuf::from("H:/inbox/ABS-204.mp4");
    low_confidence.review_reasons = vec![ReviewReason::LowConfidence];
    let job_id = repo
        .create_ingest_job(&[PathBuf::from("H:/inbox")], &[auto, missing_code, low_confidence])
        .unwrap();

    let review_items = repo
        .list_ingest_items_filtered(
            job_id,
            &IngestItemFilters {
                decision: Some(IngestDecision::NeedsReview),
                review_reason: Some(ReviewReason::LowConfidence),
                has_code: Some(true),
            },
        )
        .unwrap();
    let uncoded_items = repo
        .list_ingest_items_filtered(
            job_id,
            &IngestItemFilters {
                decision: None,
                review_reason: None,
                has_code: Some(false),
            },
        )
        .unwrap();

    assert_eq!(review_items.len(), 1);
    assert_eq!(review_items[0].normalized_code, Some("ABS-204".to_string()));
    assert_eq!(uncoded_items.len(), 1);
    assert_eq!(uncoded_items[0].file_name, "unknown.mp4");
}

#[test]
fn repository_lists_ingest_items_by_persisted_ids_for_archive_preview() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut auto = ingest_item("ABP-525", 0.95);
    auto.decision = IngestDecision::AutoArchive;
    auto.review_reasons = vec![];
    auto.file_name = "ABP-525.mp4".to_string();
    auto.path = PathBuf::from("H:/inbox/ABP-525.mp4");
    let mut review = ingest_item("ABS-204", 0.5);
    review.file_name = "ABS-204.mp4".to_string();
    review.path = PathBuf::from("H:/inbox/ABS-204.mp4");
    review.review_reasons = vec![ReviewReason::LowConfidence];
    let job_id = repo
        .create_ingest_job(&[PathBuf::from("H:/inbox")], &[auto, review])
        .unwrap();
    let ids: Vec<i64> = repo
        .list_ingest_items(job_id)
        .unwrap()
        .into_iter()
        .map(|item| item.id.unwrap())
        .collect();

    let selected = repo.list_ingest_items_by_ids(&ids).unwrap();

    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0].normalized_code, Some("ABP-525".to_string()));
    assert_eq!(selected[1].normalized_code, Some("ABS-204".to_string()));
}

#[test]
fn repository_resolves_ingest_item_into_work_and_file_version() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut item = ingest_item("ABP-525", 0.82);
    item.path = PathBuf::from("H:/CineMingle/JAV_output/Actress/ABP-525/ABP-525.mp4");
    item.source_root = PathBuf::from("H:/CineMingle/JAV_output/Actress");
    item.file_name = "ABP-525.mp4".to_string();
    item.duration_seconds = Some(3661);
    item.width = Some(1920);
    item.height = Some(1080);
    item.codec = Some("hevc".to_string());
    item.metadata = Some(ProviderMetadata {
        provider: "local".to_string(),
        title_zh: Some("本地标题".to_string()),
        original_title: Some("Local Title".to_string()),
        aliases: vec!["ABP525".to_string()],
        summary: Some("本地简介".to_string()),
        cover_url: Some("H:/CineMingle/JAV_output/Actress/ABP-525/cover.jpg".to_string()),
        release_date: None,
        confidence: 0.95,
            actors: vec![],
            genres: vec![],
            studio: None,
            director: None,
    });
    item.review_reasons = vec![ReviewReason::LowConfidence];
    let job_id = repo
        .create_ingest_job(&[item.source_root.clone()], &[item])
        .unwrap();
    let item_id = repo.list_ingest_items(job_id).unwrap()[0].id.unwrap();

    let work_id = repo
        .resolve_ingest_item(item_id, Some("abp525".to_string()))
        .unwrap();
    let stored_item = repo.list_ingest_items(job_id).unwrap().remove(0);
    let stored_job = repo.get_ingest_job(job_id).unwrap().unwrap();
    let stored_work = repo.get_work_by_code("ABP-525").unwrap().unwrap();
    let versions = repo.list_file_versions_for_work(work_id).unwrap();

    assert_eq!(stored_job.auto_count, 1);
    assert_eq!(stored_job.review_count, 0);
    assert_eq!(stored_item.normalized_code, Some("ABP-525".to_string()));
    assert_eq!(stored_item.decision, IngestDecision::AutoArchive);
    assert_eq!(stored_item.confidence, 1.0);
    assert!(stored_item.review_reasons.is_empty());
    assert_eq!(stored_item.candidate_work_id, Some(work_id));
    assert_eq!(stored_work.id, Some(work_id));
    assert_eq!(
        stored_work.cover_path,
        Some(PathBuf::from("H:/CineMingle/JAV_output/Actress/ABP-525/cover.jpg"))
    );
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].work_id, Some(work_id));
    assert_eq!(versions[0].original_file_name, "ABP-525.mp4");
    assert_eq!(versions[0].normalized_file_name, Some("ABP-525.mp4".to_string()));
    assert_eq!(versions[0].original_path, PathBuf::from("H:/CineMingle/JAV_output/Actress/ABP-525/ABP-525.mp4"));
    assert_eq!(versions[0].duration_seconds, Some(3661));
    assert_eq!(versions[0].width, Some(1920));
    assert_eq!(versions[0].height, Some(1080));
    assert_eq!(versions[0].codec, Some("hevc".to_string()));
}

#[test]
fn repository_resolves_ingest_item_into_existing_work_by_work_id() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let work_id = repo
        .upsert_work(&Work {
            id: None,
            normalized_code: Some("ABP-525".to_string()),
            title_zh: Some("已有作品".to_string()),
            source_code: None,
            code_kind: CodeKind::Standard,
            outline: None,
            poster_path: None,
            thumb_path: None,
            fanart_path: None,
        screenshot_path: None,
        gif_path: None,
            sets: vec![],
            rating_value: None,
            rating_max: None,
            rating_votes: None,
            criticrating: None,
            label: None,
            runtime_minutes: None,
            year: None,
            website: None,
            mpaa: None,
            has_video: true,
            ratings: vec![],
            original_title: None,
            aliases: vec![],
            summary: None,
            cover_path: None,
            tags: vec![],
            lists: vec![],
            rating: None,
            watch_status: WatchStatus::Unwatched,
        genres: vec![],
        studio: None,
        director: None,
        release_date: None,
        })
        .unwrap();
    let mut first = ingest_item("ABP-525", 0.95);
    first.path = PathBuf::from("H:/inbox/ABP-525.mp4");
    first.file_name = "ABP-525.mp4".to_string();
    let mut second = ingest_item("ABP-999", 0.62);
    second.path = PathBuf::from("H:/inbox/ABP-525-CD2.mp4");
    second.file_name = "ABP-525-CD2.mp4".to_string();
    second.review_reasons = vec![ReviewReason::LowConfidence, ReviewReason::CodeConflict];
    let job_id = repo
        .create_ingest_job(&[PathBuf::from("H:/inbox")], &[first, second])
        .unwrap();
    let items = repo.list_ingest_items(job_id).unwrap();
    let second_item_id = items[1].id.unwrap();
    repo.resolve_ingest_item(items[0].id.unwrap(), None).unwrap();

    let resolved_work_id = repo
        .resolve_ingest_item_to_work(second_item_id, work_id)
        .unwrap();
    let stored_item = repo
        .list_ingest_items(job_id)
        .unwrap()
        .into_iter()
        .find(|item| item.id == Some(second_item_id))
        .unwrap();
    let versions = repo.list_file_versions_for_work(work_id).unwrap();

    assert_eq!(resolved_work_id, work_id);
    assert_eq!(stored_item.normalized_code, Some("ABP-525".to_string()));
    assert_eq!(stored_item.candidate_work_id, Some(work_id));
    assert_eq!(stored_item.decision, IngestDecision::AutoArchive);
    assert!(stored_item.review_reasons.is_empty());
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].normalized_file_name, Some("ABP-525.mp4".to_string()));
    assert_eq!(versions[1].normalized_file_name, Some("ABP-525-v2.mp4".to_string()));
    assert_eq!(repo.get_ingest_job(job_id).unwrap().unwrap().auto_count, 2);
}

#[test]
fn repository_merges_existing_file_versions_into_target_work() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut target_item = ingest_item("ABP-525", 0.95);
    target_item.path = PathBuf::from("H:/inbox/ABP-525.mp4");
    target_item.file_name = "ABP-525.mp4".to_string();
    let mut source_item = ingest_item("ABS-204", 0.95);
    source_item.path = PathBuf::from("H:/inbox/ABS-204-alt.mkv");
    source_item.file_name = "ABS-204-alt.mkv".to_string();
    let job_id = repo
        .create_ingest_job(&[PathBuf::from("H:/inbox")], &[target_item, source_item])
        .unwrap();
    let items = repo.list_ingest_items(job_id).unwrap();
    let target_work_id = repo.resolve_ingest_item(items[0].id.unwrap(), None).unwrap();
    let source_work_id = repo.resolve_ingest_item(items[1].id.unwrap(), None).unwrap();
    let source_version_id = repo.list_file_versions_for_work(source_work_id).unwrap()[0].id.unwrap();

    repo.merge_file_versions_into_work(target_work_id, &[source_version_id])
        .unwrap();
    let target_versions = repo.list_file_versions_for_work(target_work_id).unwrap();
    let source_versions = repo.list_file_versions_for_work(source_work_id).unwrap();

    assert_eq!(target_versions.len(), 2);
    assert_eq!(source_versions.len(), 0);
    assert_eq!(target_versions[0].normalized_file_name, Some("ABP-525.mp4".to_string()));
    assert_eq!(target_versions[1].work_id, Some(target_work_id));
    assert_eq!(
        target_versions[1].normalized_file_name,
        Some("ABP-525-v2.mkv".to_string())
    );
}

#[test]
fn repository_retries_metadata_and_persists_updated_ingest_item() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut item = ingest_item("ABP-525", 0.95);
    item.review_reasons = vec![ReviewReason::ProviderFailed];
    item.decision = IngestDecision::NeedsReview;
    let job_id = repo
        .create_ingest_job(&[PathBuf::from("H:/inbox")], &[item])
        .unwrap();
    let item_id = repo.list_ingest_items(job_id).unwrap()[0].id.unwrap();
    let engine = IngestEngine::new(ExampleProvider);

    let updated = repo.retry_metadata_for_items(&engine, &[item_id]).unwrap();
    let stored = repo.list_ingest_items(job_id).unwrap().remove(0);
    let stored_job = repo.get_ingest_job(job_id).unwrap().unwrap();
    let work = repo.get_work_by_code("ABP-525").unwrap().unwrap();
    let versions = repo.list_file_versions_for_work(work.id.unwrap()).unwrap();

    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].candidate_work_id, work.id);
    assert_eq!(stored.decision, IngestDecision::AutoArchive);
    assert!(stored.review_reasons.is_empty());
    assert_eq!(stored.metadata.as_ref().unwrap().provider, "example");
    assert_eq!(stored.candidate_work_id, work.id);
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].normalized_file_name, Some("ABP-525.mp4".to_string()));
    assert_eq!(stored_job.auto_count, 1);
    assert_eq!(stored_job.review_count, 0);
}

#[test]
fn repository_records_archive_log_and_archived_file_version_path() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let source = temp.path().join("source");
    let archive = temp.path().join("archive");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&archive).unwrap();
    let source_file = source.join("ABP-525.mp4");
    let archived_file = archive.join("ABP-525").join("ABP-525.mp4");
    std::fs::write(&source_file, b"video").unwrap();
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut item = ingest_item("ABP-525", 0.82);
    item.path = source_file.clone();
    item.source_root = source.clone();
    item.file_name = "ABP-525.mp4".to_string();
    item.review_reasons = vec![ReviewReason::LowConfidence];
    let job_id = repo.create_ingest_job(&[source], &[item]).unwrap();
    let item_id = repo.list_ingest_items(job_id).unwrap()[0].id.unwrap();
    let work_id = repo.resolve_ingest_item(item_id, None).unwrap();

    let log_id = repo
        .record_archive_action(&ArchiveActionLog {
            id: None,
            item_id: Some(item_id),
            job_id: None,
            from_path: source_file,
            to_path: archived_file.clone(),
            status: "moved".to_string(),
            message: None,
            created_at: None,
        })
        .unwrap();
    let logs = repo.list_archive_action_logs().unwrap();
    let versions = repo.list_file_versions_for_work(work_id).unwrap();

    assert!(log_id > 0);
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].item_id, Some(item_id));
    assert_eq!(logs[0].job_id, Some(job_id));
    assert_eq!(logs[0].status, "moved");
    assert_eq!(logs[0].to_path, archived_file);
    assert!(logs[0].created_at.is_some());
    assert_eq!(versions[0].archived_path, Some(logs[0].to_path.clone()));
}

#[test]
fn repository_excludes_already_archived_items_from_archive_candidates() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let source = temp.path().join("source");
    let archive = temp.path().join("archive");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&archive).unwrap();
    let source_file = source.join("ABP-525.mp4");
    let archived_file = archive.join("ABP-525").join("ABP-525.mp4");
    std::fs::write(&source_file, b"video").unwrap();
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut item = ingest_item("ABP-525", 0.95);
    item.decision = IngestDecision::AutoArchive;
    item.review_reasons = vec![];
    item.path = source_file.clone();
    item.source_root = source;
    item.file_name = "ABP-525.mp4".to_string();
    let job_id = repo.create_ingest_job(&[item.source_root.clone()], &[item]).unwrap();
    let item_id = repo.list_ingest_items(job_id).unwrap()[0].id.unwrap();
    repo
        .record_archive_action(&ArchiveActionLog {
            id: None,
            item_id: Some(item_id),
            job_id: None,
            from_path: source_file,
            to_path: archived_file,
            status: "moved".to_string(),
            message: None,
            created_at: None,
        })
        .unwrap();

    let candidates = repo.list_archive_candidate_items_by_ids(&[item_id]).unwrap();

    assert!(candidates.is_empty());
}

#[test]
fn execute_archive_actions_moves_file_and_persists_log_and_archived_path() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let archive = temp.path().join("archive");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&archive).unwrap();
    let source_file = source.join("ABP-525.mp4");
    let archived_file = archive.join("ABP-525").join("ABP-525.mp4");
    std::fs::write(&source_file, b"video").unwrap();
    let repo = Repository::open(&temp.path().join("library.sqlite")).unwrap();
    repo.migrate().unwrap();
    let mut item = ingest_item("ABP-525", 0.82);
    item.source_root = source.clone();
    item.path = source_file.clone();
    item.file_name = "ABP-525.mp4".to_string();
    let job_id = repo.create_ingest_job(&[source.clone()], &[item]).unwrap();
    let item_id = repo.list_ingest_items(job_id).unwrap()[0].id.unwrap();
    let work_id = repo.resolve_ingest_item(item_id, None).unwrap();
    let action = ArchiveAction {
        item_id: Some(item_id),
        work_code: "ABP-525".to_string(),
        from_path: source_file.clone(),
        to_path: archived_file.clone(),
        original_file_name: "ABP-525.mp4".to_string(),
        normalized_file_name: "ABP-525.mp4".to_string(),
    };

    let logs = execute_archive_actions(&[action], &[source], &archive, Some(&repo)).unwrap();
    let versions = repo.list_file_versions_for_work(work_id).unwrap();
    let stored_logs = repo.list_archive_action_logs().unwrap();

    assert!(!source_file.exists());
    assert_eq!(std::fs::read(&archived_file).unwrap(), b"video");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, "moved");
    assert!(logs[0].id.is_some());
    assert_eq!(logs[0].job_id, Some(job_id));
    assert!(logs[0].created_at.is_some());
    assert_eq!(stored_logs.len(), 1);
    assert_eq!(stored_logs[0].to_path, archived_file);
    assert_eq!(versions[0].archived_path, Some(stored_logs[0].to_path.clone()));
}

#[test]
fn full_archive_pipeline_plans_executes_logs_and_excludes_archived() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source").join("IDBD-815");
    let archive = temp.path().join("archive");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&archive).unwrap();
    let video = source.join("IDBD-815-cd2.mp4");
    std::fs::write(&video, b"video-bytes").unwrap();
    std::fs::write(
        source.join("IDBD-815.nfo"),
        r#"<movie><num>IDBD-815</num><title>Sample Title</title><premiered>2020-02-13</premiered></movie>"#,
    )
    .unwrap();
    std::fs::write(source.join("IDBD-815.jpg"), b"cover").unwrap();
    let mut items = Scanner::scan_sources(&[temp.path().join("source")]).unwrap();
    assert_eq!(items.len(), 1);
    let engine = IngestEngine::new(media_manager::provider::DisabledProvider);
    items = items.into_iter().map(|i| engine.decide(i)).collect();
    assert_eq!(items[0].decision, IngestDecision::AutoArchive);
    assert_eq!(items[0].normalized_code.as_deref(), Some("IDBD-815"));
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let job_id = repo.create_ingest_job(&[temp.path().join("source")], &items).unwrap();
    let item_id = repo.list_ingest_items(job_id).unwrap()[0].id.unwrap();
    let work_id = repo.resolve_ingest_item(item_id, None).unwrap();
    let candidates = repo.list_archive_candidate_items_for_job(job_id).unwrap();
    assert_eq!(candidates.len(), 1);
    let plan = ArchivePlanner::new(archive.clone()).preview(&candidates).unwrap();
    assert!(plan.conflicts.is_empty());
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].normalized_file_name, "IDBD-815.mp4");
    let logs = execute_archive_actions(&plan.actions, &[temp.path().join("source")], &archive, Some(&repo)).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, "moved");
    let dest = plan.actions[0].to_path.clone();
    assert!(!video.exists());
    assert_eq!(std::fs::read(&dest).unwrap(), b"video-bytes");
    let stored_logs = repo.list_archive_action_logs().unwrap();
    assert_eq!(stored_logs.len(), 1);
    assert_eq!(stored_logs[0].to_path, dest);
    assert_eq!(stored_logs[0].status, "moved");
    let versions = repo.list_file_versions_for_work(work_id).unwrap();
    assert_eq!(versions[0].archived_path, Some(dest.clone()));
    let remaining = repo.list_archive_candidate_items_for_job(job_id).unwrap();
    assert!(remaining.is_empty(), "archived item must be excluded from future candidates");
}

#[test]
fn execute_archive_actions_marks_failed_moves_for_review() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let archive = temp.path().join("archive");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&archive).unwrap();
    let missing_source_file = source.join("ABP-525.mp4");
    let archived_file = archive.join("ABP-525").join("ABP-525.mp4");
    let repo = Repository::open(&temp.path().join("library.sqlite")).unwrap();
    repo.migrate().unwrap();
    let mut item = ingest_item("ABP-525", 0.82);
    item.source_root = source.clone();
    item.path = missing_source_file.clone();
    item.file_name = "ABP-525.mp4".to_string();
    let job_id = repo.create_ingest_job(&[source.clone()], &[item]).unwrap();
    let item_id = repo.list_ingest_items(job_id).unwrap()[0].id.unwrap();
    repo.resolve_ingest_item(item_id, None).unwrap();
    let action = ArchiveAction {
        item_id: Some(item_id),
        work_code: "ABP-525".to_string(),
        from_path: missing_source_file,
        to_path: archived_file,
        original_file_name: "ABP-525.mp4".to_string(),
        normalized_file_name: "ABP-525.mp4".to_string(),
    };

    let logs = execute_archive_actions(&[action], &[source], &archive, Some(&repo)).unwrap();
    let stored_item = repo.list_ingest_items(job_id).unwrap().remove(0);
    let stored_job = repo.get_ingest_job(job_id).unwrap().unwrap();

    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, "failed");
    assert!(logs[0].message.is_some());
    assert_eq!(stored_item.decision, IngestDecision::Failed);
    assert!(stored_item.review_reasons.contains(&ReviewReason::MoveFailed));
    assert_eq!(stored_job.auto_count, 0);
    assert_eq!(stored_job.failed_count, 1);
}

#[test]
fn repository_revalidates_move_failed_item_when_source_file_returns() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    std::fs::create_dir_all(&source).unwrap();
    let source_file = source.join("ABP-525.mp4");
    let repo = Repository::open(&temp.path().join("library.sqlite")).unwrap();
    repo.migrate().unwrap();
    let mut item = ingest_item("ABP-525", 0.95);
    item.source_root = source.clone();
    item.path = source_file.clone();
    item.file_name = "ABP-525.mp4".to_string();
    item.decision = IngestDecision::AutoArchive;
    item.review_reasons = vec![];
    let job_id = repo.create_ingest_job(&[source], &[item]).unwrap();
    let item_id = repo.list_ingest_items(job_id).unwrap()[0].id.unwrap();
    repo
        .record_archive_action(&ArchiveActionLog {
            id: None,
            item_id: Some(item_id),
            job_id: None,
            from_path: source_file.clone(),
            to_path: temp.path().join("archive").join("ABP-525").join("ABP-525.mp4"),
            status: "failed".to_string(),
            message: Some("missing source".to_string()),
            created_at: None,
        })
        .unwrap();
    assert_eq!(repo.get_ingest_job(job_id).unwrap().unwrap().failed_count, 1);
    std::fs::write(&source_file, b"video").unwrap();

    let updated = repo.revalidate_move_failed_items(&[item_id]).unwrap();
    let stored_item = repo.list_ingest_items(job_id).unwrap().remove(0);
    let stored_job = repo.get_ingest_job(job_id).unwrap().unwrap();

    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].decision, IngestDecision::AutoArchive);
    assert!(!updated[0].review_reasons.contains(&ReviewReason::MoveFailed));
    assert_eq!(stored_item.decision, IngestDecision::AutoArchive);
    assert!(stored_item.review_reasons.is_empty());
    assert_eq!(stored_job.auto_count, 1);
    assert_eq!(stored_job.failed_count, 0);
}

fn ingest_item(code: &str, confidence: f32) -> IngestItem {
    IngestItem {
        id: Some(1),
        job_id: Some(1),
        source_root: PathBuf::from("H:/inbox"),
        path: PathBuf::from("H:/inbox/ABP001.mp4"),
        file_name: "ABP001.mp4".to_string(),
        size_bytes: 1024,
        normalized_code: Some(code.to_string()),
        confidence,
        decision: IngestDecision::NeedsReview,
        review_reasons: vec![],
        code_conflict: None,
        metadata: None,
        candidate_work_id: None,
        duration_seconds: None,
        width: None,
        height: None,
        codec: None,
        file_hash: Some("hash".to_string()),
    }
}

#[test]
fn repository_deletes_duplicate_candidate_file_and_marks_item_ignored() {
    let temp = tempfile::tempdir().unwrap();
    let source_root = temp.path().join("inbox");
    fs::create_dir_all(&source_root).unwrap();
    let file_path = source_root.join("theme_video.mp4");
    fs::write(&file_path, b"fake ad video bytes").unwrap();
    assert!(file_path.exists());

    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut dup = ingest_item("ABP-525", 0.8);
    dup.source_root = source_root.clone();
    dup.path = file_path.clone();
    dup.file_name = "theme_video.mp4".to_string();
    dup.normalized_code = None;
    dup.decision = IngestDecision::DuplicateCandidate;
    dup.review_reasons = vec![ReviewReason::DuplicateFile, ReviewReason::MissingCode];
    let job_id = repo
        .create_ingest_job(&[source_root.clone()], &[dup])
        .unwrap();
    let item_id = repo.list_ingest_items(job_id).unwrap()[0].id.unwrap();

    let updated = repo.delete_items(&[item_id]).unwrap();

    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].decision, IngestDecision::Ignored);
    assert!(!file_path.exists(), "source file should be deleted from disk");
    let stored = repo.list_ingest_items(job_id).unwrap().remove(0);
    assert_eq!(stored.decision, IngestDecision::Ignored);
    let stored_job = repo.get_ingest_job(job_id).unwrap().unwrap();
    assert_eq!(stored_job.review_count, 0);
}

#[test]
fn repository_delete_refuses_non_deletable_decisions() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut auto = ingest_item("ABP-525", 0.95);
    auto.decision = IngestDecision::AutoArchive;
    auto.review_reasons = vec![];
    let job_id = repo
        .create_ingest_job(&[auto.source_root.clone()], &[auto])
        .unwrap();
    let item_id = repo.list_ingest_items(job_id).unwrap()[0].id.unwrap();

    // AutoArchive items must never be silently deleted - they hold real media.
    let result = repo.delete_items(&[item_id]);
    assert!(result.is_err(), "deleting an AutoArchive item must be rejected");
}

#[test]
fn repository_delete_skips_already_absent_file_but_marks_ignored() {
    let temp = tempfile::tempdir().unwrap();
    let source_root = temp.path().join("inbox");
    fs::create_dir_all(&source_root).unwrap();
    let file_path = source_root.join("gone_trailer.mp4");
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let mut dup = ingest_item("ABP-525", 0.8);
    dup.source_root = source_root.clone();
    dup.path = file_path.clone();
    dup.file_name = "gone_trailer.mp4".to_string();
    dup.normalized_code = None;
    dup.decision = IngestDecision::DuplicateCandidate;
    dup.review_reasons = vec![ReviewReason::DuplicateFile];
    let job_id = repo
        .create_ingest_job(&[source_root.clone()], &[dup])
        .unwrap();
    let item_id = repo.list_ingest_items(job_id).unwrap()[0].id.unwrap();

    let updated = repo.delete_items(&[item_id]).unwrap();

    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].decision, IngestDecision::Ignored);
}

// ===== local metadata enrichment: actors + genres =====

#[test]
fn scanner_extracts_actor_names_from_local_nfo_blocks() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("inbox").join("ABP-525");
    std::fs::create_dir_all(&source).unwrap();
    let video = source.join("ABP-525.mp4");
    std::fs::write(&video, b"video").unwrap();
    std::fs::write(
        source.join("ABP-525.nfo"),
        r#"
        <movie>
          <title>Sample</title>
          <actor>
            <name>Melody Marks</name>
            <type>Actor</type>
          </actor>
          <actor>
            <name>Yua Mikami</name>
            <type>Actor</type>
          </actor>
          <actor>
            <name>未知演员</name>
            <type>Actor</type>
          </actor>
        </movie>
        "#,
    )
    .unwrap();

    let items = Scanner::scan_sources(&[temp.path().join("inbox")]).unwrap();
    let metadata = items[0].metadata.as_ref().unwrap();

    // actors parsed from <actor><name> blocks, scraper placeholder dropped
    assert_eq!(
        metadata.actors,
        vec!["Melody Marks".to_string(), "Yua Mikami".to_string()]
    );
}

#[test]
fn scanner_extracts_genres_and_drops_noise_tags() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("inbox").join("SSNI-452");
    std::fs::create_dir_all(&source).unwrap();
    let video = source.join("SSNI-452.mp4");
    std::fs::write(&video, b"video").unwrap();
    std::fs::write(
        source.join("SSNI-452.nfo"),
        r#"
        <movie>
          <title>Sample</title>
          <genre>H264</genre>
          <genre>1080P</genre>
          <genre>SSNI</genre>
          <genre>性感内衣</genre>
          <genre>巨乳</genre>
          <genre>单体作品</genre>
          <studio>S1 NO.1 STYLE</studio>
          <director>前田文豪</director>
        </movie>
        "#,
    )
    .unwrap();

    let items = Scanner::scan_sources(&[temp.path().join("inbox")]).unwrap();
    let metadata = items[0].metadata.as_ref().unwrap();

    // noise (codecs/resolution/code-prefix) dropped, real genres kept, order preserved
    assert_eq!(
        metadata.genres,
        vec!["性感内衣".to_string(), "巨乳".to_string(), "单体作品".to_string()]
    );
    assert_eq!(metadata.studio.as_deref(), Some("S1 NO.1 STYLE"));
    assert_eq!(metadata.director.as_deref(), Some("前田文豪"));
}

// ===== actor entity + alias model =====

#[test]
fn repository_creates_actor_entity_for_new_name_and_links_to_work() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let work_id = repo
        .upsert_work(&Work {
            id: None,
            normalized_code: Some("ABP-525".to_string()),
            title_zh: None,
            original_title: None,
            aliases: vec![],
            summary: None,
            source_code: None,
            code_kind: CodeKind::Standard,
            outline: None,
            poster_path: None,
            thumb_path: None,
            fanart_path: None,
        screenshot_path: None,
        gif_path: None,
            sets: vec![],
            rating_value: None,
            rating_max: None,
            rating_votes: None,
            criticrating: None,
            label: None,
            runtime_minutes: None,
            year: None,
            website: None,
            mpaa: None,
            has_video: true,
            ratings: vec![],
            cover_path: None,
            tags: vec![],
            lists: vec![],
            rating: None,
            watch_status: WatchStatus::Unwatched,
            genres: vec![],
            studio: None,
            director: None,
            release_date: None,
        })
        .unwrap();

    repo.set_work_actors(work_id, &["Melody Marks".to_string()], "local-nfo")
        .unwrap();

    let actors = repo.list_work_actors(work_id).unwrap();
    assert_eq!(actors.len(), 1);
    assert_eq!(actors[0].primary_name, "Melody Marks");
}

#[test]
fn repository_resolves_same_actor_name_to_single_entity_across_works() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let w1 = repo.upsert_work(&work_stub("ABP-525")).unwrap();
    let w2 = repo.upsert_work(&work_stub("SSNI-452")).unwrap();

    repo.set_work_actors(w1, &["三上悠亜".to_string()], "local-nfo").unwrap();
    repo.set_work_actors(w2, &["三上悠亜".to_string()], "local-nfo").unwrap();

    // same name -> one actor entity, linked from two works
    let a1 = repo.list_work_actors(w1).unwrap();
    let a2 = repo.list_work_actors(w2).unwrap();
    assert_eq!(a1.len(), 1);
    assert_eq!(a2.len(), 1);
    assert_eq!(a1[0].id.unwrap(), a2[0].id.unwrap(), "same name must resolve to one actor entity");
}

#[test]
fn repository_merges_two_actors_when_alias_added() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let w1 = repo.upsert_work(&work_stub("ABP-525")).unwrap();
    let w2 = repo.upsert_work(&work_stub("SSNI-452")).unwrap();

    // two different spellings -> two actor entities initially
    repo.set_work_actors(w1, &["三上悠亜".to_string()], "local-nfo").unwrap();
    repo.set_work_actors(w2, &["Yua Mikami".to_string()], "local-nfo").unwrap();
    let a1 = repo.list_work_actors(w1).unwrap();
    let a2 = repo.list_work_actors(w2).unwrap();
    assert_ne!(a1[0].id.unwrap(), a2[0].id.unwrap());

    // online enrichment later: mark them as the same person by aliasing
    let merged_id = repo
        .merge_actors(a1[0].id.unwrap(), a2[0].id.unwrap())
        .unwrap();
    let after1 = repo.list_work_actors(w1).unwrap();
    let after2 = repo.list_work_actors(w2).unwrap();
    // both works now point at the merged (surviving) entity
    let surviving = if merged_id == a1[0].id.unwrap() { a1[0].id.unwrap() } else { a2[0].id.unwrap() };
    assert_eq!(after1[0].id.unwrap(), surviving);
    assert_eq!(after2[0].id.unwrap(), surviving);
    // both names are retained as aliases of the survivor
    let names = repo.list_actor_names(surviving).unwrap();
    assert!(names.contains(&"三上悠亜".to_string()));
    assert!(names.contains(&"Yua Mikami".to_string()));
}

#[test]
fn repository_adds_alias_to_existing_actor_without_duplicate_entity() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();
    let w1 = repo.upsert_work(&work_stub("ABP-525")).unwrap();
    repo.set_work_actors(w1, &["三上悠亜".to_string()], "local-nfo").unwrap();
    let actor = repo.list_work_actors(w1).unwrap().remove(0);

    // later enrichment: the same actor is also known as Yua Mikami
    repo.add_actor_alias(actor.id.unwrap(), "Yua Mikami", "online").unwrap();

    // resolving the alias name now returns the SAME entity, not a new one
    repo.set_work_actors(w1, &["Yua Mikami".to_string()], "online").unwrap();
    let actors = repo.list_work_actors(w1).unwrap();
    assert_eq!(actors.len(), 1, "alias must not create a duplicate actor");
    assert_eq!(actors[0].id.unwrap(), actor.id.unwrap());
    let names = repo.list_actor_names(actor.id.unwrap()).unwrap();
    assert!(names.contains(&"三上悠亜".to_string()));
    assert!(names.contains(&"Yua Mikami".to_string()));
}

fn work_stub(code: &str) -> Work {
    Work {
        id: None,
        normalized_code: Some(code.to_string()),
        source_code: None,
        code_kind: CodeKind::Standard,
        title_zh: None,
        original_title: None,
        aliases: vec![],
        summary: None,
        outline: None,
        cover_path: None,
        poster_path: None,
        thumb_path: None,
        fanart_path: None,
        screenshot_path: None,
        gif_path: None,
        tags: vec![],
        sets: vec![],
        lists: vec![],
        rating: None,
        rating_value: None,
        rating_max: None,
        rating_votes: None,
        criticrating: None,
        watch_status: WatchStatus::Unwatched,
        genres: vec![],
        studio: None,
        label: None,
        director: None,
        release_date: None,
        runtime_minutes: None,
        year: None,
        website: None,
        mpaa: None,
        has_video: true,
        ratings: vec![],
    }
}


#[test]
fn repository_auto_syncs_actors_when_resolving_ingest_item() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("inbox").join("ABP-525");
    std::fs::create_dir_all(&source).unwrap();
    let video = source.join("ABP-525.mp4");
    std::fs::write(&video, b"video").unwrap();
    std::fs::write(
        source.join("ABP-525.nfo"),
        r#"
        <movie>
          <title>Sample</title>
          <actor><name>Melody Marks</name><type>Actor</type></actor>
          <actor><name>Yua Mikami</name><type>Actor</type></actor>
        </movie>
        "#,
    )
    .unwrap();

    let db_path = temp.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();

    // scan + decide yields an item whose metadata carries the actors
    let items = Scanner::scan_sources(&[temp.path().join("inbox")]).unwrap();
    let mut item = items.into_iter().next().unwrap();
    item.decision = IngestDecision::NeedsReview;
    item.normalized_code = Some("ABP-525".to_string());
    let job_id = repo
        .create_ingest_job(&[temp.path().join("inbox")], &[item.clone()])
        .unwrap();
    let item_id = repo.list_ingest_items(job_id).unwrap()[0].id.unwrap();

    // resolving promotes the item into a work AND must sync actors
    repo.resolve_ingest_item(item_id, Some("ABP-525".to_string())).unwrap();
    let work_id = repo
        .get_work_by_code("ABP-525")
        .unwrap()
        .unwrap()
        .id
        .unwrap();

    let actors = repo.list_work_actors(work_id).unwrap();
    let names: Vec<String> = actors.iter().map(|a| a.primary_name.clone()).collect();
    assert!(names.contains(&"Melody Marks".to_string()), "actors = {:?}", names);
    assert!(names.contains(&"Yua Mikami".to_string()), "actors = {:?}", names);
}
