use crate::domain::{FileVersion, Work};
use std::path::Path;

pub fn attach_version_to_work(work: &Work, version: &mut FileVersion) {
    version.work_id = work.id;
    let extension = extension_with_dot(&version.original_path);
    version.normalized_file_name = Some(format!(
        "{}{}",
        work.normalized_code.as_deref().unwrap_or(""),
        extension
    ));
}

fn extension_with_dot(path: &Path) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_default()
}
