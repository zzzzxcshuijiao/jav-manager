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
