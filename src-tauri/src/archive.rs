use crate::domain::{
    ArchiveAction, ArchiveConflict, ArchivePlan, IngestDecision, IngestItem, ReviewReason,
};
use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct ArchivePlanner {
    archive_root: PathBuf,
}

impl ArchivePlanner {
    pub fn new(archive_root: PathBuf) -> Self {
        Self { archive_root }
    }

    pub fn preview(&self, items: &[IngestItem]) -> Result<ArchivePlan> {
        let mut actions = Vec::new();
        let mut conflicts = Vec::new();
        let mut version_counts: HashMap<String, usize> = HashMap::new();
        for item in items {
            if item.decision != IngestDecision::AutoArchive {
                continue;
            }

            let Some(code) = &item.normalized_code else {
                continue;
            };
            if !item.path.exists() {
                conflicts.push(ArchiveConflict {
                    item_id: item.id,
                    path: item.path.clone(),
                    reason: ReviewReason::MoveFailed,
                    message: format!(
                        "source file does not exist: {}",
                        item.path.to_string_lossy()
                    ),
                });
                continue;
            }
            let next_index = version_counts.entry(code.clone()).or_insert(0);
            *next_index += 1;
            let file_name = next_available_archive_name(&self.archive_root, code, &item.path, *next_index);
            let to_path = self.archive_root.join(code).join(&file_name);
            *next_index = archive_version_index_from_name(code, &file_name).unwrap_or(*next_index);
            actions.push(ArchiveAction {
                item_id: item.id,
                work_code: code.clone(),
                from_path: item.path.clone(),
                to_path,
                original_file_name: item.file_name.clone(),
                normalized_file_name: file_name,
            });
        }

        Ok(ArchivePlan {
            id: None,
            actions,
            conflicts,
        })
    }
}

pub fn normalized_file_name(code: &str, source_path: &Path, version_index: usize) -> String {
    let extension = source_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{}", ext.to_ascii_lowercase()))
        .unwrap_or_default();

    if version_index <= 1 {
        format!("{code}{extension}")
    } else {
        format!("{code}-v{version_index}{extension}")
    }
}

fn next_available_archive_name(
    archive_root: &Path,
    code: &str,
    source_path: &Path,
    start_index: usize,
) -> String {
    let mut version_index = start_index.max(1);
    loop {
        let file_name = normalized_file_name(code, source_path, version_index);
        if !archive_root.join(code).join(&file_name).exists() {
            return file_name;
        }
        version_index += 1;
    }
}

fn archive_version_index_from_name(code: &str, file_name: &str) -> Option<usize> {
    let stem = Path::new(file_name).file_stem()?.to_str()?;
    if stem == code {
        return Some(1);
    }
    stem.strip_prefix(&format!("{code}-v"))?.parse().ok()
}
