# Compila o audio-capture.exe com o MSVC (Visual Studio Build Tools, carga C++).
# Saída: app/resources/bin/audio-capture.exe (CRT estático: não precisa de VC++ Redist).
$ErrorActionPreference = 'Stop'

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'Visual Studio Build Tools com C++ não encontrado.' }
$vcvars = Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'

$root = $PSScriptRoot
$buildDir = Join-Path $root 'build'
$outDir = [IO.Path]::GetFullPath((Join-Path $root '..\..\app\resources\bin'))
New-Item -ItemType Directory -Force $buildDir, $outDir | Out-Null

$sources = (Get-ChildItem (Join-Path $root 'src') -Filter *.cpp | ForEach-Object { "`"$($_.FullName)`"" }) -join ' '
$flags = '/nologo /O2 /EHsc /std:c++17 /W4 /MT /DUNICODE /D_UNICODE /D_WIN32_WINNT=0x0A00 /DNTDDI_VERSION=0x0A00000A'
$libs = 'ole32.lib mmdevapi.lib avrt.lib winmm.lib'
$cmd = "`"$vcvars`" >nul && cl $flags /Fo`"$buildDir\\`" $sources /Fe`"$outDir\audio-capture.exe`" /link $libs"

cmd /c $cmd
if ($LASTEXITCODE -ne 0) { throw 'Falha ao compilar o audio-capture.exe' }
Write-Host "OK: $outDir\audio-capture.exe"
