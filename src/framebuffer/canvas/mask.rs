use framebuffer::cgmath::Point2;
use framebuffer::common;
use framebuffer::PixelCanvas;

/// MaskCanvas represents any canvas which can be masked.
/// There is a blanket impl for all PixelCanvases.
pub trait MaskCanvas<'a, C, F>
where
    C: PixelCanvas,
    F: FnMut(Point2<i32>) -> bool,
{
    fn mask(&'a mut self, mask: F) -> MaskedCanvas<'a, C, F>;
}

/// A masked canvas, implements PixelCanvas itself.
pub struct MaskedCanvas<'a, C, F>
where
    C: PixelCanvas,
    F: FnMut(Point2<i32>) -> bool,
{
    source: &'a mut C,
    mask: F,
}

impl<'a, C, F> PixelCanvas for MaskedCanvas<'a, C, F>
where
    C: PixelCanvas,
    F: FnMut(Point2<i32>) -> bool,
{
    /// Writes a single pixel at `pos` with value `v`
    #[inline]
    fn write_pixel(&mut self, pos: Point2<i32>, v: common::color) {
        if (self.mask)(pos) {
            self.source.write_pixel(pos, v)
        }
    }
}

impl<'a, C, F> MaskCanvas<'a, C, F> for C
where
    C: PixelCanvas,
    F: FnMut(Point2<i32>) -> bool,
{
    fn mask(&'a mut self, mask: F) -> MaskedCanvas<'a, C, F> {
        MaskedCanvas { source: self, mask }
    }
}
