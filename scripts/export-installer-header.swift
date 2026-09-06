// Export a small brand lockup from the existing application icon. No new logo.
// Run from arcrelay-desktop: swift scripts/export-installer-header.swift
import AppKit

let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
let icon = NSImage(contentsOf: root.appendingPathComponent("icons/icon.png"))!
let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 300, pixelsHigh: 114,
                             bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
                             isPlanar: false, colorSpaceName: .deviceRGB,
                             bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
NSColor.white.setFill()
NSRect(x: 0, y: 0, width: 300, height: 114).fill()
icon.draw(in: NSRect(x: 20, y: 25, width: 64, height: 64))
("ArcRelay" as NSString).draw(at: NSPoint(x: 98, y: 39), withAttributes: [
    .font: NSFont.systemFont(ofSize: 32, weight: .semibold),
    .foregroundColor: NSColor(srgbRed: 6 / 255, green: 21 / 255, blue: 47 / 255, alpha: 1)
])
NSGraphicsContext.restoreGraphicsState()
// NSIS uses ordinary 24-bit Windows bitmaps, without an alpha channel.
let rgb = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 300, pixelsHigh: 114,
                          bitsPerSample: 8, samplesPerPixel: 3, hasAlpha: false,
                          isPlanar: false, colorSpaceName: .deviceRGB,
                          bytesPerRow: 900, bitsPerPixel: 24)!
for y in 0..<114 {
    for x in 0..<300 {
        for channel in 0..<3 {
            rgb.bitmapData![y * rgb.bytesPerRow + x * 3 + channel] =
                bitmap.bitmapData![y * bitmap.bytesPerRow + x * 4 + channel]
        }
    }
}
try rgb.representation(using: .bmp, properties: [:])!.write(
    to: root.appendingPathComponent("installer/assets/header.bmp"))
