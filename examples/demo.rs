#![feature(nll)]
#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate log;
extern crate env_logger;

#[macro_use]
extern crate libremarkable;
use libremarkable::framebuffer::common::*;
use libremarkable::framebuffer::refresh::PartialRefreshMode;
use libremarkable::framebuffer::storage;
use libremarkable::framebuffer::vector::*;
use libremarkable::framebuffer::{FramebufferDraw, FramebufferIO, FramebufferRefresh};
use libremarkable::image::GenericImage;
use libremarkable::input::{gpio, multitouch, wacom, InputDevice};
use libremarkable::ui_extensions::element::{
    UIConstraintRefresh, UIElement, UIElementHandle, UIElementWrapper,
};
use libremarkable::{appctx, battery, image};

#[cfg(feature = "enable-runtime-benchmarking")]
use libremarkable::stopwatch;

extern crate chrono;
use chrono::{DateTime, Local};

extern crate atomic;
use atomic::Atomic;

use std::collections::VecDeque;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread::sleep;
use std::time::Duration;

#[derive(Copy, Clone, PartialEq)]
enum DrawMode {
    Draw(usize),
    PolyDraw(usize),
    Erase(usize),
}
impl DrawMode {
    fn set_size(self, new_size: usize) -> Self {
        match self {
            DrawMode::Draw(_) => DrawMode::Draw(new_size),
            DrawMode::PolyDraw(_) => DrawMode::PolyDraw(new_size),
            DrawMode::Erase(_) => DrawMode::Erase(new_size),
        }
    }
    fn color_as_string(self) -> String {
        match self {
            DrawMode::Draw(_) => "Black",
            DrawMode::PolyDraw(_) => "Black (New)",
            DrawMode::Erase(_) => "White",
        }.into()
    }
    fn get_size(self) -> usize {
        match self {
            DrawMode::Draw(s) => s,
            DrawMode::PolyDraw(s) => s,
            DrawMode::Erase(s) => s,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
enum TouchMode {
    OnlyUI,
    Bezier,
    Circles,
    Diamonds,
    FillDiamonds,
}
impl TouchMode {
    fn toggle(self) -> Self {
        match self {
            TouchMode::OnlyUI => TouchMode::Bezier,
            TouchMode::Bezier => TouchMode::Circles,
            TouchMode::Circles => TouchMode::Diamonds,
            TouchMode::Diamonds => TouchMode::FillDiamonds,
            TouchMode::FillDiamonds => TouchMode::OnlyUI,
        }
    }
    fn to_string(self) -> String {
        match self {
            TouchMode::OnlyUI => "None",
            TouchMode::Bezier => "Bezier",
            TouchMode::Circles => "Circles",
            TouchMode::Diamonds => "Diamonds",
            TouchMode::FillDiamonds => "⬛Diamonds",
        }.into()
    }
}

// This region will have the following size at rest:
//   raw: 5896 kB
//   zstd: 10 kB
const CANVAS_REGION: mxcfb_rect = mxcfb_rect {
    top: 720,
    left: 0,
    height: 1080,
    width: 1404,
};

lazy_static! {
    static ref G_TOUCH_MODE: Atomic<TouchMode> = Atomic::new(TouchMode::OnlyUI);
    static ref G_DRAW_MODE: Atomic<DrawMode> = Atomic::new(DrawMode::Draw(2));
    static ref UNPRESS_OBSERVED: AtomicBool = AtomicBool::new(false);
    static ref WACOM_IN_RANGE: AtomicBool = AtomicBool::new(false);
    static ref WACOM_HISTORY: Mutex<VecDeque<(Vec2, i32)>> = Mutex::new(VecDeque::new());
    static ref G_COUNTER: Mutex<u32> = Mutex::new(0);
    static ref LAST_REFRESHED_CANVAS_RECT: Atomic<mxcfb_rect> = Atomic::new(mxcfb_rect::invalid());
    static ref SAVED_CANVAS: Mutex<Option<storage::CompressedCanvasState>> = Mutex::new(None);
}

// ####################
// ## Button Handlers
// ####################

fn on_save_canvas(app: &mut appctx::ApplicationContext, _element: UIElementHandle) {
    start_bench!(stopwatch, save_canvas);
    let framebuffer = app.get_framebuffer_ref();
    match framebuffer.dump_region(CANVAS_REGION) {
        Err(err) => println!("Failed to dump buffer: {0}", err),
        Ok(buff) => {
            let mut hist = SAVED_CANVAS.lock().unwrap();
            *hist = Some(storage::CompressedCanvasState::new(
                buff.as_slice(),
                CANVAS_REGION.height,
                CANVAS_REGION.width,
            ));
        }
    };
    end_bench!(save_canvas);
}

fn on_zoom_out(app: &mut appctx::ApplicationContext, _element: UIElementHandle) {
    start_bench!(stopwatch, zoom_out);
    let framebuffer = app.get_framebuffer_ref();
    match framebuffer.dump_region(CANVAS_REGION) {
        Err(err) => println!("Failed to dump buffer: {0}", err),
        Ok(buff) => {
            let resized = image::DynamicImage::ImageRgb8(
                storage::rgbimage_from_u8_slice(
                    CANVAS_REGION.width,
                    CANVAS_REGION.height,
                    buff.as_slice(),
                ).unwrap(),
            ).resize(
                (CANVAS_REGION.width as f32 / 1.25f32) as u32,
                (CANVAS_REGION.height as f32 / 1.25f32) as u32,
                image::imageops::Nearest,
            );

            // Get a clean image the size of the canvas
            let mut new_image =
                image::DynamicImage::new_rgb8(CANVAS_REGION.width, CANVAS_REGION.height);
            new_image.invert();

            // Copy the resized image into the subimage
            new_image.copy_from(&resized, CANVAS_REGION.width / 8, CANVAS_REGION.height / 8);

            framebuffer.draw_image(
                &new_image.as_rgb8().unwrap(),
                CANVAS_REGION.top as usize,
                CANVAS_REGION.left as usize,
            );
            framebuffer.partial_refresh(
                &CANVAS_REGION,
                PartialRefreshMode::Async,
                waveform_mode::WAVEFORM_MODE_GC16_FAST,
                display_temp::TEMP_USE_REMARKABLE_DRAW,
                dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH,
                0,
                false,
            );
        }
    };
    end_bench!(zoom_out);
}

fn on_blur_canvas(app: &mut appctx::ApplicationContext, _element: UIElementHandle) {
    start_bench!(stopwatch, blur_canvas);
    let framebuffer = app.get_framebuffer_ref();
    match framebuffer.dump_region(CANVAS_REGION) {
        Err(err) => println!("Failed to dump buffer: {0}", err),
        Ok(buff) => {
            let dynamic = image::DynamicImage::ImageRgb8(
                storage::rgbimage_from_u8_slice(
                    CANVAS_REGION.width,
                    CANVAS_REGION.height,
                    buff.as_slice(),
                ).unwrap(),
            ).blur(0.6f32);

            framebuffer.draw_image(
                &dynamic.as_rgb8().unwrap(),
                CANVAS_REGION.top as usize,
                CANVAS_REGION.left as usize,
            );
            framebuffer.partial_refresh(
                &CANVAS_REGION,
                PartialRefreshMode::Async,
                waveform_mode::WAVEFORM_MODE_GC16_FAST,
                display_temp::TEMP_USE_REMARKABLE_DRAW,
                dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH,
                0,
                false,
            );
        }
    };
    end_bench!(blur_canvas);
}

fn on_invert_canvas(app: &mut appctx::ApplicationContext, element: UIElementHandle) {
    start_bench!(stopwatch, invert);
    let framebuffer = app.get_framebuffer_ref();
    match framebuffer.dump_region(CANVAS_REGION) {
        Err(err) => println!("Failed to dump buffer: {0}", err),
        Ok(mut buff) => {
            buff.iter_mut().for_each(|p| {
                *p = !(*p);
            });
            match framebuffer.restore_region(CANVAS_REGION, &buff) {
                Err(e) => println!("Error while restoring region: {0}", e),
                Ok(_) => {
                    framebuffer.partial_refresh(
                        &CANVAS_REGION,
                        PartialRefreshMode::Async,
                        waveform_mode::WAVEFORM_MODE_GC16_FAST,
                        display_temp::TEMP_USE_REMARKABLE_DRAW,
                        dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH,
                        0,
                        false,
                    );
                }
            };
        }
    };
    end_bench!(invert);

    // Invert the draw color as well for more natural UX
    on_toggle_eraser(app, element);
}

fn on_load_canvas(app: &mut appctx::ApplicationContext, _element: UIElementHandle) {
    start_bench!(stopwatch, load_canvas);
    match *SAVED_CANVAS.lock().unwrap() {
        None => {}
        Some(ref compressed_state) => {
            let framebuffer = app.get_framebuffer_ref();
            let decompressed = compressed_state.decompress();

            match framebuffer.restore_region(CANVAS_REGION, &decompressed) {
                Err(e) => println!("Error while restoring region: {0}", e),
                Ok(_) => {
                    framebuffer.partial_refresh(
                        &CANVAS_REGION,
                        PartialRefreshMode::Async,
                        waveform_mode::WAVEFORM_MODE_GC16_FAST,
                        display_temp::TEMP_USE_REMARKABLE_DRAW,
                        dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH,
                        0,
                        false,
                    );
                }
            };
        }
    };
    end_bench!(load_canvas);
}

fn on_touch_rustlogo(app: &mut appctx::ApplicationContext, _element: UIElementHandle) {
    let framebuffer = app.get_framebuffer_ref();
    let new_press_count = {
        let mut v = G_COUNTER.lock().unwrap();
        *v += 1;
        (*v).clone()
    };

    // First drawing with GC16_FAST to draw it thoroughly and then
    // alternating between DU which has more artifacts but is faster.
    let waveform = if new_press_count % 2 == 0 {
        waveform_mode::WAVEFORM_MODE_DU
    } else {
        waveform_mode::WAVEFORM_MODE_GC16_FAST
    };

    let rect = framebuffer.draw_text(
        240,
        1140,
        format!("{0}", new_press_count),
        65,
        color::BLACK,
        false,
    );
    framebuffer.partial_refresh(
        &rect,
        PartialRefreshMode::Wait,
        waveform,
        display_temp::TEMP_USE_MAX,
        dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH,
        0,
        false,
    );
}

fn on_toggle_eraser(app: &mut appctx::ApplicationContext, _: UIElementHandle) {
    let (new_mode, name) = match G_DRAW_MODE.load(Ordering::Relaxed) {
        DrawMode::Erase(s) => (DrawMode::Draw(s), "Black".to_owned()),
        DrawMode::Draw(s) => (DrawMode::PolyDraw(s), "Black (New)".to_owned()),
        DrawMode::PolyDraw(s) => (DrawMode::Erase(s), "White".to_owned()),
    };
    G_DRAW_MODE.store(new_mode, Ordering::Relaxed);

    let indicator = app.get_element_by_name("colorIndicator");
    if let UIElement::Text { ref mut text, .. } = indicator.unwrap().write().inner {
        *text = name;
    }
    app.draw_element("colorIndicator");
}

fn on_change_touchdraw_mode(app: &mut appctx::ApplicationContext, _: UIElementHandle) {
    let new_val = G_TOUCH_MODE.load(Ordering::Relaxed).toggle();
    G_TOUCH_MODE.store(new_val, Ordering::Relaxed);

    let indicator = app.get_element_by_name("touchModeIndicator");
    if let UIElement::Text { ref mut text, .. } = indicator.unwrap().write().inner {
        *text = new_val.to_string();
    }
    // Make sure you aren't trying to draw the element while you are holding a write lock.
    // It doesn't seem to cause a deadlock however it may cause higher lock contention.
    app.draw_element("touchModeIndicator");
}

// ####################
// ## Miscellaneous
// ####################

fn draw_color_test_rgb(app: &mut appctx::ApplicationContext, _element: UIElementHandle) {
    let fb = app.get_framebuffer_ref();

    let img_rgb565 = image::load_from_memory(include_bytes!("../assets/colorspace.png")).unwrap();
    fb.draw_image(
        &img_rgb565.as_rgb8().unwrap(),
        CANVAS_REGION.top as usize,
        CANVAS_REGION.left as usize,
    );
    fb.partial_refresh(
        &CANVAS_REGION,
        PartialRefreshMode::Wait,
        waveform_mode::WAVEFORM_MODE_GC16,
        display_temp::TEMP_USE_PAPYRUS,
        dither_mode::EPDC_FLAG_USE_DITHERING_PASSTHROUGH,
        0,
        false,
    );
}

fn change_brush_width(app: &mut appctx::ApplicationContext, delta: isize) {
    let current = G_DRAW_MODE.load(Ordering::Relaxed);
    let new_size = current.get_size() as isize + delta;
    if new_size < 1 || new_size > 99 {
        return;
    }

    G_DRAW_MODE.store(current.set_size(new_size as usize), Ordering::Relaxed);

    let element = app.get_element_by_name("displaySize").unwrap();
    if let UIElement::Text { ref mut text, .. } = element.write().inner {
        *text = format!("size: {0}", new_size);
    }
    app.draw_element("displaySize");
}

fn loop_update_topbar(app: &mut appctx::ApplicationContext, millis: u64) {
    let time_label = app.get_element_by_name("time").unwrap();
    let battery_label = app.get_element_by_name("battery").unwrap();
    loop {
        // Get the datetime
        let dt: DateTime<Local> = Local::now();

        if let UIElement::Text { ref mut text, .. } = time_label.write().inner {
            *text = format!("{}", dt.format("%F %r"));
        }

        if let UIElement::Text { ref mut text, .. } = battery_label.write().inner {
            *text = format!(
                "{0:<128}",
                format!(
                    "{0} — {1}%",
                    battery::human_readable_charging_status().unwrap(),
                    battery::percentage().unwrap()
                )
            );
        }
        app.draw_element("time");
        app.draw_element("battery");
        sleep(Duration::from_millis(millis));
    }
}

// ####################
// ## Input Handlers
// ####################

fn on_wacom_input(app: &mut appctx::ApplicationContext, input: wacom::WacomEvent) {
    match input {
        wacom::WacomEvent::Draw {
            y,
            x,
            pressure,
            tilt_x: _,
            tilt_y: _,
        } => {
            let mut wacom_stack = WACOM_HISTORY.lock().unwrap();

            // This is so that we can click the buttons outside the canvas region
            // normally meant to be touched with a finger using our stylus
            if !CANVAS_REGION.contains_point(y as u32, x as u32) {
                wacom_stack.clear();
                if UNPRESS_OBSERVED.fetch_and(false, Ordering::Relaxed) {
                    match app.find_active_region(y as u16, x as u16) {
                        Some((region, _)) => (region.handler)(app, region.element.clone()),
                        None => {}
                    };
                }
                return;
            }

            let (col, mult, use_poly) = match G_DRAW_MODE.load(Ordering::Relaxed) {
                DrawMode::Draw(s) => (color::BLACK, s, false),
                DrawMode::PolyDraw(s) => (color::BLACK, s, true),
                DrawMode::Erase(s) => (color::WHITE, s * 3, false),
            };

            wacom_stack.push_back((Vec2 { x: x, y: y }, pressure as i32));

            drop(x);
            drop(y);
            drop(pressure);

            while wacom_stack.len() >= 3 {
                let framebuffer = app.get_framebuffer_ref();
                let rect = if use_poly {
                    let points = vec![
                        wacom_stack.pop_front().unwrap(),
                        wacom_stack.get(0).unwrap().clone(),
                        wacom_stack.get(1).unwrap().clone(),
                    ];
                    let radii: Vec<f32> = points
                        .iter()
                        .map(|point| ((mult as f32 * (point.1 as f32) / 2048.) / 2.0))
                        .collect();
                    // calculate control points
                    let start_point = (points[2].0 + points[1].0) / 2.0;
                    let ctrl_point = points[1].0;
                    let end_point = (points[1].0 + points[0].0) / 2.0;
                    // calculate radii
                    let start_width = (radii[2] + radii[1]) / 2.0;
                    let ctrl_width = radii[1];
                    let end_width = (radii[1] + radii[0]) / 2.0;
                    framebuffer.draw_dynamic_bezier(
                        (start_point, start_width),
                        (ctrl_point, ctrl_width),
                        (end_point, end_width),
                        10,
                        col,
                    )
                } else {
                    let beginpt = wacom_stack.pop_front().unwrap();
                    let controlpt = wacom_stack.pop_front().unwrap();
                    let endpt = wacom_stack.get(0).unwrap();
                    let width = mult as f32 * (controlpt.1 as f32) / 2048.;
                    framebuffer.draw_dynamic_bezier(
                        (Vec2::from(beginpt.0), (width)),
                        (Vec2::from(controlpt.0), (width)),
                        (Vec2::from(endpt.0), (width)),
                        10,
                        col,
                    )
                };

                if !LAST_REFRESHED_CANVAS_RECT
                    .load(Ordering::Relaxed)
                    .contains_rect(&rect)
                {
                    framebuffer.partial_refresh(
                        &rect,
                        PartialRefreshMode::Async,
                        waveform_mode::WAVEFORM_MODE_DU,
                        display_temp::TEMP_USE_REMARKABLE_DRAW,
                        dither_mode::EPDC_FLAG_EXP1,
                        DRAWING_QUANT_BIT,
                        false,
                    );
                    LAST_REFRESHED_CANVAS_RECT.store(rect, Ordering::Relaxed);
                }
            }
        }
        wacom::WacomEvent::InstrumentChange { pen, state } => {
            match pen {
                // Whether the pen is in range
                wacom::WacomPen::ToolPen => {
                    WACOM_IN_RANGE.store(state, Ordering::Relaxed);
                }
                // Whether the pen is actually making contact
                wacom::WacomPen::Touch => {
                    // Stop drawing when instrument has left the vicinity of the screen
                    if !state {
                        let mut wacom_stack = WACOM_HISTORY.lock().unwrap();
                        wacom_stack.clear();
                    }
                }
                _ => unreachable!(),
            }
        }
        wacom::WacomEvent::Hover {
            y: _,
            x: _,
            distance,
            tilt_x: _,
            tilt_y: _,
        } => {
            // If the pen is hovering, don't record its coordinates as the origin of the next line
            if distance > 1 {
                let mut wacom_stack = WACOM_HISTORY.lock().unwrap();
                wacom_stack.clear();
                UNPRESS_OBSERVED.store(true, Ordering::Relaxed);
            }
        }
        _ => {}
    };
}

fn on_touch_handler(app: &mut appctx::ApplicationContext, input: multitouch::MultitouchEvent) {
    let framebuffer = app.get_framebuffer_ref();
    match input {
        multitouch::MultitouchEvent::Touch {
            gesture_seq: _,
            finger_id: _,
            y,
            x,
        } => {
            if !CANVAS_REGION.contains_point(y.into(), x.into()) {
                return;
            }
            let rect = match G_TOUCH_MODE.load(Ordering::Relaxed) {
                TouchMode::Bezier => framebuffer.draw_bezier(
                    Vec2 {
                        x: x as f32,
                        y: y as f32,
                    },
                    Vec2 {
                        x: (x + 155) as f32,
                        y: (y + 14) as f32,
                    },
                    Vec2 {
                        x: (x + 200) as f32,
                        y: (y + 200) as f32,
                    },
                    2.0,
                    1000,
                    color::BLACK,
                ),
                TouchMode::Circles => {
                    framebuffer.draw_circle(y as usize, x as usize, 20, color::BLACK)
                }
                m @ TouchMode::Diamonds | m @ TouchMode::FillDiamonds => framebuffer.draw_polygon(
                    vec![
                        IntVec2 {
                            x: x as i32 - 10,
                            y: y as i32,
                        },
                        IntVec2 {
                            x: x as i32,
                            y: y as i32 + 20,
                        },
                        IntVec2 {
                            x: x as i32 + 10,
                            y: y as i32,
                        },
                        IntVec2 {
                            x: x as i32,
                            y: y as i32 - 20,
                        },
                    ],
                    match m {
                        TouchMode::Diamonds => false,
                        TouchMode::FillDiamonds => true,
                        _ => false,
                    },
                    color::BLACK,
                ),
                _ => return,
            };
            framebuffer.partial_refresh(
                &rect,
                PartialRefreshMode::Async,
                waveform_mode::WAVEFORM_MODE_DU,
                display_temp::TEMP_USE_REMARKABLE_DRAW,
                dither_mode::EPDC_FLAG_USE_DITHERING_ALPHA,
                DRAWING_QUANT_BIT,
                false,
            );
        }
        _ => {}
    }
}

fn on_button_press(app: &mut appctx::ApplicationContext, input: gpio::GPIOEvent) {
    let (btn, new_state) = match input {
        gpio::GPIOEvent::Press { button } => (button, true),
        gpio::GPIOEvent::Unpress { button } => (button, false),
        _ => return,
    };

    // Ignoring the unpressed event
    if !new_state {
        return;
    }

    // Simple but effective accidental button press filtering
    if WACOM_IN_RANGE.load(Ordering::Relaxed) {
        return;
    }

    match btn {
        gpio::PhysicalButton::RIGHT => {
            let new_state = match app.is_input_device_active(InputDevice::Multitouch) {
                true => {
                    app.deactivate_input_device(InputDevice::Multitouch);
                    "Enable Touch"
                }
                false => {
                    app.activate_input_device(InputDevice::Multitouch);
                    "Disable Touch"
                }
            };

            match app.get_element_by_name("tooltipRight") {
                Some(ref elem) => {
                    if let UIElement::Text {
                        ref mut text,
                        scale: _,
                        foreground: _,
                        border_px: _,
                    } = elem.write().inner
                    {
                        *text = new_state.to_string();
                    }
                }
                None => {}
            }
            app.draw_element("tooltipRight");
            return;
        }
        gpio::PhysicalButton::MIDDLE | gpio::PhysicalButton::LEFT => {
            app.clear(btn == gpio::PhysicalButton::MIDDLE);
            app.draw_elements();
        }
        gpio::PhysicalButton::POWER => {
            Command::new("systemctl")
                .arg("start")
                .arg("xochitl")
                .spawn()
                .unwrap();
            std::process::exit(0);
        }
        gpio::PhysicalButton::WAKEUP => {
            println!("WAKEUP button(?) pressed(?)");
        }
    };
}

fn main() {
    env_logger::init();

    // Takes callback functions as arguments
    // They are called with the event and the &mut framebuffer
    let mut app: appctx::ApplicationContext =
        appctx::ApplicationContext::new(on_button_press, on_wacom_input, on_touch_handler);

    // Alternatively we could have called `app.execute_lua("fb.clear()")`
    app.clear(true);

    // A rudimentary way to declare a scene and layout
    app.add_element(
        "logo",
        UIElementWrapper {
            y: 10,
            x: 900,
            refresh: UIConstraintRefresh::Refresh,

            /* We could have alternatively done this:

               // Create a clickable region for multitouch input and associate it with its handler fn
               app.create_active_region(10, 900, 240, 480, on_touch_rustlogo);
            */
            onclick: Some(on_touch_rustlogo),
            inner: UIElement::Image {
                img: image::load_from_memory(include_bytes!("../assets/rustlang.png")).unwrap(),
            },
            ..Default::default()
        },
    );

    // Draw the borders for the canvas region
    app.add_element(
        "canvasRegion",
        UIElementWrapper {
            y: (CANVAS_REGION.top - 2) as usize,
            x: CANVAS_REGION.left as usize,
            refresh: UIConstraintRefresh::RefreshAndWait,
            onclick: None,
            inner: UIElement::Region {
                height: (CANVAS_REGION.height + 3) as usize,
                width: (CANVAS_REGION.width + 1) as usize,
                border_px: 2,
                border_color: color::BLACK,
            },
            ..Default::default()
        },
    );

    app.add_element(
        "colortest-rgb",
        UIElementWrapper {
            y: 300,
            x: 960,
            refresh: UIConstraintRefresh::Refresh,

            onclick: Some(draw_color_test_rgb),
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Show RGB Test Image".to_owned(),
                scale: 35,
                border_px: 3,
            },
            ..Default::default()
        },
    );

    // Zoom Out Button
    app.add_element(
        "zoomoutButton",
        UIElementWrapper {
            y: 370,
            x: 960,
            refresh: UIConstraintRefresh::Refresh,

            onclick: Some(on_zoom_out),
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Zoom Out".to_owned(),
                scale: 45,
                border_px: 5,
            },
            ..Default::default()
        },
    );
    // Blur Toggle
    app.add_element(
        "blurToggle",
        UIElementWrapper {
            y: 370,
            x: 1155,
            refresh: UIConstraintRefresh::Refresh,

            onclick: Some(on_blur_canvas),
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Blur".to_owned(),
                scale: 45,
                border_px: 5,
            },
            ..Default::default()
        },
    );
    // Invert Toggle
    app.add_element(
        "invertToggle",
        UIElementWrapper {
            y: 370,
            x: 1247,
            refresh: UIConstraintRefresh::Refresh,

            onclick: Some(on_invert_canvas),
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Invert".to_owned(),
                scale: 45,
                border_px: 5,
            },
            ..Default::default()
        },
    );

    // Save/Restore Controls
    app.add_element(
        "saveButton",
        UIElementWrapper {
            y: 440,
            x: 960,
            refresh: UIConstraintRefresh::Refresh,

            onclick: Some(on_save_canvas),
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Save".to_owned(),
                scale: 45,
                border_px: 5,
            },
            ..Default::default()
        },
    );

    app.add_element(
        "restoreButton",
        UIElementWrapper {
            y: 440,
            x: 1080,
            refresh: UIConstraintRefresh::Refresh,

            onclick: Some(on_load_canvas),
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Load".to_owned(),
                scale: 45,
                border_px: 5,
            },
            ..Default::default()
        },
    );

    // Touch Mode Toggle
    app.add_element(
        "touchMode",
        UIElementWrapper {
            y: 510,
            x: 960,
            refresh: UIConstraintRefresh::Refresh,

            onclick: Some(on_change_touchdraw_mode),
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Touch Mode".to_owned(),
                scale: 45,
                border_px: 5,
            },
            ..Default::default()
        },
    );
    app.add_element(
        "touchModeIndicator",
        UIElementWrapper {
            y: 510,
            x: 1210,
            refresh: UIConstraintRefresh::Refresh,

            onclick: None,
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "None".to_owned(),
                scale: 40,
                border_px: 0,
            },
            ..Default::default()
        },
    );

    // Color Mode Toggle
    app.add_element(
        "colorToggle",
        UIElementWrapper {
            y: 580,
            x: 960,
            refresh: UIConstraintRefresh::Refresh,

            onclick: Some(on_toggle_eraser),
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Draw Color".to_owned(),
                scale: 45,
                border_px: 5,
            },
            ..Default::default()
        },
    );
    app.add_element(
        "colorIndicator",
        UIElementWrapper {
            y: 580,
            x: 1210,
            refresh: UIConstraintRefresh::Refresh,

            onclick: None,
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: G_DRAW_MODE.load(Ordering::Relaxed).color_as_string(),
                scale: 40,
                border_px: 0,
            },
            ..Default::default()
        },
    );

    // Size Controls
    app.add_element(
        "decreaseSize",
        UIElementWrapper {
            y: 670,
            x: 960,
            refresh: UIConstraintRefresh::Refresh,
            onclick: Some(|appctx, _| {
                change_brush_width(appctx, -1);
            }),
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "-".to_owned(),
                scale: 90,
                border_px: 5,
            },
            ..Default::default()
        },
    );
    app.add_element(
        "displaySize",
        UIElementWrapper {
            y: 670,
            x: 1030,
            refresh: UIConstraintRefresh::Refresh,
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: format!("size: {0}", G_DRAW_MODE.load(Ordering::Relaxed).get_size()),
                scale: 45,
                border_px: 0,
            },
            ..Default::default()
        },
    );
    app.add_element(
        "increaseSize",
        UIElementWrapper {
            y: 670,
            x: 1210,
            refresh: UIConstraintRefresh::Refresh,
            onclick: Some(|appctx, _| {
                change_brush_width(appctx, 1);
            }),
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "+".to_owned(),
                scale: 90,
                border_px: 5,
            },
            ..Default::default()
        },
    );

    app.add_element(
        "exitToXochitl",
        UIElementWrapper {
            y: 50,
            x: 30,
            refresh: UIConstraintRefresh::Refresh,

            onclick: None,
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Press POWER to return to reMarkable".to_owned(),
                scale: 35,
                border_px: 0,
            },
            ..Default::default()
        },
    );
    app.add_element(
        "availAt",
        UIElementWrapper {
            y: 620,
            x: 30,
            refresh: UIConstraintRefresh::Refresh,
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Available at:".to_owned(),
                scale: 70,
                border_px: 0,
            },
            ..Default::default()
        },
    );
    app.add_element(
        "github",
        UIElementWrapper {
            y: 690,
            x: 30,
            refresh: UIConstraintRefresh::Refresh,
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "github.com/canselcik/libremarkable".to_owned(),
                scale: 55,
                border_px: 0,
            },
            ..Default::default()
        },
    );
    app.add_element(
        "l1",
        UIElementWrapper {
            y: 350,
            x: 30,
            refresh: UIConstraintRefresh::Refresh,
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Low Latency eInk Display Partial Refresh API".to_owned(),
                scale: 45,
                border_px: 0,
            },
            ..Default::default()
        },
    );
    app.add_element(
        "l3",
        UIElementWrapper {
            y: 400,
            x: 30,
            refresh: UIConstraintRefresh::Refresh,
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Capacitive Multitouch Input Support".to_owned(),
                scale: 45,
                border_px: 0,
            },
            ..Default::default()
        },
    );
    app.add_element(
        "l2",
        UIElementWrapper {
            y: 450,
            x: 30,
            refresh: UIConstraintRefresh::Refresh,
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Physical Button Support".to_owned(),
                scale: 45,
                border_px: 0,
            },
            ..Default::default()
        },
    );
    app.add_element(
        "l4",
        UIElementWrapper {
            y: 500,
            x: 30,
            refresh: UIConstraintRefresh::Refresh,
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Wacom Digitizer Support".to_owned(),
                scale: 45,
                border_px: 0,
            },
            ..Default::default()
        },
    );

    app.add_element(
        "tooltipLeft",
        UIElementWrapper {
            y: 1850,
            x: 15,
            refresh: UIConstraintRefresh::Refresh,
            onclick: None,
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Quick Redraw".to_owned(), // maybe quick redraw for the demo or waveform change?
                scale: 50,
                border_px: 0,
            },
            ..Default::default()
        },
    );
    app.add_element(
        "tooltipMiddle",
        UIElementWrapper {
            y: 1850,
            x: 565,
            refresh: UIConstraintRefresh::Refresh,
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Full Redraw".to_owned(),
                scale: 50,
                border_px: 0,
            },
            ..Default::default()
        },
    );
    app.add_element(
        "tooltipRight",
        UIElementWrapper {
            y: 1850,
            x: 1112,
            refresh: UIConstraintRefresh::Refresh,
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: "Disable Touch".to_owned(),
                scale: 50,
                border_px: 0,
            },
            ..Default::default()
        },
    );

    // Create the top bar's time and battery labels. We can mutate these later.
    let dt: DateTime<Local> = Local::now();
    app.add_element(
        "battery",
        UIElementWrapper {
            y: 215,
            x: 30,
            refresh: UIConstraintRefresh::Refresh,
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: format!(
                    "{0:<128}",
                    format!(
                        "{0} — {1}%",
                        battery::human_readable_charging_status().unwrap(),
                        battery::percentage().unwrap()
                    )
                ),
                scale: 44,
                border_px: 0,
            },
            ..Default::default()
        },
    );
    app.add_element(
        "time",
        UIElementWrapper {
            y: 150,
            x: 30,
            refresh: UIConstraintRefresh::Refresh,
            inner: UIElement::Text {
                foreground: color::BLACK,
                text: format!("{}", dt.format("%F %r")),
                scale: 75,
                border_px: 0,
            },
            ..Default::default()
        },
    );

    // Draw the scene
    app.draw_elements();

    // Get a &mut to the framebuffer object, exposing many convenience functions
    let appref = app.upgrade_ref();
    let clock_thread = std::thread::spawn(move || {
        loop_update_topbar(appref, 30 * 1000);
    });

    app.execute_lua(
        r#"
      function draw_box(y, x, height, width, borderpx, bordercolor)
        local maxy = y+height;
        local maxx = x+width;
        for cy=y,maxy,1 do
          for cx=x,maxx,1 do
            if (math.abs(cx-x) < borderpx or math.abs(maxx-cx) < borderpx) or
               (math.abs(cy-y) < borderpx or math.abs(maxy-cy) < borderpx) then
              fb.set_pixel(cy, cx, bordercolor);
            end
          end
        end
      end

      top = 430;
      left = 570;
      width = 320;
      height = 90;
      borderpx = 3;
      draw_box(top, left, height, width, borderpx, 255);

      -- Draw black text inside the box. Notice the text is bottom aligned.
      fb.draw_text(top+55, left+22, '...also supports Lua', 30, 255);

      -- Update the drawn rect w/ `deep_plot=false` and `wait_for_update_complete=true`
      fb.refresh(top, left, height, width, false, true);
    "#,
    );

    info!("Init complete. Beginning event dispatch...");

    // Blocking call to process events from digitizer + touchscreen + physical buttons
    app.dispatch_events(true, true, true);
    clock_thread.join().unwrap();
}
