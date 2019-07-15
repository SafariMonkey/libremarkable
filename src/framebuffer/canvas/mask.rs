use framebuffer::cgmath::Point2;
use framebuffer::common;
use framebuffer::{PixelCanvas, Region};

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

/// Implement Region for MaskedCanvas<C>
/// if the C implements it
impl<'a, C, F> Region for MaskedCanvas<'a, C, F>
where
    C: PixelCanvas + Region,
    F: FnMut(Point2<i32>) -> bool,
{
    fn get_region(&self) -> common::mxcfb_rect {
        self.source.get_region()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use framebuffer::cgmath::Vector2;
    use framebuffer::FramebufferDraw;

    struct Mock<'a> {
        pixel_writes: &'a mut Vec<Point2<i32>>,
    }
    impl<'a> Mock<'a> {
        fn write_pixel(&mut self, point: Point2<i32>) {
            self.pixel_writes.push(point)
        }
        fn clear(&mut self) {
            self.pixel_writes.clear()
        }
    }

    impl<'a> PixelCanvas for Mock<'a> {
        fn write_pixel(&mut self, point: Point2<i32>, _col: common::color) {
            self.write_pixel(point)
        }
    }

    #[test]
    fn test_draw_bool_mask() {
        let mut mock = Mock {
            pixel_writes: &mut Vec::new(),
        };
        mock.mask(|_| true)
            .write_pixel(Point2 { x: 100, y: 100 }, common::color::BLACK);
        assert_eq!(mock.pixel_writes, &vec![Point2 { x: 100, y: 100 }]);

        mock.clear();
        mock.mask(|_| false)
            .write_pixel(Point2 { x: 100, y: 100 }, common::color::BLACK);
        assert_eq!(mock.pixel_writes, &vec![]);
    }

    #[test]
    fn test_draw_checker_mask() {
        let mut mock = Mock {
            pixel_writes: &mut Vec::new(),
        };
        mock.mask(|Point2 { x, y }| (x % 2 < 1) ^ (y % 2 < 1))
            .fill_rect(
                Point2 { x: 100, y: 100 },
                Vector2 { x: 2, y: 2 },
                common::color::BLACK,
            );
        assert_eq!(
            mock.pixel_writes,
            &vec![Point2 { x: 101, y: 100 }, Point2 { x: 100, y: 101 }]
        );
    }
}
