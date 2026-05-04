# One-shot: env check, install espup/ldproxy/toolchain, then release build.
# ESP firmware: this script injects default --target for release; do not rely on repo .cargo default target.
# Usage: .\build.ps1  or  .\build.ps1 --target xtensa-esp32s3-espidf [--package-profile <name>]
#        .\build.ps1 clean           清理项目根与短路径 D:\pc_b 的 target（路径过长时只需跑一次）
#        .\build.ps1 --flash          构建后烧录（数字菜单，默认 1=仅更新；与 build.sh Linux 部署菜单风格一致）
#        .\build.ps1 --flash-update   构建后烧录且不擦除（仅存储格式兼容时可用）
#        .\build.ps1 build-c6         构建 vendored ESP32-C6 hosted slave firmware
#        .\build.ps1 flash-c6         烧录板载 ESP32-C6 hosted slave firmware
#        .\build.ps1 flash-all        先烧 C6，再烧 P4 主固件
#        $env:ESPFLASH_PORT="COM3"; .\build.ps1 --flash  跳过端口选择，直接烧录到 COM3
$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::InputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
try { cmd /c chcp 65001 *>$null } catch {}
Set-Location $PSScriptRoot

if ($args.Count -gt 0 -and @("build-c6", "flash-c6", "flash-all") -contains $args[0]) {
  & (Join-Path $PSScriptRoot "scripts\esp_hosted_c6.ps1") @args
  exit $LASTEXITCODE
}

$env:CARGO_TARGET_DIR = Join-Path $PSScriptRoot "target"
# ESP-IDF / kconfgen 读 sdkconfig 时若用系统默认编码（中文 Windows 为 GBK）会报 UnicodeDecodeError，强制 Python 使用 UTF-8
if ($env:OS -eq "Windows_NT") { $env:PYTHONUTF8 = "1" }

# 解析 --flash / --flash-update / --no-monitor / --package-profile，并从 BOARD 解析 target/features（与 build.sh 一致）
$flashUpdate = $false
$doFlash = $false
$noMonitor = $false
$packageProfile = if ($env:PACKAGE_PROFILE) { $env:PACKAGE_PROFILE } else { "" }
$buildArgs = @()
for ($i = 0; $i -lt $args.Count; $i++) {
  switch -Regex ($args[$i]) {
    '^--flash$' {
      $flashUpdate = $false
      $doFlash = $true
      continue
    }
    '^--flash-update$' {
      $flashUpdate = $true
      $doFlash = $true
      continue
    }
    '^--no-monitor$' {
      $noMonitor = $true
      continue
    }
    '^--package-profile$' {
      if (($i + 1) -ge $args.Count) {
        Write-Error "--package-profile requires a value"
        exit 1
      }
      $i++
      $packageProfile = $args[$i]
      continue
    }
    '^--package-profile=(.+)$' {
      $packageProfile = $Matches[1]
      continue
    }
    default {
      $buildArgs += $args[$i]
    }
  }
}

function Get-TargetMcuFromTriple {
  param([string]$TargetTriple)
  switch ($TargetTriple) {
    "xtensa-esp32-espidf" { return "esp32" }
    "xtensa-esp32s2-espidf" { return "esp32s2" }
    "xtensa-esp32s3-espidf" { return "esp32s3" }
    "riscv32imc-esp-espidf" { return "esp32c3" }
    "riscv32imac-esp-espidf" { return "esp32c6" }
    "riscv32imafc-esp-espidf" { return "esp32p4" }
    default { return $null }
  }
}

function Get-DefaultSdkconfigOverlayForTarget {
  param([string]$TargetTriple)
  switch ($TargetTriple) {
    "xtensa-esp32s3-espidf" { return "sdkconfig.defaults.esp32s3.board" }
    "riscv32imafc-esp-espidf" { return "sdkconfig.defaults.esp32p4.board" }
    default { return $null }
  }
}

function Get-PackageProfileTargetKind {
  param([string]$TargetTriple)
  if ($TargetTriple -like "*-unknown-linux*") {
    return "linux"
  }
  return "esp"
}

function Invoke-BeetlePackageProfileResolver {
  param([string[]]$ResolverArgs)
  $resolver = Join-Path $PSScriptRoot "scripts\expand_cargo_features.py"
  if (-not (Test-Path $resolver)) {
    Write-Error "Package-profile resolver not found: $resolver"
    exit 1
  }
  $pythonCmd = Get-Command python -ErrorAction SilentlyContinue
  if (-not $pythonCmd) {
    $pythonCmd = Get-Command python3 -ErrorAction SilentlyContinue
  }
  if (-not $pythonCmd -and (Get-Command py -ErrorAction SilentlyContinue)) {
    $pythonCmd = "py"
  }
  if (-not $pythonCmd) {
    Write-Error "python not found. build.ps1 requires python/python3/py to resolve package profiles from Cargo.toml."
    exit 1
  }
  $manifest = Join-Path $PSScriptRoot "Cargo.toml"
  if ($pythonCmd -eq "py") {
    $output = & py -3 $resolver --manifest $manifest @ResolverArgs
  } else {
    $output = & $pythonCmd.Source $resolver --manifest $manifest @ResolverArgs
  }
  if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
  }
  return ($output | Out-String).Trim()
}

function Normalize-FlashSize {
  param([string]$FlashSize)
  $value = if ($null -eq $FlashSize) { "" } else { $FlashSize }
  $value = ($value -replace '\s+', '').ToUpperInvariant()
  switch ($value) {
    "8MB" { return "8MB" }
    "16MB" { return "16MB" }
    "32MB" { return "32MB" }
    default { return $null }
  }
}

function Get-BoardFromChipFlash {
  param(
    [string]$Chip,
    [string]$FlashSize
  )
  $normalized = Normalize-FlashSize -FlashSize $FlashSize
  if (-not $normalized) { return $null }
  switch ($Chip) {
    "esp32p4" {
      if ($normalized -eq "16MB") { return "esp32-p4-nano-16mb" }
      return $null
    }
    "esp32s3" {
      switch ($normalized) {
        "8MB" { return "esp32-s3-8mb" }
        "16MB" { return "esp32-s3-16mb" }
        "32MB" { return "esp32-s3-32mb" }
        default { return $null }
      }
    }
    default { return $null }
  }
}

function Get-AutodetectFlashPort {
  if ($env:ESPFLASH_PORT) {
    return $env:ESPFLASH_PORT
  }
  $ports = @([System.IO.Ports.SerialPort]::GetPortNames() | Sort-Object)
  if ($ports.Count -eq 1) {
    return $ports[0]
  }
  return $null
}

function Get-DetectedEspBoard {
  if (-not (Get-Command espflash -ErrorAction SilentlyContinue)) {
    return $null
  }
  $port = Get-AutodetectFlashPort
  if (-not $port) {
    return $null
  }
  $output = & espflash board-info --port $port --non-interactive 2>$null
  if ($LASTEXITCODE -ne 0) {
    return $null
  }
  $chip = $null
  $flashSize = $null
  foreach ($line in ($output | Out-String).Split([Environment]::NewLine, [StringSplitOptions]::RemoveEmptyEntries)) {
    if (-not $chip -and $line -match '^Chip type:\s*([a-z0-9]+)') {
      $chip = $Matches[1]
    }
    if (-not $flashSize -and $line -match '^Flash size:\s*([0-9]+MB)') {
      $flashSize = $Matches[1]
    }
  }
  if (-not $chip -or -not $flashSize) {
    return $null
  }
  $board = Get-BoardFromChipFlash -Chip $chip -FlashSize $flashSize
  if (-not $board) {
    return $null
  }
  return [pscustomobject]@{
    Board = $board
    Port = $port
    Chip = $chip
    FlashSize = (Normalize-FlashSize -FlashSize $flashSize)
  }
}

$buildTarget = "xtensa-esp32s3-espidf"
$buildProfile = "release-size"
$buildFeatures = @()
$boardSdkconfigOverlay = $null
$autoDetectedBoard = $null
$cliBuildTarget = $null
for ($i = 0; $i -lt $buildArgs.Count; $i++) {
  if ($buildArgs[$i] -eq "--target" -and ($i + 1) -lt $buildArgs.Count) {
    $cliBuildTarget = $buildArgs[$i + 1]
    break
  }
  if ($buildArgs[$i] -like "--target=*") {
    $cliBuildTarget = $buildArgs[$i].Substring("--target=".Length)
    break
  }
}
if (-not $env:BOARD -and -not $cliBuildTarget) {
  $autoDetectedBoard = Get-DetectedEspBoard
  if ($autoDetectedBoard) {
    $env:BOARD = $autoDetectedBoard.Board
  }
}
if ($env:BOARD) {
  if ($env:BOARD -notmatch '^[a-z0-9-]+$') {
    Write-Error "BOARD must contain only [a-z0-9-]. Got: $env:BOARD"
    exit 1
  }
  $presetsPath = Join-Path $PSScriptRoot "board_presets.toml"
  if (-not (Test-Path $presetsPath)) {
    Write-Error "BOARD=$env:BOARD set but board_presets.toml not found"
    exit 1
  }
  $inSection = $false
  $partitionTable = ""
  foreach ($line in (Get-Content $presetsPath)) {
    if ($line -match '^\[boards\.(.+)\]') {
      $inSection = ($matches[1] -eq $env:BOARD)
    } elseif ($inSection) {
      if ($line -match 'target\s*=\s*"([^"]+)"') { $buildTarget = $matches[1] }
      if ($line -match 'partition_table\s*=\s*"([^"]+)"') { $partitionTable = $matches[1] }
      if ($line -match 'sdkconfig_overlay\s*=\s*"([^"]+)"') { $boardSdkconfigOverlay = $matches[1] }
    }
  }
  if (-not $partitionTable) {
    switch ($env:BOARD) {
      "esp32-s3-8mb"  { $partitionTable = "partitions_8mb.csv" }
      "esp32-s3-32mb" { $partitionTable = "partitions_32mb.csv" }
      default         { $partitionTable = "partitions.csv" }
    }
  }
} else {
  $partitionTable = "partitions.csv"
}
# 若命令行已传 --target，以命令行为准
if ($cliBuildTarget) {
  $buildTarget = $cliBuildTarget
}
# 防止路径穿越：target 仅允许字母数字、连字符、下划线
if ($buildTarget -notmatch '^[a-zA-Z0-9_-]+$') {
  Write-Error "Invalid --target (no path chars): $buildTarget"
  exit 1
}
$targetMcu = Get-TargetMcuFromTriple -TargetTriple $buildTarget
if (-not $targetMcu) {
  Write-Error "Unsupported ESP target triple: $buildTarget"
  exit 1
}
if (-not $boardSdkconfigOverlay) {
  $boardSdkconfigOverlay = Get-DefaultSdkconfigOverlayForTarget -TargetTriple $buildTarget
}
if ($boardSdkconfigOverlay -and -not (Test-Path (Join-Path $PSScriptRoot $boardSdkconfigOverlay))) {
  Write-Error "sdkconfig overlay not found: $boardSdkconfigOverlay"
  exit 1
}

if (-not [string]::IsNullOrWhiteSpace($packageProfile) -and $packageProfile -notmatch '^[a-z0-9+_-]+$') {
  Write-Error "Invalid package profile: $packageProfile"
  exit 1
}
if ([string]::IsNullOrWhiteSpace($packageProfile)) {
  $targetKind = Get-PackageProfileTargetKind -TargetTriple $buildTarget
  $packageProfile = Invoke-BeetlePackageProfileResolver @(
    "--default-target-kind",
    $targetKind,
    "--format",
    "value"
  )
}
$packageProfileFeaturesCsv = Invoke-BeetlePackageProfileResolver @(
  "--package-profile",
  $packageProfile,
  "--format",
  "csv"
)
$buildFeatures = @("--no-default-features", "--features", $packageProfileFeaturesCsv)
$env:BEETLE_PACKAGE_PROFILE = $packageProfile

# 从 buildTarget 推断 espflash --chip（用于后续打印与烧录）
$flashChipDerived = $targetMcu

# Print detected hardware and current build config (English)
function Write-BuildStatus {
  param(
    [string]$Step,
    [switch]$BeforeFlash,
    [string]$ChosenPort = "",
    [string]$BinPath = ""
  )
  Write-Host ""
  Write-Host "========== $Step ==========" -ForegroundColor Cyan
  Write-Host "  Project root:      $BuildRoot"
  Write-Host "  Build target:      $buildTarget"
  Write-Host "  BOARD (optional):  $(if ($env:BOARD) { $env:BOARD } else { '(not set)' })"
  Write-Host "  Target MCU:        $(if ($targetMcu) { $targetMcu } else { '(N/A)' })"
  Write-Host "  Chip (for flash): $(if ($flashChipDerived) { $flashChipDerived } else { '(N/A)' })"
  Write-Host "  Partition table:   $partitionTable"
  Write-Host "  SDKCONFIG overlay: $(if ($boardSdkconfigOverlay) { $boardSdkconfigOverlay } else { '(none)' })"
  Write-Host "  Package profile:   $(if ($packageProfile) { $packageProfile } else { '(none)' })"
  if ($autoDetectedBoard) {
    Write-Host "  Auto-detected:     $($autoDetectedBoard.Board) via $($autoDetectedBoard.Chip)/$($autoDetectedBoard.FlashSize) on $($autoDetectedBoard.Port)"
  }
  Write-Host "  Features:          $(if ($buildFeatures.Count -gt 0) { $buildFeatures -join ' ' } else { '(none)' })"
  Write-Host "  Profile:           $buildProfile"
  if ($BeforeFlash -and $ChosenPort) {
    Write-Host "  Serial port:        $ChosenPort"
    Write-Host "  Partition table:    $(if ($partitionTableForFlash) { $partitionTableForFlash } else { $partitionCsv })"
    Write-Host "  Bootloader:         $bootloaderBin"
    if ($BinPath) { Write-Host "  Firmware binary:    $BinPath" }
  }
  Write-Host ""
}

function Get-ModelPartitionOffset {
  param([string]$PartitionCsv)
  if (-not (Test-Path $PartitionCsv)) { return $null }
  foreach ($line in Get-Content -Path $PartitionCsv) {
    $trimmed = $line.Trim()
    if (-not $trimmed -or $trimmed.StartsWith("#")) { continue }
    $parts = $line.Split(",")
    if ($parts.Count -lt 4) { continue }
    $name = $parts[0].Trim()
    $offset = $parts[3].Trim()
    if ($name -eq "model" -and -not [string]::IsNullOrWhiteSpace($offset)) {
      return $offset
    }
  }
  return $null
}

function Get-SrModelsBin {
  $buildDir = Join-Path $releaseDir "build"
  if (-not (Test-Path $buildDir)) { return $null }
  $matches = Get-ChildItem -Path $buildDir -Filter "srmodels.bin" -Recurse -File -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending
  if ($matches) { return $matches[0].FullName }
  return $null
}

function Get-FileMd5Hex {
  param([string]$Path)
  if (-not (Test-Path $Path)) { return $null }
  return (Get-FileHash -Path $Path -Algorithm MD5).Hash.ToLowerInvariant()
}

function Get-DeviceRegionMd5Hex {
  param(
    [string]$ChosenPort,
    [string]$Address,
    [string]$Size
  )
  $output = & espflash checksum-md5 --port $ChosenPort --chip $flashChip $Address $Size 2>&1
  if ($LASTEXITCODE -ne 0) { return $null }
  $matches = [regex]::Matches(($output | Out-String), '\b[0-9a-fA-F]{32}\b')
  if ($matches.Count -eq 0) { return $null }
  return $matches[$matches.Count - 1].Value.ToLowerInvariant()
}

function Write-ModelPartition {
  param([string]$ChosenPort)
  $modelOffset = Get-ModelPartitionOffset -PartitionCsv $partitionCsv
  if (-not $modelOffset) {
    Write-Host "  Model partition:    not present in $partitionTable (wake-word model flash skipped)" -ForegroundColor Yellow
    return $true
  }

  $modelBin = Get-SrModelsBin
  if (-not $modelBin) {
    Write-Host "Error: model partition exists but srmodels.bin was not generated." -ForegroundColor Red
    Write-Host "  Expected under: $releaseDir\build\esp-idf-sys-*\out\build\srmodels\srmodels.bin" -ForegroundColor Gray
    return $false
  }

  Write-Host ""
  Write-Host "========== Flashing wake-word model ==========" -ForegroundColor Cyan
  Write-Host ""
  Write-Host "  Model image:  $modelBin" -ForegroundColor Gray
  Write-Host "  Model offset: $modelOffset" -ForegroundColor Gray
  if (-not $eraseBeforeFlash) {
    $modelSize = (Get-Item -Path $modelBin).Length
    $localMd5 = Get-FileMd5Hex -Path $modelBin
    if ($localMd5) {
      $deviceMd5 = Get-DeviceRegionMd5Hex -ChosenPort $ChosenPort -Address $modelOffset -Size $modelSize
      if ($deviceMd5) {
        Write-Host "  Model MD5(local):  $localMd5" -ForegroundColor Gray
        Write-Host "  Model MD5(device): $deviceMd5" -ForegroundColor Gray
        if ($localMd5 -eq $deviceMd5) {
          Write-Host "✓ Wake-word model unchanged; skipping model flash." -ForegroundColor Green
          return $true
        }
      } else {
        Write-Host "  Model MD5(device): unavailable; model will be reflashed" -ForegroundColor Yellow
      }
    } else {
      Write-Host "  Model MD5(local):  unavailable; model will be reflashed" -ForegroundColor Yellow
    }
  }
  & espflash write-bin --port $ChosenPort --chip $flashChip $modelOffset $modelBin
  if ($LASTEXITCODE -ne 0) {
    Write-Host "Wake-word model flash failed (exit $LASTEXITCODE)." -ForegroundColor Red
    return $false
  }
  Write-Host "✓ Wake-word model flashed." -ForegroundColor Green
  return $true
}

function Start-EspMonitor {
  param(
    [string]$ChosenPort,
    [string]$BinPath
  )
  if ($noMonitor) { return }
  Write-Host ""
  Write-Host "========== Opening serial monitor ==========" -ForegroundColor Cyan
  Write-Host ""
  & espflash monitor --port $ChosenPort --chip $flashChip --elf $BinPath
}

$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  Write-Error "cargo not found. Install Rust: https://rustup.rs"
  exit 1
}

# clean：项目根 + 短路径（若存在）一起清理，只跑一次
if ($args -contains "clean") {
  $cleanArgs = $args | Where-Object { $_ -ne "clean" }
  Write-Host ""
  Write-Host "========== Step: Cleaning build artifacts (project root and short path if used) ==========" -ForegroundColor Cyan
  Write-Host "  Running: cargo clean (project root)..." -ForegroundColor Gray
  & cargo clean @cleanArgs
  $rootExit = $LASTEXITCODE
  if ($env:OS -eq "Windows_NT" -and $PSScriptRoot.Length + 78 -gt 88) {
    $shortRoot = "D:\pc_b"
    if (Test-Path (Join-Path $shortRoot "Cargo.toml")) {
      Write-Host "  Running: cargo clean (short path $shortRoot)..." -ForegroundColor Gray
      Push-Location $shortRoot; & cargo clean @cleanArgs; $shortExit = $LASTEXITCODE; Pop-Location
      if ($shortExit -ne 0) { exit $shortExit }
    }
  }
  exit $rootExit
}

# Windows 路径过长时：subst 无效（canonicalize 会得到真实路径）。改为在短路径下同步项目并重入构建，再把 target 拷回。
$BuildRoot = $PSScriptRoot
if ($env:OS -eq "Windows_NT" -and -not $env:CARGO_TARGET_DIR -and -not $env:PC_ORIGINAL_PROJECT_ROOT) {
  $projLen = $PSScriptRoot.Length
  if ($projLen + 78 -gt 88) {
    $shortRoot = "D:\pc_b"
    if (-not (Test-Path "D:\pc")) { New-Item -ItemType Directory -Path "D:\pc" -Force | Out-Null }
    if (Test-Path $shortRoot) {
      if (-not (Test-Path (Join-Path $shortRoot "Cargo.toml"))) {
        $existing = Get-ChildItem $shortRoot -Force -ErrorAction SilentlyContinue
        if ($existing) {
          Write-Host "WARNING: $shortRoot exists and is not a project copy; sync will overwrite it." -ForegroundColor Yellow
          $r = Read-Host "Continue? (y/n)"
          if ($r.Trim().ToLowerInvariant() -ne 'y' -and $r.Trim().ToLowerInvariant() -ne 'yes') { exit 0 }
        }
      }
    } else { New-Item -ItemType Directory -Path $shortRoot -Force | Out-Null }
    Write-Host ""
    Write-Host "========== Step: Syncing project to short path (esp-idf-sys path length limit) ==========" -ForegroundColor Cyan
    Write-Host "  Source: $PSScriptRoot  ->  Destination: $shortRoot" -ForegroundColor Gray
    $robocopy = Get-Command robocopy -ErrorAction SilentlyContinue
    if ($robocopy) {
      $null = & robocopy $PSScriptRoot $shortRoot /E /XD target .git /NFL /NDL /NJH /NJS /R:1 /W:1
        # 项目根已 cargo clean 时，同步清理短路径下的 target，避免需跑两个目录
        $projTarget = Join-Path $PSScriptRoot "target"
        $shortTarget = Join-Path $shortRoot "target"
        if (-not (Test-Path $projTarget) -and (Test-Path $shortTarget)) {
          Remove-Item -Recurse -Force $shortTarget -ErrorAction SilentlyContinue
        }
        if (Test-Path (Join-Path $shortRoot "Cargo.toml")) {
        $env:PC_ORIGINAL_PROJECT_ROOT = $PSScriptRoot
        & (Join-Path $shortRoot "build.ps1") @args
        $exitCode = $LASTEXITCODE
        if (Test-Path (Join-Path $shortRoot "target")) {
          $destTarget = Join-Path $PSScriptRoot "target"
          Write-Host ""
          Write-Host "========== Step: Copying target back to project ==========" -ForegroundColor Cyan
          Write-Host "  From: $shortRoot\target  ->  To: $destTarget" -ForegroundColor Gray
          if (-not (Test-Path $destTarget)) { New-Item -ItemType Directory -Path $destTarget -Force | Out-Null }
          $rc = & robocopy (Join-Path $shortRoot "target") $destTarget /E /IS /IT /NFL /NDL /NJH /NJS
          if ($rc -ge 8) { exit $rc }
        }
        exit $exitCode
      }
    }
    Write-Host "WARNING: Could not sync to short path; build may fail with 'Too long output directory'." -ForegroundColor Yellow
  }
}
# 烧录时从此目录找二进制（与 CARGO_TARGET_DIR 一致）
$effectiveTargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $BuildRoot "target" }

function Get-EspComponentGraphInputs {
  $paths = @(
    "Cargo.toml",
    "build.rs",
    "components_esp32s3.lock",
    "components_esp32p4.lock",
    "sdkconfig.defaults",
    "sdkconfig.defaults.esp32s3",
    "sdkconfig.defaults.esp32s3.8mb.board",
    "sdkconfig.defaults.esp32s3.board",
    "sdkconfig.defaults.esp32s3.32mb.board",
    "sdkconfig.defaults.esp32p4",
    "sdkconfig.defaults.esp32p4.board",
    "third_party/esp-idf-sys/build/native/cargo_driver/config.rs"
  )

  foreach ($path in $paths) {
    $fullPath = Join-Path $BuildRoot $path
    if (Test-Path $fullPath -PathType Leaf) {
      $fullPath
    }
  }

  $componentsDir = Join-Path $BuildRoot "components"
  if (Test-Path $componentsDir -PathType Container) {
    Get-ChildItem -Path $componentsDir -File -Recurse |
      Where-Object { $_.Name -ne ".DS_Store" } |
      Sort-Object FullName |
      ForEach-Object { $_.FullName }
  }
}

function Get-EspComponentGraphHash {
  $records = New-Object System.Collections.Generic.List[string]
  foreach ($file in Get-EspComponentGraphInputs) {
    $hash = (Get-FileHash -Algorithm SHA256 -Path $file).Hash.ToLowerInvariant()
    $records.Add("$hash *$file")
  }

  $payload = [System.Text.Encoding]::UTF8.GetBytes(($records -join "`n"))
  $sha256 = [System.Security.Cryptography.SHA256]::Create()
  try {
    return ([System.BitConverter]::ToString($sha256.ComputeHash($payload))).Replace("-", "").ToLowerInvariant()
  } finally {
    $sha256.Dispose()
  }
}

function Refresh-EspComponentGraphCache {
  if ($buildTarget -like "*-unknown-linux*") {
    return
  }

  $stampDir = Join-Path $effectiveTargetDir "$buildTarget\$buildProfile"
  $stampFile = Join-Path $stampDir ".beetle-esp-component-graph.sha256"
  $currentHash = Get-EspComponentGraphHash
  $cachedHash = ""

  if (Test-Path $stampFile -PathType Leaf) {
    $cachedHash = (Get-Content $stampFile -Raw).Trim()
  }

  if ($currentHash -eq $cachedHash) {
    return
  }

  if (-not (Test-Path $stampDir -PathType Container)) {
    New-Item -ItemType Directory -Path $stampDir -Force | Out-Null
  }

  $buildDir = Join-Path $stampDir "build"
  if (Test-Path $buildDir -PathType Container) {
    Get-ChildItem -Path $buildDir -Directory -Filter "esp-idf-sys-*" | ForEach-Object {
      Remove-Item -Recurse -Force $_.FullName
    }
  }

  Set-Content -Path $stampFile -Value "$currentHash`n" -NoNewline
}

# 烧录时显式传入分区表与 bootloader。优先用本次构建生成的 partition-table.bin（与 bootloader 同源），避免传 CSV 时解析/格式导致未写入正确表。
$releaseDir = Join-Path $effectiveTargetDir "$buildTarget\$buildProfile"
$bootloaderBin = Join-Path $releaseDir "bootloader.bin"
$partitionTableBin = Join-Path $releaseDir "partition-table.bin"
$partitionCsv = Join-Path $BuildRoot $partitionTable
$flashExtra = @()
$partitionTableForFlash = $null
if (Test-Path $bootloaderBin) {
  $partitionTableForFlash = if (Test-Path $partitionTableBin) { $partitionTableBin } else { $partitionCsv }
  if (Test-Path $partitionTableForFlash) {
    $flashExtra = @("--bootloader", $bootloaderBin, "--partition-table", $partitionTableForFlash)
  }
}
# 从 buildTarget（如 xtensa-esp32s3-espidf）推断 espflash --chip，避免写死
$flashChip = if ($buildTarget -match "(esp32[a-z0-9]+)") { $Matches[1] } else { $null }
if (-not $flashChip -and $doFlash) {
  Write-Error "Cannot derive chip from target for flash: $buildTarget (expected e.g. esp32s3 in triple)"
  exit 1
}

Write-BuildStatus -Step "Detected hardware / build config"

# 烧录模式：数字菜单 + 默认 1（仅更新），与 build.sh 上 Linux 部署交互风格一致。--flash-update 跳过菜单。设置 $eraseBeforeFlash
function Select-FlashMode {
  param([string]$ChosenPort, [string]$TargetTriple, [bool]$FlashUpdate)
  if ($FlashUpdate) {
    Write-Host "✓ Flash mode: update only — entire flash will NOT be erased." -ForegroundColor Green
    Write-Host "  Storage files are kept only when the storage partition offset, size, and format are unchanged."
    Write-Host ""
    $script:eraseBeforeFlash = $false
    return
  }
  if ($env:BEETLE_FLASH_MODE -eq "full-erase") {
    $script:eraseBeforeFlash = $true
    Write-Host "! Flash mode: full chip erase forced by BEETLE_FLASH_MODE=full-erase." -ForegroundColor Yellow
    Write-Host ""
    return
  }
  Write-Host "========== Flash mode ==========" -ForegroundColor Cyan
  Write-Host ""
  Write-Host "  1) Update flash — keep NVS; storage files are preserved only if partition offset/size/format are unchanged"
  Write-Host "  2) Full chip erase then flash — wipes entire flash (factory reset / partition change)"
  Write-Host "  3) Cancel"
  Write-Host ""
  while ($true) {
    $fc = Read-Host "Select [1-3] (default 1)"
    if ([string]::IsNullOrWhiteSpace($fc)) { $fc = "1" }
    $fc = $fc.Trim()
    switch ($fc) {
      "1" {
        $script:eraseBeforeFlash = $false
        Write-Host "✓ Update flash: no full erase." -ForegroundColor Green
        Write-Host ""
        return
      }
      "2" {
        Write-Host "⚠ Entire flash will be erased on $ChosenPort; firmware target: $TargetTriple" -ForegroundColor Yellow
        $confirm = Read-Host "Type 'yes' to confirm full erase and flash"
        if ($confirm.Trim() -ne 'yes') {
          Write-Host "Aborted."
          exit 0
        }
        $script:eraseBeforeFlash = $true
        Write-Host ""
        return
      }
      "3" {
        Write-Host "Cancelled."
        exit 0
      }
      default {
        Write-Host "Invalid option — enter 1, 2, or 3" -ForegroundColor Yellow
      }
    }
  }
}

# 串口格式校验（仅 COM 数字），防止命令注入与误指向
function Test-ValidFlashPort {
  param([string]$Port)
  return $Port -and ($Port -match '^COM[0-9]+$')
}
# 烧录前确保 espflash 已安装，缺则自动 cargo install
function Ensure-Espflash {
  if (Get-Command espflash -ErrorAction SilentlyContinue) { return }
  Write-Host ""
  Write-Host "========== Step: Ensuring espflash is installed ==========" -ForegroundColor Cyan
  Write-Host "  espflash not found. Running: cargo install espflash" -ForegroundColor Gray
  $saved = $env:RUSTUP_TOOLCHAIN
  $env:RUSTUP_TOOLCHAIN = "stable"
  try {
    cmd /c "cargo install espflash"
    if ($LASTEXITCODE -ne 0) {
      Write-Error "cargo install espflash failed (exit $LASTEXITCODE)"
      exit 1
    }
  } finally {
    if ($saved) { $env:RUSTUP_TOOLCHAIN = $saved } else { Remove-Item Env:RUSTUP_TOOLCHAIN -ErrorAction SilentlyContinue }
  }
  $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
  if (-not (Get-Command espflash -ErrorAction SilentlyContinue)) {
    Write-Error "espflash install failed. Try manually: cargo install espflash"
    exit 1
  }
}
# 交互式选择烧录串口：无 ESPFLASH_PORT 时枚举串口并让用户选择
function Get-FlashPort {
  if ($env:ESPFLASH_PORT) {
    if (-not (Test-ValidFlashPort $env:ESPFLASH_PORT)) {
      Write-Error "ESPFLASH_PORT must be COM followed by digits (e.g. COM3). Got: $env:ESPFLASH_PORT"
      exit 1
    }
    return $env:ESPFLASH_PORT
  }
  $ports = @([System.IO.Ports.SerialPort]::GetPortNames() | Sort-Object | Where-Object { Test-ValidFlashPort $_ })
  if ($ports.Count -eq 0) {
    Write-Host "No serial ports found. Plug in the board and try again, or set ESPFLASH_PORT=COMx" -ForegroundColor Yellow
    exit 1
  }
  if ($ports.Count -eq 1) {
    Write-Host "  Detected 1 serial port: $($ports[0])" -ForegroundColor Gray
    return $ports[0]
  }
  Write-Host "  Detected $($ports.Count) serial ports. Select port to flash (ESP board):" -ForegroundColor Gray
  for ($i = 0; $i -lt $ports.Count; $i++) {
    Write-Host "  $($i + 1). $($ports[$i])"
  }
  do {
    $sel = Read-Host "Enter number (1-$($ports.Count))"
    $num = 0
    if ([int]::TryParse($sel.Trim(), [ref]$num) -and $num -ge 1 -and $num -le $ports.Count) {
      return $ports[$num - 1]
    }
    Write-Host "Invalid, enter 1-$($ports.Count)"
  } while ($true)
}

function Set-EspPath {
  $exportPs1 = @(
    "$env:USERPROFILE\export-esp.ps1",
    "$env:USERPROFILE\.espup\export-esp.ps1",
    "$env:LOCALAPPDATA\esp-rs\export-esp.ps1"
  )
  foreach ($f in $exportPs1) {
    if (Test-Path $f) {
      . $f
      return
    }
  }
  $espBase = "$env:USERPROFILE\.rustup\toolchains\esp"
  if (Test-Path $espBase) {
    $gccDir = Get-ChildItem -Path $espBase -Recurse -Directory -ErrorAction SilentlyContinue |
      Where-Object {
        (Test-Path (Join-Path $_.FullName "bin\xtensa-esp32s3-elf-gcc.exe")) -or
        (Test-Path (Join-Path $_.FullName "bin\riscv32-esp-elf-gcc.exe"))
      } |
      Select-Object -First 1
    if ($gccDir) {
      $env:PATH = (Join-Path $gccDir.FullName "bin") + ";" + $env:PATH
    }
  }
}

Set-EspPath

# On Windows: auto-add Git for Windows / MSYS2 / MinGW bin to PATH if dlltool not in PATH
if ($env:OS -eq "Windows_NT" -and -not (Get-Command dlltool -ErrorAction SilentlyContinue)) {
  $searchDirs = @(
    "C:\Program Files\Git\usr\bin",
    "C:\Program Files\Git\mingw64\bin",
    "C:\Program Files (x86)\Git\usr\bin",
    "C:\msys64\mingw64\bin",
    "C:\msys64\usr\bin"
  )
  foreach ($d in $searchDirs) {
    if (Test-Path "$d\dlltool.exe") {
      $env:PATH = "$d;$env:PATH"
      Write-Host ">>> Auto-added to PATH: $d"
      break
    }
  }
}

# On Windows, prefer downloading espup/ldproxy prebuilt to avoid MSVC/GNU build issues
function Get-EspupWindows {
  $dest = "$env:USERPROFILE\.cargo\bin\espup.exe"
  if (Test-Path $dest) { return $true }
  $url = "https://github.com/esp-rs/espup/releases/latest/download/espup-x86_64-pc-windows-msvc.exe"
  Write-Host ">>> Downloading espup (Windows prebuilt)..."
  try {
    Invoke-WebRequest -Uri $url -OutFile $dest -UseBasicParsing
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    return $true
  } catch {
    Remove-Item $dest -Force -ErrorAction SilentlyContinue
    return $false
  }
}

# On Windows, download ldproxy prebuilt from esp-rs/embuild (no dlltool needed)
function Get-LdproxyWindows {
  $dest = "$env:USERPROFILE\.cargo\bin\ldproxy.exe"
  if (Test-Path $dest) { return $true }
  $url = "https://github.com/esp-rs/embuild/releases/download/ldproxy-v0.3.2/ldproxy-x86_64-pc-windows-msvc.zip"
  Write-Host ">>> Downloading ldproxy (Windows prebuilt)..."
  $zip = Join-Path $env:TEMP "ldproxy-windows.zip"
  $extractDir = Join-Path $env:TEMP "ldproxy_dl"
  try {
    Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
    if (Test-Path $extractDir) { Remove-Item $extractDir -Recurse -Force }
    Expand-Archive -Path $zip -DestinationPath $extractDir -Force
    $exe = Get-ChildItem -Path $extractDir -Filter "ldproxy.exe" -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($exe) {
      $cargoBin = "$env:USERPROFILE\.cargo\bin"
      if (-not (Test-Path $cargoBin)) { New-Item -ItemType Directory -Path $cargoBin -Force | Out-Null }
      Copy-Item $exe.FullName -Destination $dest -Force
      $env:PATH = "$cargoBin;$env:PATH"
      return $true
    }
    return $false
  } catch {
    return $false
  } finally {
    Remove-Item $zip -Force -ErrorAction SilentlyContinue
    Remove-Item $extractDir -Recurse -Force -ErrorAction SilentlyContinue
  }
}

# On Windows without dlltool: only stable-gnu can build ldproxy; we do not use MSVC
function Get-StableToolchainForInstall {
  if (-not ($env:OS -eq "Windows_NT")) { return "stable" }
  if (Get-Command dlltool -ErrorAction SilentlyContinue) {
    $gnu = "stable-x86_64-pc-windows-gnu"
    $list = rustup toolchain list 2>$null | Out-String
    if ($list -like "*$gnu*") { return $gnu }
    Write-Host ">>> Installing toolchain $gnu..."
    rustup install $gnu | Out-Host
    return $gnu
  }
  Write-Host ""
  Write-Host "On Windows, ldproxy download failed. Add Git for Windows bin to PATH:" -ForegroundColor Red
  Write-Host "  e.g.  C:\Program Files\Git\usr\bin   or  C:\Program Files\Git\mingw64\bin"
  Write-Host "Then run this script again."
  exit 1
}

# Install ESP toolchain if ESP GCC is missing
if (-not (Get-Command xtensa-esp32s3-elf-gcc -ErrorAction SilentlyContinue) -and -not (Get-Command riscv32-esp-elf-gcc -ErrorAction SilentlyContinue)) {
  Write-Host ""
  Write-Host "========== Step: Installing ESP Rust toolchain (espup) ==========" -ForegroundColor Cyan
  Write-Host "  ESP GCC toolchains not found. Running espup install." -ForegroundColor Gray
  if (-not (Get-Command espup -ErrorAction SilentlyContinue)) {
    if ($env:OS -eq "Windows_NT") {
      if (-not (Get-EspupWindows)) {
        $toolchain = Get-StableToolchainForInstall
        Write-Host ">>> Installing espup (using $toolchain)..."
        $env:RUSTUP_TOOLCHAIN = $toolchain
        cargo install espup
        Remove-Item Env:RUSTUP_TOOLCHAIN -ErrorAction SilentlyContinue
      }
    } else {
      $env:RUSTUP_TOOLCHAIN = "stable"
      cargo install espup
      Remove-Item Env:RUSTUP_TOOLCHAIN -ErrorAction SilentlyContinue
    }
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
  }
  espup install
  Set-EspPath
  if (-not (Get-Command xtensa-esp32s3-elf-gcc -ErrorAction SilentlyContinue) -and -not (Get-Command riscv32-esp-elf-gcc -ErrorAction SilentlyContinue)) {
    Write-Error "ESP GCC toolchains still not found after espup install"
    exit 1
  }
}

# Install ldproxy if missing (on Windows try prebuilt first, else cargo install with dlltool)
if (-not (Get-Command ldproxy -ErrorAction SilentlyContinue)) {
  Write-Host ""
  Write-Host "========== Step: Installing ldproxy (linker wrapper) ==========" -ForegroundColor Cyan
  if ($env:OS -eq "Windows_NT" -and (Get-LdproxyWindows)) {
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
  } else {
    $toolchain = Get-StableToolchainForInstall
    Write-Host ">>> Installing ldproxy (using $toolchain)..."
    $env:RUSTUP_TOOLCHAIN = $toolchain
    cargo install ldproxy
    Remove-Item Env:RUSTUP_TOOLCHAIN -ErrorAction SilentlyContinue
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
  }
}

# 构建参数：release 始终在未传 --target 时注入默认 ESP triple（与 build.sh 一致）
$releaseArgs = @()
if ($buildArgs -notcontains "--target") { $releaseArgs += "--target", $buildTarget }
if ($buildFeatures.Count -gt 0) { $releaseArgs += $buildFeatures }
$releaseArgs += $buildArgs

# Windows: run cargo in a cmd that has run vcvars64 (so LIB/kernel32.lib is set). Requires VS with "Desktop dev with C++" and Windows 10/11 SDK. If LNK1181 persists, run build.cmd from "x64 Native Tools Command Prompt for VS".
if ($env:OS -eq "Windows_NT") {
  $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
  if (Test-Path $vswhere) {
    $vsPath = (& $vswhere -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null)
    if ($vsPath) {
      $vsPath = $vsPath.Trim()
      $vcvars = Join-Path $vsPath "VC\Auxiliary\Build\vcvars64.bat"
      $vsDevCmd = Join-Path $vsPath "Common7\Tools\VsDevCmd.bat"
      $devBat = if (Test-Path $vcvars) { $vcvars } elseif (Test-Path $vsDevCmd) { $vsDevCmd } else { $null }
      $sdkLib = $null
      foreach ($base in @("${env:ProgramFiles(x86)}\Windows Kits\10\Lib", "${env:ProgramFiles}\Windows Kits\10\Lib")) {
        if (Test-Path $base) {
          $ver = Get-ChildItem $base -Directory -ErrorAction SilentlyContinue | Sort-Object Name -Descending | Select-Object -First 1
          if ($ver) {
            $um64 = Join-Path $ver.FullName "um\x64"
            if (Test-Path (Join-Path $um64 "kernel32.lib")) { $sdkLib = $um64; break }
          }
        }
      }
      if ($devBat) {
        Write-Host ""
        Write-Host "========== Step: Building release (MSVC environment) ==========" -ForegroundColor Cyan
        Write-Host "  Target: $buildTarget  |  Root: $BuildRoot" -ForegroundColor Gray
        Refresh-EspComponentGraphCache
        $argStr = ($releaseArgs | ForEach-Object { "`"$_`"" }) -join " "
        $libLine = if ($sdkLib) { "set `"LIB=$sdkLib;%LIB%`"" } else { "" }
        $cargoBuildCmd = if ($buildProfile -eq "release") { "cargo build --release $argStr" } else { "cargo build --profile $buildProfile $argStr" }
        $bat = @"
@echo off
set "VSCMD_SKIP_SENDTELEMETRY=1"
call "$devBat"
$libLine
cd /d "$BuildRoot"
$cargoBuildCmd
"@
        $batFile = Join-Path $env:TEMP "pc_cargo_build.bat"
        $bat | Out-File -FilePath $batFile -Encoding ASCII
        try {
          & cmd /c "`"$batFile`""
          $buildExit = $LASTEXITCODE
          if ($buildExit -eq 0 -and $doFlash) {
            $bin = Join-Path $effectiveTargetDir "$buildTarget\$buildProfile\beetle.exe"
            if (-not (Test-Path $bin)) {
              Write-Error "Binary not found: $bin"
              exit 1
            }
            Ensure-Espflash
            $chosenPort = Get-FlashPort
            Write-Host ""
            Write-Host "=========================================="
            Write-Host "  Beetle — Flash to device"
            Write-Host "=========================================="
            Write-Host ""
            Write-BuildStatus -Step "Flash: hardware and paths" -BeforeFlash -ChosenPort $chosenPort -BinPath $bin
            Write-Host "========== Checking connection ==========" -ForegroundColor Cyan
            Write-Host ""
            $null = & espflash board-info --port $chosenPort --chip $flashChip 2>$null
            if ($LASTEXITCODE -eq 0) {
              Write-Host "✓ board-info OK" -ForegroundColor Green
            } else {
              Write-Host "⚠ Could not read board-info from $chosenPort (connection or chip mismatch)." -ForegroundColor Yellow
              Write-Host "  Proceeding anyway; if flash fails, check port, download mode, or chip."
            }
            Write-Host ""
            Select-FlashMode -ChosenPort $chosenPort -TargetTriple $buildTarget -FlashUpdate:$flashUpdate
            if ($eraseBeforeFlash) {
              Write-Host "========== Erasing entire flash ==========" -ForegroundColor Cyan
              Write-Host ""
              Write-Host "  Port: $chosenPort  |  Chip: $flashChip" -ForegroundColor Gray
              espflash erase-flash --port $chosenPort --chip $flashChip
              if ($LASTEXITCODE -ne 0) { Write-Host "Erase failed (exit $LASTEXITCODE)." -ForegroundColor Red; exit $LASTEXITCODE }
              Write-Host "✓ Erase completed. Waiting 2s before flash." -ForegroundColor Green
              Start-Sleep -Seconds 2
            } else {
              Write-Host "========== Skipping full erase (update flash) ==========" -ForegroundColor Cyan
              Write-Host ""
              Write-Host "  ✓ NVS and other flash regions are left unchanged only when the storage format is unchanged." -ForegroundColor Green
              Write-Host "  Port: $chosenPort  |  Chip: $flashChip" -ForegroundColor Gray
            }
            Write-Host ""
            Write-Host "========== Flashing firmware ==========" -ForegroundColor Cyan
            Write-Host ""
            Write-Host "  Binary: $bin  |  Partition table: $(if ($partitionTableForFlash) { $partitionTableForFlash } else { $partitionCsv })" -ForegroundColor Gray
            & espflash flash --port $chosenPort --chip $flashChip @flashExtra $bin
            if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
            if (-not (Write-ModelPartition -ChosenPort $chosenPort)) { exit 1 }
            Write-Host ""
            Write-Host "✓ Flash complete." -ForegroundColor Green
            if (-not $noMonitor) {
              Start-EspMonitor -ChosenPort $chosenPort -BinPath $bin
              exit $LASTEXITCODE
            }
            exit 0
          }
          exit $buildExit
        } finally {
          Remove-Item $batFile -Force -ErrorAction SilentlyContinue
        }
      }
    }
  }
}

$env:ESP_IDF_SDKCONFIG_DEFAULTS = if ($boardSdkconfigOverlay) {
  "sdkconfig.defaults;sdkconfig.defaults.$targetMcu;$boardSdkconfigOverlay"
} else {
  "sdkconfig.defaults;sdkconfig.defaults.$targetMcu"
}

Write-Host ""
Write-Host "========== Step: Building release ==========" -ForegroundColor Cyan
Write-Host "  Target: $buildTarget  |  Root: $BuildRoot" -ForegroundColor Gray
Refresh-EspComponentGraphCache
if ($buildProfile -eq "release") {
  cargo build --release @releaseArgs
} else {
  cargo build --profile $buildProfile @releaseArgs
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if ($doFlash) {
  $bin = Join-Path $effectiveTargetDir "$buildTarget\$buildProfile\beetle.exe"
  if (-not (Test-Path $bin)) {
    Write-Error "Binary not found: $bin"
    exit 1
  }
  Ensure-Espflash
  $chosenPort = Get-FlashPort
  Write-Host ""
  Write-Host "=========================================="
  Write-Host "  Beetle — Flash to device"
  Write-Host "=========================================="
  Write-Host ""
  Write-BuildStatus -Step "Flash: hardware and paths" -BeforeFlash -ChosenPort $chosenPort -BinPath $bin
  Write-Host "========== Checking connection ==========" -ForegroundColor Cyan
  Write-Host ""
  $null = & espflash board-info --port $chosenPort --chip $flashChip 2>$null
  if ($LASTEXITCODE -eq 0) {
    Write-Host "✓ board-info OK" -ForegroundColor Green
  } else {
    Write-Host "⚠ Could not read board-info from $chosenPort (connection or chip mismatch)." -ForegroundColor Yellow
    Write-Host "  Proceeding anyway; if flash fails, check port, download mode, or chip."
  }
  Write-Host ""
  Select-FlashMode -ChosenPort $chosenPort -TargetTriple $buildTarget -FlashUpdate:$flashUpdate
  if ($eraseBeforeFlash) {
    Write-Host "========== Erasing entire flash ==========" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "  Port: $chosenPort  |  Chip: $flashChip" -ForegroundColor Gray
    espflash erase-flash --port $chosenPort --chip $flashChip
    if ($LASTEXITCODE -ne 0) { Write-Host "Erase failed (exit $LASTEXITCODE)." -ForegroundColor Red; exit $LASTEXITCODE }
    Write-Host "✓ Erase completed. Waiting 2s before flash." -ForegroundColor Green
    Start-Sleep -Seconds 2
  } else {
    Write-Host "========== Skipping full erase (update flash) ==========" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "  ✓ NVS and other flash regions are left unchanged only when the storage format is unchanged." -ForegroundColor Green
    Write-Host "  Port: $chosenPort  |  Chip: $flashChip" -ForegroundColor Gray
  }
  Write-Host ""
  Write-Host "========== Flashing firmware ==========" -ForegroundColor Cyan
  Write-Host ""
  Write-Host "  Binary: $bin  |  Partition table: $(if ($partitionTableForFlash) { $partitionTableForFlash } else { $partitionCsv })" -ForegroundColor Gray
  & espflash flash --port $chosenPort --chip $flashChip @flashExtra $bin
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  if (-not (Write-ModelPartition -ChosenPort $chosenPort)) { exit 1 }
  Write-Host ""
  Write-Host "✓ Flash complete." -ForegroundColor Green
  if (-not $noMonitor) {
    Start-EspMonitor -ChosenPort $chosenPort -BinPath $bin
    exit $LASTEXITCODE
  }
}
