package ui

import (
	"errors"

	"github.com/egoist/mygo/internal/gpu/d3d11"
	"github.com/egoist/mygo/internal/platform"
)

func newGPURenderer(n platform.SurfaceNative) (gpuRenderer, error) {
	if n.HWND == 0 {
		return nil, errors.New("the surface has no window")
	}
	return d3d11.New(n.HWND)
}
