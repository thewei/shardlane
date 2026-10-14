package gpu

import (
	"testing"

	"github.com/egoist/mygo/internal/gpu/gputest"
	"github.com/egoist/mygo/internal/scene"
)

// BenchmarkBuildSprites turns the scene of a 2D game's frame at 1920×1080
// into instances: tiles 3 times as large under 300 sprites, all of one
// sheet, which make one draw call.
func BenchmarkBuildSprites(b *testing.B) { benchBuild(b, 3, 3, 300) }

// BenchmarkBuildSpritesZoomedOut is the same game zoomed out to tiles 1.5
// times as large, under 1,000 sprites: about 7,000 instances.
func BenchmarkBuildSpritesZoomedOut(b *testing.B) { benchBuild(b, 1.5, 3, 1000) }

func benchBuild(b *testing.B, scale float32, layers, sprites int) {
	s := gputest.SpriteScene(1920, 1080, scale, layers, sprites)
	var bld Builder
	image := func(*scene.Image) uintptr { return 1 }
	for b.Loop() {
		bld.Build(s, image)
	}
	b.ReportMetric(float64(len(bld.Instances)), "instances")
	b.ReportMetric(float64(len(bld.Batches)), "batches")
}
