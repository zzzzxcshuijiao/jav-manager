# Feedback Package

`scripts/collect-feedback.ps1` collects a shareable support bundle from the real user environment without starting Tauri, WebView2, or the in-app browser.

## Normal Collection

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/collect-feedback.ps1
```

The script writes a timestamped directory and zip file under `feedback/`.

## Fast Collection

Use this when you only need environment, git state, and app diagnostics:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/collect-feedback.ps1 -SkipTests -SkipBuild
```

You can also skip one side of verification:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/collect-feedback.ps1 -SkipRustTests
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/collect-feedback.ps1 -SkipNodeTests
```

## Custom App Data Path

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/collect-feedback.ps1 -AppDataPath "$env:APPDATA\local.media-manager"
```

## What It Collects

- Git branch, commit, status, recent log, and worktree list.
- Tool paths and key Windows environment variables.
- Optional `npm test`, `cargo test --manifest-path src-tauri/Cargo.toml -j 1`, `npx tsc --noEmit`, and `npm run build` outputs.
- Recent files from app-data `logs/` and `diagnostics/`.
- SQLite presence and best-effort table counts when `sqlite3` is installed.
- A machine-readable `manifest.json` and human-readable `summary.md`.

## Safety Boundary

The script does not copy media files, NFO files, image files, video files, or the SQLite database itself. Missing app data or optional tools are recorded in the package instead of failing the collection.
