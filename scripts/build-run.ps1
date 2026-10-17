# Builds this fork and starts it (same settings and data as the installed MooshieUI), with a progress window.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$repo  = Split-Path -Parent $PSScriptRoot
$built = Join-Path $repo 'src-tauri\target\release\comfyui-desktop.exe'
$log   = Join-Path $env:TEMP 'mooshieui-build.log'

$form = New-Object System.Windows.Forms.Form
$form.Text = 'Building MooshieUI (fork)'
$form.ClientSize = New-Object System.Drawing.Size(620, 366)
$form.StartPosition = 'CenterScreen'
$form.FormBorderStyle = 'FixedDialog'
$form.MaximizeBox = $false

$label = New-Object System.Windows.Forms.Label
$label.Location = New-Object System.Drawing.Point(12, 12)
$label.Size = New-Object System.Drawing.Size(596, 24)
$label.Font = New-Object System.Drawing.Font('Segoe UI', 11)

$bar = New-Object System.Windows.Forms.ProgressBar
$bar.Location = New-Object System.Drawing.Point(12, 44)
$bar.Size = New-Object System.Drawing.Size(596, 22)

$box = New-Object System.Windows.Forms.TextBox
$box.Location = New-Object System.Drawing.Point(12, 80)
$box.Size = New-Object System.Drawing.Size(596, 236)
$box.Multiline = $true
$box.ReadOnly = $true
$box.ScrollBars = 'Vertical'
$box.Font = New-Object System.Drawing.Font('Consolas', 9)

$btnLog = New-Object System.Windows.Forms.Button
$btnLog.Text = 'Open log'
$btnLog.Location = New-Object System.Drawing.Point(12, 326)
$btnLog.Size = New-Object System.Drawing.Size(120, 28)
$btnLog.Visible = $false

$btnClose = New-Object System.Windows.Forms.Button
$btnClose.Text = 'Close'
$btnClose.Location = New-Object System.Drawing.Point(512, 326)
$btnClose.Size = New-Object System.Drawing.Size(96, 28)
$btnClose.Visible = $false

$form.Controls.AddRange(@($label, $bar, $box, $btnLog, $btnClose))
$btnClose.Add_Click({ $form.Close() })
$btnLog.Add_Click({ Start-Process -FilePath 'notepad.exe' -ArgumentList $log })

function Show-Step([string]$text, [int]$pct) {
    $label.Text = $text
    $bar.Value = [Math]::Min(100, [Math]::Max(0, $pct))
    [System.Windows.Forms.Application]::DoEvents()
}

function Show-Log {
    if (Test-Path -LiteralPath $log) {
        $box.Text = (@(Get-Content -LiteralPath $log -Tail 14 -ErrorAction SilentlyContinue) -join "`r`n")
        $box.SelectionStart = $box.TextLength
        $box.ScrollToCaret()
    }
    [System.Windows.Forms.Application]::DoEvents()
}

function Run-Logged([string]$step, [int]$from, [int]$to, [string]$cmd) {
    $p = Start-Process -FilePath 'cmd.exe' -ArgumentList "/c $cmd >> `"$log`" 2>&1" -WorkingDirectory $repo -WindowStyle Hidden -PassThru
    $start = Get-Date
    while (-not $p.HasExited) {
        $mins = ((Get-Date) - $start).TotalMinutes
        Show-Step ("{0} ({1:0.0} min)" -f $step, $mins) ([int]($from + ($to - $from) * [Math]::Min(0.95, $mins / 15)))
        Show-Log
        Start-Sleep -Milliseconds 800
    }
    $p.WaitForExit()
    if ($p.ExitCode -ne 0) { throw "$step failed (exit code $($p.ExitCode)). See the log." }
}

$form.Add_Shown({
    try {
        Set-Content -LiteralPath $log -Value '' -Encoding utf8
        Run-Logged 'Installing packages' 2 10 'npm ci --no-audit --no-fund'
        # --no-bundle: just the exe; no installer or updater files (those need upstream's signing key).
        # Calls tauri directly: npm run tauri's wrapper loses the quotes on its --config JSON on Windows.
        Run-Logged 'Building app (first build takes a while)' 10 92 'npx tauri build --no-bundle'

        if (Get-Process comfyui-desktop -ErrorAction SilentlyContinue | Where-Object { $_.Path -ne $built }) {
            Show-Step 'Built. Close the installed MooshieUI to start the new one...' 95
            while (Get-Process comfyui-desktop -ErrorAction SilentlyContinue | Where-Object { $_.Path -ne $built }) {
                [System.Windows.Forms.Application]::DoEvents()
                Start-Sleep -Milliseconds 500
            }
        }

        Show-Step 'Starting MooshieUI (fork)' 100
        Start-Process -FilePath $built
        Start-Sleep -Seconds 2
        $form.Close()
    } catch {
        $label.Text = 'Failed'
        $bar.Value = 0
        Show-Log
        $box.AppendText("`r`nERROR: $($_.Exception.Message)`r`n")
        $btnLog.Visible = $true
        $btnClose.Visible = $true
    }
})

[void]$form.ShowDialog()
