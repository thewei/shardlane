package nativeui

/**
 * [INPUT]: 依赖 internal/sshconfig（Entry 索引、EditHostBlock 单块定向改写、SaveFile 原子写）、ui.TextInput、共享 DS 组件
 * [OUTPUT]: hostEntryDetail 的就地编辑行 hostFormRows（别名 + HostName/User/Port/ProxyJump 输入框，Return 保存）及其保存事务 openHostForm/saveHostForm
 * [POS]: Phase 2 结构化编辑：仅限单字面 Host 块；保存是单块定向改写（注释与其余指令逐字保留）+ 陈旧围栏 + .bak 备份；raw 编辑器保持全量编辑入口
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"errors"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/sshconfig"
)

// hostsFormState is the draft for one host block's managed directives. It
// holds no file text: a save rewrites exactly the managed directives inside
// the indexed block, leaving every other line untouched on disk.
type hostsFormState struct {
	open         bool
	entryKey     string
	alias        string
	hostName     string
	user         string
	port         string
	proxyJump    string
	loaded       string // the prefill serialization; dirty is a diff against it
	confirmClose bool
	err          string
	saving       bool
}

// hostFormFields pairs one managed directive's canonical name with its
// draft field; match order is also the render and write order.
type hostFormField struct {
	directive string
	value     func(*hostsFormState) *string
}

var hostFormFields = []hostFormField{
	{"HostName", func(f *hostsFormState) *string { return &f.hostName }},
	{"User", func(f *hostsFormState) *string { return &f.user }},
	{"Port", func(f *hostsFormState) *string { return &f.port }},
	{"ProxyJump", func(f *hostsFormState) *string { return &f.proxyJump }},
}

// hostFormEditable reports whether an entry may take the structured form:
// one literal pattern only — patterns and Match blocks are ssh's own
// business, and oversized blocks fall back to the raw editor.
func hostFormEditable(entry sshconfig.Entry) bool {
	return entry.Kind == sshconfig.KindHost && entry.Alias() != "" &&
		len(entry.Lines) < maxFormBlockLines
}

// maxFormBlockLines bounds the blocks the form will rewrite; beyond it the
// raw editor is the honest tool.
const maxFormBlockLines = 400

// openHostForm prefills the draft from the entry's indexed facts.
func (s *Shell) openHostForm(entry sshconfig.Entry) {
	draft := hostsFormState{
		open:     true,
		entryKey: hostEntryKey(entry),
		alias:    entry.Alias(),
	}
	values := map[string]string{}
	for _, kv := range entry.Facts() {
		key := strings.ToLower(kv.Key)
		if _, seen := values[key]; !seen {
			values[key] = strings.TrimSpace(kv.Value)
		}
	}
	for _, field := range hostFormFields {
		*field.value(&draft) = values[strings.ToLower(field.directive)]
	}
	draft.loaded = draft.serialize()
	s.hosts.form = draft
}

func (f *hostsFormState) serialize() string {
	parts := []string{f.alias}
	for _, field := range hostFormFields {
		parts = append(parts, *field.value(f))
	}
	return strings.Join(parts, "\x1f")
}

func (s *Shell) hostsFormDirty() bool {
	f := &s.hosts.form
	return f.open && f.serialize() != f.loaded
}

func (s *Shell) cancelHostForm() {
	if s.hostsFormDirty() && !s.hosts.form.confirmClose {
		s.hosts.form.confirmClose = true
		return
	}
	s.hosts.form = hostsFormState{}
}

// hostFormRows are the open draft's rows in the detail's grouped list: the
// alias, then the managed directives in their fixed order. Return in any
// field saves, as in Settings.
func (s *Shell) hostFormRows(c *ui.Context, entry sshconfig.Entry) []settingsRow {
	f := &s.hosts.form
	input := func(label string, value *string, placeholder string) settingsRow {
		return row(label, "", func() {
			field := ui.TextInput(c, value).Width(240).Label(label)
			if placeholder != "" {
				field.Placeholder(placeholder)
			}
			if field.Submitted() {
				s.saveHostForm(entry)
			}
		})
	}
	rows := []settingsRow{input("Host", &f.alias, "")}
	for _, field := range hostFormFields {
		placeholder := ""
		if field.directive == "Port" {
			placeholder = "22"
		}
		rows = append(rows, input(field.directive, field.value(f), placeholder))
	}
	return rows
}

// unmanagedDirectives are the facts the form does not edit: everything but
// the managed directives, in file order.
func unmanagedDirectives(facts []sshconfig.KV) []sshconfig.KV {
	var rest []sshconfig.KV
	for _, kv := range facts {
		managed := false
		for _, field := range hostFormFields {
			if strings.EqualFold(kv.Key, field.directive) {
				managed = true
				break
			}
		}
		if !managed {
			rest = append(rest, kv)
		}
	}
	return rest
}

// validateHostForm checks the draft before any disk work.
func validateHostForm(f hostsFormState) error {
	if strings.TrimSpace(f.alias) == "" {
		return errors.New("host name is required")
	}
	if f.port != "" {
		port, err := strconv.Atoi(f.port)
		if err != nil || port < 1 || port > 65535 {
			return errors.New("port must be a number between 1 and 65535")
		}
	}
	return nil
}

// hostFormDelta diffs the draft against the indexed facts into the
// directive sets/removes and the header rename for EditHostBlock.
func hostFormDelta(entry sshconfig.Entry, f hostsFormState) (sets []sshconfig.KV, removes []string, header string) {
	current := map[string]string{}
	for _, kv := range entry.Facts() {
		key := strings.ToLower(kv.Key)
		if _, seen := current[key]; !seen {
			current[key] = strings.TrimSpace(kv.Value)
		}
	}
	header = strings.TrimSpace(f.alias)
	for _, field := range hostFormFields {
		value := strings.TrimSpace(*field.value(&f))
		was := current[strings.ToLower(field.directive)]
		switch {
		case value == "" && was != "":
			removes = append(removes, field.directive)
		case value != "" && value != was:
			sets = append(sets, sshconfig.KV{Key: field.directive, Value: value})
		}
	}
	return sets, removes, header
}

// saveHostForm rewrites the managed directives of exactly this block on
// disk. The stale fence inside EditHostBlock refuses the write when the
// file moved under the draft, and SaveFile keeps the usual .bak backup.
func (s *Shell) saveHostForm(entry sshconfig.Entry) {
	f := &s.hosts.form
	if !f.open || f.saving {
		return
	}
	if err := validateHostForm(*f); err != nil {
		f.err = err.Error()
		return
	}
	// A dirty raw-editor buffer over the same file would silently revert
	// this write the next time it saves — refuse instead.
	if s.hosts.editing && s.hosts.editorFile == entry.File && s.hostsEditorDirty() {
		f.err = "Save or discard the raw editor first — it holds unsaved changes for this file"
		return
	}
	sets, removes, header := hostFormDelta(entry, *f)
	nothingToDo := header == entry.Alias() && len(sets) == 0 && len(removes) == 0
	f.saving = true
	f.err = ""
	go func() {
		var err error
		if nothingToDo {
			err = errors.New("no changes to save")
		} else {
			_, err = rewriteHostBlockFile(entry, header, sets, removes)
		}
		s.applyOnUI(func() {
			f.saving = false
			if err != nil {
				if errors.Is(err, sshconfig.ErrStaleBlock) {
					s.refreshHostsIndex(true)
					f.err = "The file changed since it was loaded — the list refreshed, try again"
					return
				}
				f.err = err.Error()
				return
			}
			slog.Info("host form saved", "directives", len(sets)+len(removes))
			s.hosts.form = hostsFormState{}
			s.status = "Host saved — previous copy kept as " + filepath.Base(entry.File) + ".bak"
			s.refreshHostsIndex(true)
		})
	}()
}

// rewriteHostBlockFile reads the entry's file, applies the single-block
// edit, and persists it through the atomic SaveFile path.
func rewriteHostBlockFile(entry sshconfig.Entry, header string, sets []sshconfig.KV, removes []string) (string, error) {
	data, err := os.ReadFile(entry.File)
	if err != nil {
		return "", fmt.Errorf("read %s: %w", entry.File, err)
	}
	if len(data) > hostsEditorMaxBytes {
		return "", errors.New("file too large to edit here")
	}
	lines := strings.Split(string(data), "\n")
	out, err := sshconfig.EditHostBlock(lines, entry.Line, strings.Join(entry.Patterns, " "), header, sets, removes)
	if err != nil {
		return "", err
	}
	joined := strings.Join(out, "\n")
	if err := sshconfig.SaveFile(entry.File, joined); err != nil {
		return "", err
	}
	return joined, nil
}
