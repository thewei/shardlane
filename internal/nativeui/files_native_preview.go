package nativeui

/**
 * [INPUT]: rightPanel.selectedFilePath/currentRoot、filesview.PreviewFlavor/ReadTextHead/DecodeImageBounded、stdlib image 解码 + ui.NewBitmap
 * [OUTPUT]: Files 预览的 Native 兜底层（FILES-09）：选中后的后台分级计划（flavor×size）、原生有界文本头视图、原生 ui.Image 视图（像素预算）与失败真实态、clearNativeFilesPreview 清理点
 * [POS]: WebView 兜不住的文件在此原生渲染，不再给死胡同空态；解码全部离 UI lane，ui.NewBitmap 回 UI lane；不新增 Herdr 方法
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"os"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/filesview"
)

// openFilePreview records the tree selection and routes it through the
// preview plan: the WebView tier hears a files:preview event for files it
// can render; the Native tier takes oversized ones without publishing.
// The size probe runs off the render closure; the plan lands on the UI lane.
func (s *Shell) openFilePreview(path string) {
	st := &s.rightPanel
	if st.selectedFilePath == path {
		return
	}
	st.selectedFilePath = path
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

// probePreview stats the file and reads its flavor off the UI lane.
func probePreview(path string) (int64, filesview.Flavor) {
	var size int64
	if info, err := os.Stat(path); err == nil {
		size = info.Size()
	}
	return size, filesview.PreviewFlavor(path)
}

// applyPreviewPlan routes the selection to its tier (UI lane). A failed
// stat (file just removed) still plans on flavor alone; the tiers surface
// their own read errors.
func (s *Shell) applyPreviewPlan(path, root string, size int64, flavor filesview.Flavor) {
	st := &s.rightPanel
	if st.currentRoot != root || st.selectedFilePath != path {
		return // stale: the user moved on while the probe ran
	}
	ext := strings.ToLower(filepath.Ext(path))
	switch {
	case flavor != filesview.FlavorImage && size <= filesview.PreviewCap,
		flavor == filesview.FlavorImage && size <= filesview.ImagePreviewCap:
		// WebView tier: content reaches the page through the bridge.
		s.publishFilesPreview()
	case flavor == filesview.FlavorImage && ext == ".svg":
		// SVG is XML source: an oversized one reads natively as text.
		s.beginNativeText(path)
	case flavor == filesview.FlavorImage && nativeImageDecodable(ext):
		s.beginNativeImage(path)
	case flavor == filesview.FlavorImage:
		// Oversized AVIF/ICO: neither tier decodes them — say so truthfully.
		st.nativePreviewPath = path
		st.nativeImageFailed = "This image is too large to preview. Reveal it in Finder to open it."
	default:
		s.beginNativeText(path)
	}
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

// beginNativeText loads the bounded text head for an oversized text file.
func (s *Shell) beginNativeText(path string) {
	st := &s.rightPanel
	st.nativePreviewPath = path
	st.nativeText = nil
	st.nativeTextFailed = ""
	st.nativeTextLoading = true
	st.nativeImage = nil
	st.nativeImageLoading = false
	st.nativeImageFailed = ""
	gen := st.filesGen.Load()
	root := st.currentRoot
	apply := func(preview filesview.Preview, err error) {
		if gen != st.filesGen.Load() || st.currentRoot != root || st.selectedFilePath != path {
			return
		}
		st.nativeTextLoading = false
		if err != nil {
			st.nativeTextFailed = err.Error()
			return
		}
		st.nativeText = &preview
	}
	if s.win == nil {
		preview, err := filesview.ReadTextHead(path)
		apply(preview, err)
		return
	}
	go func() {
		preview, err := filesview.ReadTextHead(path)
		s.applyOnWindow(func() { apply(preview, err) })
	}()
}

// beginNativeImage decodes an oversized image for the Native tier: the
// bounded decode runs off the UI lane, the Bitmap wraps on it (texture
// upload wants the main thread).
func (s *Shell) beginNativeImage(path string) {
	st := &s.rightPanel
	st.nativePreviewPath = path
	st.nativeText = nil
	st.nativeTextLoading = false
	st.nativeTextFailed = ""
	st.nativeImage = nil
	st.nativeImageFailed = ""
	st.nativeImageLoading = true
	gen := st.filesGen.Load()
	root := st.currentRoot
	if s.win == nil {
		// Headless (tests): decode inline; the Bitmap wrap needs the GPU,
		// so headless state carries only the outcome.
		// The decode result is intentionally discarded: headless state
		// carries only the outcome, never the bitmap.
		if _, err := filesview.DecodeImageBounded(path); err != nil {
			st := &s.rightPanel
			if gen != st.filesGen.Load() || st.currentRoot != root || st.selectedFilePath != path {
				return
			}
			st.nativeImageLoading = false
			st.nativeImageFailed = err.Error()
			return
		}
		st := &s.rightPanel
		if gen != st.filesGen.Load() || st.currentRoot != root || st.selectedFilePath != path {
			return
		}
		st.nativeImageLoading = false
		return
	}
	go func() {
		img, err := filesview.DecodeImageBounded(path)
		s.callOnWindow(func() {
			st := &s.rightPanel
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

// clearNativeFilesPreview drops the native tier's state (selection change,
// root change, removal of the selected file).
func (s *Shell) clearNativeFilesPreview() {
	st := &s.rightPanel
	st.nativePreviewPath = ""
	st.nativeText = nil
	st.nativeTextLoading = false
	st.nativeTextFailed = ""
	st.nativeImage = nil
	st.nativeImageLoading = false
	st.nativeImageFailed = ""
}

// nativePreviewBody renders the native tier's take over of the center:
// the decoded image, the bounded text head, or the truthful failure.
func (s *Shell) nativePreviewBody(c *ui.Context, selected string) {
	st := &s.rightPanel
	t := c.Theme()
	sp := Spacing()
	ui.Column(c).FillWidth().Grow(1).MinWidth(0).MinHeight(0).Children(func() {
		ui.Row(c).FillWidth().Padding(sp.S, sp.L).Gap(sp.M).AlignItems(ui.Center).Children(func() {
			ui.Text(c, filepath.Base(selected)).FontSize(Typography().Body).FontWeight(WeightSemibold).
				Grow(1).SingleLine().Tooltip(selected)
			if st.nativeText != nil {
				ui.Text(c, filesview.FormatFileSize(st.nativeText.SizeBytes)).
					FontSize(Typography().Caption).TextColor(t.TextMuted)
				if st.nativeText.Truncated {
					statusPill(c, "Showing the first 256 KiB", ToneAttention)
				}
			} else if st.nativeImage != nil {
				ui.Text(c, "Native view (oversized for the web preview)").
					FontSize(Typography().Caption).TextColor(t.TextMuted)
			}
		})
		switch {
		case st.nativeTextLoading || st.nativeImageLoading:
			ui.Column(c).Grow(1).Center().Children(func() {
				ui.Spinner(c).Size(16, 16)
			})
		case st.nativeImageFailed != "":
			ui.Column(c).Grow(1).Center().Children(func() {
				emptyState(c, "Can't preview this image", st.nativeImageFailed)
			})
		case st.nativeTextFailed != "":
			ui.Column(c).Grow(1).Center().Children(func() {
				emptyState(c, "Can't read this file", st.nativeTextFailed)
			})
		case st.nativeImage != nil:
			ui.Box(c).Grow(1).FillWidth().Padding(sp.L).Children(func() {
				ui.Image(c, st.nativeImage).Fit(ui.ScaleDown).Grow(1).FillWidth()
			})
		case st.nativeText != nil:
			display := st.nativeText.Content
			// The old renderer's accessibility bound: one ui.Text per
			// preview, so a huge single line cannot explode the tree.
			if len(display) > 120*1024 {
				display = display[:120*1024]
				display = strings.ToValidUTF8(display, "")
				display += "\n\n[Large file: showing the first 120 KiB of the head]"
			}
			ui.Scroll(c).Grow(1).MinHeight(0).Children(func() {
				ui.Column(c).FillWidth().Padding(sp.L).Children(func() {
					ui.Text(c, display).Font(gdCodeFont()).FontSize(Typography().BodySmall).
						TextColor(t.Text)
				})
			})
		default:
			ui.Column(c).Grow(1).Center().Children(func() {
				ui.Spinner(c).Size(16, 16)
			})
		}
	})
}
