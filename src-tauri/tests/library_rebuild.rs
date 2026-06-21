use media_manager::domain::{CodeKind, Work, WorkRating, WatchStatus};
use media_manager::storage::Repository;
use tempfile::tempdir;

#[test]
fn migrate_adds_rich_work_columns_and_relation_tables() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();

    let columns = repo.debug_table_columns("works").unwrap();
    assert!(columns.contains(&"source_code".to_string()));
    assert!(columns.contains(&"code_kind".to_string()));
    assert!(columns.contains(&"runtime_minutes".to_string()));
    assert!(columns.contains(&"year".to_string()));
    assert!(columns.contains(&"website".to_string()));
    assert!(columns.contains(&"mpaa".to_string()));
    assert!(columns.contains(&"outline".to_string()));
    assert!(columns.contains(&"poster_path".to_string()));
    assert!(columns.contains(&"thumb_path".to_string()));
    assert!(columns.contains(&"fanart_path".to_string()));
    assert!(columns.contains(&"criticrating".to_string()));
    assert!(columns.contains(&"rating_value".to_string()));
    assert!(columns.contains(&"rating_max".to_string()));
    assert!(columns.contains(&"rating_votes".to_string()));
    assert!(columns.contains(&"has_video".to_string()));
    assert!(repo.debug_table_exists("tags").unwrap());
    assert!(repo.debug_table_exists("work_tags").unwrap());
    assert!(repo.debug_table_exists("sets").unwrap());
    assert!(repo.debug_table_exists("work_sets").unwrap());
    assert!(repo.debug_table_exists("labels").unwrap());
    assert!(repo.debug_table_exists("studios").unwrap());
    assert!(repo.debug_table_exists("work_ratings").unwrap());
}

#[test]
fn upsert_work_persists_rich_metadata_and_relations() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("library.sqlite");
    let repo = Repository::open(&db_path).unwrap();
    repo.migrate().unwrap();

    let work = Work {
        id: None,
        normalized_code: None,
        source_code: Some("TheLifeErotic.19.06.20".to_string()),
        code_kind: CodeKind::Nonstandard,
        title_zh: Some("TheLifeErotic.19.06.20-Rope 2".to_string()),
        original_title: Some("TheLifeErotic.19.06.20-Rope 2".to_string()),
        aliases: vec![],
        summary: Some("Plot text".to_string()),
        outline: None,
        cover_path: Some("https://example.com/cover.jpg".to_string().into()),
        poster_path: Some("poster.jpg".to_string().into()),
        thumb_path: Some("thumb.jpg".to_string().into()),
        fanart_path: Some("fanart.jpg".to_string().into()),
        tags: vec!["中文字幕".to_string(), "微變態".to_string()],
        sets: vec!["The Life Erotic".to_string()],
        lists: vec![],
        rating: None,
        rating_value: Some(4.0),
        rating_max: Some(5),
        rating_votes: Some(2),
        criticrating: Some(80.0),
        watch_status: WatchStatus::Unwatched,
        genres: vec![],
        studio: Some("Metartnetwork".to_string()),
        label: Some("The Life Erotic".to_string()),
        director: None,
        release_date: Some("2019-06-20".to_string()),
        runtime_minutes: Some(9),
        year: Some(2019),
        website: Some("https://javdb.com/v/0yqx7".to_string()),
        mpaa: Some("JP-18+".to_string()),
        has_video: false,
        ratings: vec![WorkRating { source: "javdb".to_string(), value: 4.0, max: 5, votes: Some(2) }],
    };

    let work_id = repo.upsert_work(&work).unwrap();
    let stored = repo.get_work_detail(work_id).unwrap().unwrap();
    assert_eq!(stored.work.source_code.as_deref(), Some("TheLifeErotic.19.06.20"));
    assert_eq!(stored.work.code_kind, CodeKind::Nonstandard);
    assert_eq!(stored.tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["中文字幕", "微變態"]);
    assert_eq!(stored.sets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["The Life Erotic"]);
    assert_eq!(stored.ratings[0].source, "javdb");
    assert!(!stored.work.has_video);
}
