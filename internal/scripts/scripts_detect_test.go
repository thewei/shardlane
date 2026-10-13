package scripts

import (
	"os"
	"path/filepath"
	"strconv"
	"testing"
)

func writeTree(t *testing.T, files map[string]string) string {
	t.Helper()
	root := t.TempDir()
	for name, content := range files {
		path := filepath.Join(root, name)
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	return root
}

func findCommand(cmds []DetectedCommand, name string) (DetectedCommand, bool) {
	for _, cmd := range cmds {
		if cmd.Name == name {
			return cmd, true
		}
	}
	return DetectedCommand{}, false
}

func TestDetectPackageJSONScripts(t *testing.T) {
	root := writeTree(t, map[string]string{
		"package.json": `{"name":"x","scripts":{"dev":"vite","build":"vite build","x:$weird":"bad"}}`,
		"bun.lock":     `{"":"%"}`,
	})
	cmds := Detect(root)
	if len(cmds) != 2 {
		t.Fatalf("want 2 valid scripts, got %d: %+v", len(cmds), cmds)
	}
	dev, ok := findCommand(cmds, "dev")
	if !ok || dev.Command != "bun run dev" || dev.Source != "package.json" {
		t.Fatalf("dev entry wrong: %+v (ok=%v)", dev, ok)
	}
}

func TestDetectPackageJSONRunnerByLockfile(t *testing.T) {
	cases := map[string]string{
		"pnpm-lock.yaml":    "pnpm run",
		"yarn.lock":         "yarn",
		"package-lock.json": "npm run",
	}
	for lock, runner := range cases {
		root := writeTree(t, map[string]string{
			"package.json": `{"scripts":{"test":"vitest"}}`,
			lock:           "# lock",
		})
		cmds := Detect(root)
		if len(cmds) != 1 || cmds[0].Command != runner+" test" {
			t.Fatalf("lock %s: want %q runner, got %+v", lock, runner, cmds)
		}
	}
}

func TestDetectMakefileTargets(t *testing.T) {
	root := writeTree(t, map[string]string{
		"Makefile": "# comment\n" +
			".PHONY: build test\n" +
			"build:\n" +
			"\tgo build ./...\n" +
			"test ::\n" +
			"\tgo test ./...\n" +
			"PREFIX := /usr/local\n" +
			"release: build\n" +
			"\techo done\n",
	})
	cmds := Detect(root)
	if len(cmds) != 3 {
		t.Fatalf("want build/test/release, got %+v", cmds)
	}
	for _, want := range []string{"make build", "make test", "make release"} {
		found := false
		for _, cmd := range cmds {
			if cmd.Command == want {
				found = true
			}
		}
		if !found {
			t.Fatalf("missing %q in %+v", want, cmds)
		}
	}
	if _, ok := findCommand(cmds, "PREFIX"); ok {
		t.Fatalf("variable assignment must not become a target: %+v", cmds)
	}
}

func TestDetectJustfileRecipes(t *testing.T) {
	root := writeTree(t, map[string]string{
		"justfile": "default:\n" +
			"    echo hi\n" +
			"deploy env:\n" +
			"    echo {{env}}\n" +
			"x := \"1\"\n" +
			"@inline:\n" +
			"    echo no\n",
	})
	cmds := Detect(root)
	if len(cmds) != 2 {
		t.Fatalf("want default+deploy, got %+v", cmds)
	}
	deploy, ok := findCommand(cmds, "deploy")
	if !ok || deploy.Command != "just deploy" {
		t.Fatalf("deploy recipe wrong: %+v (ok=%v)", deploy, ok)
	}
}

func TestDetectMissingRootIsNil(t *testing.T) {
	if cmds := Detect(filepath.Join(t.TempDir(), "absent")); cmds != nil {
		t.Fatalf("absent root must yield nil, got %+v", cmds)
	}
	if cmds := Detect(""); cmds != nil {
		t.Fatalf("empty root must yield nil, got %+v", cmds)
	}
	// A file, not a directory, is also rejected.
	file := filepath.Join(t.TempDir(), "f")
	if err := os.WriteFile(file, []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	if cmds := Detect(file); cmds != nil {
		t.Fatalf("file root must yield nil, got %+v", cmds)
	}
}

func TestDetectCapsAndDedupes(t *testing.T) {
	content := `{"scripts":{`
	for i := 0; i < maxDetectCommands+10; i++ {
		if i > 0 {
			content += ","
		}
		content += `"script` + strconv.Itoa(i) + `":"true"`
	}
	content += `}}`
	root := writeTree(t, map[string]string{"package.json": content})
	if cmds := Detect(root); len(cmds) != maxDetectCommands {
		t.Fatalf("detection must cap at %d, got %d", maxDetectCommands, len(cmds))
	}
}

func TestMergeCommandsUserWins(t *testing.T) {
	defs := []ScriptDefinition{{ID: "1", Name: "dev", Command: "my own dev"}, {ID: "2", Name: "custom", Command: "custom cmd"}}
	detected := []DetectedCommand{
		{Name: "dev", Command: "npm run dev", Source: "package.json"},
		{Name: "build", Command: "make build", Source: "Makefile"},
	}
	merged := MergeCommands(defs, detected)
	if len(merged) != 3 {
		t.Fatalf("want 3 merged entries, got %+v", merged)
	}
	if merged[0].Name != "dev" || !merged[0].User || merged[0].Command != "my own dev" {
		t.Fatalf("user definition must win and lead: %+v", merged[0])
	}
	if merged[1].Name != "custom" || !merged[1].User {
		t.Fatalf("user entries keep their order: %+v", merged[1])
	}
	if merged[2].Name != "build" || merged[2].User {
		t.Fatalf("detected entry follows: %+v", merged[2])
	}
}

func TestLastUsedStoreRoundtrip(t *testing.T) {
	path := filepath.Join(t.TempDir(), "last-commands.json")
	store, err := NewLastUsedStore(path)
	if err != nil {
		t.Fatal(err)
	}
	if got := store.Get("/a"); got != "" {
		t.Fatalf("empty store must read empty, got %q", got)
	}
	if err := store.Set("/a", "dev"); err != nil {
		t.Fatal(err)
	}
	if err := store.Set("/b", "test"); err != nil {
		t.Fatal(err)
	}
	reopened, err := NewLastUsedStore(path)
	if err != nil {
		t.Fatal(err)
	}
	if got := reopened.Get("/a"); got != "dev" {
		t.Fatalf("persisted last-used lost: %q", got)
	}
	if got := reopened.Get("/b"); got != "test" {
		t.Fatalf("second root lost: %q", got)
	}
}

func TestLastUsedStoreMemoryMode(t *testing.T) {
	store, err := NewLastUsedStore("")
	if err != nil {
		t.Fatal(err)
	}
	if err := store.Set("/x", "build"); err != nil {
		t.Fatal(err)
	}
	if got := store.Get("/x"); got != "build" {
		t.Fatalf("memory store must serve the value: %q", got)
	}
}
