package parser

import "testing"

func streamed(src string) string {
	d := New(Default)
	d.SetStreaming(true)
	d.Set(src)
	return d.HTML()
}

func TestHealing(t *testing.T) {
	for _, c := range []struct{ src, want string }{
		{"Some **bol", "<p>Some <strong>bol</strong></p>\n"},
		{"Some **bold** and *ita", "<p>Some <strong>bold</strong> and <em>ita</em></p>\n"},
		{"**a *b", "<p><strong>a <em>b</em></strong></p>\n"},
		{"a ~~gone", "<p>a <del>gone</del></p>\n"},
		{"run `go te", "<p>run <code>go te</code></p>\n"},
		{"run ``a`", "<p>run <code>a</code></p>\n"},
		{"hello **", "<p>hello</p>\n"},
		{"hello *", "<p>hello</p>\n"},
		{"hello `", "<p>hello</p>\n"},
		{"2*3 is", "<p>2*3 is</p>\n"},
		{"snake_case_name", "<p>snake_case_name</p>\n"},
		{"see [the docs](https://exa", `<p>see <a href="https://exa">the docs</a></p>` + "\n"},
		{"see [the docs](", `<p>see <a href="">the docs</a></p>` + "\n"},
		{"math $$x^2", `<p>math <span class="math display">x^2</span></p>` + "\n"},
		{"A paragraph\n-", "<p>A paragraph</p>\n"},
		{"A paragraph\n==", "<p>A paragraph</p>\n"},
		{"A paragraph\n---\n", "<h2>A paragraph</h2>\n"},
		{"Intro\n\n| a | b |\n", "<p>Intro</p>\n"},
		{"Intro\n\n| a | b |\n|--", "<p>Intro</p>\n"},
		{"| a | b |\n|---|---|\n| 1 | 2", "<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n<tbody>\n<tr>\n<td>1</td>\n<td>2</td>\n</tr>\n</tbody>\n</table>\n"},
		{"| a | b |\n|---|---|\n| 1 | **2", "<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n<tbody>\n<tr>\n<td>1</td>\n<td><strong>2</strong></td>\n</tr>\n</tbody>\n</table>\n"},
		{"```go\nfunc main() {", "<pre><code class=\"language-go\">func main() {\n</code></pre>\n"},
		{"caf\xc3", "<p>caf</p>\n"},
		{"Done **bold**\n\nnext", "<p>Done <strong>bold</strong></p>\n<p>next</p>\n"},
	} {
		if got := streamed(c.src); got != c.want {
			t.Errorf("%q:\ngot  %q\nwant %q", c.src, got, c.want)
		}
	}
}

// TestHealingOnlyTail checks that blocks already finished show as
// written.
func TestHealingOnlyTail(t *testing.T) {
	got := streamed("a **b\n\nc **d")
	want := "<p>a **b</p>\n<p>c <strong>d</strong></p>\n"
	if got != want {
		t.Errorf("got %q, want %q", got, want)
	}
}

func TestPendingLink(t *testing.T) {
	d := New(Default)
	d.SetStreaming(true)
	d.Set("see [docs](https://exa")
	in := d.Inlines(1)
	if len(in) < 2 || in[1].Kind != Link || !in[1].Pending {
		t.Fatalf("inlines %+v", in)
	}
	d.Append("mple.com) now")
	in = d.Inlines(1)
	if in[1].Kind != Link || in[1].Pending || in[1].Dest != "https://example.com" {
		t.Fatalf("inlines %+v", in)
	}
}
