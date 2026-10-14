package nativeui

/**
 * [INPUT]: rightPanel.selectedFilePath/currentRoot/markdownSource、filesview.PreviewFlavor/ReadText/DecodeImageBounded、ui/editor（只读代码视图）、stdlib image 解码 + ui.NewBitmap
 * [OUTPUT]: Files 预览的分级计划（flavor×size×偏好）、原生代码视图（editor.View，Buffer 离 UI lane 构建）、原生 ui.Image 视图（像素预算）、查看条（名称/大小/截断/Markdown 渲染⇄源码）与失败真实态、clearNativeFilesPreview 清理点
 * [POS]: 产品决策 2026-10-14：文本/代码在原生 editor 里显示，WebView 只渲染 HTML 才擅长的（Markdown 渲染、网页图片）；读取与解码全部离 UI lane，ui.NewBitmap 与 editor.State 回 UI lane；不新增 Herdr 方法
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"os"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/wh-studio/herdr-client/internal/filesview"
)

// openFilePreview records the tree selection and routes it through the
// preview plan.
func (s *Shell) openFilePreview(path string) {
	st := &s.rightPanel
	if st.selectedFilePath == path {
		return
	}
	st.selectedFilePath = path
	s.planFilePreview(path)
}

// planFilePreview picks the selection's tier: the WebView page hears a
// files:preview event for what HTML suits (rendered Markdown, web
// images); the native tier takes everything else without publishing it.
// The size probe runs off the render closure; the plan lands on the UI lane.
func (s *Shell) planFilePreview(path string) {
	st := &s.rightPanel
	s.clearNativeFilesPreview()
	root := st.currentRoot
	if s.win == nil {
		// Headless (tests): probe and decide inline like requestDirectory.
		size, flavor := probePreview(path)
		s.applyPreviewPlan(path, root, size, flavor)
		return
	}
	go func() {
		size, flavor := probePreview(path)
		s.callOnWindow(func() { s.applyPreviewPlan(path, root, size, flavor) })
	}()
}

// replanFilesPreview re-routes the open file after a preference changed
// which tier may take it.
func (s *Shell) replanFilesPreview() {
	if path := s.rightPanel.selectedFilePath; path != "" {
		s.planFilePreview(path)
	}
}

// setMarkdownSource switches the sticky Rendered/Source choice and re-routes
// an open Markdown file; the size is already known, so no second probe runs.
func (s *Shell) setMarkdownSource(source bool) {
	st := &s.rightPanel
	if st.markdownSource == source {
		return
	}
	st.markdownSource = source
	path := st.selectedFilePath
	if path == "" || filesview.PreviewFlavor(path) != filesview.FlavorMarkdown {
		return
	}
	size := st.previewSize
	s.clearNativeFilesPreview()
	s.applyPreviewPlan(path, st.currentRoot, size, filesview.FlavorMarkdown)
}

// probePreview stats the file and reads its flavor off the UI lane.
func probePreview(path string) (int64, filesview.Flavor) {
	var size int64
	if info, err := os.Stat(path); err == nil {
		size = info.Size()
	}
	return size, filesview.PreviewFlavor(path)
}

// webPreviewUsable reports whether the WebView page may take a file: the
// preference is on and its creation has not failed.
func (s *Shell) webPreviewUsable() bool {
	return s.useWebviewPreview() && !s.filesWebviewBroken
}

// applyPreviewPlan routes the selection to its tier (UI lane). A failed
// stat (file just removed) still plans on flavor alone; the tiers surface
// their own read errors.
func (s *Shell) applyPreviewPlan(path, root string, size int64, flavor filesview.Flavor) {
	st := &s.rightPanel
	if st.currentRoot != root || st.selectedFilePath != path {
		return // stale: the user moved on while the probe ran
	}
	st.previewSize = size
	st.previewPlanned = path
	ext := strings.ToLower(filepath.Ext(path))
	web := s.webPreviewUsable()
	switch {
	case web && flavor == filesview.FlavorImage && size <= filesview.ImagePreviewCap,
		web && flavor == filesview.FlavorMarkdown && !st.markdownSource && size <= filesview.PreviewCap:
		// WebView tier: content reaches the page through the bridge.
	case flavor == filesview.FlavorImage && ext == ".svg":
		// SVG is XML source: it reads natively as text.
		s.beginNativeText(path, flavor)
	case flavor == filesview.FlavorImage && nativeImageDecodable(ext):
		s.beginNativeImage(path)
	case flavor == filesview.FlavorImage:
		// AVIF/ICO decode only in the page — say so truthfully.
		s.claimNativePreview(path)
		if web {
			st.nativeImageFailed = "This image is too large to preview. Reveal it in Finder to open it."
		} else {
			st.nativeImageFailed = "This format previews only with “Rich file preview” on (Settings → General)."
		}
	default:
		s.beginNativeText(path, flavor)
	}
	// The page hears the new owner: the path, or none when native took it.
	s.publishFilesPreview()
}

// nativeImageDecodable reports whether the Native tier can decode the
// format (the decoder set ui's bitmap support registers: PNG, JPEG, GIF,
// WebP, BMP).
func nativeImageDecodable(ext string) bool {
	switch ext {
	case ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp":
		return true
	}
	return false
}

// claimNativePreview hands the center to the native tier for path with
// every native slot empty.
func (s *Shell) claimNativePreview(path string) {
	st := &s.rightPanel
	st.nativePreviewPath = path
	st.nativeText = nil
	st.nativeCode = nil
	st.nativeTextLoading = false
	st.nativeTextFailed = ""
	st.nativeImage = nil
	st.nativeImageLoading = false
	st.nativeImageFailed = ""
}

// beginNativeText loads a text file into the code view. The read and the
// editor Buffer (a rope, safe from any goroutine) build off the UI lane;
// the State that shows it is made on it.
func (s *Shell) beginNativeText(path string, flavor filesview.Flavor) {
	st := &s.rightPanel
	s.claimNativePreview(path)
	st.nativeTextLoading = true
	gen := st.filesGen.Load()
	root := st.currentRoot
	name := filepath.Base(path)
	load := func() (filesview.Preview, *editor.Buffer, error) {
		preview, err := filesview.ReadText(path, root)
		if err != nil || preview.IsBinary {
			return preview, nil, err
		}
		buf := editor.NewBuffer(preview.Content)
		buf.SetLanguage(editor.LanguageFor(name))
		preview.Content = "" // the Buffer holds the text now
		return preview, buf, nil
	}
	apply := func(preview filesview.Preview, buf *editor.Buffer, err error) {
		if gen != st.filesGen.Load() || st.currentRoot != root || st.selectedFilePath != path {
			return
		}
		st.nativeTextLoading = false
		if err != nil {
			st.nativeTextFailed = err.Error()
			return
		}
		st.nativeText = &preview
		if buf != nil {
			st.nativeCode = &editor.State{
				Buffer:       buf,
				ReadOnly:     true,
				NoActiveLine: true, // a viewer has no line being worked on
				Label:        name,
				Placeholder:  "Empty file",
				// Prose wraps; code scrolls sideways.
				SoftWrap: flavor == filesview.FlavorMarkdown || buf.Language() == nil,
			}
		}
	}
	if s.win == nil {
		preview, buf, err := load()
		apply(preview, buf, err)
		return
	}
	go func() {
		preview, buf, err := load()
		s.applyOnWindow(func() { apply(preview, buf, err) })
	}()
}

// beginNativeImage decodes an image for the Native tier: the bounded
// decode runs off the UI lane, the Bitmap wraps on it (texture upload
// wants the main thread).
func (s *Shell) beginNativeImage(path string) {
	st := &s.rightPanel
	s.claimNativePreview(path)
	st.nativeImageLoading = true
	gen := st.filesGen.Load()
	root := st.currentRoot
	if s.win == nil {
		// Headless (tests): decode inline; the Bitmap wrap needs the GPU,
		// so headless state carries only the outcome, never the bitmap.
		_, err := filesview.DecodeImageBounded(path)
		if gen != st.filesGen.Load() || st.currentRoot != root || st.selectedFilePath != path {
			return
		}
		st.nativeImageLoading = false
		if err != nil {
			st.nativeImageFailed = err.Error()
		}
		return
	}
	go func() {
		img, err := filesview.DecodeImageBounded(path)
		s.callOnWindow(func() {
			if gen != st.filesGen.Load() || st.currentRoot != root || st.selectedFilePath != path {
				return
			}
			st.nativeImageLoading = false
			if err != nil {
				st.nativeImageFailed = err.Error()
				return
			}
			st.nativeImage = ui.NewBitmap(img)
		})
	}()
}

// clearNativeFilesPreview drops the per-selection preview state (selection
// change, root change, removal of the selected file); the sticky
// markdownSource choice stays.
func (s *Shell) clearNativeFilesPreview() {
	st := &s.rightPanel
	st.previewSize = 0
	st.previewPlanned = ""
	s.claimNativePreview("")
}

// filesPreviewBar is the open file's strip over every tier: name, size,
// the limit that cut it, and the Rendered/Source choice of a Markdown file
// the page could render.
func (s *Shell) filesPreviewBar(c *ui.Context, selected string) {
	st := &s.rightPanel
	t := c.Theme()
	typ := Typography()
	sp := Spacing()
	// The segmented choice sets the bar's height; every file keeps it, so
	// the code never jumps between a Markdown file and the rest.
	ui.Row(c).FillWidth().MinHeight(Controls().RegularHeight+2*sp.S).Padding(sp.S, sp.L).Gap(sp.M).AlignItems(ui.Center).
		BorderWidth(0, 0, 1, 0).BorderColor(designTokens(t.Dark).BorderSubtle).Children(func() {
		ui.Text(c, filepath.Base(selected)).FontSize(typ.Body).FontWeight(WeightSemibold).
			Grow(1).SingleLine().Tooltip(selected)
		if st.previewSize > 0 {
			ui.Text(c, filesview.FormatFileSize(st.previewSize)).
				FontSize(typ.Caption).TextColor(t.TextMuted)
		}
		if st.nativeText != nil && st.nativeText.Truncated {
			statusPill(c, "Showing the first "+filesview.FormatFileSize(filesview.TextPreviewCap), ToneAttention)
		}
		if st.previewPlanned == selected && filesview.PreviewFlavor(selected) == filesview.FlavorMarkdown &&
			s.webPreviewUsable() && st.previewSize <= filesview.PreviewCap {
			choice := 0
			if st.markdownSource {
				choice = 1
			}
			if ui.Segmented(c, &choice, "Rendered", "Source").Label("Markdown view").Changed() {
				s.setMarkdownSource(choice == 1)
			}
		}
	})
}

// nativePreviewBody renders the native tier's take over of the center:
// the code view, the decoded image, or the truthful state.
func (s *Shell) nativePreviewBody(c *ui.Context) {
	st := &s.rightPanel
	switch {
	case st.nativeTextLoading || st.nativeImageLoading:
		ui.Column(c).Grow(1).Center().Children(func() {
			ui.Spinner(c).Size(16, 16)
		})
	case st.nativeImageFailed != "":
		emptyState(c, "Can't preview this image", st.nativeImageFailed)
	case st.nativeTextFailed != "":
		emptyState(c, "Can't read this file", st.nativeTextFailed)
	case st.nativeImage != nil:
		ui.Box(c).Grow(1).FillWidth().Padding(Spacing().L).Children(func() {
			ui.Image(c, st.nativeImage).Fit(ui.ScaleDown).Grow(1).FillWidth()
		})
	case st.nativeText != nil && st.nativeText.IsBinary:
		emptyState(c, "Binary file", "Reveal it in Finder to open it.")
	case st.nativeCode != nil:
		// The theme follows the window each frame: an appearance change
		// repaints an open file without reloading it.
		st.nativeCode.Theme = codeEditorTheme(c.Theme())
		editor.View(c, st.nativeCode).FillWidth().Grow(1).MinHeight(0)
	default:
		ui.Column(c).Grow(1).Center().Children(func() {
			ui.Spinner(c).Size(16, 16)
		})
	}
}
