use std::f64::consts::{FRAC_PI_6, PI, TAU};

use api::{Extent, Id, Vec3};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlImageElement};

use crate::components::map::{Config, ObjectData};
use crate::error::Error;

const HALF_SPAN: f64 = FRAC_PI_6;

/// Information about an avatar to render.
pub(crate) struct RenderAvatar<'a> {
    pub(crate) transform: api::Transform,
    pub(crate) look_at: Option<Vec3>,
    pub(crate) image: Option<Id>,
    pub(crate) color: api::Color,
    pub(crate) name: Option<&'a str>,
    pub(crate) player: bool,
    pub(crate) selected: bool,
    pub(crate) hidden: bool,
}

impl<'a> RenderAvatar<'a> {
    pub(crate) fn from_data(data: &'a ObjectData) -> Self {
        Self {
            transform: *data.transform,
            look_at: *data.look_at,
            image: *data.image,
            color: data.color.unwrap_or_else(api::Color::neutral),
            name: data.name.as_deref(),
            player: false,
            selected: false,
            hidden: *data.hidden,
        }
    }
}

/// Encapsulates the canvas <-> world coordinate transform for a given frame.
pub(crate) struct ViewTransform {
    pub(crate) scale: f64,
    center_x: f64,
    center_y: f64,
}

impl ViewTransform {
    pub(crate) fn new(canvas: &HtmlCanvasElement, w: &Config) -> Self {
        let canvas_min = canvas.width().min(canvas.height()) as f64;
        let world_w = (w.extent.x.end - w.extent.x.start) as f64;
        let world_h = (w.extent.y.end - w.extent.y.start) as f64;
        let scale = (canvas_min / world_w.max(world_h)) * *w.zoom as f64;

        let world_mid_x = ((w.extent.x.start + w.extent.x.end) / 2.0) as f64;
        let world_mid_y = ((w.extent.y.start + w.extent.y.end) / 2.0) as f64;
        let center_x = canvas.width() as f64 / 2.0 + w.pan.x - world_mid_x * scale;
        let center_y = canvas.height() as f64 / 2.0 + w.pan.y - world_mid_y * scale;

        Self {
            scale,
            center_x,
            center_y,
        }
    }

    pub(crate) fn world_to_canvas(&self, world_x: f32, world_z: f32) -> (f64, f64) {
        let x = self.center_x + world_x as f64 * self.scale;
        let y = self.center_y - world_z as f64 * self.scale;
        (x, y)
    }

    pub(crate) fn canvas_to_world(&self, canvas_x: f64, canvas_y: f64) -> (f32, f32) {
        let world_x = ((canvas_x - self.center_x) / self.scale) as f32;
        let world_z = ((self.center_y - canvas_y) / self.scale) as f32;
        (world_x, world_z)
    }
}

/// Draws a 30-degree directional arc just outside the token circle to indicate
/// facing. `angle` is the canvas-space angle (radians) of the facing direction.
/// The arc is centred on that angle and spans +/-15 degrees.
pub(crate) fn draw_facing_arc(
    cx: &CanvasRenderingContext2d,
    x: f64,
    y: f64,
    radius: f64,
    angle: f64,
    line_width: f64,
) -> Result<(), wasm_bindgen::JsValue> {
    cx.set_line_width(line_width);
    cx.begin_path();
    cx.arc(x, y, radius, angle - HALF_SPAN, angle + HALF_SPAN)?;
    cx.stroke();
    Ok(())
}

/// Draws a world-space grid with thicker lines at the origin axes.
/// Non-origin lines are drawn first, then origin lines on top.
pub(crate) fn draw_grid(
    cx: &CanvasRenderingContext2d,
    t: &ViewTransform,
    extent: &Extent,
    zoom: f32,
) {
    const GRID_STEP: f32 = 2.0;
    const EPS: f32 = GRID_STEP * 0.01;

    cx.set_stroke_style_str("#2a2a2a");
    cx.set_line_width(zoom as f64 * 0.5);

    let mut x = (extent.x.start / GRID_STEP).ceil() * GRID_STEP;

    while x <= extent.x.end + EPS {
        if x.abs() >= EPS {
            let (px, py1) = t.world_to_canvas(x, extent.y.start);
            let (_, py2) = t.world_to_canvas(x, extent.y.end);
            cx.begin_path();
            cx.move_to(px, py1);
            cx.line_to(px, py2);
            cx.stroke();
        }

        x += GRID_STEP;
    }

    let mut z = (extent.y.start / GRID_STEP).ceil() * GRID_STEP;

    while z <= extent.y.end + EPS {
        if z.abs() >= EPS {
            let (px1, py) = t.world_to_canvas(extent.x.start, z);
            let (px2, _) = t.world_to_canvas(extent.x.end, z);
            cx.begin_path();
            cx.move_to(px1, py);
            cx.line_to(px2, py);
            cx.stroke();
        }

        z += GRID_STEP;
    }

    cx.set_stroke_style_str("#888888");
    cx.set_line_width(zoom as f64 * 1.5);

    if extent.x.contains(0.0) {
        let (px, py1) = t.world_to_canvas(0.0, extent.y.start);
        let (_, py2) = t.world_to_canvas(0.0, extent.y.end);
        cx.begin_path();
        cx.move_to(px, py1);
        cx.line_to(px, py2);
        cx.stroke();
    }

    if extent.y.contains(0.0) {
        let (px1, py) = t.world_to_canvas(extent.x.start, 0.0);
        let (px2, _) = t.world_to_canvas(extent.x.end, 0.0);
        cx.begin_path();
        cx.move_to(px1, py);
        cx.line_to(px2, py);
        cx.stroke();
    }
}

/// Renders a single avatar token (circular image or filled circle) with a
/// facing arc. `get_image` is called to look up the loaded `HtmlImageElement`
/// for the given id.
pub(crate) fn draw_avatar_token(
    cx: &CanvasRenderingContext2d,
    t: &ViewTransform,
    a: &RenderAvatar,
    token_radius: f64,
    arrow_target: Option<(f32, f32)>,
    get_image: impl Fn(Id) -> Option<HtmlImageElement>,
) -> Result<(), Error> {
    let (x, y) = t.world_to_canvas(a.transform.position.x, a.transform.position.z);

    if a.selected {
        cx.set_stroke_style_str("#ffffff");
        cx.set_line_width(token_radius * 0.08);
        cx.begin_path();
        cx.arc(x, y, token_radius * 1.2, -FRAC_PI_6, FRAC_PI_6)?;
        cx.stroke();

        cx.begin_path();
        cx.arc(x, y, token_radius * 1.2, PI - FRAC_PI_6, PI + FRAC_PI_6)?;
        cx.stroke();
    }

    let image_drawn = 'draw: {
        let Some(id) = a.image else {
            break 'draw false;
        };

        let Some(img) = get_image(id) else {
            break 'draw false;
        };

        let iw = img.natural_width() as f64;
        let ih = img.natural_height() as f64;

        let scale = (token_radius * 2.0) / iw.min(ih);
        let dw = iw * scale;
        let dh = ih * scale;

        cx.save();
        cx.begin_path();
        cx.arc(x, y, token_radius, 0.0, TAU)?;
        cx.clip();

        cx.draw_image_with_html_image_element_and_dw_and_dh(
            &img,
            x - dw / 2.0,
            y - dh / 2.0,
            dw,
            dh,
        )?;

        cx.restore();
        true
    };

    if !image_drawn {
        cx.set_fill_style_str(&a.color.to_css_string());
        cx.begin_path();
        cx.arc(x, y, token_radius, 0.0, TAU)?;
        cx.fill();
    }

    let front = if a.player
        && let Some((mx, my)) = arrow_target
    {
        let (x, y) = (a.transform.position.x, a.transform.position.z);
        let angle_rad = (my - y).atan2(mx - x);
        let dir_x = angle_rad.cos();
        let dir_z = angle_rad.sin();
        Vec3::new(dir_x, 0.0, dir_z)
    } else {
        a.transform.front
    };

    if front.x.hypot(front.z) > 0.01 {
        let angle = (-front.z as f64).atan2(front.x as f64);
        let arc_radius = token_radius * 1.5;
        cx.set_stroke_style_str(&a.color.to_css_string());
        draw_facing_arc(cx, x, y, arc_radius, angle, token_radius * 0.25)?;
    }

    if let Some(name) = &a.name {
        let font_size = (token_radius * 0.6).max(10.0);
        cx.set_font(&format!("bold {font_size}px sans-serif"));
        cx.set_text_align("center");

        let facing_up = front.x.hypot(front.z) > 0.01 && {
            let angle = (-front.z as f64).atan2(front.x as f64);
            angle.sin() < 0.0
        };

        let (name_y, baseline) = if facing_up {
            (y + token_radius + 4.0, "top")
        } else {
            (y - token_radius - 4.0, "bottom")
        };

        cx.set_text_baseline(baseline);
        cx.set_shadow_color("rgba(0,0,0,0.8)");
        cx.set_shadow_blur(3.0);
        cx.set_fill_style_str("#ffffff");
        let _ = cx.fill_text(name, x, name_y);
        cx.set_shadow_blur(0.0);
    }

    if a.hidden {
        draw_hidden_badge(cx, x, y, token_radius)?;
    }

    Ok(())
}

/// Draws a small eye-slash badge at the top-right edge of a token to indicate
/// the object is hidden from remote peers.
fn draw_hidden_badge(
    cx: &CanvasRenderingContext2d,
    x: f64,
    y: f64,
    token_radius: f64,
) -> Result<(), Error> {
    let badge_r = token_radius * 0.38;
    // Position at 45° (top-right edge)
    let bx = x + token_radius * std::f64::consts::FRAC_1_SQRT_2;
    let by = y - token_radius * std::f64::consts::FRAC_1_SQRT_2;

    cx.save();

    // Dark pill background
    cx.set_fill_style_str("rgba(20,20,20,0.85)");
    cx.begin_path();
    cx.arc(bx, by, badge_r, 0.0, TAU)?;
    cx.fill();

    // Eye almond (two arcs meeting at left/right points)
    let ew = badge_r * 0.75;
    let eh = badge_r * 0.45;
    cx.set_stroke_style_str("#e05252");
    cx.set_line_width(badge_r * 0.22);
    cx.begin_path();
    cx.ellipse(bx, by, ew, eh, 0.0, 0.0, TAU)?;
    cx.stroke();

    // Pupil dot
    cx.set_fill_style_str("#e05252");
    cx.begin_path();
    cx.arc(bx, by, badge_r * 0.18, 0.0, TAU)?;
    cx.fill();

    // Slash line
    cx.set_stroke_style_str("#e05252");
    cx.set_line_width(badge_r * 0.22);
    cx.begin_path();
    cx.move_to(bx - ew * 0.85, by + eh * 1.1);
    cx.line_to(bx + ew * 0.85, by - eh * 1.1);
    cx.stroke();

    cx.restore();
    Ok(())
}

/// Draws a look-at eye indicator at the given world position.
pub(crate) fn draw_look_at(
    cx: &CanvasRenderingContext2d,
    t: &ViewTransform,
    target: Vec3,
    color: &api::Color,
    zoom: f64,
) -> Result<(), Error> {
    let eye_width = 24.0 * zoom;
    let eye_height = 12.0 * zoom;
    let radius = 6.0 * zoom;

    let color = color.to_css_string();

    let (ex, ey) = t.world_to_canvas(target.x, target.z);

    cx.save();
    cx.set_stroke_style_str(&color);
    cx.set_line_width(2.0 * zoom);
    cx.begin_path();
    cx.ellipse(ex, ey, eye_width / 2.0, eye_height / 2.0, 0.0, 0.0, TAU)?;
    cx.stroke();

    cx.set_fill_style_str(&color);
    cx.begin_path();
    cx.arc(ex, ey, radius, 0.0, TAU)?;
    cx.fill();

    cx.restore();
    Ok(())
}

/// Renders a single avatar preview (no grid, no look_at eye) centered on the
/// canvas. Used for the settings preview.
pub(crate) fn draw_avatar_preview(
    cx: &CanvasRenderingContext2d,
    canvas: &HtmlCanvasElement,
    avatar: &RenderAvatar,
    get_image: impl Fn(Id) -> Option<HtmlImageElement>,
) -> Result<(), Error> {
    let w = canvas.width() as f64;
    let h = canvas.height() as f64;

    cx.clear_rect(0.0, 0.0, w, h);

    let token_radius = w.min(h) / 4.0;
    let center_x = w / 2.0;
    let center_y = h / 2.0;

    // Draw avatar token
    let image_drawn = 'draw: {
        let Some(id) = avatar.image else {
            break 'draw false;
        };

        let Some(img) = get_image(id) else {
            break 'draw false;
        };

        let iw = img.natural_width() as f64;
        let ih = img.natural_height() as f64;

        let scale = (token_radius * 2.0) / iw.min(ih);
        let dw = iw * scale;
        let dh = ih * scale;

        cx.save();
        cx.begin_path();
        cx.arc(center_x, center_y, token_radius, 0.0, TAU)?;
        cx.clip();

        cx.draw_image_with_html_image_element_and_dw_and_dh(
            &img,
            center_x - dw / 2.0,
            center_y - dh / 2.0,
            dw,
            dh,
        )?;

        cx.restore();
        true
    };

    if !image_drawn {
        cx.set_fill_style_str(&avatar.color.to_css_string());
        cx.begin_path();
        cx.arc(center_x, center_y, token_radius, 0.0, TAU)?;
        cx.fill();
    }

    // Draw facing arc
    let front = avatar.transform.front;

    if front.x.hypot(front.z) > 0.01 {
        let angle = (-front.z as f64).atan2(front.x as f64);
        let arc_radius = token_radius * 1.5;
        cx.set_stroke_style_str(&avatar.color.to_css_string());

        draw_facing_arc(
            cx,
            center_x,
            center_y,
            arc_radius,
            angle,
            token_radius * 0.25,
        )?;
    }

    // Draw name above the token.
    if let Some(name) = &avatar.name {
        let font_size = (token_radius * 0.5).max(12.0);
        cx.set_font(&format!("bold {font_size}px sans-serif"));
        cx.set_text_align("center");
        cx.set_text_baseline("bottom");
        let name_y = center_y - token_radius - 8.0;
        cx.set_shadow_color("rgba(0,0,0,0.8)");
        cx.set_shadow_blur(3.0);
        cx.set_fill_style_str("#ffffff");
        let _ = cx.fill_text(name, center_x, name_y);
        cx.set_shadow_blur(0.0);
    }

    Ok(())
}
