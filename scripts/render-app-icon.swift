#!/usr/bin/env swift
// [INPUT]: 依赖 AppKit 的原生 SVG 光栅化（macOS 11+）；输入为品牌 SVG 与输出目录
// [OUTPUT]: 按 Apple macOS 图标网格（图形占 824/1024，居中留边，真透明）生成 10 级 app-icon 阶梯 PNG
// [POS]: assets/app-icon 阶梯的唯一再生入口；包装侧 package-macos.sh 用 iconutil 从该阶梯重建 icns
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

import AppKit

let args = CommandLine.arguments
guard args.count >= 3 else {
    FileHandle.standardError.write(Data("usage: render-app-icon.swift <svg> <out-dir>\n".utf8))
    exit(2)
}
let svgPath = args[1]
let outDir = args[2]

guard let svg = NSImage(contentsOfFile: svgPath) else {
    FileHandle.standardError.write(Data("error: cannot load SVG \(svgPath)\n".utf8))
    exit(1)
}

func rasterize(size: Int) -> NSBitmapImageRep {
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    rep.size = NSSize(width: size, height: size)
    NSGraphicsContext.saveGraphicsState()
    let ctx = NSGraphicsContext(bitmapImageRep: rep)!
    NSGraphicsContext.current = ctx
    ctx.imageInterpolation = .high
    let inner = CGFloat(size) * 824.0 / 1024.0
    let offset = (CGFloat(size) - inner) / 2.0
    svg.draw(in: NSRect(x: offset, y: offset, width: inner, height: inner),
             from: .zero, operation: .sourceOver, fraction: 1.0)
    NSGraphicsContext.restoreGraphicsState()
    return rep
}

func write(_ rep: NSBitmapImageRep, name: String) {
    guard let png = rep.representation(using: .png, properties: [:]) else {
        FileHandle.standardError.write(Data("error: png encode failed for \(name)\n".utf8))
        exit(1)
    }
    try! FileManager.default.createDirectory(atPath: outDir, withIntermediateDirectories: true)
    try! png.write(to: URL(fileURLWithPath: outDir + "/" + name))
}

// (画布尺寸, 阶梯文件名)；@2x 与同尺寸 1x 共享同一光栅
let ladder: [(Int, String)] = [
    (16, "shardlane-16.png"),
    (32, "shardlane-16@2x.png"),
    (32, "shardlane-32.png"),
    (64, "shardlane-32@2x.png"),
    (128, "shardlane-128.png"),
    (256, "shardlane-128@2x.png"),
    (256, "shardlane-256.png"),
    (512, "shardlane-256@2x.png"),
    (512, "shardlane-512.png"),
    (1024, "shardlane-512@2x.png"),
]

var cache: [Int: NSBitmapImageRep] = [:]
for (size, name) in ladder {
    let rep = cache[size] ?? rasterize(size: size)
    cache[size] = rep
    write(rep, name: name)
}
print("rendered \(ladder.count) ladder files into \(outDir)")
