param(
    [ValidateSet('Verify', 'Core', 'Full')]
    [string]$Mode = 'Verify',
    [string]$RepositoryRoot = (Split-Path -Parent $PSScriptRoot),
    [string]$Workspace
)

$ErrorActionPreference = 'Stop'
$root = [System.IO.Path]::GetFullPath($RepositoryRoot)
$commit = (& git -C $root rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $commit -notmatch '^[0-9a-f]{40}$') {
    throw "Cannot resolve source commit below $root"
}
if (-not [string]::IsNullOrWhiteSpace((& git -C $root status --porcelain))) {
    throw 'Phase 2 reproduction requires a clean source checkout'
}

if ([string]::IsNullOrWhiteSpace($Workspace)) {
    $stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ')
    $Workspace = Join-Path $root "target/phase2-reproduction-$stamp"
}
$workspaceRoot = [System.IO.Path]::GetFullPath($Workspace)
if (Test-Path -LiteralPath $workspaceRoot) {
    throw "Reproduction workspace already exists: $workspaceRoot"
}
$cloneRoot = Join-Path $workspaceRoot 'repository'
New-Item -ItemType Directory -Path $workspaceRoot | Out-Null

function Invoke-NativeChecked {
    param(
        [Parameter(Mandatory)] [string]$Command,
        [Parameter(Mandatory)] [string[]]$Arguments
    )
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command failed with exit code $LASTEXITCODE"
    }
}

function Get-FileSha256 {
    param([Parameter(Mandatory)] [string]$Path)
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-NormalizedArtifact {
    param(
        [Parameter(Mandatory)] [string]$Path,
        [Parameter(Mandatory)] [string]$Kind
    )
    $text = [System.IO.File]::ReadAllText($Path).Replace("`r`n", "`n")
    switch ($Kind) {
        'property' {
            return [regex]::Replace(
                $text,
                '(?m)^elapsed_seconds=.*$',
                'elapsed_seconds=<platform-dependent>'
            )
        }
        'rejection' {
            $text = [regex]::Replace(
                $text,
                '(?m)^- Source SHA-256: `[^`]+`$',
                '- Source SHA-256: `<runtime-dependent>`'
            )
            $text = [regex]::Replace(
                $text,
                '(?m)^- Total candidate time: .*$',
                '- Total candidate time: <platform-dependent>'
            )
            return [regex]::Replace(
                $text,
                '(?m)^- Mean candidate time: .*$',
                '- Mean candidate time: <platform-dependent>'
            )
        }
        default { throw "Unknown normalization kind: $Kind" }
    }
}

function Invoke-Benchfck {
    param([Parameter(Mandatory)] [string[]]$Arguments)
    $cargoArguments = @(
        'run', '--release', '--locked', '--manifest-path',
        (Join-Path $cloneRoot 'Cargo.toml'), '--'
    ) + $Arguments
    Invoke-NativeChecked -Command 'cargo' -Arguments $cargoArguments
}

Invoke-NativeChecked -Command 'git' -Arguments @(
    'clone', '--no-local', '--no-checkout', $root, $cloneRoot
)
Invoke-NativeChecked -Command 'git' -Arguments @(
    '-C', $cloneRoot, 'checkout', '--detach', $commit
)

$checks = [System.Collections.Generic.List[object]]::new()
$fatalError = $null
Push-Location $cloneRoot
try {
    Invoke-NativeChecked -Command 'pwsh' -Arguments @(
        '-NoProfile', '-File', (Join-Path $cloneRoot 'scripts/verify-evidence.ps1'),
        '-RepositoryRoot', $cloneRoot
    )
    Invoke-Benchfck -Arguments @(
        'validate', '--input', 'evidence/batch-100-arity1.jsonl'
    )
    $checks.Add([ordered]@{
        artifact = 'published checkout'
        comparison = 'manifest_and_public_contract'
        status = 'PASS'
    })

    if ($Mode -in @('Core', 'Full')) {
        New-Item -ItemType Directory -Path '.private' -Force | Out-Null
        New-Item -ItemType Directory -Path 'target' -Force | Out-Null

        Invoke-Benchfck -Arguments @(
            'generate', '--seed', '42', '--count', '100', '--difficulty', 'hard',
            '--arity', '1', '--output', 'evidence/batch-100-arity1.jsonl',
            '--artifact-class', 'evidence',
            '--private-output', '.private/batch-100-arity1-private.jsonl',
            '--max-per-cell', '7'
        )
        Invoke-Benchfck -Arguments @(
            'matched-pairs', '--private', '.private/batch-100-arity1-private.jsonl',
            '--output', 'evidence/matched-pairs.csv', '--artifact-class', 'evidence'
        )
        Invoke-Benchfck -Arguments @(
            'budget-pilot', '--input', 'evidence/batch-100-arity1.jsonl',
            '--output', 'evidence/budget-pilot.jsonl', '--artifact-class', 'evidence'
        )
        Invoke-Benchfck -Arguments @(
            'near-duplicate-protocol', '--output', 'evidence/near-duplicate-protocol.md',
            '--artifact-class', 'evidence'
        )
        Invoke-Benchfck -Arguments @(
            'duplicate-audit', '--private', '.private/batch-100-arity1-private.jsonl',
            '--protocol', 'evidence/near-duplicate-protocol.md',
            '--output', 'evidence/duplicate-audit.md', '--artifact-class', 'evidence'
        )
        Invoke-Benchfck -Arguments @(
            'carrier-pilot', '--private', '.private/batch-100-arity1-private.jsonl',
            '--output', 'evidence/carrier-pilot.md', '--artifact-class', 'evidence'
        )
        Invoke-Benchfck -Arguments @(
            'leak-scan', '--public', 'evidence/batch-100-arity1.jsonl',
            '--private', '.private/batch-100-arity1-private.jsonl',
            '--output', 'evidence/leak-scan.md', '--artifact-class', 'evidence'
        )

        $exactArtifacts = @(
            'evidence/batch-100-arity1.jsonl',
            'evidence/budget-pilot.jsonl',
            'evidence/carrier-pilot.md',
            'evidence/duplicate-audit.md',
            'evidence/leak-scan.md',
            'evidence/matched-pairs.csv',
            'evidence/near-duplicate-protocol.md'
        )
        foreach ($relative in $exactArtifacts) {
            $expectedHash = Get-FileSha256 -Path (Join-Path $root $relative)
            $actualHash = Get-FileSha256 -Path (Join-Path $cloneRoot $relative)
            $checks.Add([ordered]@{
                artifact = $relative.Replace('\', '/')
                comparison = 'exact_sha256'
                expected_sha256 = $expectedHash
                actual_sha256 = $actualHash
                status = if ($expectedHash -eq $actualHash) { 'PASS' } else { 'FAIL' }
            })
        }
    }

    if ($Mode -eq 'Full') {
        Invoke-Benchfck -Arguments @(
            'probe', '--seed', '42', '--count', '1000', '--candidates', '500',
            '--difficulty', 'hard', '--arity', '1',
            '--output', 'target/rejection-probe-500.jsonl'
        )
        Invoke-Benchfck -Arguments @(
            'rejection-histogram', '--input', 'target/rejection-probe-500.jsonl',
            '--output', 'evidence/rejection-histogram.md', '--probe-seed', '42',
            '--probe-count', '1000', '--probe-arity', '1',
            '--probe-difficulty', 'hard', '--artifact-class', 'evidence'
        )
        Invoke-Benchfck -Arguments @(
            'property10k', '--output', 'evidence/property-10k.log',
            '--artifact-class', 'evidence'
        )

        foreach ($runtimeArtifact in @(
            @{ Path = 'evidence/property-10k.log'; Kind = 'property' },
            @{ Path = 'evidence/rejection-histogram.md'; Kind = 'rejection' }
        )) {
            $expected = Get-NormalizedArtifact `
                -Path (Join-Path $root $runtimeArtifact.Path) -Kind $runtimeArtifact.Kind
            $actual = Get-NormalizedArtifact `
                -Path (Join-Path $cloneRoot $runtimeArtifact.Path) -Kind $runtimeArtifact.Kind
            $checks.Add([ordered]@{
                artifact = $runtimeArtifact.Path
                comparison = 'normalized_runtime_fields'
                status = if ($expected -ceq $actual) { 'PASS' } else { 'FAIL' }
            })
        }
    }

    Invoke-NativeChecked -Command 'pwsh' -Arguments @(
        '-NoProfile', '-File', (Join-Path $cloneRoot 'scripts/verify-evidence.ps1'),
        '-RepositoryRoot', $cloneRoot
    )
}
catch {
    $fatalError = $_.Exception.Message
    $checks.Add([ordered]@{
        artifact = 'reproduction process'
        comparison = 'command_execution'
        status = 'FAIL'
        detail = $fatalError
    })
}
finally {
    Pop-Location
}

$rustcVersion = (& rustc --version).Trim()
$cargoVersion = (& cargo --version).Trim()
$failed = @($checks | Where-Object { $_.status -ne 'PASS' })
$report = [ordered]@{
    schema_version = 'benchfck.phase2-reproduction.v1'
    status = if ($failed.Count -eq 0) { 'PASS' } else { 'FAIL' }
    mode = $Mode
    source_commit = $commit
    generated_at_utc = [DateTime]::UtcNow.ToString('o')
    platform = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    powershell = $PSVersionTable.PSVersion.ToString()
    rustc = $rustcVersion
    cargo = $cargoVersion
    clean_checkout = $cloneRoot
    checks = $checks
    limitations = @(
        'Verify mode validates a clean checkout but does not regenerate evidence.',
        'Core mode regenerates the seven byte-deterministic Phase 2 artifacts.',
        'Full mode normalizes documented runtime-only fields in two timing-sensitive artifacts.',
        'Independent reproduction still requires a third party to run and sign this protocol.'
    )
}
$reportPath = Join-Path $workspaceRoot 'reproduction-report.json'
$report | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $reportPath -Encoding utf8
if ($failed.Count -ne 0) {
    throw "Phase 2 reproduction failed ($fatalError); see $reportPath"
}
Write-Output "Phase 2 $Mode reproduction PASS at commit $commit"
Write-Output "Report: $reportPath"
