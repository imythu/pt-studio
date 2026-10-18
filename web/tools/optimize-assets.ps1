# Optimize Frieren assets for web and generate PWA/Touch icons
Add-Type -AssemblyName System.Drawing

$root = (Get-Item $PSScriptRoot).Parent.FullName
$frierenDir = Join-Path $root "assets\frieren"
$assetsDir = Join-Path $root "assets"

function Resize-ImageFile {
    param(
        [string]$SrcPath,
        [string]$DstPath,
        [int]$MaxDim,
        [long]$Quality = 88L
    )

    $fullSrc = (Resolve-Path $SrcPath).Path
    $bytes = [System.IO.File]::ReadAllBytes($fullSrc)
    $ms = New-Object System.IO.MemoryStream($bytes, 0, $bytes.Length)
    $img = [System.Drawing.Image]::FromStream($ms)

    $w = $img.Width
    $h = $img.Height
    $scale = 1.0
    if ($w -gt $MaxDim -or $h -gt $MaxDim) {
        if ($w -ge $h) { $scale = [double]$MaxDim / [double]$w }
        else { $scale = [double]$MaxDim / [double]$h }
    }
    $nw = [Math]::Max(1, [int][Math]::Round($w * $scale))
    $nh = [Math]::Max(1, [int][Math]::Round($h * $scale))

    $bmp = New-Object System.Drawing.Bitmap($nw, $nh)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $g.DrawImage($img, 0, 0, $nw, $nh)
    $g.Dispose()
    $img.Dispose()
    $ms.Dispose()

    if ($DstPath.EndsWith('.png')) {
        $bmp.Save($DstPath, [System.Drawing.Imaging.ImageFormat]::Png)
    } else {
        $codec = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() | Where-Object { $_.MimeType -eq 'image/jpeg' } | Select-Object -First 1
        $ep = New-Object System.Drawing.Imaging.EncoderParameters(1)
        $ep.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter([System.Drawing.Imaging.Encoder]::Quality, $Quality)
        $bmp.Save($DstPath, $codec, $ep)
        $ep.Dispose()
    }
    $bmp.Dispose()
    $dstSize = (Get-Item $DstPath).Length
    Write-Output "Generated $([System.IO.Path]::GetFileName($DstPath)): ${nw}x${nh} ($([Math]::Round($dstSize/1KB, 1)) KB)"
}

# 1. Generate PWA and Touch Icons from frieren-icon-generated.jpg
$iconSrc = Join-Path $frierenDir "frieren-icon-generated.jpg"
Resize-ImageFile -SrcPath $iconSrc -DstPath (Join-Path $assetsDir "icon-512.png") -MaxDim 512
Resize-ImageFile -SrcPath $iconSrc -DstPath (Join-Path $assetsDir "icon-192.png") -MaxDim 192
Resize-ImageFile -SrcPath $iconSrc -DstPath (Join-Path $assetsDir "apple-touch-icon.png") -MaxDim 180

# 2. Optimize oversized 4K wallpapers down to 2K (max 2560px), reducing bundle size from ~48MB to ~6MB
$wallpapers = @(
    "frieren-magic-journey.png",
    "frieren-sunset-horizon.jpg",
    "frieren-sky-landscape.jpg",
    "frieren-portrait-bluemoon.jpg",
    "frieren-grimoire.jpg",
    "frieren-portrait-staff.jpg",
    "frieren-hero-field.jpg"
)

foreach ($wp in $wallpapers) {
    $src = Join-Path $frierenDir $wp
    if (Test-Path $src) {
        $origSize = (Get-Item $src).Length
        if ($origSize -gt 1.5MB) {
            $tmp = "$src.tmp.jpg"
            Resize-ImageFile -SrcPath $src -DstPath $tmp -MaxDim 2560 -Quality 88L
            Remove-Item $src -Force
            # Keep original extension or rename
            $dest = if ($src.EndsWith('.png')) { [System.IO.Path]::ChangeExtension($src, '.jpg') } else { $src }
            Move-Item $tmp $dest -Force
            $newSize = (Get-Item $dest).Length
            Write-Output "Optimized $($wp): $([Math]::Round($origSize/1MB, 2)) MB -> $([Math]::Round($newSize/1KB, 1)) KB"
        }
    }
}
