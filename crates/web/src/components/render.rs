use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, FRAC_PI_6, PI, TAU};

use api::{Extent, Id, Vec3};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlImageElement};

use crate::components::map::Config;
use crate::error::Error;
use crate::objects::{ObjectData, ObjectKind};

const HALF_SPAN: f64 = FRAC_PI_6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Visibility {
    Remote,
    Local,
    None,
}

impl Visibility {
    #[inline]
    pub(crate) fn is_hidden(&self) -> bool {
        matches!(self, Self::Local | Self::None)
    }

    #[inline]
    pub(crate) fn is_local_hidden(&self) -> bool {
        matches!(self, Self::None)
    }
}

pub(crate) struct RenderToken<'a> {
    pub(crate) transform: api::Transform,
    pub(crate) look_at: Option<Vec3>,
    pub(crate) image: Option<Id>,
    pub(crate) color: api::Color,
    pub(crate) name: Option<&'a str>,
    pub(crate) player: bool,
    pub(crate) selected: bool,
    pub(crate) hidden: Visibility,
    pub(crate) token_radius: f32,
}

impl<'a> RenderToken<'a> {
    pub(crate) fn from_data(
        data: &'a ObjectData,
        visibility: impl FnOnce(Id) -> Visibility,
    ) -> Option<Self> {
        let token = match &data.kind {
            ObjectKind::Token(token) => token,
            _ => return None,
        };

        Some(Self {
            transform: *token.transform,
            look_at: *token.look_at,
            image: *token.image,
            color: token.color.unwrap_or_else(api::Color::neutral),
            name: data.name.as_deref(),
            player: false,
            selected: false,
            hidden: data.visibility().max(visibility(*data.group)),
            token_radius: *token.token_radius,
        })
    }
}

pub(crate) struct RenderStatic {
    pub(crate) transform: api::Transform,
    pub(crate) image: Option<Id>,
    pub(crate) color: api::Color,
    pub(crate) selected: bool,
    pub(crate) hidden: bool,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl RenderStatic {
    pub(crate) fn from_data(data: &ObjectData) -> Option<Self> {
        let s = match &data.kind {
            ObjectKind::Static(s) => s,
            _ => return None,
        };

        Some(Self {
            transform: *s.transform,
            image: *s.image,
            color: s.color.unwrap_or_else(api::Color::neutral),
            selected: false,
            hidden: *s.hidden,
            width: *s.width,
            height: *s.height,
        })
    }
}

/// Two point coordinates in canvas space.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Canvas2 {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

impl Canvas2 {
    #[inline]
    pub(crate) fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

pub(crate) struct ViewTransform {
    pub(crate) scale: f64,
    center_x: f64,
    center_y: f64,
}

impl ViewTransform {
    pub(crate) fn preview(canvas: &HtmlCanvasElement) -> Self {
        Self {
            scale: 50.0,
            center_x: canvas.width() as f64 / 2.0,
            center_y: canvas.height() as f64 / 2.0,
        }
    }

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

    pub(crate) fn world_to_canvas(&self, world_x: f32, world_z: f32) -> Canvas2 {
        let x = self.center_x + world_x as f64 * self.scale;
        let y = self.center_y - world_z as f64 * self.scale;
        Canvas2::new(x, y)
    }

    pub(crate) fn canvas_to_world(&self, canvas_x: f64, canvas_y: f64) -> Vec3 {
        let world_x = ((canvas_x - self.center_x) / self.scale) as f32;
        let world_z = ((self.center_y - canvas_y) / self.scale) as f32;
        Vec3::new(world_x, 0.0, world_z)
    }
}

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

pub(crate) fn draw_grid(
    cx: &CanvasRenderingContext2d,
    t: &ViewTransform,
    extent: &Extent,
    zoom: f32,
) {
    const GRID_STEP: f32 = 1.0;
    const EPS: f32 = GRID_STEP * 0.01;

    cx.set_stroke_style_str("#2a2a2a");
    cx.set_line_width(zoom as f64 * 0.5);

    let mut x = (extent.x.start / GRID_STEP).ceil() * GRID_STEP;

    while x <= extent.x.end + EPS {
        if x.abs() >= EPS {
            let c1 = t.world_to_canvas(x, extent.y.start);
            let c2 = t.world_to_canvas(x, extent.y.end);

            cx.begin_path();
            cx.move_to(c1.x, c1.y);
            cx.line_to(c1.x, c2.y);
            cx.stroke();
        }

        x += GRID_STEP;
    }

    let mut z = (extent.y.start / GRID_STEP).ceil() * GRID_STEP;

    while z <= extent.y.end + EPS {
        if z.abs() >= EPS {
            let c1 = t.world_to_canvas(extent.x.start, z);
            let c2 = t.world_to_canvas(extent.x.end, z);

            cx.begin_path();
            cx.move_to(c1.x, c1.y);
            cx.line_to(c2.x, c1.y);
            cx.stroke();
        }

        z += GRID_STEP;
    }

    cx.set_stroke_style_str("#888888");
    cx.set_line_width(zoom as f64 * 1.5);

    if extent.x.contains(0.0) {
        let c1 = t.world_to_canvas(0.0, extent.y.start);
        let c2 = t.world_to_canvas(0.0, extent.y.end);
        cx.begin_path();
        cx.move_to(c1.x, c1.y);
        cx.line_to(c1.x, c2.y);
        cx.stroke();
    }

    if extent.y.contains(0.0) {
        let c1 = t.world_to_canvas(extent.x.start, 0.0);
        let c2 = t.world_to_canvas(extent.x.end, 0.0);
        cx.begin_path();
        cx.move_to(c1.x, c1.y);
        cx.line_to(c2.x, c1.y);
        cx.stroke();
    }
}

fn draw_hidden_badge(
    cx: &CanvasRenderingContext2d,
    x: f64,
    y: f64,
    token_radius: f64,
) -> Result<(), Error> {
    let badge_r = token_radius * 0.38;
    let bx = x + token_radius * FRAC_1_SQRT_2;
    let by = y - token_radius * FRAC_1_SQRT_2;

    cx.save();

    let ew = badge_r * 0.75;
    let eh = badge_r * 0.45;

    cx.set_stroke_style_str("#e05252");
    cx.set_line_width(badge_r * 0.22);
    cx.begin_path();
    cx.ellipse(bx, by, ew, eh, 0.0, 0.0, TAU)?;
    cx.stroke();

    cx.set_fill_style_str("#e05252");
    cx.begin_path();
    cx.arc(bx, by, badge_r * 0.18, 0.0, TAU)?;
    cx.fill();

    cx.set_stroke_style_str("#e05252");
    cx.set_line_width(badge_r * 0.22);
    cx.begin_path();
    cx.move_to(bx - ew * 0.85, by + eh * 1.1);
    cx.line_to(bx + ew * 0.85, by - eh * 1.1);
    cx.stroke();

    cx.restore();
    Ok(())
}

pub(crate) fn draw_look_at(
    cx: &CanvasRenderingContext2d,
    t: &ViewTransform,
    target: Vec3,
    color: &api::Color,
    zoom: f64,
) -> Result<(), Error> {
    let radius = 5.0 * zoom;

    let color = color.to_transparent_rgba(0.5);

    let e = t.world_to_canvas(target.x, target.z);

    cx.set_fill_style_str(&color);
    cx.begin_path();
    cx.arc(e.x, e.y, radius, 0.0, TAU)?;
    cx.fill();

    cx.restore();
    Ok(())
}

pub(crate) fn draw_token_token(
    cx: &CanvasRenderingContext2d,
    t: &ViewTransform,
    token: &RenderToken,
    arrow_target: Option<&Vec3>,
    get_image: impl Fn(Id) -> Option<HtmlImageElement>,
) -> Result<(), Error> {
    let pos = t.world_to_canvas(token.transform.position.x, token.transform.position.z);

    let token_radius = token.token_radius as f64 * t.scale;

    let color = token.color.to_css_string();

    if token.selected {
        cx.set_stroke_style_str("#ffffff");
        cx.set_line_width(token_radius * 0.1);
        cx.begin_path();
        cx.arc(pos.x, pos.y, token_radius * 1.0, 0.0, PI * 2.0)?;
        cx.stroke();
    }

    let image_drawn = 'draw: {
        let Some(id) = token.image else {
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
        cx.arc(pos.x, pos.y, token_radius, 0.0, TAU)?;
        cx.clip();

        cx.draw_image_with_html_image_element_and_dw_and_dh(
            &img,
            pos.x - dw / 2.0,
            pos.y - dh / 2.0,
            dw,
            dh,
        )?;

        cx.restore();
        true
    };

    if !image_drawn {
        cx.set_fill_style_str(&color);
        cx.begin_path();
        cx.arc(pos.x, pos.y, token_radius, 0.0, TAU)?;
        cx.fill();
    }

    let front = if token.player
        && let Some(m) = arrow_target
    {
        token.transform.position.direction_to(*m)
    } else {
        token.transform.front
    };

    if front.x.hypot(front.z) > 0.01 {
        let angle = front.angle_xz() as f64;
        let arc_radius = token_radius * 1.5;
        let color = token.color.to_transparent_rgba(0.5);
        cx.set_stroke_style_str(&color);
        draw_facing_arc(cx, pos.x, pos.y, arc_radius, angle, token_radius * 0.25)?;
    }

    if let Some(name) = &token.name {
        let font_size = (token_radius * 0.6).max(10.0);
        cx.set_font(&format!("bold {font_size}px sans-serif"));
        cx.set_text_align("center");

        let facing_up = front.x.hypot(front.z) > 0.01 && { front.angle_xz().sin() < 0.0 };

        let (name_y, baseline) = if facing_up {
            (pos.y + token_radius + 4.0, "top")
        } else {
            (pos.y - token_radius - 4.0, "bottom")
        };

        cx.set_text_baseline(baseline);
        cx.set_shadow_color("rgba(0,0,0,0.8)");
        cx.set_shadow_blur(3.0);
        cx.set_fill_style_str("#ffffff");
        let _ = cx.fill_text(name, pos.x, name_y);
        cx.set_shadow_blur(0.0);
    }

    if token.hidden.is_hidden() {
        draw_hidden_badge(cx, pos.x, pos.y, token_radius)?;
    }

    Ok(())
}
pub(crate) fn draw_static_token(
    cx: &CanvasRenderingContext2d,
    t: &ViewTransform,
    s: &RenderStatic,
    get_image: impl Fn(Id) -> Option<HtmlImageElement>,
) -> Result<(), Error> {
    let pos = t.world_to_canvas(s.transform.position.x, s.transform.position.z);

    let hw = s.width as f64 / 2.0 * t.scale;
    let hh = s.height as f64 / 2.0 * t.scale;

    let angle = s.transform.front.angle_xz() as f64;
    let rotation = angle - FRAC_PI_2;

    let color = s.color.to_css_string();

    cx.save();
    cx.translate(pos.x, pos.y)?;
    cx.rotate(rotation)?;

    let image_drawn = 'draw: {
        let Some(id) = s.image else {
            break 'draw false;
        };

        let Some(img) = get_image(id) else {
            break 'draw false;
        };

        let iw = img.natural_width() as f64;
        let ih = img.natural_height() as f64;

        let sx = (hw * 2.0) / iw;
        let sy = (hh * 2.0) / ih;
        let scale = sx.max(sy);
        let dw = iw * scale;
        let dh = ih * scale;

        cx.save();
        cx.rect(-hw, -hh, hw * 2.0, hh * 2.0);
        cx.clip();

        cx.draw_image_with_html_image_element_and_dw_and_dh(&img, -dw / 2.0, -dh / 2.0, dw, dh)?;

        cx.restore();
        true
    };

    if !image_drawn {
        cx.set_fill_style_str(&color);
        cx.fill_rect(-hw, -hh, hw * 2.0, hh * 2.0);
    }

    if s.selected {
        cx.set_stroke_style_str("#ffffff");
        cx.set_line_width(t.scale * 0.025);
        cx.stroke_rect(-hw, -hh, hw * 2.0, hh * 2.0);
    }

    cx.restore();

    if s.hidden {
        let badge_size = hw.hypot(hh) * 0.38;

        draw_hidden_badge(cx, pos.x, pos.y, badge_size)?;
    }

    Ok(())
}
