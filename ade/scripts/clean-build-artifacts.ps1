<#
.SYNOPSIS
  Reclaim disk from rebuildable build artefacts across every Orrery checkout.

.DESCRIPTION
  Reports (default) or deletes cargo `target`, `node_modules`, and front-end
  output directories under the known Orrery roots.

  Everything it removes is REBUILDABLE: `cargo build` / `pnpm install` restore it.
  It never touches source, git metadata, or application data.

.SAFETY
  Six independent guards, each of which alone would prevent a wrong delete:

    1. ROOT WHITELIST   - the resolved path must sit under an allow-listed root.
    2. PREFIX-SAFE      - the root check appends a separator, so `C:\proj` never
                          matches `C:\project-other`.
    3. NAME WHITELIST   - the directory's own name must be an exact match from a
                          fixed list. No globs, no patterns.
    4. NO REPARSE       - junctions and symlinks are skipped outright, so a link
                          inside node_modules can never redirect the delete.
    5. POSITIVE MARKER  - a `target` dir must carry a real cargo marker
                          (CACHEDIR.TAG / .rustc_info.json), and must NOT contain
                          a Cargo.toml. A source crate named `target` is skipped.
    6. DRY RUN DEFAULT  - nothing is deleted without -Execute.

.PARAMETER Execute
  Actually delete. Without it the script only reports.

.PARAMETER KeepDays
  Skip artefacts modified within this many days. Default 0 (delete all matched).

.PARAMETER Kind
  Limit to 'cargo', 'node', or 'web'. Default: all.

.EXAMPLE
  .\clean-build-artifacts.ps1
  .\clean-build-artifacts.ps1 -KeepDays 7
  .\clean-build-artifacts.ps1 -Execute -KeepDays 14
#>
[CmdletBinding()]
param(
  [switch] $Execute,
  [int]    $KeepDays = 0,
  [ValidateSet('all', 'cargo', 'node', 'web')]
  [string] $Kind = 'all',

  # Skip discovery and delete the paths in this file, one per line.
  # Every safety guard below still runs against each path, so a bad list
  # cannot widen what this script is willing to touch.
  [string] $ListFile,

  # Concurrent robocopy workers. The workload is many small trees with Defender
  # inspecting every unlink, so this is CPU-bound: ~CPU count is the sweet spot.
  [int] $Workers = [Math]::Max(4, [Environment]::ProcessorCount)
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------- guard 1 + 2
# Only ever look inside these. Anything resolving outside is refused.
$AllowedRoots = @(
  'C:\Users\narut\AppData\Roaming\com.kouji.orrery\worktrees',
  'C:\Users\narut\Desktop\projects'
) | Where-Object { Test-Path -LiteralPath $_ }

if (-not $AllowedRoots) { throw 'No allowed root exists on this machine. Refusing to run.' }

function Test-UnderAllowedRoot {
  param([string] $Path)
  $full = [System.IO.Path]::GetFullPath($Path)
  foreach ($r in $AllowedRoots) {
    $root = [System.IO.Path]::GetFullPath($r).TrimEnd('\') + '\'   # trailing sep = prefix-safe
    if ($full.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) { return $true }
  }
  return $false
}

# -------------------------------------------------------------------- guard 3
# Exact directory names only. Grouped so -Kind can narrow them.
$Names = @{
  cargo = @('target', 'target-test')
  node  = @('node_modules')
  web   = @('dist', 'dist-ext', '.angular', 'out-tsc', '.nx', 'playwright-report', 'test-results')
}
$Wanted = if ($Kind -eq 'all') { $Names.cargo + $Names.node + $Names.web } else { $Names[$Kind] }

# Never descend into these while searching (also stops nested node_modules noise).
$PruneAt = @('node_modules', 'target', 'target-test', '.git', 'dist', '.angular')

# -------------------------------------------------------------------- guard 5
function Test-IsRealCargoTarget {
  param([string] $Path)
  # A source crate that happens to be named `target` has a Cargo.toml. A build dir does not.
  if (Test-Path -LiteralPath (Join-Path $Path 'Cargo.toml')) { return $false }
  foreach ($m in @('CACHEDIR.TAG', '.rustc_info.json', 'debug', 'release')) {
    if (Test-Path -LiteralPath (Join-Path $Path $m)) { return $true }
  }
  return $false
}

function Test-IsRealNodeModules {
  param([string] $Path)
  if (Test-Path -LiteralPath (Join-Path $Path 'Cargo.toml')) { return $false }
  # Any populated node_modules has package dirs or a store marker.
  foreach ($m in @('.modules.yaml', '.package-lock.json', '.bin', '.pnpm')) {
    if (Test-Path -LiteralPath (Join-Path $Path $m)) { return $true }
  }
  return ((Get-ChildItem -LiteralPath $Path -Directory -Force -ErrorAction SilentlyContinue |
           Select-Object -First 1) -ne $null)
}

# ------------------------------------------------------------------ discovery
# Manual walk so we can prune; Get-ChildItem -Recurse would descend into every
# node_modules and take minutes.
function Find-Artefacts {
  param([string] $Root)
  $found = New-Object System.Collections.ArrayList
  $stack = New-Object System.Collections.Stack
  $stack.Push($Root)

  while ($stack.Count -gt 0) {
    $dir = $stack.Pop()
    $kids = Get-ChildItem -LiteralPath $dir -Directory -Force -ErrorAction SilentlyContinue
    foreach ($k in $kids) {

      # -------------------------------------------------------------- guard 4
      if ($k.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { continue }

      if ($Wanted -contains $k.Name) {
        $null = $found.Add($k)
        continue                      # never recurse into something we will delete
      }
      if ($PruneAt -notcontains $k.Name) { $stack.Push($k.FullName) }
    }
  }
  return $found
}

# Measure-Object emits no Sum property for an empty pipeline, and StrictMode
# turns reading it into a throw. Every sum in this script goes through here.
function Get-SafeSum {
  param($Items, [string] $Property)
  if (-not $Items) { return 0.0 }
  $m = $Items | Measure-Object -Property $Property -Sum
  if (-not $m) { return 0.0 }
  if (-not ($m.PSObject.Properties.Name -contains 'Sum')) { return 0.0 }
  if ($null -eq $m.Sum) { return 0.0 }
  return [double] $m.Sum
}

function Get-SizeGB {
  param([string] $Path)
  $files = Get-ChildItem -LiteralPath $Path -Recurse -File -Force -ErrorAction SilentlyContinue
  return [math]::Round((Get-SafeSum $files 'Length') / 1GB, 2)
}

$candidates = New-Object System.Collections.ArrayList

if ($ListFile) {
  if (-not (Test-Path -LiteralPath $ListFile)) { throw "List file not found: $ListFile" }
  Write-Host "Loading list from $ListFile" -ForegroundColor Cyan

  foreach ($line in (Get-Content -LiteralPath $ListFile)) {
    $p = $line.Trim()
    if (-not $p) { continue }
    if (-not (Test-Path -LiteralPath $p)) { continue }          # already gone

    # Every guard re-applied. A hand-edited list gets no more privilege
    # than a discovered one.
    if (-not (Test-UnderAllowedRoot $p)) {                                    # guard 1+2
      Write-Host "  REFUSED (outside allowed roots): $p" -ForegroundColor Red; continue
    }
    $item = Get-Item -LiteralPath $p -Force
    if (-not $item.PSIsContainer) {
      Write-Host "  REFUSED (not a directory): $p" -ForegroundColor Red; continue
    }
    if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {    # guard 4
      Write-Host "  SKIPPED (junction/symlink): $p" -ForegroundColor Yellow; continue
    }
    if ($Wanted -notcontains $item.Name) {                                    # guard 3
      Write-Host "  REFUSED (name not on whitelist): $p" -ForegroundColor Red; continue
    }
    if (($Names.cargo -contains $item.Name) -and -not (Test-IsRealCargoTarget $p)) {   # guard 5
      Write-Host "  REFUSED (not a cargo target): $p" -ForegroundColor Red; continue
    }
    if (($Names.node -contains $item.Name) -and -not (Test-IsRealNodeModules $p)) {
      Write-Host "  REFUSED (not a node_modules): $p" -ForegroundColor Red; continue
    }

    $null = $candidates.Add([pscustomobject]@{
      GB   = -1.0
      Days = [int](New-TimeSpan -Start $item.LastWriteTime).TotalDays
      Kind = $item.Name
      Path = $item.FullName
      Skip = $false        # the list was already filtered by age when it was built
    })
  }
  Write-Host ("Loaded {0} directories." -f $candidates.Count) -ForegroundColor Cyan
}
else {

Write-Host 'Scanning...' -ForegroundColor Cyan
foreach ($root in $AllowedRoots) {
  foreach ($d in (Find-Artefacts -Root $root)) {

    if (-not (Test-UnderAllowedRoot $d.FullName)) { continue }          # guard 1+2

    $isCargo = $Names.cargo -contains $d.Name
    $isNode  = $Names.node  -contains $d.Name
    if ($isCargo -and -not (Test-IsRealCargoTarget $d.FullName)) { continue }   # guard 5
    if ($isNode  -and -not (Test-IsRealNodeModules  $d.FullName)) { continue }

    $ageDays = [int](New-TimeSpan -Start $d.LastWriteTime).TotalDays
    # Sizing is a full recursive enumeration per directory - the dominant cost on
    # a 273 GB tree with Defender inspecting every file. It buys a nicer report
    # and nothing else, so -Execute skips it and measures the disk delta instead.
    $null = $candidates.Add([pscustomobject]@{
      GB     = $(if ($Execute) { -1.0 } else { Get-SizeGB $d.FullName })
      Days   = $ageDays
      Kind   = $d.Name
      Path   = $d.FullName
      Skip   = ($ageDays -lt $KeepDays)
    })
  }
}

}   # end discovery branch

$keep = @($candidates | Where-Object { $_.Skip })
$kill = @($candidates | Where-Object { -not $_.Skip } | Sort-Object GB -Descending)

'{0,8}  {1,6}  {2,-18} {3}' -f 'GB', 'AGE', 'KIND', 'PATH'
'-' * 118
foreach ($c in $kill) {
  $sz = $(if ($c.GB -lt 0) { '      -' } else { '{0,7}' -f $c.GB })
  '{0}  {1,5}d  {2,-18} {3}' -f $sz, $c.Days, $c.Kind, $c.Path
}
'-' * 118
if ($Execute) {
  '{0} directories queued for deletion (sizes skipped for speed)' -f $kill.Count
} else {
  '{0} directories, {1} GB reclaimable' -f $kill.Count, ([math]::Round((Get-SafeSum $kill 'GB'), 1))
}
if ($keep.Count) {
  if ($Execute) { '{0} skipped as newer than {1} days' -f $keep.Count, $KeepDays }
  else { '{0} skipped as newer than {1} days ({2} GB kept warm)' -f $keep.Count, $KeepDays, ([math]::Round((Get-SafeSum $keep 'GB'),1)) }
}

if (-not $Execute) {
  ''
  Write-Host 'DRY RUN - nothing deleted. Re-run with -Execute to remove the above.' -ForegroundColor Yellow
  return
}

# --------------------------------------------------------------------- delete
# robocopy /MIR from an empty dir is the fastest large-tree delete on Windows:
# multi-threaded, and it sidesteps the per-item pipeline overhead that makes
# Remove-Item -Recurse crawl on trees of 100k+ files.
$empty = Join-Path ([System.IO.Path]::GetTempPath()) ('orrery-empty-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $empty -Force | Out-Null

function Get-FreeGB {
  [math]::Round((Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='C:'").FreeSpace / 1GB, 1)
}
$freeBefore = Get-FreeGB
# Guards run here, sequentially: they are metadata-only and cost nothing. Only the
# actual deletion is parallelised, so no check is weakened by concurrency.
$failed = @()
$targets = foreach ($c in $kill) {
  if (-not (Test-UnderAllowedRoot $c.Path)) { $failed += "$($c.Path) (root check)"; continue }   # re-checked
  if (-not (Test-Path -LiteralPath $c.Path)) { continue }                                        # already gone
  $item = Get-Item -LiteralPath $c.Path -Force
  if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { continue }              # re-checked
  $c.Path
}
$targets = @($targets)
Write-Host ("Deleting {0} directories with {1} parallel workers..." -f $targets.Count, $Workers) -ForegroundColor Cyan

# One robocopy at a time was the old shape, and it was wrong for this workload:
# each spawn costs ~150 ms and a 3 MB node_modules never lets /MT stretch, so on
# hundreds of small trees the run is dominated by process startup. Deleting
# DIFFERENT trees concurrently is what actually helps - and with Defender
# inspecting every unlink, the real bottleneck is CPU, not disk queue depth.
try {
  $results = $targets | ForEach-Object -ThrottleLimit $Workers -Parallel {
    $p = $_
    $empty = $using:empty
    $null = robocopy $empty $p /MIR /MT:8 /R:1 /W:1 /NFL /NDL /NJH /NJS /NC /NS /NP
    $rc = $LASTEXITCODE
    if ($rc -ge 8) { return "FAIL|$p|robocopy $rc" }
    Remove-Item -LiteralPath $p -Recurse -Force -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $p) { return "FAIL|$p|still present - in use?" }
    return "OK|$p"
  }
} finally {
  Remove-Item -LiteralPath $empty -Recurse -Force -ErrorAction SilentlyContinue
}

foreach ($r in $results) {
  if ($r -like 'FAIL|*') { $parts = $r -split '\|', 3; $failed += "$($parts[1]) ($($parts[2]))" }
}
Write-Host ("Removed {0} of {1}." -f (@($results | Where-Object { $_ -like 'OK|*' }).Count), $targets.Count)

''
$freeAfter = Get-FreeGB
Write-Host ('Freed {0} GB  ({1} GB -> {2} GB free).' -f [math]::Round($freeAfter - $freeBefore,1), $freeBefore, $freeAfter) -ForegroundColor Green
if ($failed.Count) {
  Write-Host ('{0} could not be removed:' -f $failed.Count) -ForegroundColor Yellow
  $failed | ForEach-Object { "  $_" }
}
