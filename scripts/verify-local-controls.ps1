param(
    [string]$RepositoryRoot = (Split-Path -Parent $PSScriptRoot),
    [string]$PrivateBatch,
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$root = [System.IO.Path]::GetFullPath($RepositoryRoot)
if ([string]::IsNullOrWhiteSpace($PrivateBatch)) {
    $PrivateBatch = Join-Path $root '.private/batch-100-arity1-private.jsonl'
}
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $root 'target/local-controls'
}
$privatePath = [System.IO.Path]::GetFullPath($PrivateBatch)
$outputRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
$evidenceRoot = [System.IO.Path]::GetFullPath((Join-Path $root 'evidence'))

if (-not (Test-Path -LiteralPath $privatePath -PathType Leaf)) {
    throw "Missing private batch: $privatePath"
}
if ($privatePath.StartsWith($evidenceRoot, [System.StringComparison]::OrdinalIgnoreCase) -or
    $outputRoot.StartsWith($evidenceRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Private control inputs and outputs must stay outside evidence/'
}

New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
$rawRecords = Get-Content -LiteralPath $privatePath
$firstItem = $null
foreach ($line in $rawRecords) {
    $record = $line | ConvertFrom-Json -Depth 100
    if ($record.record_type -eq 'item') {
        $firstItem = $record
        break
    }
}
if ($null -eq $firstItem) {
    throw 'Private batch contains no answer-bearing item record'
}

$itemId = [string]$firstItem.data.item_id
$subsetLines = [System.Collections.Generic.List[string]]::new()
$familyNames = [System.Collections.Generic.HashSet[string]]::new()
$taskCount = 0
foreach ($line in $rawRecords) {
    $record = $line | ConvertFrom-Json -Depth 100
    if (($record.record_type -eq 'item' -and $record.data.item_id -eq $itemId) -or
        ($record.record_type -eq 'task' -and $record.data.item_id -eq $itemId)) {
        $subsetLines.Add($line)
        if ($record.record_type -eq 'task') {
            $taskCount++
            $null = $familyNames.Add([string]$record.data.family)
        }
    }
}
if (($familyNames | Sort-Object) -join ',' -ne 'T1,T2,T3') {
    throw "Selected item is not family-complete: $(($familyNames | Sort-Object) -join ',')"
}

$subsetPath = Join-Path $outputRoot 'private-subset.jsonl'
[System.IO.File]::WriteAllLines($subsetPath, $subsetLines, [System.Text.UTF8Encoding]::new($false))

$solvers = @('perfect', 'off-by-one-pointer', 'drift-after-k', 'ignore-wrap')
$summary = [ordered]@{
    schema_version = 'benchfck.local-control-summary.v1'
    status = 'PASS'
    source_item_id = $itemId
    task_count = $taskCount
    families = @('T1', 'T2', 'T3')
    solvers = @()
}

foreach ($solver in $solvers) {
    $metricsPath = Join-Path $outputRoot "$solver-metrics.jsonl"
    & cargo run --release --locked --manifest-path (Join-Path $root 'Cargo.toml') -- `
        mock-run --input $subsetPath --output $metricsPath --solver $solver
    if ($LASTEXITCODE -ne 0) {
        throw "mock-run failed for solver $solver"
    }

    $metrics = @(Get-Content -LiteralPath $metricsPath | ForEach-Object {
        $_ | ConvertFrom-Json -Depth 100
    })
    if ($metrics.Count -ne $taskCount) {
        throw "$solver emitted $($metrics.Count) metrics for $taskCount tasks"
    }
    $metricFamilies = @($metrics | ForEach-Object { [string]$_.family } | Sort-Object -Unique)
    if (($metricFamilies -join ',') -ne 'T1,T2,T3') {
        throw "$solver output is not family-complete: $($metricFamilies -join ',')"
    }

    $correct = @($metrics | Where-Object { $_.correct }).Count
    $incorrect = $metrics.Count - $correct
    if ($solver -eq 'perfect' -and $incorrect -ne 0) {
        throw "Perfect control produced $incorrect incorrect metric(s)"
    }
    if ($solver -ne 'perfect' -and $incorrect -eq 0) {
        throw "Flawed control $solver was not detected"
    }

    $summary.solvers += [ordered]@{
        solver = $solver
        correct = $correct
        incorrect = $incorrect
        families = $metricFamilies
    }
}

$summaryPath = Join-Path $outputRoot 'control-summary.json'
$summary | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $summaryPath -Encoding utf8
Write-Output "Local perfect/flawed controls verified for $itemId across T1/T2/T3."
Write-Output "Diagnostic summary: $summaryPath"
