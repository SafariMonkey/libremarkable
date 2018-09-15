use std;

use image::RgbImage;
use libc;
use line_drawing;
use rusttype::{point, Scale};

use framebuffer;
use framebuffer::common::*;
use framebuffer::core;
use framebuffer::graphics;
use framebuffer::vector::*;
use framebuffer::FramebufferIO;

impl<'a> framebuffer::FramebufferDraw for core::Framebuffer<'a> {
    fn draw_image(&mut self, img: &RgbImage, top: usize, left: usize) -> mxcfb_rect {
        for (x, y, pixel) in img.enumerate_pixels() {
            self.write_pixel(
                top + y as usize,
                left + x as usize,
                color::RGB(pixel.data[0], pixel.data[1], pixel.data[2]),
            );
        }
        mxcfb_rect {
            top: top as u32,
            left: left as u32,
            width: img.width(),
            height: img.height(),
        }
    }

    fn draw_line(
        &mut self,
        y0: i32,
        x0: i32,
        y1: i32,
        x1: i32,
        width: usize,
        v: color,
    ) -> mxcfb_rect {
        let stamp = &mut |x, y| match width {
            1 => self.write_pixel(y as usize, x as usize, v),
            _ => self.fill_rect(
                (y - (width / 2) as i32) as usize,
                (x - (width / 2) as i32) as usize,
                width,
                width,
                v,
            ),
        };
        let margin = ((width + 1) / 2) as u32;
        graphics::stamp_along_line(stamp, y0, x0, y1, x1).expand(margin)
    }

    fn draw_polygon(&mut self, points: Vec<IntVec2>, fill: bool, c: color) -> mxcfb_rect {
        graphics::draw_polygon(&mut |x, y| self.write_pixel(y, x, c), points, fill)
    }

    fn draw_circle(&mut self, y: usize, x: usize, rad: usize, v: color) -> mxcfb_rect {
        for (x, y) in line_drawing::BresenhamCircle::new(x as i32, y as i32, rad as i32) {
            self.write_pixel(y as usize, x as usize, v);
        }
        mxcfb_rect {
            top: y as u32 - rad as u32,
            left: x as u32 - rad as u32,
            width: 2 * rad as u32,
            height: 2 * rad as u32,
        }
    }

    fn fill_circle(&mut self, y: usize, x: usize, rad: usize, v: color) -> mxcfb_rect {
        let rad_square = (rad * rad) as i32;
        let search_distance: i32 = (rad + 1) as i32;
        for y_offset in { (-search_distance)..search_distance } {
            let y_square = y_offset * y_offset;
            for x_offset in (-search_distance)..search_distance {
                let x_square = x_offset * x_offset;
                if x_square + y_square <= rad_square {
                    self.write_pixel(y + y_offset as usize, x + x_offset as usize, v);
                }
            }
        }
        mxcfb_rect {
            top: y as u32 - rad as u32,
            left: x as u32 - rad as u32,
            width: 2 * rad as u32,
            height: 2 * rad as u32,
        }
    }

    fn draw_bezier(
        &mut self,
        startpt: Vec2,
        ctrlpt: Vec2,
        endpt: Vec2,
        width: f32,
        samples: i32,
        v: color,
    ) -> mxcfb_rect {
        self.draw_dynamic_bezier(
            (startpt, width),
            (ctrlpt, width),
            (endpt, width),
            samples,
            v,
        )
    }

    fn draw_dynamic_bezier(
        &mut self,
        startpt: (Vec2, f32),
        ctrlpt: (Vec2, f32),
        endpt: (Vec2, f32),
        samples: i32,
        v: color,
    ) -> mxcfb_rect {
        graphics::draw_dynamic_bezier(
            &mut |x, y| self.write_pixel(y, x, v),
            startpt,
            ctrlpt,
            endpt,
            samples,
        )
    }

    fn draw_text(
        &mut self,
        y: usize,
        x: usize,
        text: String,
        size: usize,
        col: color,
        dryrun: bool,
    ) -> mxcfb_rect {
        let scale = Scale {
            x: size as f32,
            y: size as f32,
        };

        // The starting positioning of the glyphs (top left corner)
        let start = point(x as f32, y as f32);

        let dfont = &mut self.default_font.clone();

        let mut min_y = y;
        let mut max_y = y;
        let mut min_x = x;
        let mut max_x = x;

        let components = col.to_rgb8();
        let c1 = f32::from(255 - components[0]);
        let c2 = f32::from(255 - components[1]);
        let c3 = f32::from(255 - components[2]);

        // Loop through the glyphs in the text, positing each one on a line
        for glyph in dfont.layout(&text, scale, start) {
            if let Some(bounding_box) = glyph.pixel_bounding_box() {
                // Draw the glyph into the image per-pixel by using the draw closure
                let bbmax_y = bounding_box.max.y as usize;
                let bbmax_x = bounding_box.max.x as usize;
                let bbmin_y = bounding_box.min.y as usize;
                let bbmin_x = bounding_box.min.x as usize;
                if bbmax_y > max_y {
                    max_y = bbmax_y;
                }
                if bbmax_x > max_x {
                    max_x = bbmax_x;
                }
                if bbmin_y < min_y {
                    min_y = bbmin_y;
                }
                if bbmin_x < min_x {
                    min_x = bbmin_x;
                }

                if dryrun {
                    continue;
                }

                glyph.draw(|x, y, v| {
                    let mult = (1.0 - v).min(1.0);
                    self.write_pixel(
                        (y + bounding_box.min.y as u32) as usize,
                        (x + bounding_box.min.x as u32) as usize,
                        color::RGB((c1 * mult) as u8, (c2 * mult) as u8, (c3 * mult) as u8),
                    )
                });
            }
        }
        // return the height and width of the drawn text so that refresh can be called on it
        mxcfb_rect {
            top: min_y as u32,
            left: min_x as u32,
            height: (max_y - min_y) as u32,
            width: (max_x - min_x) as u32,
        }
    }

    fn draw_rect(
        &mut self,
        y: usize,
        x: usize,
        height: usize,
        width: usize,
        border_px: usize,
        c: color,
    ) {
        // top horizontal
        self.draw_line(
            y as i32,
            x as i32,
            y as i32,
            (x + width) as i32,
            border_px,
            c,
        );

        // left vertical
        self.draw_line(
            y as i32,
            x as i32,
            (y + height) as i32,
            x as i32,
            border_px,
            c,
        );

        // bottom horizontal
        self.draw_line(
            (y + height) as i32,
            x as i32,
            (y + height) as i32,
            (x + width) as i32,
            border_px,
            c,
        );

        // right vertical
        self.draw_line(
            y as i32,
            (x + width) as i32,
            (y + height) as i32,
            (x + width) as i32,
            border_px,
            c,
        );
    }

    fn fill_rect(&mut self, y: usize, x: usize, height: usize, width: usize, c: color) {
        for ypos in y..y + height {
            for xpos in x..x + width {
                self.write_pixel(ypos, xpos, c);
            }
        }
    }

    fn clear(&mut self) {
        let h = self.var_screen_info.yres as usize;
        let line_length = self.fix_screen_info.line_length as usize;
        unsafe {
            libc::memset(
                self.frame.data() as *mut libc::c_void,
                std::i32::MAX,
                line_length * h,
            );
        }
    }
}
