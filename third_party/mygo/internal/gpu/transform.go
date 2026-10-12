package gpu

import (
	"github.com/egoist/mygo/internal/raster"
	"github.com/egoist/mygo/internal/scene"
	"image"
	"slices"
)

type clipMaskSlot struct {
	shapes []scene.ClipShape
	area   image.Rectangle
	image  *scene.Image
}

func (b *Builder) prepare(in *Instance, m scene.Affine, c *clip) bool {
	b.preparedMask = 0
	if c.scissor.Empty() || in.Rect[2] <= 0 || in.Rect[3] <= 0 {
		return false
	}
	if _, ok := m.Inverse(); !ok {
		return false
	}
	in.Transform0, in.Transform1, in.Transform2 = [4]float32{1, 0, 0, 0}, [4]float32{0, 1, 0, 0}, [4]float32{}
	if m.Set {
		in.Transform0 = [4]float32{m.A, m.C, m.X, 1}
		in.Transform1 = [4]float32{m.B, m.D, m.Y, 0}
		in.Transform2 = [4]float32{m.G, m.H, 0, 0}
	}
	in.Clip, in.ClipRadii = rect(c.rect), c.radii
	if !c.masked {
		return true
	}
	// A hard clip sharing the drawing's coordinates is cheap to evaluate in
	// the fragment shader. Keep the rounded ancestor's mask stationary even
	// when a text input or custom clipped drawing moves within it.
	if c.shape.Radii == [4]float32{} && c.shape.Transform == m && m.Set && len(b.stack) > 1 {
		parent := &b.stack[len(b.stack)-2]
		if parent.masked {
			b.preparedMask = b.clipTexture(parent, b.clipShapes[:len(b.clipShapes)-1])
			if b.preparedMask == 0 {
				return false
			}
			in.Clip = rect(c.rect)
			in.ClipRadii = [4]float32{float32(parent.scissor.Left), float32(parent.scissor.Top), float32(parent.scissor.Right - parent.scissor.Left), float32(parent.scissor.Bottom - parent.scissor.Top)}
			in.Transform1[3] = 2
			return true
		}
	}
	b.preparedMask = b.clipTexture(c, b.clipShapes)
	if b.preparedMask == 0 {
		return false
	}
	in.Clip = [4]float32{float32(c.scissor.Left), float32(c.scissor.Top), float32(c.scissor.Right - c.scissor.Left), float32(c.scissor.Bottom - c.scissor.Top)}
	in.ClipRadii = [4]float32{}
	in.Transform1[3] = 1
	return true
}

func (b *Builder) clipTexture(c *clip, shapes []scene.ClipShape) uintptr {
	if c.maskTexture != 0 {
		return c.maskTexture
	}
	for len(b.maskSlots) <= c.maskSlot {
		b.maskSlots = append(b.maskSlots, clipMaskSlot{})
	}
	slot := &b.maskSlots[c.maskSlot]
	area := image.Rect(int(c.scissor.Left), int(c.scissor.Top), int(c.scissor.Right), int(c.scissor.Bottom))
	if slot.image == nil || slot.area != area || !slices.Equal(slot.shapes, shapes) {
		slot.image = raster.ClipMask(slot.image, shapes, area)
		slot.area = area
		slot.shapes = append(slot.shapes[:0], shapes...)
	}
	c.maskTexture = b.imageTexture(slot.image)
	return c.maskTexture
}
