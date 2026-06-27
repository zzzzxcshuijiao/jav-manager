param(
    [string]$OutputDirectory = "",
    [string]$AppDataPath = "",
    [switch]$SkipTests,
    [switch]$SkipNodeTests,
    [switch]$SkipRustTests,
    [switch]$SkipBuild,
    [switch]$SkipAppData,
    [switch]$NoArchive
)

$ErrorActionPreference = "Stop"
$script:Utf8NoBom = New-Object System.Text.UTF8Encoding $false

<#
    Write text as UTF-8 without BOM so generated diagnostics do not change
    repository encoding conventions when checked locally.
#>
function Write-Utf8NoBom {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Content
    )

    $parent = Split-Path -Parent $Path
    if ($parent -and -not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }

    [System.IO.File]::WriteAllText($Path, $Content, $script:Utf8NoBom)
}

<#
    Return an executable path for a tool, with optional Windows-specific
    fallbacks for tools that may be installed but absent from PATH.
#>
function Resolve-ToolPath {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [string[]]$FallbackPaths = @()
    )

    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    foreach ($fallback in $FallbackPaths) {
        if ($fallback -and (Test-Path -LiteralPath $fallback)) {
            return $fallback
        }
    }

    return $null
}

<#
    Run a diagnostic command, capture stdout and stderr into a file, and
    return metadata without failing the whole feedback collection.
#>
function Invoke-FeedbackCommand {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [AllowNull()][AllowEmptyString()][string]$Executable = "",
        [string[]]$Arguments = @(),
        [Parameter(Mandatory = $true)][string]$OutputPath,
        [bool]$Optional = $true
    )

    $startedAt = Get-Date
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $exitCode = 0
    $status = "completed"
    $outputLines = New-Object System.Collections.Generic.List[string]
    $outputLines.Add("Command: $Executable $($Arguments -join ' ')")
    $outputLines.Add("Started: $($startedAt.ToString('o'))")
    $outputLines.Add("")

    try {
        $executableFound = $false
        if ($Executable) {
            $executableFound = [bool](Get-Command $Executable -ErrorAction SilentlyContinue)
            if (-not $executableFound -and (Test-Path -LiteralPath $Executable)) {
                $executableFound = $true
            }
        }

        if (-not $executableFound) {
            $status = "skipped"
            $exitCode = $null
            $outputLines.Add("Skipped: executable was not found.")
        } else {
            $global:LASTEXITCODE = 0
            $rawOutput = & $Executable @Arguments 2>&1
            $exitCode = $global:LASTEXITCODE
            if ($null -eq $exitCode) {
                $exitCode = 0
            }

            foreach ($line in $rawOutput) {
                $outputLines.Add($line.ToString())
            }
        }
    } catch {
        $status = if ($Optional) { "failed" } else { "failed-required" }
        $exitCode = 1
        $outputLines.Add("Error: $($_.Exception.Message)")
    } finally {
        $stopwatch.Stop()
        $outputLines.Add("")
        $outputLines.Add("Finished: $((Get-Date).ToString('o'))")
        $outputLines.Add("DurationMs: $($stopwatch.ElapsedMilliseconds)")
        if ($null -ne $exitCode) {
            $outputLines.Add("ExitCode: $exitCode")
        }
        Write-Utf8NoBom -Path $OutputPath -Content ($outputLines -join [Environment]::NewLine)
    }

    [PSCustomObject]@{
        name       = $Name
        status     = $status
        exitCode   = $exitCode
        durationMs = $stopwatch.ElapsedMilliseconds
        outputPath = $OutputPath
    }
}

<#
    Copy safe app-data diagnostics and describe unavailable data without
    copying media, NFO, image, video, or the SQLite database itself.
#>
function Copy-AppDataDiagnostics {
    param(
        [Parameter(Mandatory = $true)][string]$SourcePath,
        [Parameter(Mandatory = $true)][string]$DestinationPath,
        [switch]$Skip
    )

    New-Item -ItemType Directory -Path $DestinationPath -Force | Out-Null
    $statusPath = Join-Path $DestinationPath "app-data-status.txt"
    $statusLines = New-Object System.Collections.Generic.List[string]
    $statusLines.Add("AppDataPath: $SourcePath")

    if ($Skip) {
        $statusLines.Add("Skipped by -SkipAppData.")
        Write-Utf8NoBom -Path $statusPath -Content ($statusLines -join [Environment]::NewLine)
        return [PSCustomObject]@{
            available = $false
            skipped   = $true
            path      = $SourcePath
            logs      = 0
            snapshots = 0
            sqlite    = $false
        }
    }

    if (-not (Test-Path -LiteralPath $SourcePath)) {
        $statusLines.Add("App data path was not found.")
        Write-Utf8NoBom -Path $statusPath -Content ($statusLines -join [Environment]::NewLine)
        return [PSCustomObject]@{
            available = $false
            skipped   = $false
            path      = $SourcePath
            logs      = 0
            snapshots = 0
            sqlite    = $false
        }
    }

    $statusLines.Add("App data path exists.")
    $logsCopied = 0
    $snapshotsCopied = 0

    $logsPath = Join-Path $SourcePath "logs"
    if (Test-Path -LiteralPath $logsPath) {
        $logsDest = Join-Path $DestinationPath "logs"
        New-Item -ItemType Directory -Path $logsDest -Force | Out-Null
        Get-ChildItem -LiteralPath $logsPath -File -ErrorAction SilentlyContinue |
            Where-Object { $_.Extension -in @(".jsonl", ".log", ".txt") } |
            Sort-Object LastWriteTime -Descending |
            Select-Object -First 5 |
            ForEach-Object {
                $target = Join-Path $logsDest $_.Name
                Copy-Item -LiteralPath $_.FullName -Destination $target -Force
                $logsCopied += 1
            }
    }

    $diagnosticsPath = Join-Path $SourcePath "diagnostics"
    if (Test-Path -LiteralPath $diagnosticsPath) {
        $diagnosticsDest = Join-Path $DestinationPath "diagnostics"
        New-Item -ItemType Directory -Path $diagnosticsDest -Force | Out-Null
        Get-ChildItem -LiteralPath $diagnosticsPath -File -Filter "*.json" -ErrorAction SilentlyContinue |
            Sort-Object LastWriteTime -Descending |
            Select-Object -First 5 |
            ForEach-Object {
                $target = Join-Path $diagnosticsDest $_.Name
                Copy-Item -LiteralPath $_.FullName -Destination $target -Force
                $snapshotsCopied += 1
            }
    }

    $databasePath = Join-Path $SourcePath "library.sqlite"
    $hasSqlite = Test-Path -LiteralPath $databasePath
    if ($hasSqlite) {
        $database = Get-Item -LiteralPath $databasePath
        $statusLines.Add("SQLite: present, $($database.Length) bytes, modified $($database.LastWriteTime.ToString('o')).")
        Write-SqliteSummary -DatabasePath $databasePath -DestinationPath (Join-Path $DestinationPath "sqlite-summary.txt")
    } else {
        $statusLines.Add("SQLite: library.sqlite was not found.")
    }

    $statusLines.Add("Logs copied: $logsCopied")
    $statusLines.Add("Diagnostic snapshots copied: $snapshotsCopied")
    $statusLines.Add("Media/NFO/image/video contents are intentionally not copied.")
    Write-Utf8NoBom -Path $statusPath -Content ($statusLines -join [Environment]::NewLine)

    [PSCustomObject]@{
        available = $true
        skipped   = $false
        path      = $SourcePath
        logs      = $logsCopied
        snapshots = $snapshotsCopied
        sqlite    = $hasSqlite
    }
}

<#
    Write a best-effort SQLite summary when sqlite3 is installed, otherwise
    record why the database was not queried.
#>
function Write-SqliteSummary {
    param(
        [Parameter(Mandatory = $true)][string]$DatabasePath,
        [Parameter(Mandatory = $true)][string]$DestinationPath
    )

    $sqlite = Resolve-ToolPath -Name "sqlite3"
    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add("Database: $DatabasePath")

    if (-not $sqlite) {
        $lines.Add("sqlite3 was not found; table counts were not collected.")
        Write-Utf8NoBom -Path $DestinationPath -Content ($lines -join [Environment]::NewLine)
        return
    }

    $queries = @(
        ".tables",
        "SELECT 'works', COUNT(*) FROM works;",
        "SELECT 'file_versions', COUNT(*) FROM file_versions;",
        "SELECT 'pipeline_runs', COUNT(*) FROM pipeline_runs;",
        "SELECT 'scrape_jobs', COUNT(*) FROM scrape_jobs;",
        "SELECT 'exceptions', COUNT(*) FROM exceptions;",
        "SELECT 'holding', COUNT(*) FROM holding;",
        "SELECT 'archive_action_logs', COUNT(*) FROM archive_action_logs;"
    )

    foreach ($query in $queries) {
        $lines.Add("")
        $lines.Add("sqlite> $query")
        try {
            $result = & $sqlite -readonly $DatabasePath $query 2>&1
            foreach ($line in $result) {
                $lines.Add($line.ToString())
            }
            if ($global:LASTEXITCODE -ne 0) {
                $lines.Add("ExitCode: $global:LASTEXITCODE")
            }
        } catch {
            $lines.Add("Error: $($_.Exception.Message)")
        }
    }

    Write-Utf8NoBom -Path $DestinationPath -Content ($lines -join [Environment]::NewLine)
}

<#
    Create a short Markdown index for humans opening the feedback directory.
#>
function Write-Summary {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$RunDirectory,
        [Parameter(Mandatory = $true)]$Manifest
    )

    $lines = @(
        "# media-manager feedback package",
        "",
        "Created: $($Manifest.createdAt)",
        "Workspace: $($Manifest.workspace)",
        "Git commit: $($Manifest.gitCommit)",
        "",
        "## Options",
        "",
        "- Skip tests: $($Manifest.options.skipTests)",
        "- Skip Node tests: $($Manifest.options.skipNodeTests)",
        "- Skip Rust tests: $($Manifest.options.skipRustTests)",
        "- Skip build: $($Manifest.options.skipBuild)",
        "- Skip app data: $($Manifest.options.skipAppData)",
        "",
        "## App data",
        "",
        "- Available: $($Manifest.appData.available)",
        "- Logs copied: $($Manifest.appData.logs)",
        "- Diagnostic snapshots copied: $($Manifest.appData.snapshots)",
        "- SQLite present: $($Manifest.appData.sqlite)",
        "",
        "## Command outputs",
        ""
    )

    foreach ($command in $Manifest.commands) {
        $lines += "- $($command.name): $($command.status), exit $($command.exitCode), $($command.durationMs) ms"
    }

    $lines += ""
    $lines += "Open manifest.json for machine-readable details."
    $lines += "Directory: $RunDirectory"

    Write-Utf8NoBom -Path $Path -Content ($lines -join [Environment]::NewLine)
}

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $repoRoot

if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path $repoRoot "feedback"
}

if (-not $AppDataPath) {
    $AppDataPath = Join-Path $env:APPDATA "local.media-manager"
}

New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$runDirectory = Join-Path $OutputDirectory "feedback-$timestamp"
$suffix = 1
while (Test-Path -LiteralPath $runDirectory) {
    $runDirectory = Join-Path $OutputDirectory "feedback-$timestamp-$suffix"
    $suffix += 1
}

New-Item -ItemType Directory -Path $runDirectory -Force | Out-Null
$environmentDirectory = Join-Path $runDirectory "environment"
$commandDirectory = Join-Path $runDirectory "commands"
$appDataDirectory = Join-Path $runDirectory "app-data"
New-Item -ItemType Directory -Path $environmentDirectory, $commandDirectory, $appDataDirectory -Force | Out-Null

$git = Resolve-ToolPath -Name "git"
$npm = Resolve-ToolPath -Name "npm"
$npx = Resolve-ToolPath -Name "npx"
$cargo = Resolve-ToolPath -Name "cargo" -FallbackPaths @("C:\Users\DELL\.cargo\bin\cargo.exe")
$node = Resolve-ToolPath -Name "node"

$toolLines = @(
    "node: $node",
    "npm: $npm",
    "npx: $npx",
    "cargo: $cargo",
    "git: $git",
    "",
    "APPDATA: $env:APPDATA",
    "SystemRoot: $env:SystemRoot",
    "windir: $env:windir",
    "PATH: $env:PATH"
)
Write-Utf8NoBom -Path (Join-Path $environmentDirectory "tool-paths.txt") -Content ($toolLines -join [Environment]::NewLine)

$commandResults = New-Object System.Collections.Generic.List[object]
$commandResults.Add((Invoke-FeedbackCommand -Name "git status" -Executable $git -Arguments @("status", "--short", "--branch") -OutputPath (Join-Path $commandDirectory "git-status.txt")))
$commandResults.Add((Invoke-FeedbackCommand -Name "git log" -Executable $git -Arguments @("log", "--oneline", "-10") -OutputPath (Join-Path $commandDirectory "git-log.txt")))
$commandResults.Add((Invoke-FeedbackCommand -Name "git worktree list" -Executable $git -Arguments @("worktree", "list") -OutputPath (Join-Path $commandDirectory "git-worktree-list.txt")))

if (-not $SkipTests -and -not $SkipNodeTests) {
    $commandResults.Add((Invoke-FeedbackCommand -Name "npm test" -Executable $npm -Arguments @("test") -OutputPath (Join-Path $commandDirectory "npm-test.txt")))
}

if (-not $SkipTests -and -not $SkipRustTests) {
    $commandResults.Add((Invoke-FeedbackCommand -Name "cargo test" -Executable $cargo -Arguments @("test", "--manifest-path", "src-tauri/Cargo.toml", "-j", "1") -OutputPath (Join-Path $commandDirectory "cargo-test.txt")))
}

if (-not $SkipBuild) {
    $commandResults.Add((Invoke-FeedbackCommand -Name "tsc noEmit" -Executable $npx -Arguments @("tsc", "--noEmit") -OutputPath (Join-Path $commandDirectory "tsc-noemit.txt")))
    $commandResults.Add((Invoke-FeedbackCommand -Name "npm run build" -Executable $npm -Arguments @("run", "build") -OutputPath (Join-Path $commandDirectory "npm-build.txt")))
}

$appData = Copy-AppDataDiagnostics -SourcePath $AppDataPath -DestinationPath $appDataDirectory -Skip:$SkipAppData

$gitCommit = "unknown"
if ($git) {
    try {
        $gitCommit = (& $git "rev-parse" "HEAD" 2>$null | Select-Object -First 1).ToString()
    } catch {
        $gitCommit = "unknown"
    }
}

$manifest = [PSCustomObject]@{
    schemaVersion = 1
    createdAt     = (Get-Date).ToString("o")
    workspace     = $repoRoot
    gitCommit     = $gitCommit
    options       = [PSCustomObject]@{
        skipTests   = [bool]$SkipTests
        skipNodeTests = [bool]$SkipNodeTests
        skipRustTests = [bool]$SkipRustTests
        skipBuild   = [bool]$SkipBuild
        skipAppData = [bool]$SkipAppData
        noArchive   = [bool]$NoArchive
    }
    appData       = $appData
    commands      = @($commandResults.ToArray())
}

Write-Utf8NoBom -Path (Join-Path $runDirectory "manifest.json") -Content ($manifest | ConvertTo-Json -Depth 8)
Write-Summary -Path (Join-Path $runDirectory "summary.md") -RunDirectory $runDirectory -Manifest $manifest

if ($NoArchive) {
    Write-Output "Feedback directory: $runDirectory"
    exit 0
}

$archivePath = "$runDirectory.zip"
if (Test-Path -LiteralPath $archivePath) {
    Remove-Item -LiteralPath $archivePath -Force
}
Compress-Archive -Path (Join-Path $runDirectory "*") -DestinationPath $archivePath -Force

Write-Output "Feedback directory: $runDirectory"
Write-Output "Feedback package: $archivePath"
