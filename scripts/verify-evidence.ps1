param(
    [string]$RepositoryRoot = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = 'Stop'
$root = [System.IO.Path]::GetFullPath($RepositoryRoot)
$evidenceRoot = Join-Path $root 'evidence'
$manifestPath = Join-Path $evidenceRoot 'MANIFEST.txt'

if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw "Missing evidence manifest: $manifestPath"
}

$declared = @{}
foreach ($line in Get-Content -LiteralPath $manifestPath) {
    if ([string]::IsNullOrWhiteSpace($line) -or $line.StartsWith('#')) {
        continue
    }
    if ($line -notmatch '^([0-9a-f]{64})  (evidence/.+)$') {
        throw "Malformed manifest line: $line"
    }
    $relative = $Matches[2]
    if ($relative -eq 'evidence/MANIFEST.txt') {
        throw 'MANIFEST.txt must not list itself'
    }
    $declared[$relative] = $Matches[1]
}

$actualFiles = Get-ChildItem -LiteralPath $evidenceRoot -File -Recurse |
    Where-Object { $_.FullName -ne $manifestPath -and $_.Name -ne 'README.md' }

foreach ($file in $actualFiles) {
    $relative = [System.IO.Path]::GetRelativePath($root, $file.FullName).Replace('\', '/')
    if (-not $declared.ContainsKey($relative)) {
        throw "Unmanifested evidence file: $relative"
    }
    $actualHash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $declared[$relative]) {
        throw "Hash mismatch: $relative"
    }
    $declared.Remove($relative)
}

if ($declared.Count -ne 0) {
    throw "Manifest references missing evidence files: $($declared.Keys -join ', ')"
}

Write-Output "Evidence manifest verified: $($actualFiles.Count) artifact(s)."
