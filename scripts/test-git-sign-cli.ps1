$ErrorActionPreference = 'Stop'

function Assert-Success($Result, $Message) {
    if ($Result.ExitCode -ne 0) {
        throw "$Message`nSTDOUT:`n$($Result.StdOut)`nSTDERR:`n$($Result.StdErr)"
    }
}

function Assert-Contains($Text, $Expected, $Message) {
    if (-not $Text.Contains($Expected)) {
        throw "$Message`nExpected to contain: $Expected`nActual:`n$Text"
    }
}

function Run-Command {
    param(
        [string]$FilePath,
        [string[]]$Arguments,
        [string]$WorkingDirectory = (Get-Location).Path
    )

    $stdoutFile = [System.IO.Path]::GetTempFileName()
    $stderrFile = [System.IO.Path]::GetTempFileName()

    try {
        $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
        $startInfo.FileName = $FilePath
        $startInfo.WorkingDirectory = $WorkingDirectory
        $startInfo.UseShellExecute = $false
        $startInfo.RedirectStandardOutput = $true
        $startInfo.RedirectStandardError = $true

        foreach ($argument in $Arguments) {
            [void]$startInfo.ArgumentList.Add($argument)
        }

        $process = [System.Diagnostics.Process]::Start($startInfo)
        $process.WaitForExit()

        [System.IO.File]::WriteAllText($stdoutFile, $process.StandardOutput.ReadToEnd())
        [System.IO.File]::WriteAllText($stderrFile, $process.StandardError.ReadToEnd())

        return [pscustomobject]@{
            ExitCode = $process.ExitCode
            StdOut = [System.IO.File]::ReadAllText($stdoutFile)
            StdErr = [System.IO.File]::ReadAllText($stderrFile)
        }
    }
    finally {
        Remove-Item $stdoutFile, $stderrFile -ErrorAction SilentlyContinue
    }
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$bwAgentExe = Join-Path $repoRoot 'target\debug\bw-agent.exe'

Write-Host '==> Building bw-agent binary'
$build = Run-Command -FilePath 'cargo' -Arguments @('build', '--package', 'bw-agent') -WorkingDirectory $repoRoot
Assert-Success $build 'cargo build failed'

if (-not (Test-Path $bwAgentExe)) {
    throw "Expected binary not found: $bwAgentExe"
}

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("bw-agent-git-sign-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tempDir | Out-Null

try {
    $keyBase = Join-Path $tempDir 'test_key'
    $dataFile = Join-Path $tempDir 'test_data.txt'
    $allowedSigners = Join-Path $tempDir 'allowed_signers'
    $signatureFile = "$dataFile.sig"

    Write-Host '==> Generating temporary SSH keypair'
    $keygen = Run-Command -FilePath 'ssh-keygen' -Arguments @('-t', 'ed25519', '-f', $keyBase, '-N', '', '-C', 'test@example.com') -WorkingDirectory $tempDir
    Assert-Success $keygen 'ssh-keygen key generation failed'

    Set-Content -Path $dataFile -Value 'hello git signing' -NoNewline

    Write-Host '==> Signing test data with ssh-keygen'
    $sign = Run-Command -FilePath 'ssh-keygen' -Arguments @('-Y', 'sign', '-n', 'git', '-f', $keyBase, $dataFile) -WorkingDirectory $tempDir
    Assert-Success $sign 'ssh-keygen signing failed'

    if (-not (Test-Path $signatureFile)) {
        throw "Signature file not created: $signatureFile"
    }

    $pubKey = Get-Content "$keyBase.pub" -Raw
    Set-Content -Path $allowedSigners -Value ("test@example.com " + $pubKey.Trim())

    Write-Host '==> Testing check-novalidate'
    $checkNoValidate = Run-Command -FilePath $bwAgentExe -Arguments @('git-sign', '-Y', 'check-novalidate', '-n', 'git', '-s', $signatureFile, $dataFile) -WorkingDirectory $repoRoot
    Assert-Success $checkNoValidate 'check-novalidate failed'
    Assert-Contains $checkNoValidate.StdOut 'Good "git" signature with ED25519 key SHA256:' 'check-novalidate output mismatch'

    Write-Host '==> Testing find-principals'
    $findPrincipals = Run-Command -FilePath $bwAgentExe -Arguments @('git-sign', '-Y', 'find-principals', '-f', $allowedSigners, '-s', $signatureFile, $dataFile) -WorkingDirectory $repoRoot
    Assert-Success $findPrincipals 'find-principals failed'
    Assert-Contains $findPrincipals.StdOut 'test@example.com' 'find-principals output mismatch'

    Write-Host '==> Testing verify'
    $verify = Run-Command -FilePath $bwAgentExe -Arguments @('git-sign', '-Y', 'verify', '-n', 'git', '-f', $allowedSigners, '-I', 'test@example.com', '-s', $signatureFile, $dataFile) -WorkingDirectory $repoRoot
    Assert-Success $verify 'verify failed'
    Assert-Contains $verify.StdOut 'Good "git" signature for test@example.com with ED25519 key SHA256:' 'verify output mismatch'

    Write-Host '==> Testing match-principals'
    $matchPrincipals = Run-Command -FilePath $bwAgentExe -Arguments @('git-sign', '-Y', 'match-principals', '-f', $allowedSigners, '-I', 'test@example.com') -WorkingDirectory $repoRoot
    Assert-Success $matchPrincipals 'match-principals failed'
    Assert-Contains $matchPrincipals.StdOut 'test@example.com' 'match-principals output mismatch'

    Write-Host ''
    Write-Host 'All git-sign CLI checks passed.' -ForegroundColor Green
}
finally {
    Remove-Item $tempDir -Recurse -Force -ErrorAction SilentlyContinue
}
