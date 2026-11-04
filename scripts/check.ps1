# Format, lint, test, and build the plugin for WebAssembly. Run before every
# commit. Builds at low priority (two jobs, below-normal priority).
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
if (-not $env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = '2' }
try { (Get-Process -Id $PID).PriorityClass = 'BelowNormal' } catch { }
function Run([string]$what) {
    Invoke-Expression $what
    if ($LASTEXITCODE -ne 0) { throw "failed: $what" }
}
Run 'cargo fmt --all --check'
Run 'cargo clippy --workspace --all-targets -- -D warnings'
Run 'cargo test --workspace'
Run 'cargo build -p goarrow --release --target wasm32-unknown-unknown'
Write-Output 'plugin: target/wasm32-unknown-unknown/release/goarrow.wasm (manifest: plugin.json)'
