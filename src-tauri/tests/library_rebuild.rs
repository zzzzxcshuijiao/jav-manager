use media_manager::domain::{CodeKind, Work, WorkRating, WatchStatus};
use media_manager::storage::Repository;
use tempfile::tempdir;
use std::fs;
use std::path::Path;

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

// ===== Task 3: rebuild orchestration (multi-CD merge, nonstandard codes) =====

const SAMPLE_IPX_607_MAIN: &str = r#"<movie>
  <num>IPX-607</num>
  <title><![CDATA[IPX-607 作品主标题]]></title>
  <originaltitle><![CDATA[IPX-607 Original Title]]></originaltitle>
  <plot><![CDATA[主碟剧情]]></plot>
  <runtime>120</runtime>
  <year>2019</year>
  <studio>IPX Studio</studio>
  <label>IPX Label</label>
  <director>张三</director>
  <premiered>2019-05-01</premiered>
  <tag>中文字幕</tag>
  <tag>高畫質</tag>
  <genre>巨乳</genre>
  <actor><name>月乃瀬華</name></actor>
  <ratings><rating name="javdb" max="5" default="true"><value>4.2</value><votes>120</votes></rating></ratings>
</movie>"#;

const SAMPLE_IPX_607_CD2: &str = r#"<movie>
  <num>IPX-607</num>
  <title><![CDATA[IPX-607 CD2]]></title>
  <plot><![CDATA[第二碟剧情]]></plot>
  <runtime>120</runtime>
  <actor><name>月乃瀬華</name></actor>
  <tag>高清</tag>
</movie>"#;

const SAMPLE_THE_LIFE_EROTIC: &str = r#"<movie>
  <num>TheLifeErotic.19.06.20</num>
  <title><![CDATA[TheLifeErotic.19.06.20-Rope 2]]></title>
  <originaltitle><![CDATA[TheLifeErotic.19.06.20-Rope 2]]></originaltitle>
  <plot><![CDATA[Plot text]]></plot>
  <runtime>9</runtime>
  <year>2019</year>
  <set>The Life Erotic</set>
  <studio>Metartnetwork</studio>
  <label>The Life Erotic</label>
  <premiered>2019-06-20</premiered>
  <tag>中文字幕</tag>
  <tag>微變態</tag>
  <genre>H264</genre>
  <ratings><rating name="javdb" max="5" default="true"><value>4.0</value><votes>2</votes></rating></ratings>
</movie>"#;

const SAMPLE_MINIMAL_AV: &str = r#"<movie>
  <num>ABP-001</num>
  <title><![CDATA[ABP-001]]></title>
  <tag>测试标签</tag>
</movie>"#;

/// Tempdir-backed library fixture: write_nfo/write_video create nested files;
/// open_repo opens a SQLite repository without migrating, so each test drives
/// its own migrate (mirroring how the rest of the suite exercises the repo).
struct TestLibrary {
    _tmp: tempfile::TempDir,
    root: std::path::PathBuf,
}

impl TestLibrary {
    fn new() -> Self {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        Self { _tmp: tmp, root }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn write_nfo(&self, rel: &str, content: &str) {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
    }

    fn write_video(&self, rel: &str, size: usize) {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, vec![0u8; size]).unwrap();
    }

    fn open_repo(&self) -> Repository {
        let db_path = self.root.join("library.sqlite");
        Repository::open(&db_path).unwrap()
    }
}

/// Build a minimal standard Work for seeding pre-rebuild state.
fn sample_work(code: &str) -> Work {
    Work {
        id: None,
        normalized_code: Some(code.to_string()),
        source_code: Some(code.to_string()),
        code_kind: CodeKind::Standard,
        title_zh: Some(code.to_string()),
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
        has_video: false,
        ratings: vec![],
    }
}

#[test]
fn rebuild_merges_multi_cd_nfos_into_one_work_and_multiple_versions() {
    let sandbox = TestLibrary::new();
    sandbox.write_nfo("Aoi\\IPX-607\\IPX-607.nfo", SAMPLE_IPX_607_MAIN);
    sandbox.write_nfo("Aoi\\IPX-607\\IPX-607-cd2.nfo", SAMPLE_IPX_607_CD2);
    sandbox.write_video("Aoi\\IPX-607\\IPX-607.mp4", 1024 * 1024);
    sandbox.write_video("Aoi\\IPX-607\\IPX-607-cd2.mp4", 1024 * 1024);

    let repo = sandbox.open_repo();
    repo.migrate().unwrap();
    let report = repo.rebuild_library(&[sandbox.root().to_path_buf()]).unwrap();

    assert_eq!(report.works_created, 1);
    assert_eq!(report.works_merged, 1);
    let works = repo.list_works().unwrap();
    assert_eq!(works.len(), 1);
    let detail = repo.get_work_detail(works[0].id.unwrap()).unwrap().unwrap();
    assert_eq!(detail.file_versions.len(), 2);
    assert!(detail.work.has_video);
}

#[test]
fn rebuild_marks_nonstandard_num_as_nonstandard_and_keeps_source_code() {
    let sandbox = TestLibrary::new();
    sandbox.write_nfo("Violet\\TheLifeErotic\\TheLifeErotic.19.06.20-C.nfo", SAMPLE_THE_LIFE_EROTIC);

    let repo = sandbox.open_repo();
    repo.migrate().unwrap();
    repo.rebuild_library(&[sandbox.root().to_path_buf()]).unwrap();

    let works = repo.list_works().unwrap();
    assert_eq!(works.len(), 1);
    assert_eq!(works[0].source_code.as_deref(), Some("TheLifeErotic.19.06.20"));
    assert!(works[0].normalized_code.is_none());
    assert_eq!(works[0].code_kind, CodeKind::Nonstandard);
    assert!(!works[0].has_video);
}

#[test]
fn rebuild_rolls_back_when_relation_write_fails() {
    let sandbox = TestLibrary::new();
    sandbox.write_nfo("Broken\\ABP-001\\ABP-001.nfo", SAMPLE_MINIMAL_AV);

    let repo = sandbox.open_repo();
    repo.migrate().unwrap();
    // Seed a real work so rollback has meaningful state to restore: the
    // rebuild's clear step deletes it, then trips on the dropped tags table,
    // and the transaction must restore the seeded row.
    repo.upsert_work(&sample_work("SEED-001")).unwrap();
    repo.debug_drop_table("tags").unwrap();

    let before = repo.list_works().unwrap();
    let result = repo.rebuild_library(&[sandbox.root().to_path_buf()]);
    assert!(result.is_err());
    let after = repo.list_works().unwrap();
    assert_eq!(before, after);
}
