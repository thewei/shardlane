package main

import (
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

// finderHasCustomIcon is the Finder flag of folders with a custom icon.
const finderHasCustomIcon = 0x0400

// dmgFileName is "<Name> <Version> <arch>.dmg", as "My App 1.2.0
// arm64.dmg", or "<Name> <Version>.dmg" for darwin/universal, which is for
// every Mac: each architecture's disk image is named apart, so that the
// builds of two, uploaded to one release or bucket, keep both.
func dmgFileName(c *Config, goarch string) string {
	if goarch == "universal" {
		return fsName(c.Name) + " " + fsName(c.Version) + ".dmg"
	}
	return fsName(c.Name) + " " + fsName(c.Version) + " " + goarch + ".dmg"
}

// buildDMG packages app, built for goarch, into a compressed, read-only
// disk image in dir and returns its path. Opening the image shows the app
// next to a link to /Applications; the Finder layout comes from a generated
// .DS_Store (see dsstore.go). Only hdiutil, which ships with macOS, is
// needed.
func buildDMG(c *Config, app, dir, goarch string, opts buildOptions) (_ string, err error) {
	file := dmgFileName(c, goarch)
	t := con.start("Creating " + file)
	defer func() { t.end(err, "Created "+file) }()
	work, err := os.MkdirTemp(dir, ".dmg-")
	if err != nil {
		return "", err
	}
	defer os.RemoveAll(work)

	// The image starts as a copy of a folder holding all it shows, which
	// hdiutil sizes: on HFS+ every file takes whole blocks, more than the
	// sizes add up to. ditto keeps the bundle exactly as signed, and copies
	// it without cloning: hdiutil counts the blocks a file takes, fewer
	// than its size in a sparse file, which HFS+ cannot keep sparse.
	src := filepath.Join(work, "src")
	appName := filepath.Base(app)
	if err := os.Mkdir(src, 0o755); err != nil {
		return "", err
	}
	if err := command("ditto", "--noclone", app, filepath.Join(src, appName)); err != nil {
		return "", err
	}
	if err := os.Symlink("/Applications", filepath.Join(src, "Applications")); err != nil {
		return "", err
	}
	// Finder hides the .app extension of bundles by default; hiding it with
	// the Finder flag, like create-dmg does, would make the signature fail
	// strict verification.
	if err := writeDSStore(filepath.Join(src, ".DS_Store"), appName); err != nil {
		return "", err
	}
	icon := filepath.Join(app, "Contents", "Resources", bundleIcon)
	hasIcon := fileExists(icon)
	if hasIcon {
		if err := copyFile(icon, filepath.Join(src, ".VolumeIcon.icns"), 0o644); err != nil {
			return "", err
		}
	}
	rw := filepath.Join(work, "rw.dmg")
	if err := hdiutilRetrying(hdiutilCreateArgs(c.MacOS.DMGTitle, src, rw)...); err != nil {
		return "", err
	}

	mnt := filepath.Join(work, "mnt")
	if err := os.Mkdir(mnt, 0o755); err != nil {
		return "", err
	}
	if err := command("hdiutil", "attach", "-readwrite", "-noverify", "-noautoopen", "-nobrowse", "-mountpoint", mnt, rw); err != nil {
		return "", err
	}
	attached := true
	defer func() {
		if attached {
			_ = exec.Command("hdiutil", "detach", "-force", mnt).Run()
		}
	}()
	if hasIcon {
		if err := setFinderFlags(mnt, finderHasCustomIcon); err != nil {
			return "", err
		}
	}
	// Created by macOS while the image is mounted.
	_ = os.RemoveAll(filepath.Join(mnt, ".fseventsd"))
	_ = os.RemoveAll(filepath.Join(mnt, ".Trashes"))
	if err := detach(mnt); err != nil {
		return "", err
	}
	attached = false

	// LZMA gives the smallest images; it needs macOS 10.15, below the
	// minimum MyGo supports.
	dmg := filepath.Join(dir, file)
	if err := command("hdiutil", "convert", rw, "-format", "ULMO", "-ov", "-o", dmg); err != nil {
		return "", err
	}
	if opts.sign != "-" {
		if err := command("codesign", "--force", "--timestamp", "--sign", opts.sign, dmg); err != nil {
			return "", err
		}
	}
	t.done("Created " + file)
	if c.MacOS.Notarize != nil && !opts.skipNotarize {
		if err := notarize(c.MacOS.Notarize, dmg); err != nil {
			return "", err
		}
		// The ticket covers the app too: staple it for updates, which
		// install the bare app.
		if err := command("xcrun", "stapler", "staple", app); err != nil {
			return "", err
		}
	}
	return dmg, nil
}

// hdiutilCreateArgs creates a writable HFS+ image of src, as large as its
// contents need. hdiutil runs without -quiet, which would leave out of the
// error why it failed.
func hdiutilCreateArgs(volume, src, out string) []string {
	return []string{
		"create", "-ov",
		"-volname", volume,
		"-srcfolder", src,
		"-fs", "HFS+", "-fsargs", "-c c=64,a=16,e=16",
		"-format", "UDRW",
		out,
	}
}

// hdiutilRetrying runs hdiutil, again while it fails with "Resource busy",
// which "hdiutil create" reports with exit status 1 when a service such as
// Spotlight or an antivirus holds an image or a file it reads: it waits 2,
// 4, 8 then 16 seconds before trying again, as create-dmg does.
func hdiutilRetrying(args ...string) error {
	var err error
	for i := range 5 {
		if err = command("hdiutil", args...); err == nil || !hdiutilBusy(err) {
			return err
		}
		if i < 4 {
			time.Sleep(time.Duration(2<<i) * time.Second)
		}
	}
	return err
}

// hdiutilBusy reports whether hdiutil failed because a resource was busy.
func hdiutilBusy(err error) bool {
	return strings.Contains(err.Error(), "Resource busy")
}

// setFinderFlags sets the Finder flags of a file or folder, which are
// stored at offset 8 of its com.apple.FinderInfo attribute.
func setFinderFlags(path string, flags uint16) error {
	var info [32]byte
	info[8], info[9] = byte(flags>>8), byte(flags)
	return command("/usr/bin/xattr", "-wx", "com.apple.FinderInfo", hex.EncodeToString(info[:]), path)
}

// detach unmounts a disk image, retrying while macOS services such as
// Spotlight still hold the volume, which on busy machines like CI runners
// can outlast a forced detach: after a few polite attempts it forces, for
// up to half a minute in all.
func detach(mnt string) error {
	var err error
	for i := range 10 {
		args := []string{"detach", mnt}
		if i >= 4 {
			args = []string{"detach", "-force", mnt}
		}
		if err = command("hdiutil", args...); err == nil {
			return nil
		}
		time.Sleep(time.Duration(i+1) * 500 * time.Millisecond)
	}
	return err
}

// notarize submits a disk image to Apple's notary service, waits for the
// verdict and staples the ticket to the image.
func notarize(n *Notarize, dmg string) (err error) {
	t := con.start("Notarizing " + filepath.Base(dmg))
	t.set("usually takes a few minutes")
	defer func() { t.end(err, "Notarized "+filepath.Base(dmg)) }()
	args := []string{"notarytool", "submit", dmg, "--keychain-profile", n.KeychainProfile, "--wait", "--output-format", "json"}
	if n.Keychain != "" {
		args = append(args, "--keychain", n.Keychain)
	}
	cmd := exec.Command("xcrun", args...)
	errOut := con.output("notarytool", os.Stderr)
	cmd.Stderr = errOut
	out, err := cmd.Output()
	errOut.flush()
	var result struct {
		ID      string `json:"id"`
		Status  string `json:"status"`
		Message string `json:"message"`
	}
	if i := strings.LastIndexByte(string(out), '{'); i >= 0 {
		_ = json.Unmarshal(out[i:], &result)
	}
	if err != nil || result.Status != "Accepted" {
		msg := fmt.Sprintf("notarization failed (%s", strings.TrimSpace(result.Status+" "+result.Message))
		if err != nil {
			msg += fmt.Sprintf(": %v", err)
		}
		msg += ")"
		if result.ID != "" {
			msg += fmt.Sprintf("; see xcrun notarytool log %s --keychain-profile %q", result.ID, n.KeychainProfile)
		}
		return fmt.Errorf("%s\n%s", msg, strings.TrimSpace(string(out)))
	}
	return command("xcrun", "stapler", "staple", dmg)
}

func fileExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}
