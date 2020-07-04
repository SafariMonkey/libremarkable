use image::RgbImage;
use rusttype::{point, Font, Scale};

use crate::framebuffer;
use crate::framebuffer::cgmath::*;
use crate::framebuffer::common::*;
use crate::framebuffer::core::Framebuffer;
use crate::framebuffer::graphics;
use crate::framebuffer::FramebufferIO;

pub trait DefaultFont<'a> {
    fn get_default_font<'temp>(&'temp self) -> &'temp Font<'a>
    where
        'a: 'temp;
}

impl<'a> DefaultFont<'a> for Framebuffer<'a> {
    fn get_default_font<'temp>(&'temp self) -> &'temp Font<'a>
    where
        'a: 'temp,
    {
        &self.default_font
    }
}

impl<'a, T: FramebufferIO + DefaultFont<'a>> framebuffer::FramebufferDraw for T {
    fn draw_image(&mut self, img: &RgbImage, pos: Point2<i32>) -> mxcfb_rect {
        for (x, y, pixel) in img.enumerate_pixels() {
            let pixel_pos = pos + vec2(x as i32, y as i32);
            self.write_pixel(
                pixel_pos.cast().unwrap(),
                color::RGB(pixel.data[0], pixel.data[1], pixel.data[2]),
            );
        }
        mxcfb_rect {
            top: pos.y as u32,
            left: pos.x as u32,
            width: img.width(),
            height: img.height(),
        }
    }

    fn draw_line(
        &mut self,
        start: Point2<i32>,
        end: Point2<i32>,
        width: u32,
        v: color,
    ) -> mxcfb_rect {
        let stamp = &mut |p| match width {
            1 => self.write_pixel(p, v),
            _ => self.fill_rect(
                p - Vector2::<i32> {
                    x: width as i32 / 2,
                    y: width as i32 / 2,
                },
                Vector2 { x: width, y: width },
                v,
            ),
        };
        let margin = ((width + 1) / 2) as u32;
        graphics::stamp_along_line(stamp, start, end).expand(margin)
    }

    fn draw_polygon(&mut self, points: &[cgmath::Point2<i32>], fill: bool, c: color) -> mxcfb_rect {
        if fill {
            graphics::fill_polygon(&mut |p| self.write_pixel(p, c), points)
        } else {
            let num_edges = points.len();
            let mut rect = mxcfb_rect::invalid();
            for i in 0..num_edges {
                let p0 = points[i];
                let p1 = points[(i + 1) % num_edges];
                rect = rect.merge_rect(&self.draw_line(p0, p1, 1, c));
            }
            rect
        }
    }

    fn draw_circle(&mut self, pos: cgmath::Point2<i32>, rad: u32, v: color) -> mxcfb_rect {
        for (x, y) in line_drawing::BresenhamCircle::new(pos.x, pos.y, rad as i32) {
            self.write_pixel(Point2 { x, y }, v);
        }
        mxcfb_rect {
            top: pos.y as u32 - rad as u32,
            left: pos.x as u32 - rad as u32,
            width: 2 * rad as u32,
            height: 2 * rad as u32,
        }
    }

    fn fill_circle(&mut self, pos: cgmath::Point2<i32>, rad: u32, v: color) -> mxcfb_rect {
        let rad_square = (rad * rad) as i32;
        let search_distance: i32 = (rad + 1) as i32;
        for y in (-search_distance)..search_distance {
            let y_square = y * y;
            for x in (-search_distance)..search_distance {
                let x_square = x * x;
                if x_square + y_square <= rad_square {
                    self.write_pixel(pos + Vector2 { x, y }, v);
                }
            }
        }
        mxcfb_rect {
            top: pos.y as u32 - rad as u32,
            left: pos.x as u32 - rad as u32,
            width: 2 * rad as u32,
            height: 2 * rad as u32,
        }
    }

    fn draw_bezier(
        &mut self,
        startpt: Point2<f32>,
        ctrlpt: Point2<f32>,
        endpt: Point2<f32>,
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
        startpt: (Point2<f32>, f32),
        ctrlpt: (Point2<f32>, f32),
        endpt: (Point2<f32>, f32),
        samples: i32,
        v: color,
    ) -> mxcfb_rect {
        graphics::draw_dynamic_bezier(
            &mut |p| self.write_pixel(p, v),
            startpt,
            ctrlpt,
            endpt,
            samples,
        )
    }

    fn draw_text(
        &mut self,
        pos: Point2<f32>,
        text: String,
        size: f32,
        col: color,
        dryrun: bool,
    ) -> mxcfb_rect {
        let scale = Scale {
            x: size as f32,
            y: size as f32,
        };

        // The starting positioning of the glyphs (top left corner)
        let start = point(pos.x, pos.y);

        let dfont = self.get_default_font().clone();

        let mut min_y = pos.y.floor().max(0.0) as u32;
        let mut max_y = pos.y.ceil().max(0.0) as u32;
        let mut min_x = pos.x.floor().max(0.0) as u32;
        let mut max_x = pos.x.ceil().max(0.0) as u32;

        let components = col.to_rgb8();
        let c1 = f32::from(255 - components[0]);
        let c2 = f32::from(255 - components[1]);
        let c3 = f32::from(255 - components[2]);

        // Loop through the glyphs in the text, positing each one on a line
        for glyph in dfont.layout(&text, scale, start) {
            if let Some(bounding_box) = glyph.pixel_bounding_box() {
                // Draw the glyph into the image per-pixel by using the draw closure
                let bbmax_y = bounding_box.max.y as u32;
                let bbmax_x = bounding_box.max.x as u32;
                let bbmin_y = bounding_box.min.y as u32;
                let bbmin_x = bounding_box.min.x as u32;
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
                        Point2 {
                            x: (x + bounding_box.min.x as u32) as i32,
                            y: (y + bounding_box.min.y as u32) as i32,
                        },
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

    fn draw_rect(&mut self, pos: Point2<i32>, size: Vector2<u32>, border_px: u32, c: color) {
        let top_left = pos;
        let top_right = pos + vec2(size.x as i32, 0);
        let bottom_left = pos + vec2(0, size.y as i32);
        let bottom_right = pos + size.cast().unwrap();

        // top horizontal
        self.draw_line(top_left, top_right, border_px, c);

        // left vertical
        self.draw_line(top_left, bottom_left, border_px, c);

        // bottom horizontal
        self.draw_line(top_right, bottom_right, border_px, c);

        // right vertical
        self.draw_line(bottom_left, bottom_right, border_px, c);
    }

    fn fill_rect(&mut self, pos: Point2<i32>, size: Vector2<u32>, c: color) {
        for ypos in pos.y..pos.y + size.y as i32 {
            for xpos in pos.x..pos.x + size.x as i32 {
                self.write_pixel(
                    Point2 {
                        x: xpos as i32,
                        y: ypos as i32,
                    },
                    c,
                );
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn __assert_framebuffer_impls_framebuffer_draw()
    where
        Framebuffer<'static>: framebuffer::FramebufferDraw,
    {
    }
}
