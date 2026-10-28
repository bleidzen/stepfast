param([switch]$Test, [switch]$NoDeploy, [string]$TestFile)
$ErrorActionPreference = 'Stop'
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
$env:PATH = "$cargoBin;$env:PATH"
$sysroot = & rustc +stable-x86_64-pc-windows-gnu --print sysroot
if ($LASTEXITCODE -ne 0) { throw 'Installera Rust med x86_64-pc-windows-gnu först.' }
$env:PATH = "$sysroot\lib\rustlib\x86_64-pc-windows-gnu\bin;$env:PATH"
$env:RUSTFLAGS = '-C linker=rust-lld -C link-self-contained=yes'
if ($TestFile) { $env:STEP_FAST_TEST_FILE = $TestFile; & cargo +stable-x86_64-pc-windows-gnu test --release --manifest-path "$PSScriptRoot\Cargo.toml" external_step_file -- --ignored --nocapture }
elseif ($Test) { & cargo +stable-x86_64-pc-windows-gnu test --release --manifest-path "$PSScriptRoot\Cargo.toml" }
else { & cargo +stable-x86_64-pc-windows-gnu build --release --manifest-path "$PSScriptRoot\Cargo.toml" }
if ($LASTEXITCODE -ne 0) { throw 'Bygget misslyckades.' }
if (-not $Test -and -not $TestFile -and -not $NoDeploy) { Copy-Item -LiteralPath "$PSScriptRoot\target\release\step-fast.exe" -Destination "$PSScriptRoot\..\StepFast.exe" -Force }
