// -----------------------------------------------------------------------------
// [INPUT]: key/click/rclick/scroll/bounds arguments; native event posting additionally
//          requires SHARDLANE_UI_DRIVER=native and SHARDLANE_ALLOW_GLOBAL_INPUT=1.
// [OUTPUT]: deterministic CGEvent probes for explicitly-authorized real-device
//           measurements plus read-only window/input-source inspection.
// [POS]: native hardware probe used by the pacing/scroll acceptance harnesses;
//        Computer Use is the preferred app-scoped driver and never calls this.
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
// -----------------------------------------------------------------------------
// Key-repeat pacing probe event tool.
// Posts hardware-faithful CGEvents (autorepeat-flagged key downs, mouse clicks)
// and queries window geometry, so the pacing A/B harness can run unattended.
//
// Usage:
//   evpost key <keycode> <count> <interval_us> [input_source_id]
//   evpost click <x_pt> <y_pt>
//   evpost rclick <x_pt> <y_pt>
//   evpost bounds <pid>            -> prints "x y w h" (points) of the main window
//   evpost sources                 -> lists input sources

import CoreGraphics
import Carbon.HIToolbox
import Foundation
import AppKit

struct StdErrStream: TextOutputStream {
    mutating func write(_ string: String) { FileHandle.standardError.write(Data(string.utf8)) }
}
var stdErr = StdErrStream()

/// CGEvent posting is a global desktop side effect. Keep the guard in the
/// binary as well as in the shell harness so a direct invocation is safe.
func requireGlobalInput(_ operation: String) -> Bool {
    let env = ProcessInfo.processInfo.environment
    guard env["SHARDLANE_UI_DRIVER"] == "native",
          env["SHARDLANE_ALLOW_GLOBAL_INPUT"] == "1" else {
        print("evpost: refusing global \(operation); set SHARDLANE_UI_DRIVER=native SHARDLANE_ALLOW_GLOBAL_INPUT=1 for an explicit real-device probe", to: &stdErr)
        return false
    }
    return true
}

func forEachInputSource(_ body: (TISInputSource) -> Void) {
    guard let sources = TISCreateInputSourceList(nil, false)?.takeRetainedValue() as? [TISInputSource] else { return }
    for src in sources { body(src) }
}

func sourceString(_ src: TISInputSource, _ key: CFString) -> String {
    guard let ptr = TISGetInputSourceProperty(src, key) else { return "?" }
    return Unmanaged<CFString>.fromOpaque(ptr).takeUnretainedValue() as String
}

func selectSource(id wanted: String) -> Bool {
    var found = false
    forEachInputSource { src in
        guard !found, sourceString(src, kTISPropertyInputSourceID) == wanted else { return }
        found = TISSelectInputSource(src) == noErr
    }
    return found
}

func postKeys(_ args: [String]) -> Int32 {
    guard requireGlobalInput("keyboard events") else { return 64 }
    guard args.count >= 3,
          let keycode = Int(args[0]),
          let count = Int(args[1]),
          let intervalUs = Int(args[2]),
          count >= 0,
          intervalUs >= 0 else {
            print("usage: key <keycode> <count> <interval_us> [input_source_id]", to: &stdErr)
            return 2
    }
    if args.count >= 4 {
        guard selectSource(id: args[3]) else {
            print("failed to select input source \(args[3])", to: &stdErr)
            return 3
        }
        usleep(300_000)
    }
    let src = CGEventSource(stateID: .combinedSessionState)
    let start = DispatchTime.now().uptimeNanoseconds
    for i in 0..<count {
        if let down = CGEvent(keyboardEventSource: src, virtualKey: CGKeyCode(keycode), keyDown: true) {
            down.setIntegerValueField(.keyboardEventAutorepeat, value: i == 0 ? 0 : 1)
            down.post(tap: .cghidEventTap)
        }
        usleep(useconds_t(intervalUs))
    }
    let elapsedMs = Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000
    print("posted \(count) keydowns keycode=\(keycode) in \(elapsedMs)ms")
    return 0
}

func postClick(_ args: [String]) -> Int32 {
    postClick(args, button: .left, downType: .leftMouseDown, upType: .leftMouseUp, label: "click")
}

func postClick(_ args: [String], button: CGMouseButton, downType: CGEventType, upType: CGEventType, label: String) -> Int32 {
    guard requireGlobalInput("\(label) mouse events") else { return 64 }
    guard args.count >= 2,
          let x = Double(args[0]), let y = Double(args[1]),
          x.isFinite, y.isFinite else {
        print("usage: \(label) <x_pt> <y_pt>", to: &stdErr)
        return 2
    }
    let src = CGEventSource(stateID: .combinedSessionState)
    let pt = CGPoint(x: x, y: y)
    if let move = CGEvent(mouseEventSource: src, mouseType: .mouseMoved, mouseCursorPosition: pt, mouseButton: button) {
        move.post(tap: .cghidEventTap)
    }
    usleep(80_000)
    if let down = CGEvent(mouseEventSource: src, mouseType: downType, mouseCursorPosition: pt, mouseButton: button) {
        down.post(tap: .cghidEventTap)
    }
    usleep(40_000)
    if let up = CGEvent(mouseEventSource: src, mouseType: upType, mouseCursorPosition: pt, mouseButton: button) {
        up.post(tap: .cghidEventTap)
    }
    print("\(label) clicked \(x),\(y)")
    return 0
}

func postScroll(_ args: [String]) -> Int32 {
    guard requireGlobalInput("scroll events") else { return 64 }
    // scroll <x> <y> <amount> <count> <interval_us> [line|pixel]
    // Posts a deterministic scroll-wheel burst at the given point (the pointer is
    // moved there first so the event lands on the hovered window). Positive
    // amount = scroll up (into terminal history); negative = scroll down.
    guard args.count >= 5,
          let x = Double(args[0]), let y = Double(args[1]),
          let amount = Double(args[2]), let count = Int(args[3]),
          let intervalUs = Int(args[4]),
          x.isFinite, y.isFinite, amount.isFinite,
          count >= 0,
          intervalUs >= 0 else {
        print("usage: scroll <x> <y> <amount> <count> <interval_us> [line|pixel]", to: &stdErr)
        return 2
    }
    let pixel = args.count >= 6 && args[5] == "pixel"
    let src = CGEventSource(stateID: .combinedSessionState)
    let pt = CGPoint(x: x, y: y)
    if let move = CGEvent(mouseEventSource: src, mouseType: .mouseMoved, mouseCursorPosition: pt, mouseButton: .left) {
        move.post(tap: .cghidEventTap)
    }
    usleep(80_000)
    let start = DispatchTime.now().uptimeNanoseconds
    for _ in 0..<count {
        if let ev = CGEvent(scrollWheelEvent2Source: src, units: pixel ? .pixel : .line,
                            wheelCount: 1, wheel1: pixel ? 0 : Int32(amount), wheel2: 0, wheel3: 0) {
            if pixel {
                // Faithful trackpad emulation: continuous (precision) gesture with a
                // double point delta, not discrete wheel clicks.
                ev.setIntegerValueField(.scrollWheelEventIsContinuous, value: 1)
                ev.setDoubleValueField(.scrollWheelEventPointDeltaAxis1, value: amount)
            }
            ev.post(tap: .cghidEventTap)
        }
        usleep(useconds_t(intervalUs))
    }
    let elapsedMs = Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000
    print("posted \(count) scroll events amount=\(amount) \(pixel ? "pixel" : "line") in \(elapsedMs)ms")
    return 0
}

func windowBounds(_ args: [String]) -> Int32 {
    guard args.count >= 1, let pid = Int(args[0]) else {
        print("usage: bounds <pid>", to: &stdErr)
        return 2
    }
    guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] else {
        return 4
    }
    var best: (Int, CGRect) = (0, .zero)
    for entry in list {
        let info = NSDictionary(dictionary: entry)
        guard let owner = info[kCGWindowOwnerPID as String] as? Int, owner == pid else { continue }
        guard let layer = info[kCGWindowLayer as String] as? Int, layer == 0 else { continue }
        guard let b = info[kCGWindowBounds as String] as? [String: NSNumber] else { continue }
        let rect = CGRect(x: b["X"]!.doubleValue, y: b["Y"]!.doubleValue,
                          width: b["Width"]!.doubleValue, height: b["Height"]!.doubleValue)
        let area = Int(rect.width * rect.height)
        if area > best.0 { best = (area, rect) }
    }
    guard best.0 > 0 else {
        print("no on-screen window for pid \(pid)", to: &stdErr)
        return 5
    }
    let r = best.1
    print("\(Int(r.minX)) \(Int(r.minY)) \(Int(r.width)) \(Int(r.height))")
    return 0
}

func listSources() {
    forEachInputSource { src in
        let id = sourceString(src, kTISPropertyInputSourceID)
        let type = sourceString(src, kTISPropertyInputSourceType)
        let selPtr = TISGetInputSourceProperty(src, kTISPropertyInputSourceIsSelected)
        let selected = selPtr.map { Unmanaged<CFBoolean>.fromOpaque($0).takeUnretainedValue() == kCFBooleanTrue } ?? false
        print("source id=\(id) type=\(type) selected=\(selected)")
    }
}

let args = CommandLine.arguments
guard args.count >= 2 else {
    print("usage: evpost <key|click|scroll|bounds|sources|src> ...", to: &stdErr)
    exit(2)
}
switch args[1] {
case "key": exit(postKeys(Array(args.dropFirst(2))))
case "click": exit(postClick(Array(args.dropFirst(2))))
case "rclick": exit(postClick(Array(args.dropFirst(2)), button: .right, downType: .rightMouseDown, upType: .rightMouseUp, label: "rclick"))
case "scroll": exit(postScroll(Array(args.dropFirst(2))))
case "bounds": exit(windowBounds(Array(args.dropFirst(2))))
case "sources": listSources(); exit(0)
case "src":
    // `src get` prints the selected input source id; `src set <id>` selects without typing.
    if args.count >= 3, args[2] == "set", args.count >= 4 {
        guard requireGlobalInput("input-source changes") else { exit(64) }
        exit(selectSource(id: args[3]) ? 0 : 3)
    }
    var current = "?"
    forEachInputSource { src in
        let selPtr = TISGetInputSourceProperty(src, kTISPropertyInputSourceIsSelected)
        let selected = selPtr.map { Unmanaged<CFBoolean>.fromOpaque($0).takeUnretainedValue() == kCFBooleanTrue } ?? false
        if selected { current = sourceString(src, kTISPropertyInputSourceID) }
    }
    print(current)
    exit(0)
default:
    print("unknown command \(args[1])", to: &stdErr)
    exit(2)
}
