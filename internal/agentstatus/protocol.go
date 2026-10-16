package agentstatus

import (
	"encoding/base64"
	"strconv"
	"strings"
	"unicode/utf8"
)

/**
 * [INPUT]: 依赖 encoding/base64、strconv、strings、unicode/utf8，无项目内依赖
 * [OUTPUT]: 对外提供 State/Kind 常量、Report 结构体、Parse、StatusWord
 * [POS]: internal/agentstatus 的协议解析层，把 OSC 7501 报告体（7501; 与终止符之间的字节）按 spec rev 0.4 解析成 Report；records.go 消费它
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// State is the `state` key's vocabulary of OSC 7501 (spec rev 0.4). Clear
// is not a state; it removes records.
type State string

const (
	StateIdle    State = "idle"
	StateWorking State = "working"
	StateDone    State = "done"
	StateBlocked State = "blocked"
	StateError   State = "error"
	StateClear   State = "clear"
)

// Kind is the `kind` key's vocabulary, valid with blocked only.
type Kind string

const (
	KindPermission Kind = "permission"
	KindQuestion   Kind = "question"
	KindAuth       Kind = "auth"
)

// limits are the spec's hard caps. A report that breaks one is discarded
// whole; pair-level syntax faults only skip the pair.
const (
	maxKey        = 16
	maxApp        = 32
	maxIDTotal    = 128
	maxIDSegment  = 32
	maxIDDepth    = 8
	maxMsgEncoded = 2732
	maxMsgPlain   = 2048
	maxTtlEncoded = 256
	maxTtlPlain   = 192
)

// Report is one parsed OSC 7501 report. Missing keys are the zero values;
// Progress nil means indeterminate.
type Report struct {
	State    State
	ID       string
	Kind     Kind
	Progress *int
	App      string
	Title    string
	Msg      string
}

// Transient reports whether the state dies at process exit or the next
// shell prompt (working and blocked do; the rest survive).
func (r Report) Transient() bool {
	return r.State == StateWorking || r.State == StateBlocked
}

// StatusWord maps the state onto Shardlane's agent status vocabulary, the
// strings apphook.NormalizeStatus already produces, so both status sources
// feed one projection field.
func (r Report) StatusWord() string {
	switch r.State {
	case StateIdle:
		return "idle"
	case StateWorking:
		return "working"
	case StateDone:
		return "done"
	case StateBlocked:
		return "blocked"
	case StateError:
		return "error"
	default:
		return ""
	}
}

// Parse turns one report body — the bytes between `7501;` and the string
// terminator — into a Report. It implements spec rev 0.4: pairs split on
// ':' and joined on '=', malformed pairs skipped, unknown keys ignored,
// last value wins; a missing or unrecognized state, or an id outside the
// path grammar, ignores the whole report; broken base64, control
// characters or an over-limit field discard the report whole.
func Parse(body []byte) (Report, bool) {
	var rep Report
	var kind, progress, app, title, msg string
	var hasKind, hasProgress, hasApp, hasTitle, hasMsg bool

	for _, rawPair := range strings.Split(string(body), ":") {
		eq := strings.IndexByte(rawPair, '=')
		if eq <= 0 {
			continue // no '=' or empty key: skip the pair
		}
		key := strings.Trim(rawPair[:eq], " ")
		value := strings.Trim(rawPair[eq+1:], " ")
		if !validKey(key) || !validValue(value) {
			continue
		}
		switch key {
		case "state":
			rep.State = State(value)
		case "id":
			rep.ID = value
		case "kind":
			kind, hasKind = value, true
		case "progress":
			progress, hasProgress = value, true
		case "app":
			app, hasApp = value, true
		case "title":
			title, hasTitle = value, true
		case "msg":
			msg, hasMsg = value, true
		}
	}

	switch rep.State {
	case StateIdle, StateWorking, StateDone, StateBlocked, StateError, StateClear:
	default:
		return Report{}, false // missing or unrecognized state: ignored whole
	}
	if rep.ID != "" && !validID(rep.ID) {
		return Report{}, false // a malformed id must not reach any record
	}

	if rep.State == StateBlocked && hasKind {
		switch Kind(kind) {
		case KindPermission, KindQuestion, KindAuth:
			rep.Kind = Kind(kind)
		}
	}
	if rep.State == StateWorking || rep.State == StateBlocked {
		if n, err := strconv.Atoi(progress); hasProgress && err == nil && n >= 0 && n <= 100 {
			rep.Progress = &n
		}
	}
	if hasApp && validApp(app) {
		rep.App = app
	}
	if hasTitle {
		s, ok := decodeText(title, maxTtlEncoded, maxTtlPlain)
		if !ok {
			return Report{}, false
		}
		rep.Title = s
	}
	if hasMsg {
		s, ok := decodeText(msg, maxMsgEncoded, maxMsgPlain)
		if !ok {
			return Report{}, false
		}
		rep.Msg = s
	}
	return rep, true
}

// decodeText decodes one base64 UTF-8 field. Padding is optional; a value
// over the encoded cap, undecodable bytes, invalid UTF-8 or control
// characters (C0, DEL, C1) discard the whole report.
func decodeText(encoded string, maxEncoded, maxPlain int) (string, bool) {
	if encoded == "" || len(encoded) > maxEncoded {
		return "", false
	}
	decoder := base64.StdEncoding
	if !strings.HasSuffix(encoded, "=") {
		decoder = base64.RawStdEncoding
	}
	plain, err := decoder.DecodeString(encoded)
	if err != nil || len(plain) > maxPlain || !utf8.Valid(plain) {
		return "", false
	}
	for _, r := range string(plain) {
		if r <= 0x1f || r == 0x7f || (r >= 0x80 && r <= 0x9f) {
			return "", false
		}
	}
	return string(plain), true
}

// validKey matches `key := [a-z]+` within the byte cap.
func validKey(key string) bool {
	if key == "" || len(key) > maxKey {
		return false
	}
	for i := 0; i < len(key); i++ {
		if key[i] < 'a' || key[i] > 'z' {
			return false
		}
	}
	return true
}

// validValue matches the value character set, which excludes ':' and ';'
// so that nothing needs escaping.
func validValue(value string) bool {
	for i := 0; i < len(value); i++ {
		c := value[i]
		switch {
		case c >= 'a' && c <= 'z', c >= 'A' && c <= 'Z', c >= '0' && c <= '9':
		case c == '_' || c == '.' || c == ',' || c == '+' || c == '/' || c == '=' || c == '-':
		default:
			return false
		}
	}
	return true
}

// validID matches `segment ("/" segment)*` with the spec's caps: 1..8
// segments of 1..32 id characters, 128 bytes in total.
func validID(id string) bool {
	if len(id) > maxIDTotal {
		return false
	}
	segments := strings.Split(id, "/")
	if len(segments) > maxIDDepth {
		return false
	}
	for _, seg := range segments {
		if seg == "" || len(seg) > maxIDSegment {
			return false
		}
		for i := 0; i < len(seg); i++ {
			c := seg[i]
			switch {
			case c >= 'a' && c <= 'z', c >= 'A' && c <= 'Z', c >= '0' && c <= '9':
			case c == '_' || c == '.' || c == '+' || c == '-':
			default:
				return false
			}
		}
	}
	return true
}

// validApp matches the app character set within 32 bytes.
func validApp(app string) bool {
	if app == "" || len(app) > maxApp {
		return false
	}
	for i := 0; i < len(app); i++ {
		c := app[i]
		switch {
		case c >= 'a' && c <= 'z', c >= 'A' && c <= 'Z', c >= '0' && c <= '9':
		case c == '_' || c == '.' || c == '+' || c == '-':
		default:
			return false
		}
	}
	return true
}
