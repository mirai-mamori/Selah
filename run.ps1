#Requires -Version 5.1
<#
.SYNOPSIS
    Selah Windows development helper.

.PARAMETER Command
    setup    Install frontend dependencies
    dev      Start the Tauri development server (default)
    build    Production build
    clean    Clean build caches
    rebuild  Clean + build
    kill     Kill running Selah processes
    open     Open the last built installer or executable
    doctor   Check the local Windows ARM64 development environment
#>
param(
    [string]$Command = "dev"
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $ScriptDir
$env:CARGO_BUILD_JOBS = "2"
$AppName = "selah-app"
$DefaultCargoTargetDir = Join-Path $ScriptDir "src-tauri\target"
$SherpaCpuSharedLibDir = Join-Path $ScriptDir ".tools\sherpa-arm64-cpu-shared\install\lib"
$WindowsRuntimeDir = Join-Path $ScriptDir "src-tauri\windows-runtime"
$LauncherLogDir = Join-Path $ScriptDir ".tools\logs"
$LauncherLogPath = Join-Path $LauncherLogDir "run.log"
$TranscriptStarted = $false

try {
    New-Item -ItemType Directory -Force -Path $LauncherLogDir | Out-Null
    Start-Transcript -Path $LauncherLogPath -Append | Out-Null
    $TranscriptStarted = $true
} catch {
    # Logging must not prevent the development launcher from starting.
}

# Keep the window open when the script is launched from Explorer.
$IsDoubleClick = ($Host.Name -eq "ConsoleHost") -and (-not $env:WT_SESSION) -and (-not $env:TERM_PROGRAM)
trap {
    Write-Host ""
    Write-Host "ERROR: $_" -ForegroundColor Red
    if ($TranscriptStarted) { Stop-Transcript | Out-Null }
    if ($IsDoubleClick) { Read-Host "Press Enter to close" }
    exit 1
}

# Explorer does not load terminal profile PATH modifications. Include common
# Node.js, Rust and package-manager locations, including WinGet's versioned
# Node.js archive directory.
function Add-PathEntry {
    param([Parameter(Mandatory)][string]$PathEntry)

    if ((Test-Path -LiteralPath $PathEntry) -and ($env:PATH -notlike "*$PathEntry*")) {
        $env:PATH = "$PathEntry;$env:PATH"
    }
}

$pathDirs = @(
    (Join-Path $ScriptDir ".tools\llvm-arm64\bin"),
    "$env:ProgramFiles\nodejs",
    "$env:APPDATA\npm",
    "$env:LOCALAPPDATA\Programs\nodejs",
    "$env:USERPROFILE\.cargo\bin",
    "$env:USERPROFILE\scoop\shims",
    "$env:LOCALAPPDATA\Volta\bin",
    "C:\ProgramData\chocolatey\bin"
)
$wingetRoot = Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages"
if (Test-Path $wingetRoot) {
    $pathDirs += @(
        Get-ChildItem -LiteralPath $wingetRoot -Directory -Filter "OpenJS.NodeJS.LTS_*" -ErrorAction SilentlyContinue |
            ForEach-Object {
                Get-ChildItem -LiteralPath $_.FullName -Directory -Filter "node-v*" -ErrorAction SilentlyContinue |
                    Select-Object -ExpandProperty FullName
            }
    )
}
foreach ($pathDir in $pathDirs) {
    Add-PathEntry $pathDir
}

# Load the local ARM64 MSVC/LLVM environment when the project toolchain is
# present. This is required by native crates such as ring and keeps the
# launcher independent from the user's global Visual Studio installation.
$vcVarsArm64 = Join-Path $ScriptDir ".tools\vs\VC\Auxiliary\Build\vcvarsarm64.bat"
if (Test-Path $vcVarsArm64) {
    # `&` (not `&&`) so the environment is still captured even if vcvarsall.bat
    # exits non-zero on a benign warning (e.g. an unused VS component missing).
    $vcEnvironment = cmd.exe /d /s /c "call `"$vcVarsArm64`" >nul & set"
    $vcEnvironmentNames = @(
        "INCLUDE",
        "LIB",
        "LIBPATH",
        "PATH",
        "VCINSTALLDIR",
        "VCToolsInstallDir",
        "VCToolsVersion",
        "WindowsSdkDir",
        "WindowsSDKVersion",
        "UCRTVersion"
    )
    foreach ($line in $vcEnvironment) {
        if ($line -match '^([^=]+)=(.*)$' -and $vcEnvironmentNames -ccontains $matches[1]) {
            Set-Item -Path "Env:$($matches[1])" -Value $matches[2]
        }
    }
}

# vcvars may rebuild PATH from its own baseline; restore project-local tools
# afterwards so npm/cargo remain discoverable from Explorer and terminals.
foreach ($pathDir in $pathDirs) {
    Add-PathEntry $pathDir
}

# Prefer the current LLVM archive over the expired copy inside the local VS
# snapshot. LLVM cannot reliably launch itself from a path containing non-ASCII
# characters, so expose it through a user-writable ASCII junction when needed.
$llvmArm64Root = Join-Path $ScriptDir ".tools\llvm-arm64"
if ($llvmArm64Root -match '[^\x00-\x7F]') {
    $toolLinksRoot = Join-Path $env:LOCALAPPDATA "Selah\tool-links"
    $llvmArm64Link = Join-Path $toolLinksRoot "llvm-arm64"
    New-Item -ItemType Directory -Force -Path $toolLinksRoot | Out-Null

    if (Test-Path -LiteralPath $llvmArm64Link) {
        $existingLink = Get-Item -LiteralPath $llvmArm64Link -Force
        $existingTarget = @($existingLink.Target) | Select-Object -First 1
        if ($existingLink.LinkType -ne "Junction" -or
            [System.IO.Path]::GetFullPath($existingTarget) -ne [System.IO.Path]::GetFullPath($llvmArm64Root)) {
            throw "$llvmArm64Link already exists and does not point to the project LLVM runtime."
        }
    } else {
        New-Item -ItemType Junction -Path $llvmArm64Link -Target $llvmArm64Root | Out-Null
    }

    $llvmArm64Root = $llvmArm64Link
}
$llvmArm64Bin = Join-Path $llvmArm64Root "bin"
if (Test-Path (Join-Path $llvmArm64Bin "clang.exe")) {
    Add-PathEntry $llvmArm64Bin
}

function Find-Arm64Compiler {
    $msvcRoot = Join-Path $ScriptDir ".tools\vs\VC\Tools\MSVC"
    if (-not (Test-Path -LiteralPath $msvcRoot)) {
        return $null
    }

    return Get-ChildItem -LiteralPath $msvcRoot -Directory |
        Sort-Object Name -Descending |
        ForEach-Object {
            $compiler = Join-Path $_.FullName "bin\Hostarm64\arm64\cl.exe"
            if (Test-Path -LiteralPath $compiler) { return $compiler }
        } |
        Select-Object -First 1
}

$arm64Cl = Find-Arm64Compiler
if ($arm64Cl) {
    # clang in the bundled Visual Studio snapshot is no longer executable on
    # this host, while the matching ARM64 MSVC compiler remains usable.
    Set-Item -Path "Env:CC" -Value $arm64Cl
    Set-Item -Path "Env:CXX" -Value $arm64Cl
    Set-Item -Path "Env:CC_aarch64_pc_windows_msvc" -Value $arm64Cl
    Set-Item -Path "Env:CC_aarch64-pc-windows-msvc" -Value $arm64Cl
    Set-Item -Path "Env:CXX_aarch64_pc_windows_msvc" -Value $arm64Cl
    Set-Item -Path "Env:CXX_aarch64-pc-windows-msvc" -Value $arm64Cl
}

function Assert-CommandAvailable {
    param([Parameter(Mandatory)][string]$Name)
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Required command '$Name' was not found in PATH. Install Node.js and Rust, then restart the terminal."
    }
}

function Invoke-NativeCommand {
    param(
        [Parameter(Mandatory)][string]$FilePath,
        [Parameter(ValueFromRemainingArguments = $true)][object[]]$Arguments
    )
    & $FilePath @Arguments
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) {
        throw "$FilePath failed with exit code $exitCode."
    }
}

function Get-CargoTargetDirectory {
    if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
        return $DefaultCargoTargetDir
    }
    return [System.IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
}

function Get-BundlePath {
    return Join-Path (Get-CargoTargetDirectory) "release\$AppName.exe"
}

function Initialize-DevelopmentEnvironment {
    $architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    if ($architecture -ne [System.Runtime.InteropServices.Architecture]::Arm64) {
        throw "This launcher is configured for Windows ARM64, but the current OS architecture is $architecture."
    }

    if (-not $arm64Cl) {
        throw "Windows ARM64 MSVC compiler not found under .tools\\vs. Restore the local VS toolchain before building."
    }

    $libClang = Join-Path $llvmArm64Bin "libclang.dll"
    if (-not (Test-Path -LiteralPath $libClang)) {
        throw "libclang.dll not found under $llvmArm64Bin. Restore the local LLVM toolchain before building."
    }

    $env:LIBCLANG_PATH = $llvmArm64Bin
    $env:LLAMA_STATIC_CRT = "1"
    $env:CMAKE_MSVC_RUNTIME_LIBRARY = "MultiThreaded"
}

function Initialize-WindowsStt {
    & (Join-Path $ScriptDir "scripts\prepare-windows-sherpa-runtime.ps1") `
        -Architecture arm64 `
        -WorkRoot (Join-Path $ScriptDir ".tools\sherpa-arm64-cpu-shared") `
        -LibStageDir $SherpaCpuSharedLibDir `
        -RuntimeStageDir $WindowsRuntimeDir

    $requiredFiles = @(
        "sherpa-onnx-c-api.dll",
        "sherpa-onnx-c-api.lib",
        "onnxruntime.dll",
        "onnxruntime.lib"
    )
    $missingFiles = $requiredFiles | Where-Object {
        -not (Test-Path -LiteralPath (Join-Path $SherpaCpuSharedLibDir $_))
    }
    if ($missingFiles) {
        throw "Windows ARM64 CPU sherpa runtime is incomplete at $SherpaCpuSharedLibDir. Missing: $($missingFiles -join ', ')"
    }

    $env:SHERPA_ONNX_LIB_DIR = $SherpaCpuSharedLibDir
    foreach ($targetDir in @("debug", "release")) {
        $fullTargetDir = Join-Path (Get-CargoTargetDirectory) $targetDir
        New-Item -ItemType Directory -Force -Path $fullTargetDir | Out-Null
        Get-ChildItem -LiteralPath $SherpaCpuSharedLibDir -Filter "*.dll" -File |
            Copy-Item -Destination $fullTargetDir -Force
    }
    return "stt-shared,self-updater"
}

function Stop-Selah {
    Write-Host "Killing running Selah processes..."
    Get-Process | Where-Object { $_.Name -eq $AppName } |
        Stop-Process -Force -ErrorAction SilentlyContinue
    $connections = Get-NetTCPConnection -LocalPort 5173 -ErrorAction SilentlyContinue
    if ($connections) {
        $connections | Select-Object -ExpandProperty OwningProcess -Unique |
            ForEach-Object { Stop-Process -Id $_ -Force -ErrorAction SilentlyContinue }
    }
    Start-Sleep -Milliseconds 500
    Write-Host "Done."
}

function Start-Dev {
    Stop-Selah
    Assert-CommandAvailable "npm"
    Assert-CommandAvailable "cargo"
    Initialize-DevelopmentEnvironment
    $sttFeatures = Initialize-WindowsStt
    Write-Host "Starting dev server..."
    Invoke-NativeCommand npm run tauri dev "--" "--features" $sttFeatures
}

function Start-Build {
    Stop-Selah
    Assert-CommandAvailable "npm"
    Assert-CommandAvailable "npx"
    Assert-CommandAvailable "cargo"
    Initialize-DevelopmentEnvironment
    $sttFeatures = Initialize-WindowsStt
    if (Test-Path "dist") { Remove-Item "dist" -Recurse -Force }
    Write-Host "Building $AppName..."
    Invoke-NativeCommand npx tauri build "--" "--features" $sttFeatures
    $bundlePath = Get-BundlePath
    if (Test-Path $bundlePath) {
        Write-Host "Build complete: $bundlePath"
        Start-Process $bundlePath
        return
    }
    $installer = Get-ChildItem (Join-Path (Get-CargoTargetDirectory) "release\bundle\nsis\*.exe") -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($installer) {
        Write-Host "Build complete: $($installer.FullName)"
        Start-Process $installer.FullName
    } else {
        throw "Build output not found."
    }
}

function Install-Prerequisites {
    Assert-CommandAvailable "npm"
    Write-Host "Installing locked frontend dependencies..."
    Invoke-NativeCommand npm ci --silent
    Write-Host "Setup complete. Run '.\run.ps1 dev' to start."
}

function Clear-Cache {
    Write-Host "Cleaning caches..."
    $targets = @(
        "dist",
        "node_modules\.vite",
        "node_modules\.cache",
        "src-tauri\gen\schemas"
    )
    foreach ($target in $targets) {
        $fullPath = Join-Path $ScriptDir $target
        if (Test-Path $fullPath) {
            Remove-Item $fullPath -Recurse -Force
            Write-Host "  Removed $target"
        }
    }

    foreach ($target in @(
        (Join-Path (Get-CargoTargetDirectory) "debug\bundle"),
        (Join-Path (Get-CargoTargetDirectory) "release\bundle")
    )) {
        if (Test-Path -LiteralPath $target) {
            Remove-Item -LiteralPath $target -Recurse -Force
            Write-Host "  Removed $target"
        }
    }
    Write-Host "Clean complete."
}

function Open-LastBuild {
    $installer = Get-ChildItem (Join-Path (Get-CargoTargetDirectory) "release\bundle\nsis\*.exe") -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($installer) {
        Start-Process $installer.FullName
    } elseif (Test-Path (Get-BundlePath)) {
        Start-Process (Get-BundlePath)
    } else {
        throw "No build found. Run '.\run.ps1 build' first."
    }
}

function Test-DevelopmentEnvironment {
    $architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    $checks = @(
        [pscustomobject]@{ Name = "Windows ARM64 host"; Passed = ($architecture -eq [System.Runtime.InteropServices.Architecture]::Arm64); Detail = $architecture },
        [pscustomobject]@{ Name = "Node.js (npm)"; Passed = [bool](Get-Command npm -ErrorAction SilentlyContinue); Detail = "npm" },
        [pscustomobject]@{ Name = "Rust (cargo)"; Passed = [bool](Get-Command cargo -ErrorAction SilentlyContinue); Detail = "cargo" },
        [pscustomobject]@{ Name = "ARM64 MSVC compiler"; Passed = [bool]$arm64Cl; Detail = $arm64Cl },
        [pscustomobject]@{ Name = "LLVM libclang"; Passed = (Test-Path -LiteralPath (Join-Path $llvmArm64Bin "libclang.dll")); Detail = $llvmArm64Bin },
        [pscustomobject]@{ Name = "sherpa CPU runtime"; Passed = (Test-Path -LiteralPath (Join-Path $SherpaCpuSharedLibDir "sherpa-onnx-c-api.dll")); Detail = $SherpaCpuSharedLibDir }
    )

    foreach ($check in $checks) {
        $status = if ($check.Passed) { "OK" } else { "MISSING" }
        $color = if ($check.Passed) { "Green" } else { "Red" }
        Write-Host ("[{0}] {1}: {2}" -f $status, $check.Name, $check.Detail) -ForegroundColor $color
    }

    if ($checks.Where({ -not $_.Passed }).Count -gt 0) {
        throw "Development environment check failed."
    }
}

switch ($Command.ToLowerInvariant()) {
    "setup"   { Install-Prerequisites }
    "dev"     { Start-Dev }
    "build"   { Start-Build }
    "clean"   { Clear-Cache }
    "rebuild" { Clear-Cache; Start-Build }
    "kill"    { Stop-Selah }
    "open"    { Open-LastBuild }
    "doctor"  { Test-DevelopmentEnvironment }
    default {
        Write-Host "Usage: .\run.ps1 [setup|dev|build|clean|rebuild|kill|open|doctor]"
        Write-Host ""
        Write-Host "  setup    Install locked frontend dependencies"
        Write-Host "  dev      Start development server (default)"
        Write-Host "  build    Production build"
        Write-Host "  clean    Clean build caches"
        Write-Host "  rebuild  Clean + build"
        Write-Host "  kill     Kill running Selah processes"
        Write-Host "  open     Open last built exe/installer"
        Write-Host "  doctor   Check the local Windows ARM64 development environment"
    }
}

if ($TranscriptStarted) { Stop-Transcript | Out-Null }
