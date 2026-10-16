package main

import (
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"
)

// TestWindowsInstaller builds the installer of an app, on Windows with the
// NSIS that mygo build downloads when it is not installed, and elsewhere
// where NSIS is installed. Its sign command lists the files it signs: the
// executable, the uninstaller and the installer. On Windows, it installs
// and uninstalls the app silently: the install makes the Start menu
// shortcut but not the desktop one the finish page offers, and the
// uninstall removes both.
func TestWindowsInstaller(t *testing.T) {
	if testing.Short() {
		t.Skip("compiles programs")
	}
	if tool, err := nsisCompiler(); err != nil {
		t.Fatal(err)
	} else if tool == "" {
		t.Skip("NSIS is not installed")
	}
	dir := testModule(t, map[string]string{
		"main.go":              "package main\n\nfunc main() {}\n",
		"mygo.json":            `{"name": "Setup Test", "identifier": "com.example.setuptest", "version": "2.0.0", "urlSchemes": ["setuptest"], "fileAssociations": [{"ext": ["setuptest"], "name": "Setup Test File"}], "windows": {"signCommand": "echo %1>>signed.txt"}}`,
		"resources/data/a.txt": "a",
		"resources/icon.png":   string(defaultIcon()),
	})
	c, err := loadConfig(dir)
	if err != nil {
		t.Fatal(err)
	}
	opts := buildOptions{sign: "-", work: t.TempDir()}
	if opts.pkg, err = packageDir(c); err != nil {
		t.Fatal(err)
	}
	artifacts, err := buildPlatform(c, "windows", "amd64", opts)
	if err != nil {
		t.Fatal(err)
	}
	setup := artifacts[len(artifacts)-1]
	if filepath.Base(setup) != "Setup Test Setup 2.0.0 amd64.exe" {
		t.Fatalf("artifacts = %q", artifacts)
	}
	// The executable, the uninstaller that makensis made, then the
	// installer.
	listed, _ := os.ReadFile(filepath.Join(dir, "signed.txt"))
	var signed []string
	for _, line := range strings.Split(strings.TrimSpace(string(listed)), "\n") {
		signed = append(signed, filepath.Base(strings.Trim(line, "\r \"")))
	}
	if len(signed) != 3 || signed[0] != "Setup Test.exe" || signed[1] == signed[0] || signed[1] == signed[2] || signed[2] != "Setup Test Setup 2.0.0 amd64.exe" {
		t.Errorf("signed %q", signed)
	}
	if runtime.GOOS != "windows" || runtime.GOARCH != "amd64" {
		return
	}
	// NSIS takes /D= unquoted, and Go quotes arguments with spaces.
	install := filepath.Join(t.TempDir(), "SetupTest")
	if out, err := exec.Command(setup, "/S", "/D="+install).CombinedOutput(); err != nil {
		t.Fatalf("installing: %v\n%s", err, out)
	}
	for _, f := range []string{"Setup Test.exe", "data/a.txt", "Uninstall.exe"} {
		if _, err := os.Stat(filepath.Join(install, filepath.FromSlash(f))); err != nil {
			t.Errorf("not installed: %s", f)
		}
	}
	registered := func(key string) bool {
		return exec.Command("reg", "query", `HKCU\Software\Classes\`+key).Run() == nil
	}
	for _, key := range []string{`.setuptest\OpenWithProgids`, `com.example.setuptest.setuptest\shell\open\command`, `setuptest\shell\open\command`} {
		if !registered(key) {
			t.Errorf("the installer did not register %s", key)
		}
	}
	shortcut := func(folder string) string {
		out, err := exec.Command("powershell", "-NoProfile", "-Command", "[Environment]::GetFolderPath('"+folder+"')").Output()
		if err != nil {
			t.Fatal(err)
		}
		return filepath.Join(strings.TrimSpace(string(out)), "Setup Test.lnk")
	}
	startMenu, desktop := shortcut("Programs"), shortcut("Desktop")
	if !fileExists(startMenu) {
		t.Errorf("the installer made no %s", startMenu)
	}
	if fileExists(desktop) {
		t.Errorf("the silent install made %s", desktop)
	}
	// Stands for the shortcut that the finish page makes.
	if err := os.WriteFile(desktop, nil, 0o644); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.Remove(desktop) })
	defer func() {
		for _, key := range []string{`com.example.setuptest.setuptest`, `setuptest`} {
			if registered(key) {
				t.Errorf("the uninstaller left %s", key)
			}
		}
		for _, f := range []string{startMenu, desktop} {
			if fileExists(f) {
				t.Errorf("the uninstaller left %s", f)
			}
		}
	}()
	uninstall(t, install)
}

// uninstall runs the uninstaller of the app installed in dir silently, and
// waits until it is done.
func uninstall(t *testing.T, dir string) {
	t.Helper()
	if out, err := exec.Command(filepath.Join(dir, "Uninstall.exe"), "/S").CombinedOutput(); err != nil {
		t.Fatalf("uninstalling: %v\n%s", err, out)
	}
	// The uninstaller copies itself away and runs from there, and removes
	// the app's folder last.
	deadline := time.Now().Add(30 * time.Second)
	for {
		if _, err := os.Stat(dir); os.IsNotExist(err) {
			return
		}
		if time.Now().After(deadline) {
			t.Fatal("the uninstaller left the app installed")
		}
		time.Sleep(100 * time.Millisecond)
	}
}

// TestWindowsSigning signs a build with a throwaway self-signed
// certificate. It runs on CI only (MYGO_TEST_SIGN), as it adds the
// certificate to the user's store for a moment.
func TestWindowsSigning(t *testing.T) {
	if runtime.GOOS != "windows" || os.Getenv("MYGO_TEST_SIGN") == "" {
		t.Skip("set MYGO_TEST_SIGN on a disposable Windows machine")
	}
	pfx := filepath.Join(t.TempDir(), "test.pfx")
	ps := func(script string) string {
		t.Helper()
		out, err := exec.Command("powershell", "-NoProfile", "-Command", script).CombinedOutput()
		if err != nil {
			t.Fatalf("powershell: %v\n%s", err, out)
		}
		return strings.TrimSpace(string(out))
	}
	ps(`$c = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=MyGo Test" -CertStoreLocation Cert:\CurrentUser\My; ` +
		`$p = ConvertTo-SecureString -String "secret" -Force -AsPlainText; ` +
		`Export-PfxCertificate -Cert $c -FilePath "` + pfx + `" -Password $p | Out-Null; Remove-Item $c.PSPath`)
	t.Setenv("MYGO_WINDOWS_CERTIFICATE_PASSWORD", "secret")
	dir := testModule(t, map[string]string{
		"main.go":   "package main\n\nfunc main() {}\n",
		"mygo.json": `{"name": "Signed App", "version": "1.0.0", "windows": {"certificate": "` + filepath.ToSlash(pfx) + `"}}`,
	})
	c, err := loadConfig(dir)
	if err != nil {
		t.Fatal(err)
	}
	opts := buildOptions{sign: "-", work: t.TempDir()}
	if opts.pkg, err = packageDir(c); err != nil {
		t.Fatal(err)
	}
	artifacts, err := buildPlatform(c, "windows", "amd64", opts)
	if err != nil {
		t.Fatal(err)
	}
	signer := func(file string) string {
		return ps(`(Get-AuthenticodeSignature "` + file + `").SignerCertificate.Subject`)
	}
	for _, a := range artifacts {
		if strings.HasSuffix(a, ".exe") {
			if subject := signer(a); subject != "CN=MyGo Test" {
				t.Errorf("%s is signed by %q", filepath.Base(a), subject)
			}
		}
	}
	// The installer writes an uninstaller signed with the app.
	install := filepath.Join(t.TempDir(), "SignedApp")
	if out, err := exec.Command(artifacts[len(artifacts)-1], "/S", "/D="+install).CombinedOutput(); err != nil {
		t.Fatalf("installing: %v\n%s", err, out)
	}
	if subject := signer(filepath.Join(install, "Uninstall.exe")); subject != "CN=MyGo Test" {
		t.Errorf("Uninstall.exe is signed by %q", subject)
	}
	uninstall(t, install)
}
