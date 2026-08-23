[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidatePattern('^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$')]
    [string]$Version
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-NativeCommand {
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,

        [Parameter()]
        [string[]]$Arguments = @()
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$FilePath failed with exit code $LASTEXITCODE."
    }
}

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
Push-Location $repoRoot

try {
    $tag = "v$Version"
    $expectedRemote = 'github.com/selfsecuredlab/lyrics-status'

    $currentBranch = (& git branch --show-current).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Could not determine the current Git branch.' }
    if ($currentBranch -ne 'main') { throw "Releases must be created from main, not $currentBranch." }

    $remoteUrl = (& git remote get-url origin).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Could not read the origin remote.' }
    if ($remoteUrl -notlike "*$expectedRemote*") {
        throw "The origin remote is not the LyricsStatus repository: $remoteUrl"
    }

    $trackedChanges = @(& git status --porcelain --untracked-files=no)
    if ($LASTEXITCODE -ne 0) { throw 'Could not inspect the Git worktree.' }
    if ($trackedChanges.Count -gt 0) {
        throw 'Commit or stash tracked changes before creating a release.'
    }

    Write-Host "Checking origin/main and release tags..." -ForegroundColor Cyan
    Invoke-NativeCommand -FilePath 'git' -Arguments @('fetch', 'origin', 'main', '--tags')

    $localCommit = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Could not read the local commit.' }
    $remoteCommit = (& git rev-parse origin/main).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Could not read origin/main.' }
    if ($localCommit -ne $remoteCommit) {
        throw 'Local main must exactly match origin/main before releasing. Push or pull first.'
    }

    & git show-ref --verify --quiet "refs/tags/$tag"
    if ($LASTEXITCODE -eq 0) { throw "Tag $tag already exists." }
    if ($LASTEXITCODE -ne 1) { throw "Could not check whether tag $tag exists." }

    $currentVersion = (& node -p "require('./package.json').version").Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Could not read the current package version.' }
    if ([version]$Version -le [version]$currentVersion) {
        throw "Version $Version must be newer than $currentVersion."
    }

    Write-Host "Compiling LyricsStatus before the release..." -ForegroundColor Cyan
    Invoke-NativeCommand -FilePath 'npm.cmd' -Arguments @('run', 'build')

    Write-Host "Preparing $tag..." -ForegroundColor Cyan
    Invoke-NativeCommand -FilePath 'npm.cmd' -Arguments @('version', $Version, '--no-git-tag-version')
    Invoke-NativeCommand -FilePath 'git' -Arguments @('add', '--', 'package.json', 'package-lock.json')
    Invoke-NativeCommand -FilePath 'git' -Arguments @('commit', '-m', "Release $tag")
    Invoke-NativeCommand -FilePath 'git' -Arguments @('tag', '-a', $tag, '-m', "LyricsStatus $tag")

    Write-Host "Publishing $tag..." -ForegroundColor Cyan
    Invoke-NativeCommand -FilePath 'git' -Arguments @('push', '--atomic', 'origin', 'main', $tag)

    Write-Host ''
    Write-Host "Release pipeline started for $tag." -ForegroundColor Green
    Write-Host "App build: https://github.com/selfsecuredlab/lyrics-status/actions/workflows/release.yml"
    Write-Host "Release:   https://github.com/selfsecuredlab/lyrics-status/releases/tag/$tag"
    Write-Host "Website:   https://github.com/selfsecuredlab/lyrics-status-website/actions/workflows/refresh-virustotal.yml"
    Write-Host ''
    Write-Host 'GitHub will build both Windows executables, publish their checksums, refresh VirusTotal, and redeploy selfsecured.me.'
}
finally {
    Pop-Location
}
