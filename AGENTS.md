# AGENTS.md  -  media-manager

Local-first media library workbench: Tauri (desktop shell) + React (UI) + Rust (core/SQLite/ingest/archive) + ffmpeg.

## HARD CONSTRAINTS (read first)

These operations crash the Codex desktop session in a non-interactive agent run. DO NOT execute any of them inside a Codex session:

- `tauri dev` / `cargo run` / `media-manager.exe`  -  the Tauri binary opens a WebView2 window that fails to init without an interactive desktop and exits 0xffffffff, destabilizing the session.
- The bundled **in-app browser (iab) plugin**: `setupBrowserRuntime` + `agent.browsers.get("iab")` + `browser.tabs.new()` + `tab.goto(...)` + `screenshot` / `domSnapshot`. The iab tab IS a WebView2 surface inside the Codex host, so these trigger the same native-layer crash as the Tauri GUI. The skill's "background by default" framing does NOT help  -  even a hidden tab spins up WebView2 rendering.

Root cause: anything that initializes/renders WebView2 in a non-interactive agent session can bring down the host.

## HOW TO VERIFY (no WebView2, no crashes)

- Frontend dev server health: pure HTTP  -  `Invoke-WebRequest http://localhost:1420 -UseBasicParsing` for a 200 + module presence (e.g. fetch `/src/App.tsx` and grep for a known symbol).
- Types: `npx tsc --noEmit`.
- Unit tests: `npm test` (vitest) and `cargo test --manifest-path src-tauri/Cargo.toml`.
- Production build: `npm run build`.
- Visual check: the USER runs `npm run dev` in their own desktop terminal and opens http://localhost:1420 in their own browser. The agent does not render the page.

If a crash already happened: code edits written via node:fs survive (immediate flush); only the dev server + session state die. Restart with `npm run dev` only  -  never re-launch the Tauri GUI or the in-app browser.


## UX PRINCIPLE - OPERATION FEEDBACK

Every operation that takes perceptible time (scan, rebuild, archive execute, metadata retry, batch resolve) MUST show an explicit loading state from the moment it starts until it completes or errors. The trigger button is disabled and shows a spinner/label swap; a status line says what is running. A long-running action with no feedback is a bug, not a missing polish item.

## TECH STACK & LAYOUT

- `src/`  -  React frontend (App.tsx, api.ts, viewModel.ts, demoData.ts, styles.css)
- `src-tauri/src/`  -  Rust core: storage.rs (SQLite repo), nfo.rs (NFO parser), library_rebuild.rs, scanner.rs, ingest.rs, commands.rs (Tauri commands)
- SQLite lives under the app data dir (`%APPDATA%/local.media-manager/library.sqlite`), NOT on H drive
- H drive holds media files and intentional local assets only

## WORKFLOW

This repo uses VibeCoding. State of truth is in `.ai_state/` (gitignored, session-local): `tasks.md`, `progress.md`, `design.md`, `lessons.md`. Check `tasks.md` for current phase and `lessons.md` for recorded pitfalls before acting.

When editing non-ASCII source (App.tsx Chinese strings), use node:fs or apply_patch, NOT PowerShell Get-Content/Set-Content (it double-transcodes UTF-8 through the GBK console code page and corrupts CJK).


## EDITING RULE - NEVER pass CJK through PowerShell string layer

PowerShell Get-Content/Set-Content and inline string literals (including -replace with backtick template strings) corrupt CJK UTF-8 through the GBK console code page. This destroyed App.tsx Chinese strings twice. For ANY edit to files containing non-ASCII (App.tsx, any Chinese labels), ALWAYS use node:fs (mcp__node_repl__js readFileSync/writeFileSync) or apply_patch. NEVER use PowerShell string operations on these files.
