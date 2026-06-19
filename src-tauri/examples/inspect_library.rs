use media_manager::storage::Repository;
use rusqlite::Connection;
use std::env;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let db_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("usage: inspect_library <library.sqlite>"))?;

    let repo = Repository::open(&db_path)?;
    repo.migrate()?;
    let source_roots = repo.get_source_roots()?;
    let archive_root = repo.get_archive_root()?;
    let restore_job = repo.get_latest_ingest_job()?;
    drop(repo);

    let conn = Connection::open(&db_path)?;
    println!("database={}", db_path.display());
    print_source_roots(&source_roots);
    print_archive_root(archive_root.as_ref());
    print_table_columns(&conn, "ingest_items")?;
    print_table_columns(&conn, "file_versions")?;
    print_count(&conn, "works")?;
    print_count(&conn, "file_versions")?;
    print_count(&conn, "archive_action_logs")?;
    print_count(&conn, "ingest_jobs")?;
    print_count(&conn, "ingest_items")?;
    print_latest_job(&conn)?;
    print_restore_job(restore_job.as_ref());
    print_media_field_summary(&conn)?;
    Ok(())
}

fn print_table_columns(conn: &Connection, table: &str) -> anyhow::Result<()> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = Vec::new();
    for row in rows {
        columns.push(row?);
    }
    println!("{table}_columns={}", columns.join(","));
    Ok(())
}

fn print_source_roots(paths: &[PathBuf]) {
    if paths.is_empty() {
        println!("source_roots=<none>");
        return;
    }
    let joined = paths
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("|");
    println!("source_roots={joined}");
}

fn print_archive_root(path: Option<&PathBuf>) {
    let Some(path) = path else {
        println!("archive_root=<none>");
        return;
    };
    println!("archive_root={}", path.display());
}

fn print_count(conn: &Connection, table: &str) -> anyhow::Result<()> {
    let count: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))?;
    println!("{table}={count}");
    Ok(())
}

fn print_latest_job(conn: &Connection) -> anyhow::Result<()> {
    let mut statement = conn.prepare(
        "
        SELECT id, total_items, auto_count, review_count, failed_count
        FROM ingest_jobs
        ORDER BY id DESC
        LIMIT 1
        ",
    )?;
    let mut rows = statement.query([])?;
    let Some(row) = rows.next()? else {
        println!("latest_job=<none>");
        return Ok(());
    };
    println!(
        "latest_job=id:{},total:{},auto:{},review:{},failed:{}",
        row.get::<_, i64>(0)?,
        row.get::<_, i64>(1)?,
        row.get::<_, i64>(2)?,
        row.get::<_, i64>(3)?,
        row.get::<_, i64>(4)?
    );
    Ok(())
}

fn print_restore_job(job: Option<&media_manager::domain::IngestJobSummary>) {
    let Some(job) = job else {
        println!("restore_job=<none>");
        return;
    };
    println!(
        "restore_job=id:{},total:{},auto:{},review:{},failed:{}",
        job.id, job.total_items, job.auto_count, job.review_count, job.failed_count
    );
}

fn print_media_field_summary(conn: &Connection) -> anyhow::Result<()> {
    let ingest_with_media: i64 = conn.query_row(
        "
        SELECT COUNT(*)
        FROM ingest_items
        WHERE duration_seconds IS NOT NULL
           OR width IS NOT NULL
           OR height IS NOT NULL
           OR codec IS NOT NULL
        ",
        [],
        |row| row.get(0),
    )?;
    let versions_with_media: i64 = conn.query_row(
        "
        SELECT COUNT(*)
        FROM file_versions
        WHERE duration_seconds IS NOT NULL
           OR width IS NOT NULL
           OR height IS NOT NULL
           OR codec IS NOT NULL
        ",
        [],
        |row| row.get(0),
    )?;
    println!("ingest_items_with_media={ingest_with_media}");
    println!("file_versions_with_media={versions_with_media}");
    Ok(())
}
