param([string]$Python = 'python')
$ErrorActionPreference = 'Stop'
$runtime = Join-Path $PSScriptRoot 'occt-runtime'
$embedded = Join-Path $runtime 'python'
$packages = Join-Path $runtime 'packages'
$archive = Join-Path $env:TEMP 'stepfast-python-3.12.10.zip'
New-Item -ItemType Directory -Force -Path $embedded, $packages | Out-Null
Invoke-WebRequest 'https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip' -OutFile $archive
Expand-Archive -LiteralPath $archive -DestinationPath $embedded -Force
Set-Content -LiteralPath (Join-Path $embedded 'python312._pth') -Encoding ascii -Value @('python312.zip', '.', '../packages')
& $Python -m pip install --target $packages 'cadquery-ocp==7.8.1.1.post1'
if ($LASTEXITCODE -ne 0) { throw 'Installationen av OpenCascade misslyckades. Använd Python 3.12 x64 med pip.' }
& (Join-Path $embedded 'python.exe') -c 'from OCP.STEPControl import STEPControl_Reader; print("OpenCascade redo")'
if ($LASTEXITCODE -ne 0) { throw 'OpenCascade kunde inte laddas.' }
