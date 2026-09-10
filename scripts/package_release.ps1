# Empaqueta los artefactos de distribución de YappyLike.
#
# Ejecuta ESTO DESPUÉS de `npm run tauri build`. Toma el instalador NSIS y el
# ejecutable de `src-tauri/target/release/` y produce en `dist-release/`, con la
# **versión en el nombre** para no pisar builds anteriores:
#   - YappyLike_Setup_x64_v<versión>.exe      (instalador NSIS renombrado)
#   - YappyLike_Portable_x64_v<versión>.zip   (versión portable con marcador PORTABLE)
#
# Uso:  pwsh scripts/package_release.ps1   (o powershell -File scripts/package_release.ps1)

$ErrorActionPreference = "Stop"

$root       = Split-Path -Parent $PSScriptRoot
$releaseDir = Join-Path $root "src-tauri/target/release"
$exe        = Join-Path $releaseDir "yappylike.exe"
$distDir    = Join-Path $root "dist-release"

if (-not (Test-Path $exe)) {
    throw "No existe $exe. Ejecuta primero: npm run tauri build"
}

# Versión desde tauri.conf.json (para nombrar los artefactos sin pisar versiones).
$conf    = Get-Content (Join-Path $root "src-tauri/tauri.conf.json") -Raw | ConvertFrom-Json
$version = $conf.version
Write-Host "Versión: $version"

New-Item -ItemType Directory -Force -Path $distDir | Out-Null

# --- 1. Instalador NSIS -> nombre de distribución ------------------------------
$setup = Get-ChildItem -Path (Join-Path $releaseDir "bundle/nsis") -Filter "*-setup.exe" -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime | Select-Object -Last 1
if ($setup) {
    $dest = Join-Path $distDir "YappyLike_Setup_x64_v$version.exe"
    Copy-Item $setup.FullName $dest -Force
    Write-Host "Instalador:  $dest  ($([math]::Round($setup.Length/1MB,1)) MB)"
} else {
    Write-Warning "No se encontró el instalador NSIS (bundle/nsis/*-setup.exe)."
}

# --- 2. Versión portable -------------------------------------------------------
$stage = Join-Path $distDir "YappyLike_Portable_x64_v$version"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Copy-Item $exe (Join-Path $stage "YappyLike.exe") -Force

# Marcador de modo portable: la app guarda config/models/logs junto al exe y no
# escribe fuera de esta carpeta.
Set-Content -Path (Join-Path $stage "PORTABLE") -Value "" -NoNewline -Encoding ascii
foreach ($d in @("models", "config", "logs")) {
    New-Item -ItemType Directory -Force -Path (Join-Path $stage $d) | Out-Null
}

$leeme = @"
YappyLike (portable)

1. Ejecuta YappyLike.exe. En el primer arranque descarga el modelo de voz
   (~400 MB) dentro de la carpeta models\.
2. Selecciona texto en cualquier app y pulsa Ctrl+Shift+Espacio para escucharlo.

Todo (config, modelo y logs) se guarda en esta misma carpeta; no se escribe fuera.
Requiere WebView2 Runtime instalado (viene con Windows 11 / Edge).
El archivo vacio PORTABLE activa este modo; si lo borras, la app usa %APPDATA%.
"@
Set-Content -Path (Join-Path $stage "LEEME.txt") -Value $leeme -Encoding utf8

$zip = Join-Path $distDir "YappyLike_Portable_x64_v$version.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
# Compress-Archive descarta los directorios vacios; usamos .NET para preservar
# la estructura models/ · config/ · logs/ tal cual la documenta el README.
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::Open($zip, 'Create')
try {
    foreach ($f in Get-ChildItem $stage -Recurse -File) {
        $rel = $f.FullName.Substring($stage.Length + 1).Replace('\', '/')
        [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $f.FullName, $rel) | Out-Null
    }
    foreach ($dir in Get-ChildItem $stage -Recurse -Directory) {
        $rel = $dir.FullName.Substring($stage.Length + 1).Replace('\', '/') + '/'
        $archive.CreateEntry($rel) | Out-Null
    }
} finally {
    $archive.Dispose()
}
Write-Host "Portable:    $zip"

Write-Host "`nArtefactos listos en $distDir"
