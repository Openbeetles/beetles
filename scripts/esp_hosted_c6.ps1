$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::InputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RootDir = Split-Path -Parent $ScriptDir
$ProjectDir = Join-Path $RootDir "third_party\esp-hosted-mcu\slave"
$BuildDir = Join-Path $RootDir "target\esp-hosted-c6\build"
$SdkconfigPath = Join-Path $RootDir "target\esp-hosted-c6\sdkconfig"
$SdkconfigDefaults = @(
  (Join-Path $ProjectDir "sdkconfig.defaults"),
  (Join-Path $ProjectDir "sdkconfig.defaults.esp32c6"),
  (Join-Path $RootDir "sdkconfig.defaults.esp32c6.hosted_p4_function_board")
) -join ";"

function Show-Usage {
  Write-Host @"
Usage:
  .\build.ps1 build-c6
  .\build.ps1 flash-c6
  .\build.ps1 flash-all [P4 build args...]

Environment:
  ESP_HOSTED_C6_PORT   Optional. Serial port for the on-board C6.
  ESPFLASH_PORT        Optional. Serial port for the P4 main firmware.
"@
}

function Ensure-ProjectPresent {
  if (-not (Test-Path (Join-Path $ProjectDir "CMakeLists.txt"))) {
    throw "Vendored esp-hosted-mcu slave project not found at $ProjectDir"
  }
}

function Set-EspPath {
  $exportPs1 = @(
    "$env:USERPROFILE\export-esp.ps1",
    "$env:USERPROFILE\.espup\export-esp.ps1",
    "$env:LOCALAPPDATA\esp-rs\export-esp.ps1"
  )
  if ($env:IDF_PATH) {
    $exportPs1 += (Join-Path $env:IDF_PATH "export.ps1")
  }
  $exportPs1 += @(
    "$env:USERPROFILE\esp\esp-idf\export.ps1",
    "$env:USERPROFILE\.espressif\esp-idf\export.ps1"
  )
  $exportPs1 += Get-ChildItem -Path "$env:USERPROFILE\.espressif" -Filter "esp-idf*" -Directory -ErrorAction SilentlyContinue |
    Sort-Object Name -Descending |
    ForEach-Object { Join-Path $_.FullName "export.ps1" }
  $exportPs1 += Get-ChildItem -Path "$env:USERPROFILE\esp" -Directory -ErrorAction SilentlyContinue |
    Sort-Object Name -Descending |
    ForEach-Object { Join-Path $_.FullName "esp-idf\export.ps1" }

  foreach ($f in $exportPs1) {
    if (Test-Path $f) {
      . $f
      if (Get-Command idf.py -ErrorAction SilentlyContinue) {
        return
      }
    }
  }
}

function Ensure-IdfPy {
  Set-EspPath
  if (-not (Get-Command idf.py -ErrorAction SilentlyContinue)) {
    throw "idf.py not found after loading ESP environment. Install/source ESP-IDF first."
  }
}

function Invoke-HostedIdf {
  param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$ExtraArgs
  )

  Ensure-ProjectPresent
  Ensure-IdfPy
  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $SdkconfigPath) | Out-Null

  Push-Location $ProjectDir
  try {
    $env:IDF_TARGET = "esp32c6"
    $env:SDKCONFIG_DEFAULTS = $SdkconfigDefaults
    & idf.py -B $BuildDir "-DIDF_TARGET=esp32c6" "-DSDKCONFIG=$SdkconfigPath" "-DSDKCONFIG_DEFAULTS=$SdkconfigDefaults" @ExtraArgs
  } finally {
    Pop-Location
  }
}

function Write-Artifacts {
  Write-Host ""
  Write-Host "========== ESP-Hosted C6 artifacts ==========" -ForegroundColor Cyan
  Write-Host "  Project:         $ProjectDir"
  Write-Host "  Build dir:       $BuildDir"
  Write-Host "  SDKCONFIG:       $SdkconfigPath"
  Write-Host "  Defaults chain:  $SdkconfigDefaults"
  Write-Host "  App binary:      $(Join-Path $BuildDir 'network_adapter.bin')"
  Write-Host "  Bootloader:      $(Join-Path $BuildDir 'bootloader\bootloader.bin')"
  Write-Host "  Partition table: $(Join-Path $BuildDir 'partition_table\partition-table.bin')"
  Write-Host ""
}

function Get-C6Port {
  if ($env:ESP_HOSTED_C6_PORT) {
    return $env:ESP_HOSTED_C6_PORT
  }

  $ports = @([System.IO.Ports.SerialPort]::GetPortNames() | Sort-Object)
  if ($ports.Count -eq 0) {
    throw "No serial ports found. Set ESP_HOSTED_C6_PORT=COMx"
  }
  if ($ports.Count -eq 1) {
    return $ports[0]
  }

  Write-Host "Detected serial ports for C6 flashing:"
  for ($i = 0; $i -lt $ports.Count; $i++) {
    Write-Host "  $($i + 1)) $($ports[$i])"
  }
  while ($true) {
    $sel = Read-Host "Select C6 port [1-$($ports.Count)]"
    $num = 0
    if ([int]::TryParse($sel, [ref]$num) -and $num -ge 1 -and $num -le $ports.Count) {
      return $ports[$num - 1]
    }
    Write-Host "Invalid selection." -ForegroundColor Yellow
  }
}

function Write-C6FlashNotice {
  Write-Host "Before flashing the on-board ESP32-C6:" -ForegroundColor Yellow
  Write-Host "  1. Connect the programmer/UART adapter to the board's PROG_C6 header."
  Write-Host "  2. Put the ESP32-P4 into bootloader mode so it does not interfere with the C6 bus."
  Write-Host ""
}

$command = if ($args.Count -gt 0) { $args[0] } else { "" }
$rest = if ($args.Count -gt 1) { $args[1..($args.Count - 1)] } else { @() }
if ($rest.Count -gt 0 -and @("-h", "--help", "help") -contains $rest[0]) {
  Show-Usage
  exit 0
}

switch ($command) {
  "" { Show-Usage; exit 0 }
  "-h" { Show-Usage; exit 0 }
  "--help" { Show-Usage; exit 0 }
  "help" { Show-Usage; exit 0 }
  "build-c6" {
    Invoke-HostedIdf build @rest
    Write-Artifacts
    exit 0
  }
  "flash-c6" {
    Write-C6FlashNotice
    Invoke-HostedIdf build
    $port = Get-C6Port
    Write-Host "Flashing ESP32-C6 on port: $port"
    Invoke-HostedIdf "-p" $port flash
    Write-Artifacts
    exit 0
  }
  "flash-all" {
    & $PSCommandPath flash-c6
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $savedBoard = $env:BOARD
    try {
      $env:BOARD = "esp32-p4-nano-16mb"
      & (Join-Path $RootDir "build.ps1") --flash @rest
      exit $LASTEXITCODE
    } finally {
      if ($null -eq $savedBoard) {
        Remove-Item Env:BOARD -ErrorAction SilentlyContinue
      } else {
        $env:BOARD = $savedBoard
      }
    }
  }
  default {
    throw "Unknown C6 hosted subcommand: $command"
  }
}
