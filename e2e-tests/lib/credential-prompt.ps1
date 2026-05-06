# credential-prompt.ps1
# Shows a native Windows Forms dialog to collect email + password.
# Outputs JSON to stdout: {"email":"...","password":"..."}
# Returns exit code 0 on OK, 1 on Cancel/Error.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File credential-prompt.ps1 [-Title "E2E Test - Account 1"] [-EmailDefault "user@example.com"]

param(
    [string]$Title = 'E2E Test - Enter Credentials',
    [string]$EmailDefault = ''
)

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$form = New-Object System.Windows.Forms.Form
$form.Text = $Title
$form.Size = New-Object System.Drawing.Size(420, 260)
$form.StartPosition = 'CenterScreen'
$form.FormBorderStyle = 'FixedDialog'
$form.MaximizeBox = $false
$form.MinimizeBox = $false
$form.TopMost = $true

# ── Email ───────────────────────────────────────────────────
$emailLabel = New-Object System.Windows.Forms.Label
$emailLabel.Text = 'Email:'
$emailLabel.Location = New-Object System.Drawing.Point(20, 25)
$emailLabel.Size = New-Object System.Drawing.Size(80, 22)
$form.Controls.Add($emailLabel)

$emailBox = New-Object System.Windows.Forms.TextBox
$emailBox.Location = New-Object System.Drawing.Point(110, 22)
$emailBox.Size = New-Object System.Drawing.Size(260, 22)
$emailBox.Text = $EmailDefault
$form.Controls.Add($emailBox)

# ── Password ────────────────────────────────────────────────
$passLabel = New-Object System.Windows.Forms.Label
$passLabel.Text = 'Password:'
$passLabel.Location = New-Object System.Drawing.Point(20, 65)
$passLabel.Size = New-Object System.Drawing.Size(80, 22)
$form.Controls.Add($passLabel)

$passBox = New-Object System.Windows.Forms.TextBox
$passBox.Location = New-Object System.Drawing.Point(110, 62)
$passBox.Size = New-Object System.Drawing.Size(260, 22)
$passBox.UseSystemPasswordChar = $true
$form.Controls.Add($passBox)

# ── Buttons ─────────────────────────────────────────────────
$okBtn = New-Object System.Windows.Forms.Button
$okBtn.Text = 'OK'
$okBtn.Location = New-Object System.Drawing.Point(190, 170)
$okBtn.Size = New-Object System.Drawing.Size(90, 30)
$okBtn.DialogResult = [System.Windows.Forms.DialogResult]::OK
$form.Controls.Add($okBtn)
$form.AcceptButton = $okBtn

$cancelBtn = New-Object System.Windows.Forms.Button
$cancelBtn.Text = 'Cancel'
$cancelBtn.Location = New-Object System.Drawing.Point(290, 170)
$cancelBtn.Size = New-Object System.Drawing.Size(90, 30)
$cancelBtn.DialogResult = [System.Windows.Forms.DialogResult]::Cancel
$form.Controls.Add($cancelBtn)
$form.CancelButton = $cancelBtn

# ── Hint ────────────────────────────────────────────────────
$hintLabel = New-Object System.Windows.Forms.Label
$hintLabel.Text = 'Credentials are used in-memory only. Nothing is saved to disk.'
$hintLabel.Location = New-Object System.Drawing.Point(20, 120)
$hintLabel.Size = New-Object System.Drawing.Size(370, 40)
$hintLabel.ForeColor = [System.Drawing.Color]::Gray
$form.Controls.Add($hintLabel)

# ── Show dialog ─────────────────────────────────────────────
$result = $form.ShowDialog()

if ($result -eq [System.Windows.Forms.DialogResult]::OK) {
    $email = $emailBox.Text
    $password = $passBox.Text
    # Output JSON to stdout (Node.js reads this)
    $json = @{ email = $email; password = $password } | ConvertTo-Json -Compress
    Write-Output $json
    $form.Close()
    exit 0
} else {
    $form.Close()
    exit 1
}
