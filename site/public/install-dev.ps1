# Installateur de ktrx-dev (l'outil de l'équipe KTRX) pour Windows (PowerShell 5.1 ou plus récent, sans droits administrateur).
#
#   irm https://ktrx-site.tiagoantunes2009.workers.dev/install-dev.ps1 | iex
#
# Variables facultatives (à définir avant la commande) :
#   $env:KTRX_VERSION = "0.5.3"        installe cette version au lieu de la dernière
#   $env:KTRX_INSTALL = "C:\chemin"    dossier d'installation (défaut : %USERPROFILE%\.ktrx ; le programme va dans bin\)
#   $env:KTRX_NO_MODIFY_PATH = "1"     ne modifie pas le PATH de l'utilisateur
#
# Le fichier téléchargé est vérifié (SHA-256 publié par le registre) avant d'être installé.

function Install-Ktrx {
    $ErrorActionPreference = 'Stop'
    # The progress bar of Windows PowerShell 5.1 slows downloads down a lot.
    $ProgressPreference = 'SilentlyContinue'
    # Windows PowerShell 5.1 may default to TLS 1.0, which the registry refuses.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $registry = 'https://ktrx-site.tiagoantunes2009.workers.dev'
    if ($env:KTRX_REGISTRY) { $registry = $env:KTRX_REGISTRY.TrimEnd('/') }
    $root = Join-Path $env:USERPROFILE '.ktrx'
    if ($env:KTRX_INSTALL) { $root = $env:KTRX_INSTALL }
    $bin = Join-Path $root 'bin'

    Write-Host 'Installation de ktrx-dev'

    if (-not [Environment]::Is64BitOperatingSystem) {
        throw 'KTRX nécessite une version 64 bits de Windows.'
    }
    if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64' -or $env:PROCESSOR_ARCHITEW6432 -eq 'ARM64') {
        Write-Host 'PC ARM : la version x64 de KTRX sera utilisée (émulation x64 de Windows 11).'
    }

    $url = "$registry/dl/dev/windows"
    if ($env:KTRX_VERSION) { $url = "$url`?version=$($env:KTRX_VERSION)" }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('ktrx-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $zip = Join-Path $tmp 'ktrx-dev.zip'
        try {
            $response = Invoke-WebRequest -Uri $url -OutFile $zip -PassThru -UseBasicParsing
        } catch {
            throw "Téléchargement impossible depuis $registry ($($_.Exception.Message)). Vérifiez la connexion Internet."
        }

        $expected = ((@($response.Headers['x-ktrx-sha256']) -join '').Trim()).ToLowerInvariant()
        if (-not $expected) {
            throw "Le registre n'a pas publié d'empreinte SHA-256 pour ce fichier : installation annulée."
        }
        $actual = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLowerInvariant()
        if ($actual -ne $expected) {
            throw 'Le fichier téléchargé est corrompu (SHA-256 inattendu). Relancez l''installation.'
        }

        $unpacked = Join-Path $tmp 'x'
        Expand-Archive -Path $zip -DestinationPath $unpacked -Force
        $source = Join-Path $unpacked 'ktrx-dev.exe'
        if (-not (Test-Path $source)) { throw 'L''archive ne contient pas ktrx-dev.exe.' }

        New-Item -ItemType Directory -Force -Path $bin | Out-Null
        $exe = Join-Path $bin 'ktrx-dev.exe'
        if (Test-Path $exe) {
            # A running ktrx.exe cannot be overwritten, but it can be renamed.
            $old = Join-Path $bin 'ktrx-dev.old.exe'
            Remove-Item $old -Force -ErrorAction SilentlyContinue
            Move-Item $exe $old -Force
        }
        Copy-Item $source $exe -Force
        Unblock-File -Path $exe -ErrorAction SilentlyContinue
        Remove-Item (Join-Path $bin 'ktrx-dev.old.exe') -Force -ErrorAction SilentlyContinue

        $version = (& $exe --version 2>&1 | Out-String).Trim()
        if ($LASTEXITCODE -ne 0) {
            throw "ktrx-dev.exe a été installé mais ne démarre pas sur cette machine : $version"
        }
        Write-Host "$version installé : $exe"

        $inPath = ($env:Path -split ';' | ForEach-Object { $_.TrimEnd('\') }) -contains $bin.TrimEnd('\')
        if ($inPath) {
            Write-Host ''
            Write-Host "C'est prêt. Essayez :  ktrx-dev login, puis ktrx-dev guide"
        } elseif ($env:KTRX_NO_MODIFY_PATH -eq '1') {
            Write-Host ''
            Write-Host "Ajoutez $bin à votre PATH pour lancer ktrx-dev par son nom."
        } else {
            $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
            $entries = @()
            if ($userPath) { $entries = $userPath -split ';' | Where-Object { $_ } }
            if (-not (($entries | ForEach-Object { $_.TrimEnd('\') }) -contains $bin.TrimEnd('\'))) {
                [Environment]::SetEnvironmentVariable('Path', (@($bin) + $entries) -join ';', 'User')
            }
            $env:Path = "$bin;$env:Path"
            Write-Host ''
            Write-Host "PATH de l'utilisateur mis à jour. ktrx-dev est utilisable dans ce terminal et dans les nouveaux."
            Write-Host 'Essayez :  ktrx-dev login, puis ktrx-dev guide'
        }
    } finally {
        Remove-Item -Recurse -Force -Path $tmp -ErrorAction SilentlyContinue
    }
}

try {
    Install-Ktrx
} catch {
    Write-Host ''
    Write-Host "Erreur : $($_.Exception.Message)" -ForegroundColor Red
}
