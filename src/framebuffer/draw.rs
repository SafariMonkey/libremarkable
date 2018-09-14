use std;

use image::RgbImage;
use libc;
use line_drawing;
use rusttype::{point, Scale};

use framebuffer;
use framebuffer::common::*;
use framebuffer::core;
use framebuffer::vector::*;
use framebuffer::FramebufferIO;

macro_rules! min {
        ($x: expr) => ($x);
        ($x: expr, $($z: expr),+) => (::std::cmp::min($x, min!($($z),*)));
}

macro_rules! max {
        ($x: expr) => ($x);
        ($x: expr, $($z: expr),+) => (::std::cmp::max($x, max!($($z),*)));
}

/// Helper function to sample pixels on the bezier curve.
fn sample_bezier(startpt: Vec2, ctrlpt: Vec2, endpt: Vec2, samples: i32) -> Vec<(f32, Vec2)> {
    let mut points = Vec::new();
    let mut lastpt = (-100, -100);
    for i in 0..samples {
        let t = (i as f32) / (samples-1) as f32;
        let precisept = Vec2 {
            x: (1.0 - t).powf(2.0) * startpt.x
                + 2.0 * (1.0 - t) * t * ctrlpt.x
                + t.powf(2.0) * endpt.x,
            y: (1.0 - t).powf(2.0) * startpt.y
                + 2.0 * (1.0 - t) * t * ctrlpt.y
                + t.powf(2.0) * endpt.y,
        };
        let pt = (precisept.x as i32, precisept.y as i32);
        // prevent oversampling
        if pt != lastpt || i == samples - 1 {
            points.push((t, precisept));
            lastpt = pt;
        }
    }
    points
}

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
        // Create local variables for moving start point
        let mut x0 = x0;
        let mut y0 = y0;

        // Get absolute x/y offset
        let dx = if x0 > x1 { x0 - x1 } else { x1 - x0 };
        let dy = if y0 > y1 { y0 - y1 } else { y1 - y0 };

        // Get slopes
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };

        // Initialize error
        let mut err = if dx > dy { dx } else { -dy } / 2;
        let mut err2;

        let (mut min_x, mut max_x, mut min_y, mut max_y) = (x0, x0, y0, y0);
        loop {
            // Set pixel
            match width {
                1 => self.write_pixel(y0 as usize, x0 as usize, v),
                _ => self.fill_rect(
                    (y0 - (width / 2) as i32) as usize,
                    (x0 - (width / 2) as i32) as usize,
                    width,
                    width,
                    v,
                ),
            }

            max_y = max!(max_y, y0);
            min_y = min!(min_y, y0);
            min_x = min!(min_x, x0);
            max_x = max!(max_x, x0);

            // Check end condition
            if x0 == x1 && y0 == y1 {
                break;
            };

            // Store old error
            err2 = 2 * err;

            // Adjust error and start position
            if err2 > -dx {
                err -= dy;
                x0 += sx;
            }
            if err2 < dy {
                err += dx;
                y0 += sy;
            }
        }

        let margin = ((width + 1) / 2) as i32;
        mxcfb_rect {
            top: (min_y - margin) as u32,
            left: (min_x - margin) as u32,
            width: (max_x - min_x + margin * 2) as u32,
            height: (max_y - min_y + margin * 2) as u32,
        }
    }

    fn draw_polygon(&mut self, points: Vec<IntVec2>, fill: bool, c: color) -> mxcfb_rect {
        // This implementation of polygon rasterisation is based on this article:
        // https://hackernoon.com/computer-graphics-scan-line-polygon-fill-algorithm-3cb47283df6

        // struct to hold edge data
        #[derive(Debug, Copy, Clone)]
        struct EdgeBucket {
            ymax: i32,
            ymin: i32,
            x: i32,
            sign: i32,
            direction: i32,
            dx: i32,
            dy: i32,
            sum: i32,
        };

        // initialise our edge table
        let mut edge_table = Vec::new();
        let num_edges = points.len();
        for i in 0..num_edges {
            let p0 = points[i];
            let p1 = points[(i + 1) % num_edges];
            let (lower, higher, direction) = if p0.y < p1.y {
                (p0, p1, 1)
            } else {
                (p1, p0, -1)
            };
            edge_table.push(EdgeBucket {
                ymax: higher.y,
                ymin: lower.y,
                x: lower.x,
                sign: if lower.x > higher.x { 1 } else { -1 },
                direction: direction,
                dx: (higher.x - lower.x).abs(),
                dy: (higher.y - lower.y).abs(),
                sum: 0,
            });
        }
        // sort the edge table by ymin
        edge_table.sort_unstable_by_key(|p| p.ymin);

        // create active list
        let mut active_list = Vec::<EdgeBucket>::new();

        // initialise scanline with lowest ymin
        let mut scanline = edge_table[0].clone().ymin;

        while edge_table.len() > 0 {
            // remove edges that end on the current scanline
            edge_table.retain(|edge| if edge.ymax == scanline { false } else { true });
            active_list.retain(|edge| if edge.ymax == scanline { false } else { true });

            // push edges that start on this scanline to the active list
            for edge in edge_table.iter() {
                if edge.ymin == scanline {
                    active_list.push(edge.clone());
                }
            }

            // sort active list by ymin, ascending
            active_list.sort_unstable_by_key(|p| p.x);

            // for every pair of edges on the active list,
            // apply the fill method selected
            if fill {
                let mut prev_x = 0;
                let mut winding_count = 0;
                for edge in active_list.iter() {
                    if winding_count != 0 {
                        for x in prev_x..edge.x {
                            self.write_pixel(scanline as usize, x as usize, c);
                        }
                    }
                    prev_x = edge.x;
                    winding_count += edge.direction;
                }
            } else {
            for pair in active_list.chunks(2) {
                if pair.len() != 2 {
                    continue;
                }
                    if pair[0].x != pair[1].x {
                        self.write_pixel(scanline as usize, pair[0].x as usize, c);
                        self.write_pixel(scanline as usize, pair[1].x as usize - 1, c);
                    }
                }
            }

            // increment scanline
            scanline += 1;

            // adjust the x of each edge based on its gradient
            for edge in &mut active_list {
                if edge.dx != 0 {
                    edge.sum += edge.dx;
                }
                while edge.sum >= edge.dy {
                    edge.x -= edge.sign;
                    edge.sum -= edge.dy;
                }
            }
        }

        // calculate bounding box
        let (min_xy, max_xy) = points.iter().fold(
            (
                IntVec2 {
                    y: std::i32::MAX,
                    x: std::i32::MAX,
                },
                IntVec2 {
                    y: std::i32::MIN,
                    x: std::i32::MIN,
                },
            ),
            |acc, p| {
                (
                    IntVec2 {
                        y: min!(acc.0.y, p.y),
                        x: min!(acc.0.x, p.x),
                    },
                    IntVec2 {
                        y: max!(acc.1.y, p.y),
                        x: max!(acc.1.x, p.x),
                    },
                )
            },
        );
        mxcfb_rect {
            top: min_xy.y as u32,
            left: min_xy.x as u32,
            width: (max_xy.x - min_xy.x) as u32,
            height: (max_xy.y - min_xy.y) as u32,
        }
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
        for current in { 1..rad + 1 } {
            for (x, y) in line_drawing::BresenhamCircle::new(x as i32, y as i32, current as i32) {
                self.write_pixel(y as usize, x as usize, v);
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
        let mut left_edge = Vec::<IntVec2>::new();
        let mut right_edge = Vec::<IntVec2>::new();
        for (t, pt) in sample_bezier(startpt.0, ctrlpt.0, endpt.0, samples) {
            // interpolate width
            let width = 2.0 * if t < 0.5 {
                startpt.1 * (0.5-t) + ctrlpt.1 * t
            } else {
                ctrlpt.1 * (1.0-t) + endpt.1 * (t-0.5)
            };

            // calculate tangent
            let velocity =
                2.0 * (1.0 - t) * (ctrlpt.0 - startpt.0) + 2.0 * t * (endpt.0 - ctrlpt.0);
            let speed = velocity.length();
            let tangent = if speed > 0.0 {
                velocity / speed
            } else {
                // handle case where control point == start/end point
                let extent = startpt.0 - endpt.0;
                if extent.length() > 0.0 {
                    extent / extent.length()
                } else {
                    // all points are the same, so no tangent exists
                    Vec2 { x: 0.0, y: 0.0 }
                }
            };

            left_edge.push(IntVec2::from(
                (pt + Vec2 {
                    x: -tangent.y * width / 2.0,
                    y: tangent.x * width / 2.0,
                }).round(),
            ));
            right_edge.push(IntVec2::from(
                (pt + Vec2 {
                    x: tangent.y * width / 2.0,
                    y: -tangent.x * width / 2.0,
                }).round(),
            ));
        }
        right_edge.reverse();
        left_edge.append(&mut right_edge);
        self.draw_polygon(left_edge, true, v)
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
