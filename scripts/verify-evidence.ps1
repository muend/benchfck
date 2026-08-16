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

$publicSchemaPath = Join-Path $root 'schemas/public-item.schema.json'
$publicSchema = Get-Content -LiteralPath $publicSchemaPath -Raw | ConvertFrom-Json -Depth 100
if ($publicSchema.properties.program_size_tier.minimum -ne 0 -or
    $publicSchema.properties.program_size_tier.maximum -ne 9) {
    throw 'Public item schema must admit exactly the declared size tiers 0 through 9'
}

$publicBatchPath = Join-Path $evidenceRoot 'batch-100-arity1.jsonl'
if (-not (Test-Path -LiteralPath $publicBatchPath -PathType Leaf)) {
    throw "Missing published public batch: $publicBatchPath"
}

$metadataIds = @{}
$taskIds = @{}
$taskItemIds = [System.Collections.Generic.HashSet[string]]::new()
$tiers = [System.Collections.Generic.HashSet[int]]::new()
$metadataCount = 0
$taskCount = 0

foreach ($line in Get-Content -LiteralPath $publicBatchPath) {
    $record = $line | ConvertFrom-Json -Depth 100
    switch ($record.record_type) {
        'public_item_metadata' {
            $metadataCount++
            $itemId = [string]$record.data.item_id
            if ($metadataIds.ContainsKey($itemId)) {
                throw "Duplicate public metadata item_id: $itemId"
            }
            $metadataIds[$itemId] = $true

            $tier = [int]$record.data.program_size_tier
            if ($tier -lt 0 -or $tier -gt 9) {
                throw "Public item $itemId has out-of-contract size tier: $tier"
            }
            $null = $tiers.Add($tier)
        }
        'task' {
            $taskCount++
            $taskId = [string]$record.data.task_id
            if ($taskIds.ContainsKey($taskId)) {
                throw "Duplicate public task_id: $taskId"
            }
            $taskIds[$taskId] = $true
            $null = $taskItemIds.Add([string]$record.data.item_id)
        }
        default {
            throw "Public batch contains inadmissible record type: $($record.record_type)"
        }
    }
}

if ($metadataCount -ne 100 -or $taskCount -ne 1510) {
    throw "Unexpected public batch shape: $metadataCount metadata records, $taskCount task records"
}
if (($tiers | Sort-Object) -join ',' -ne '0,1,2,3,4,5,6,7,8,9') {
    throw "Published batch does not occupy every declared size tier: $(($tiers | Sort-Object) -join ',')"
}
foreach ($itemId in $taskItemIds) {
    if (-not $metadataIds.ContainsKey($itemId)) {
        throw "Task references missing public metadata item_id: $itemId"
    }
}
foreach ($itemId in $metadataIds.Keys) {
    if (-not $taskItemIds.Contains($itemId)) {
        throw "Public metadata item has no tasks: $itemId"
    }
}

Write-Output "Evidence manifest verified: $($actualFiles.Count) artifact(s)."
Write-Output "Published batch contract verified: $metadataCount items, $taskCount tasks, tiers 0-9."
