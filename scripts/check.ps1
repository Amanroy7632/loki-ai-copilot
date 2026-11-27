# Runs the same checks as CI. Use it before every commit.
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

function Invoke-Step([string]$Name, [scriptblock]$Command) {
    Write-Host "==> $Name"
    & $Command
    if ($LASTEXITCODE -ne 0) { throw "$Name failed (exit $LASTEXITCODE)" }
}

Invoke-Step 'cargo fmt' { cargo fmt --all -- --check }
Invoke-Step 'cargo clippy' { cargo clippy --workspace --all-targets -- -D warnings }
Invoke-Step 'cargo clippy (ts feature)' { cargo clippy -p loki-core --features ts --all-targets -- -D warnings }
Invoke-Step 'cargo test' { cargo test --workspace }

# The contract staleness check is a POSIX script; CI runs it on Linux. Run it from Git Bash if needed.
if (Get-Command cargo-deny -ErrorAction SilentlyContinue) {
    Invoke-Step 'cargo deny' { cargo deny check }
} else {
    Write-Host "==> skipping cargo deny: install it with 'cargo install cargo-deny --locked'"
}

if (Test-Path 'apps/desktop/package.json') {
    Invoke-Step 'pnpm lint' { pnpm -r --if-present lint }
    Invoke-Step 'pnpm test' { pnpm -r --if-present test }
}

Write-Host 'All checks passed.'
