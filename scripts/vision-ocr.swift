// -----------------------------------------------------------------------------
// [INPUT]: a global-point rectangle and, optionally, the target app PID;
//          unrestricted display capture requires SHARDLANE_ALLOW_GLOBAL_CAPTURE=1.
// [OUTPUT]: Vision-recognized lines sorted in visual order as
//           `text<TAB>x,y,w,h` (global points), suitable for semantic menu clicks.
// [POS]: read-only OCR evidence helper for native acceptance fallback; the
//        Computer Use driver should prefer AX text and does not invoke this tool.
// [PROTOCOL]: Update this header on change, then check CLAUDE.md.
// -----------------------------------------------------------------------------

import CoreGraphics
import Foundation
import Vision
import AppKit

struct Region {
    let x: Double
    let y: Double
    let width: Double
    let height: Double
}

func usage() -> Never {
    FileHandle.standardError.write(
        Data("usage: vocr [--pid <pid>] <x> <y> <w> <h>\n".utf8)
    )
    exit(2)
}

func fail(_ message: String, code: Int32 = 1) -> Never {
    FileHandle.standardError.write(Data("vocr: \(message)\n".utf8))
    exit(code)
}

struct WindowInfo {
    let id: CGWindowID
    let bounds: CGRect
}

func windowInfo(for pid: Int) -> WindowInfo? {
    guard let entries = CGWindowListCopyWindowInfo(
        [.optionOnScreenOnly], kCGNullWindowID
    ) as? [[String: Any]] else {
        return nil
    }

    var best: (area: Double, info: WindowInfo)?
    for entry in entries {
        guard let owner = entry[kCGWindowOwnerPID as String] as? Int,
              owner == pid,
              let layer = entry[kCGWindowLayer as String] as? Int,
              layer == 0,
              let number = entry[kCGWindowNumber as String] as? NSNumber,
              let bounds = entry[kCGWindowBounds as String] as? [String: NSNumber],
              let width = bounds["Width"]?.doubleValue,
              let height = bounds["Height"]?.doubleValue else {
            continue
        }
        let candidate = (
            area: width * height,
            info: WindowInfo(
                id: CGWindowID(number.uint32Value),
                bounds: CGRect(
                    x: bounds["X"]?.doubleValue ?? 0,
                    y: bounds["Y"]?.doubleValue ?? 0,
                    width: width,
                    height: height
                )
            )
        )
        if best == nil || candidate.area > best!.area {
            best = candidate
        }
    }
    return best?.info
}

let rawArgs = Array(CommandLine.arguments.dropFirst())
var args = rawArgs[...]
var targetPID: Int?
if args.first == "--pid" {
    guard args.count >= 2, let pid = Int(args[args.index(after: args.startIndex)]) else {
        usage()
    }
    targetPID = pid
    args = args.dropFirst(2)
}
guard args.count == 4,
      let x = Double(args[args.startIndex]),
      let y = Double(args[args.index(args.startIndex, offsetBy: 1)]),
      let width = Double(args[args.index(args.startIndex, offsetBy: 2)]),
      let height = Double(args[args.index(args.startIndex, offsetBy: 3)]),
      width > 0,
      height > 0 else {
    usage()
}
let region = Region(x: x, y: y, width: width, height: height)

func captureImage(region: Region, targetPID: Int?) -> CGImage {
    let shot = "/tmp/vocr-\(ProcessInfo.processInfo.processIdentifier).png"
    defer { try? FileManager.default.removeItem(atPath: shot) }
    let task = Process()
    task.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
    if let pid = targetPID {
        guard let info = windowInfo(for: pid) else {
            fail("no on-screen window for pid \(pid)", code: 5)
        }
        // Capture only the selected window, then crop the requested global-point
        // rectangle. This avoids reading an occluding user window and works on
        // macOS 15 without the obsoleted CGWindowListCreateImage API.
        task.arguments = ["-x", "-l", "\(info.id)", shot]
        do {
            try task.run()
        } catch {
            fail("screencapture failed: \(error)")
        }
        task.waitUntilExit()
        guard task.terminationStatus == 0,
              let nsImage = NSImage(contentsOfFile: shot),
              let full = nsImage.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
            fail("cannot read captured window")
        }
        let scaleX = Double(full.width) / info.bounds.width
        let scaleY = Double(full.height) / info.bounds.height
        let crop = CGRect(
            x: max(0, (region.x - info.bounds.minX) * scaleX),
            y: max(0, (region.y - info.bounds.minY) * scaleY),
            width: region.width * scaleX,
            height: region.height * scaleY
        ).intersection(CGRect(x: 0, y: 0, width: full.width, height: full.height))
        guard crop.width > 0, crop.height > 0,
              let cropped = full.cropping(to: crop) else {
            fail("requested region is outside window bounds")
        }
        return cropped
    }

    guard ProcessInfo.processInfo.environment["SHARDLANE_ALLOW_GLOBAL_CAPTURE"] == "1" else {
        fail("refusing unrestricted screen capture; pass --pid <app-pid> or set SHARDLANE_ALLOW_GLOBAL_CAPTURE=1", code: 64)
    }
    task.arguments = [
        "-x",
        "-R",
        "\(Int(region.x)),\(Int(region.y)),\(Int(region.width)),\(Int(region.height))",
        shot,
    ]
    do {
        try task.run()
    } catch {
        fail("screencapture failed: \(error)")
    }
    task.waitUntilExit()
    guard task.terminationStatus == 0,
          let nsImage = NSImage(contentsOfFile: shot),
          let captured = nsImage.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
        fail("cannot read captured region")
    }
    return captured
}

let image = captureImage(region: region, targetPID: targetPID)

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = false
let handler = VNImageRequestHandler(cgImage: image, options: [:])
do {
    try handler.perform([request])
} catch {
    fail("Vision request failed: \(error)")
}

let observations = (request.results ?? []).sorted {
    // Vision's normalized origin is bottom-left; visual order is top-to-bottom.
    if abs($0.boundingBox.maxY - $1.boundingBox.maxY) > 0.01 {
        return $0.boundingBox.maxY > $1.boundingBox.maxY
    }
    return $0.boundingBox.minX < $1.boundingBox.minX
}

for observation in observations {
    guard let candidate = observation.topCandidates(1).first else { continue }
    let box = observation.boundingBox
    let globalX = region.x + box.minX * region.width
    let globalY = region.y + (1.0 - box.maxY) * region.height
    let globalWidth = box.width * region.width
    let globalHeight = box.height * region.height
    // Keep output stable and shell-friendly; callers can click the box center.
    print(
        "\(candidate.string)\t\(Int(globalX.rounded())),\(Int(globalY.rounded())),\(Int(globalWidth.rounded())),\(Int(globalHeight.rounded()))"
    )
}
