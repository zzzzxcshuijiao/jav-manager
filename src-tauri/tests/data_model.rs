use media_manager::domain::{CodeKind, WatchStatus, Work};
use media_manager::storage::Repository;

/// Minimal valid `Work` for tests. Updated in later tasks as Work gains fields.
fn sample_work() -> Work {
    Work {
        id: None,
        normalized_code: None,
        source_code: None,
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
        has_video: false,
        ratings: Vec::new(),
        watch_progress_seconds: None,
        last_played_at: None,
    }
}

#[test]
fn watch_status_roundtrips_new_variants() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();

    let mut work = sample_work();
    work.normalized_code = Some("ABP-001".to_string());
    let id = repo.upsert_work(&work).unwrap();

    for status in [
        WatchStatus::WantToWatch,
        WatchStatus::Watching,
        WatchStatus::OnHold,
    ] {
        let updated =
            repo.update_work_profile(id, Vec::new(), Vec::new(), None, status.clone()).unwrap();
        assert_eq!(updated.watch_status, status);
    }
}

#[test]
fn watch_progress_is_persisted_and_read_back() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::open(&tmp.path().join("t.sqlite")).unwrap();
    repo.migrate().unwrap();

    let mut work = sample_work();
    work.normalized_code = Some("ABP-002".to_string());
    let id = repo.upsert_work(&work).unwrap();

    let updated =
        repo.set_watch_progress(id, Some(1865), Some("2026-06-25T21:00:00Z".to_string())).unwrap();
    assert_eq!(updated.watch_progress_seconds, Some(1865));
    assert_eq!(updated.last_played_at.as_deref(), Some("2026-06-25T21:00:00Z"));

    let cleared = repo.set_watch_progress(id, None, None).unwrap();
    assert_eq!(cleared.watch_progress_seconds, None);
    assert_eq!(cleared.last_played_at, None);
}
