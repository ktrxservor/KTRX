# Installe ktrx-dev (l'outil de l'équipe KTRX) depuis la dernière release GitHub, déjà compilé.
#
#   irm https://raw.githubusercontent.com/ktrxservor/KTRX/main/scripts/install-dev.ps1 | iex
#
# Variables facultatives : $env:KTRX_DEV_VERSION = "0.6.0" (sinon la dernière), $env:KTRX_INSTALL.
# Le programme est vérifié avec SHA256SUMS.txt de la release, installé dans %USERPROFILE%\.ktrx\bin,
# sans droits administrateur. Relancer la commande met à jour.

function Install-KtrxDev {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $repo = 'ktrxservor/KTRX'
    if ($env:KTRX_GITHUB_REPO) { $repo = $env:KTRX_GITHUB_REPO }
    $base = "https://github.com/$repo/releases/latest/download"
    if ($env:KTRX_DEV_VERSION) { $base = "https://github.com/$repo/releases/download/v$($env:KTRX_DEV_VERSION.TrimStart('v'))" }
    $root = Join-Path $env:USERPROFILE '.ktrx'
    if ($env:KTRX_INSTALL) { $root = $env:KTRX_INSTALL }
    $bin = Join-Path $root 'bin'
    $file = 'ktrx-dev-windows.zip'

    Write-Host 'Installation de ktrx-dev (windows)'
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('ktrx-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $sums = (Invoke-WebRequest -Uri "$base/SHA256SUMS.txt" -UseBasicParsing).Content
        if ($sums -is [byte[]]) { $sums = [Text.Encoding]::UTF8.GetString($sums) }
        $line = ($sums -split "`n") | Where-Object { $_ -match "\s\*?$([regex]::Escape($file))\s*$" } | Select-Object -First 1
        if (-not $line) { throw "$file absent de SHA256SUMS.txt" }
        $expected = ($line -split '\s+')[0].ToLowerInvariant()

        $zip = Join-Path $tmp $file
        Invoke-WebRequest -Uri "$base/$file" -OutFile $zip -UseBasicParsing
        if ((Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant() -ne $expected) {
            throw 'empreinte SHA-256 inattendue : téléchargement corrompu'
        }
        Expand-Archive -Path $zip -DestinationPath (Join-Path $tmp 'x') -Force

        New-Item -ItemType Directory -Force -Path $bin | Out-Null
        $exe = Join-Path $bin 'ktrx-dev.exe'
        if (Test-Path $exe) {
            $old = Join-Path $bin 'ktrx-dev.old.exe'
            Remove-Item $old -Force -ErrorAction SilentlyContinue
            Move-Item $exe $old -Force
        }
        Copy-Item (Join-Path $tmp 'x\ktrx-dev.exe') $exe -Force
        Unblock-File $exe -ErrorAction SilentlyContinue
        Remove-Item (Join-Path $bin 'ktrx-dev.old.exe') -Force -ErrorAction SilentlyContinue
        Write-Host "$((& $exe --version | Out-String).Trim()) installé : $exe"

        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        $entries = @(); if ($userPath) { $entries = $userPath -split ';' | Where-Object { $_ } }
        if (-not (($entries | ForEach-Object { $_.TrimEnd('\') }) -contains $bin.TrimEnd('\'))) {
            [Environment]::SetEnvironmentVariable('Path', (@($bin) + $entries) -join ';', 'User')
        }
        if (-not (($env:Path -split ';') -contains $bin)) { $env:Path = "$bin;$env:Path" }
        Write-Host 'Ensuite : ktrx-dev login, puis ktrx-dev doctor.'
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
}

try { Install-KtrxDev } catch { Write-Host "Erreur : $($_.Exception.Message)" -ForegroundColor Red }
