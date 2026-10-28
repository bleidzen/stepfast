# Per-user Start menu shortcut and .step/.stp file association for StepFast. No admin needed.
# Run again after moving the folder.
$exe = (Resolve-Path "$PSScriptRoot\..\StepFast.exe").Path
$root = Split-Path $exe

$lnk = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\StepFast.lnk"
$shell = New-Object -ComObject WScript.Shell
$s = $shell.CreateShortcut($lnk)
$s.TargetPath = $exe
$s.WorkingDirectory = $root
$s.IconLocation = "$exe,0"
$s.Description = "STEP / FAST viewer"
$s.Save()

$classes = "HKCU:\Software\Classes"
$progId = "StepFast.step"
New-Item "$classes\$progId\DefaultIcon" -Force | Out-Null
New-Item "$classes\$progId\shell\open\command" -Force | Out-Null
Set-Item "$classes\$progId" "STEP 3D model"
Set-Item "$classes\$progId\DefaultIcon" "`"$exe`",0"
Set-Item "$classes\$progId\shell\open\command" "`"$exe`" `"%1`""

New-Item "$classes\Applications\StepFast.exe\shell\open\command" -Force | Out-Null
New-Item "$classes\Applications\StepFast.exe\SupportedTypes" -Force | Out-Null
Set-ItemProperty "$classes\Applications\StepFast.exe" FriendlyAppName "StepFast"
Set-Item "$classes\Applications\StepFast.exe\shell\open\command" "`"$exe`" `"%1`""

foreach ($ext in ".step", ".stp") {
    New-Item "$classes\$ext\OpenWithProgids" -Force | Out-Null
    Set-Item "$classes\$ext" $progId
    New-ItemProperty "$classes\$ext\OpenWithProgids" $progId -Value "" -PropertyType String -Force | Out-Null
    New-ItemProperty "$classes\Applications\StepFast.exe\SupportedTypes" $ext -Value "" -PropertyType String -Force | Out-Null
}

# Tell Explorer the associations changed so icons refresh.
Add-Type -Namespace Win32 -Name Shell -MemberDefinition '[DllImport("shell32.dll")] public static extern void SHChangeNotify(int e, int f, IntPtr a, IntPtr b);'
[Win32.Shell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
"Registered $exe"
