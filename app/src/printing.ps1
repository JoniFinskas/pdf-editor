$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$document = New-Object System.Drawing.Printing.PrintDocument
$dialog = New-Object System.Windows.Forms.PrintDialog
try {
    $pages = @(Get-ChildItem -LiteralPath $env:PDF_EDITOR_PRINT_DIRECTORY -Filter '*.bmp' | Sort-Object Name)
    if ($pages.Count -eq 0) { throw 'No printable pages' }
    $document.DocumentName = 'PDF Editor document'
    $document.PrinterSettings.MinimumPage = 1
    $document.PrinterSettings.MaximumPage = $pages.Count
    $document.PrinterSettings.FromPage = 1
    $document.PrinterSettings.ToPage = $pages.Count
    $dialog.Document = $document
    $dialog.AllowSomePages = $true
    $dialog.UseEXDialog = $true
    if ($dialog.ShowDialog() -ne [System.Windows.Forms.DialogResult]::OK) { exit 2 }
    $script:nextPage = 0
    $script:lastPage = $pages.Count - 1
    if ($document.PrinterSettings.PrintRange -eq [System.Drawing.Printing.PrintRange]::SomePages) {
        $script:nextPage = $document.PrinterSettings.FromPage - 1
        $script:lastPage = $document.PrinterSettings.ToPage - 1
    }
    $document.add_PrintPage({
        param($sender, $eventArgs)
        $bitmap = [System.Drawing.Image]::FromFile($pages[$script:nextPage].FullName)
        try {
            $box = $eventArgs.MarginBounds
            $scale = [Math]::Min($box.Width / $bitmap.Width, $box.Height / $bitmap.Height)
            $width = [single]($bitmap.Width * $scale)
            $height = [single]($bitmap.Height * $scale)
            $x = [single]($box.Left + ($box.Width - $width) / 2)
            $y = [single]($box.Top + ($box.Height - $height) / 2)
            $eventArgs.Graphics.DrawImage($bitmap, $x, $y, $width, $height)
            $script:nextPage++
            $eventArgs.HasMorePages = $script:nextPage -le $script:lastPage
        } finally { $bitmap.Dispose() }
    })
    $document.Print()
} catch {
    # Document data and printer-specific diagnostics are not emitted.
    exit 1
} finally {
    $dialog.Dispose()
    $document.Dispose()
}
