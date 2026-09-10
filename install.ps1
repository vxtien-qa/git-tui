<#
.SYNOPSIS
    Installs git-tui on Windows.

.DESCRIPTION
    Downloads the latest released binary, or builds from source with -Build.
    Build mode installs the Visual Studio C++ Build Tools and Rust if they are
    missing.

.EXAMPLE
    irm https://raw.githubusercontent.com/vxtien-qa/git-tui/master/install.ps1 | iex

.EXAMPLE
    .\install.ps1 -Build
#>
param(
    [switch]$Build
)

$ErrorActionPreference = "Stop"

$Repo = "vxtien-qa/git-tui"
$Asset = "git-tui-windows-x64.exe"
$BinName = "git-tui.exe"
$InstallDir = "$env:LOCALAPPDATA\git-tui"

Write-Host ""
Write-Host "git-tui installer" -ForegroundColor Cyan
Write-Host "  Platform: Windows x64" -ForegroundColor Gray
Write-Host ""

function Install-Binary($SourcePath) {
    if (!(Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }

    $DestPath = Join-Path $InstallDir $BinName
    Copy-Item -Path $SourcePath -Destination $DestPath -Force
    Write-Host "Installed to $DestPath" -ForegroundColor Green

    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($UserPath -notlike "*$InstallDir*") {
        Write-Host "Adding $InstallDir to your PATH" -ForegroundColor Yellow
        [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
        $env:Path = "$env:Path;$InstallDir"
        Write-Host "PATH updated. Restart the terminal for it to take effect." -ForegroundColor Gray
    }
}

if ($Build) {
    # --- Build from source ---------------------------------------------------
    Write-Host "Mode: build from source" -ForegroundColor Yellow
    Write-Host ""

    Write-Host "[1/3] Checking the C++ build tools" -ForegroundColor Yellow

    $hasVSBuildTools = $false
    if (Get-Command cl -ErrorAction SilentlyContinue) {
        $hasVSBuildTools = $true
    } else {
        $vsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
        if (Test-Path $vsWhere) {
            $vsPath = & $vsWhere -latest -products * `
                -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
                -property installationPath 2>$null
            if ($vsPath) { $hasVSBuildTools = $true }
        }
    }

    if (-not $hasVSBuildTools) {
        Write-Host "      Visual Studio Build Tools not found." -ForegroundColor Red
        if (Get-Command winget -ErrorAction SilentlyContinue) {
            Write-Host "      Installing them now, which takes 5 to 10 minutes." -ForegroundColor Yellow
            try {
                winget install Microsoft.VisualStudio.2022.BuildTools `
                    --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended" `
                    --accept-package-agreements --accept-source-agreements
                Write-Host "      Build Tools installed." -ForegroundColor Green
            } catch {
                Write-Host "      Automatic install failed. Install them manually:" -ForegroundColor Red
                Write-Host "        https://visualstudio.microsoft.com/visual-cpp-build-tools/" -ForegroundColor Gray
                Write-Host "        Select the 'Desktop development with C++' workload." -ForegroundColor Gray
                exit 1
            }
        } else {
            Write-Host "      Install the Visual Studio Build Tools manually:" -ForegroundColor Red
            Write-Host "        https://visualstudio.microsoft.com/visual-cpp-build-tools/" -ForegroundColor Gray
            exit 1
        }
    } else {
        Write-Host "      Found." -ForegroundColor Green
    }

    Write-Host "[2/3] Checking Rust" -ForegroundColor Yellow

    if (Get-Command rustc -ErrorAction SilentlyContinue) {
        Write-Host "      $(rustc --version)" -ForegroundColor Green
        if (Get-Command rustup -ErrorAction SilentlyContinue) {
            rustup update stable 2>&1 | Out-Null
        }
    } else {
        Write-Host "      Rust not found, installing via rustup" -ForegroundColor Yellow
        $rustupUrl = "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe"
        $rustupFile = Join-Path $env:TEMP "rustup-init.exe"
        try {
            Invoke-WebRequest -Uri $rustupUrl -OutFile $rustupFile -UseBasicParsing
            & $rustupFile -y --default-toolchain stable --default-host x86_64-pc-windows-msvc
            Remove-Item $rustupFile -ErrorAction SilentlyContinue

            $env:Path = [Environment]::GetEnvironmentVariable("Path", "Machine") + ";" +
                        [Environment]::GetEnvironmentVariable("Path", "User")
            $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
            if ($env:Path -notlike "*$cargoBin*") { $env:Path = "$env:Path;$cargoBin" }

            if (Get-Command rustc -ErrorAction SilentlyContinue) {
                Write-Host "      Rust installed: $(rustc --version)" -ForegroundColor Green
            } else {
                Write-Host "      Rust install failed. See https://rustup.rs" -ForegroundColor Red
                exit 1
            }
        } catch {
            Write-Host "      Could not download rustup. See https://rustup.rs" -ForegroundColor Red
            exit 1
        }
    }

    Write-Host "[3/3] Building git-tui in release mode" -ForegroundColor Yellow
    Write-Host "      The first build takes a few minutes." -ForegroundColor Gray
    Write-Host ""

    try {
        cargo build --release
    } catch {
        Write-Host ""
        Write-Host "Build failed. Two things to check:" -ForegroundColor Red
        Write-Host "  1. Restart the terminal after installing the Build Tools." -ForegroundColor Gray
        Write-Host "  2. Run: rustup default stable-x86_64-pc-windows-msvc" -ForegroundColor Gray
        exit 1
    }

    $BuiltExe = "target\release\git-tui.exe"
    if (!(Test-Path $BuiltExe)) {
        Write-Host "Build output not found: $BuiltExe" -ForegroundColor Red
        exit 1
    }
    Install-Binary $BuiltExe
} else {
    # --- Install the latest release ------------------------------------------
    $TmpFile = Join-Path $env:TEMP $Asset
    $Url = "https://github.com/$Repo/releases/latest/download/$Asset"

    Write-Host "Downloading $Asset" -ForegroundColor Yellow
    Write-Host "  $Url" -ForegroundColor Gray

    # Plain HTTPS first, so installing needs no GitHub credentials. The GitHub
    # CLI is only a fallback, for example while the repository is still private.
    $downloaded = $false
    try {
        Invoke-WebRequest -Uri $Url -OutFile $TmpFile -UseBasicParsing
        $downloaded = $true
    } catch {
        if (Get-Command gh -ErrorAction SilentlyContinue) {
            Write-Host "  Direct download failed, retrying with the GitHub CLI" -ForegroundColor Yellow
            try {
                gh release download --repo "$Repo" --pattern "$Asset" -O "$TmpFile" --clobber
                $downloaded = $true
            } catch {
                $downloaded = $false
            }
        }
    }

    if (-not $downloaded) {
        Write-Host "Download failed. See https://github.com/$Repo/releases" -ForegroundColor Red
        exit 1
    }

    Install-Binary $TmpFile
    Remove-Item $TmpFile -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "git-tui installed." -ForegroundColor Green
Write-Host ""
Write-Host "Run: git-tui" -ForegroundColor Cyan
Write-Host ""
Write-Host "git-tui drives the GitHub CLI, so it needs gh installed and authorised:" -ForegroundColor Gray
Write-Host "  1. Install gh: winget install GitHub.cli" -ForegroundColor Gray
Write-Host "  2. Sign in:    gh auth login -s repo,project,read:org" -ForegroundColor Gray
Write-Host ""
