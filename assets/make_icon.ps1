# Builds the Rusty Dock icon from the user's design, logo-1024.png (a white cog holding an
# anchor, on transparency): icon.png (256) and icon.ico (16-256), scaled with System.Drawing.
# Usage: .\assets\make_icon.ps1
param([string]$Out = $PSScriptRoot)
Add-Type -AssemblyName System.Drawing
$ErrorActionPreference = 'Stop'

$design = [Drawing.Bitmap]::FromFile((Join-Path $PSScriptRoot 'logo-1024.png'))

# A #030303 outline, $stroke px wide at 1024, so the white design shows on light backgrounds
# too: the design's silhouette, tinted #030303, drawn at offsets all around (a dilation), then
# the design itself on top. Holes narrower than the outline fill with it.
$stroke = 8
$tint = New-Object Drawing.Imaging.ColorMatrix
$tint.Matrix00 = 0; $tint.Matrix11 = 0; $tint.Matrix22 = 0
$tint.Matrix40 = 3 / 255; $tint.Matrix41 = 3 / 255; $tint.Matrix42 = 3 / 255
$outline = New-Object Drawing.Imaging.ImageAttributes
$outline.SetColorMatrix($tint)
$master = New-Object Drawing.Bitmap 1024, 1024, ([Drawing.Imaging.PixelFormat]::Format32bppArgb)
$g = [Drawing.Graphics]::FromImage($master)
$g.Clear([Drawing.Color]::Transparent)
$area = New-Object Drawing.Rectangle 0, 0, 1024, 1024
foreach ($r in ($stroke / 2), $stroke) {
    for ($i = 0; $i -lt 72; $i++) {
        $a = $i * [Math]::PI / 36
        $dest = New-Object Drawing.Rectangle ([int][Math]::Round($r * [Math]::Cos($a))), ([int][Math]::Round($r * [Math]::Sin($a))), 1024, 1024
        $g.DrawImage($design, $dest, 0, 0, 1024, 1024, [Drawing.GraphicsUnit]::Pixel, $outline)
    }
}
$g.DrawImage($design, $area)
$g.Dispose(); $design.Dispose()
$master.Save((Join-Path $Out 'logo-outlined-1024.png'), [Drawing.Imaging.ImageFormat]::Png)

function Scale([int]$size) {
    $b = New-Object Drawing.Bitmap $size, $size, ([Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [Drawing.Graphics]::FromImage($b)
    $g.InterpolationMode = 'HighQualityBicubic'; $g.PixelOffsetMode = 'HighQuality'; $g.CompositingMode = 'SourceCopy'
    $g.DrawImage($master, 0, 0, $size, $size); $g.Dispose(); $b
}

# ---- PNGs + ICO (PNG-compressed entries, Vista and later) ---------------------------------
$sizes = 16, 24, 32, 48, 64, 128, 256
$pngs = @{}
foreach ($size in $sizes) {
    $b = Scale $size
    $ms = New-Object IO.MemoryStream; $b.Save($ms, [Drawing.Imaging.ImageFormat]::Png); $pngs[$size] = $ms.ToArray()
    if ($size -eq 256) { $b.Save((Join-Path $Out 'icon.png'), [Drawing.Imaging.ImageFormat]::Png) }
    $b.Dispose()
}
$master.Dispose()
$fs = [IO.File]::Create((Join-Path $Out 'icon.ico')); $w = New-Object IO.BinaryWriter $fs
$w.Write([UInt16]0); $w.Write([UInt16]1); $w.Write([UInt16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
foreach ($size in $sizes) {
    $dim = if ($size -ge 256) { 0 } else { $size }
    $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([UInt16]1); $w.Write([UInt16]32); $w.Write([UInt32]$pngs[$size].Length); $w.Write([UInt32]$offset)
    $offset += $pngs[$size].Length
}
foreach ($size in $sizes) { $w.Write($pngs[$size]) }
$w.Dispose()
