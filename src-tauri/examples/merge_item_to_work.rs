use media_manager::domain::{CodeKind, IngestDecision, IngestItem, WatchStatus, Work};
use media_manager::storage::Repository;
use std::env;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let db_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("usage: merge_item_to_work <library.sqlite>"))?;
    let repo = Repository::open(&db_path)?;
    repo.migrate()?;
    let work_id = repo.upsert_work(&Work {
        id: None,
        normalized_code: Some("ABP-525".to_string()),
        source_code: None,
        code_kind: CodeKind::Standard,
        title_zh: Some("已有作品".to_string()),
        original_title: None,
        aliases: vec![],
        summary: None,
        outline: None,
        cover_path: None,
        poster_path: None,
        thumb_path: None,
        fanart_path: None,
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
    })?;
    let items = vec![
        ingest_item("ABP-525", "ABP-525.mp4"),
        ingest_item("ABP-999", "ABP-525-CD2.mp4"),
    ];
    let job_id = repo.create_ingest_job(&[PathBuf::from("G:/tmp/source")], &items)?;
    let stored = repo.list_ingest_items(job_id)?;
    repo.resolve_ingest_item(stored[0].id.unwrap(), None)?;
    repo.resolve_ingest_item_to_work(stored[1].id.unwrap(), work_id)?;
    println!("merged job {job_id} into work {work_id}");
    Ok(())
}

fn ingest_item(code: &str, file_name: &str) -> IngestItem {
    IngestItem {
        id: None,
        job_id: None,
        source_root: PathBuf::from("G:/tmp/source"),
        path: PathBuf::from(format!("G:/tmp/source/{file_name}")),
        file_name: file_name.to_string(),
        size_bytes: 1024,
        duration_seconds: None,
        width: None,
        height: None,
        codec: None,
        normalized_code: Some(code.to_string()),
        confidence: 0.82,
        decision: IngestDecision::NeedsReview,
        review_reasons: vec![],
        code_conflict: None,
        metadata: None,
        candidate_work_id: None,
        file_hash: None,
    }
}
