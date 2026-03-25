use std::f64::consts::{FRAC_PI_2, FRAC_PI_6, PI, TAU};

use api::{Canvas2, Color, Extent, RemoteId, Vec3};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlImageElement};

use crate::components::map::Config;
use crate::error::Error;
use crate::images::{Icon, Images};
use crate::objects::{LocalObject, ObjectKind};

const HALF_SPAN: f64 = FRAC_PI_6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Visibility {
    Remote,
    Local,
    None,
}

impl Visibility {
    #[inline]
    pub(crate) fn is_remote(&self) -> bool {
        matches!(self, Self::Remote)
    }

    #[inline]
    pub(crate) fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }

    #[inline]
    pub(crate) fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

pub(crate) struct RenderBase<'a> {
    pub(crate) name: &'a str,
    pub(crate) visibility: Visibility,
    pub(crate) selected: bool,
}

pub(crate) struct RenderObject<'a> {
    pub(crate) base: RenderBase<'a>,
    pub(crate) kind: RenderObjectKind<'a>,
}

pub(crate) enum RenderObjectKind<'a> {
    Token(RenderToken<'a>),
    Static(RenderStatic<'a>),
}

impl<'a> RenderObject<'a> {
    pub(crate) fn from_data(
        data: &'a LocalObject,
        arrow_target: Option<&'a Vec3>,
        visibility: impl FnOnce(RemoteId) -> Visibility,
    ) -> Option<Self> {
        let kind = match &data.kind {
            ObjectKind::Token(this) => RenderObjectKind::Token(RenderToken {
                transform: &this.transform,
                look_at: this.look_at.as_ref(),
                image: RemoteId::new(data.id.peer_id, *this.image),
                color: this.color.unwrap_or_else(Color::neutral),
                token_radius: *this.token_radius,
                arrow_target,
            }),
            ObjectKind::Static(this) => RenderObjectKind::Static(RenderStatic {
                transform: &this.transform,
                image: RemoteId::new(data.id.peer_id, *this.image),
                color: this.color.unwrap_or_else(Color::neutral),
                width: *this.width,
                height: *this.height,
            }),
            _ => return None,
        };

        Some(Self {
            base: RenderBase {
                name: data.name.as_str(),
                visibility: data.visibility().max(visibility(*data.group)),
                selected: false,
            },
            kind,
        })
    }

    pub(crate) fn apply_scale(&mut self, scale: f32) {
        match &mut self.kind {
            RenderObjectKind::Token(this) => {
                this.token_radius *= scale;
            }
            RenderObjectKind::Static(this) => {
                this.width *= scale;
                this.height *= scale;
            }
        }
    }
}

pub(crate) struct RenderToken<'a> {
    pub(crate) transform: &'a api::Transform,
    pub(crate) look_at: Option<&'a Vec3>,
    pub(crate) image: RemoteId,
    pub(crate) color: Color,
    pub(crate) token_radius: f32,
    pub(crate) arrow_target: Option<&'a Vec3>,
}

pub(crate) struct RenderStatic<'a> {
    pub(crate) transform: &'a api::Transform,
    pub(crate) image: RemoteId,
    pub(crate) color: Color,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

#[derive(Debug)]
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

    pub(crate) fn new(canvas: &HtmlCanvasElement, w: &Config, extent: &Extent) -> Self {
        let width = canvas.width();
        let height = canvas.height();

        let canvas_min = width.min(height) as f64;
        let world_w = (extent.x.end - extent.x.start) as f64;
        let world_h = (extent.y.end - extent.y.start) as f64;
        let scale = (canvas_min / world_w.max(world_h)) * *w.zoom as f64;

        let world_mid_x = ((extent.x.start + extent.x.end) / 2.0) as f64;
        let world_mid_y = ((extent.y.start + extent.y.end) / 2.0) as f64;
        let center_x = width as f64 / 2.0 + w.pan.x - world_mid_x * scale;
        let center_y = height as f64 / 2.0 + w.pan.y - world_mid_y * scale;

        Self {
            scale,
            center_x,
            center_y,
        }
    }

    #[inline]
    pub(crate) fn to_canvas(&self, w: Vec3) -> Canvas2 {
        let x = self.center_x + w.x as f64 * self.scale;
        let y = self.center_y - w.z as f64 * self.scale;
        Canvas2::new(x, y)
    }

    #[inline]
    pub(crate) fn to_world(&self, p: Canvas2) -> Vec3 {
        let world_x = ((p.x - self.center_x) / self.scale) as f32;
        let world_z = ((self.center_y - p.y) / self.scale) as f32;
        Vec3::new(world_x, 0.0, world_z)
    }
}

pub(crate) fn draw_background(
    cx: &CanvasRenderingContext2d,
    view: &ViewTransform,
    extent: &api::Extent,
    img: &HtmlImageElement,
) -> Result<(), Error> {
    let top_left = view.to_canvas(Vec3::new(extent.x.start, 0.0, extent.y.end));
    let bottom_right = view.to_canvas(Vec3::new(extent.x.end, 0.0, extent.y.start));

    let dest_w = bottom_right.x - top_left.x;
    let dest_h = bottom_right.y - top_left.y;

    let img_w = img.natural_width() as f64;
    let img_h = img.natural_height() as f64;

    if img_w == 0.0 || img_h == 0.0 {
        return Ok(());
    }

    let scale = (dest_w / img_w).min(dest_h / img_h);
    let draw_w = img_w * scale;
    let draw_h = img_h * scale;
    let draw_x = top_left.x + (dest_w - draw_w) / 2.0;
    let draw_y = top_left.y + (dest_h - draw_h) / 2.0;

    cx.draw_image_with_html_image_element_and_dw_and_dh(img, draw_x, draw_y, draw_w, draw_h)?;
    Ok(())
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
            let c1 = t.to_canvas(Vec3::new(x, 0.0, extent.y.start));
            let c2 = t.to_canvas(Vec3::new(x, 0.0, extent.y.end));

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
            let c1 = t.to_canvas(Vec3::new(extent.x.start, 0.0, z));
            let c2 = t.to_canvas(Vec3::new(extent.x.end, 0.0, z));

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
        let c1 = t.to_canvas(Vec3::new(0.0, 0.0, extent.y.start));
        let c2 = t.to_canvas(Vec3::new(0.0, 0.0, extent.y.end));

        cx.begin_path();
        cx.move_to(c1.x, c1.y);
        cx.line_to(c1.x, c2.y);
        cx.stroke();
    }

    if extent.y.contains(0.0) {
        let c1 = t.to_canvas(Vec3::new(extent.x.start, 0.0, 0.0));
        let c2 = t.to_canvas(Vec3::new(extent.x.end, 0.0, 0.0));

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
    images: &Images,
) -> Result<(), Error> {
    let Some(img) = images.get_icon(Icon::EyeSlashDanger) else {
        return Ok(());
    };

    let width = token_radius * 0.4;
    let x = x + token_radius * 0.8;
    let y = y - token_radius * 0.8;

    cx.save();
    cx.translate(x, y)?;
    draw_image(cx, &img, width, width)?;
    cx.restore();
    Ok(())
}

pub(crate) fn draw_look_at(
    cx: &CanvasRenderingContext2d,
    view: &ViewTransform,
    target: Vec3,
    color: Color,
) -> Result<(), Error> {
    let radius = 0.1 * view.scale;

    let color = color.to_transparent_rgba(0.5);

    let e = view.to_canvas(target);

    cx.set_fill_style_str(&color);
    cx.begin_path();
    cx.arc(e.x, e.y, radius, 0.0, TAU)?;
    cx.fill();

    cx.restore();
    Ok(())
}

pub(crate) fn draw_token(
    cx: &CanvasRenderingContext2d,
    view: &ViewTransform,
    base: &RenderBase<'_>,
    render: &RenderToken<'_>,
    images: &Images,
) -> Result<(), Error> {
    let pos = view.to_canvas(render.transform.position);

    let token_radius = render.token_radius as f64 * view.scale;

    let color = render.color.to_css_string();

    if base.selected {
        cx.set_stroke_style_str("#ffffff");
        cx.set_line_width(token_radius * 0.1);
        cx.begin_path();
        cx.arc(pos.x, pos.y, token_radius * 1.0, 0.0, PI * 2.0)?;
        cx.stroke();
    }

    let image_drawn = 'draw: {
        let Some(img) = images.get_id(&render.image) else {
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

    let front = if let Some(m) = render.arrow_target {
        render.transform.position.direction_to(*m)
    } else {
        render.transform.front
    };

    if front.x.hypot(front.z) > 0.01 {
        let angle = front.angle_xz() as f64;
        let arc_radius = token_radius * 1.5;
        let color = render.color.to_transparent_rgba(0.5);
        cx.set_stroke_style_str(&color);
        draw_facing_arc(cx, pos.x, pos.y, arc_radius, angle, token_radius * 0.25)?;
    }

    if !base.name.is_empty() {
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
        cx.fill_text(base.name, pos.x, name_y)?;
        cx.set_shadow_blur(0.0);
    }

    if base.visibility.is_local() {
        draw_hidden_badge(cx, pos.x, pos.y, token_radius, images)?;
    }

    Ok(())
}
pub(crate) fn draw_static(
    cx: &CanvasRenderingContext2d,
    view: &ViewTransform,
    base: &RenderBase<'_>,
    render: &RenderStatic<'_>,
    images: &Images,
) -> Result<(), Error> {
    let pos = view.to_canvas(render.transform.position);

    let hw = render.width as f64 / 2.0 * view.scale;
    let hh = render.height as f64 / 2.0 * view.scale;

    let angle = render.transform.front.angle_xz() as f64;
    let rotation = angle - FRAC_PI_2;

    let color = render.color.to_css_string();

    cx.save();
    cx.translate(pos.x, pos.y)?;

    let image_drawn = 'draw: {
        let Some(img) = images.get_id(&render.image) else {
            break 'draw false;
        };

        cx.rotate(rotation)?;
        draw_image(cx, &img, hw, hh)?;
        true
    };

    if !image_drawn {
        cx.set_fill_style_str(&color);
        cx.fill_rect(-hw, -hh, hw * 2.0, hh * 2.0);
    }

    if base.selected {
        cx.set_stroke_style_str("#ffffff");
        cx.set_line_width(view.scale * 0.025);
        cx.stroke_rect(-hw, -hh, hw * 2.0, hh * 2.0);
    }

    cx.restore();

    if base.visibility.is_local() {
        let badge_size = hw.hypot(hh) * 0.5;
        draw_hidden_badge(cx, pos.x, pos.y, badge_size, images)?;
    }

    Ok(())
}

fn draw_image(
    cx: &CanvasRenderingContext2d,
    img: &HtmlImageElement,
    hw: f64,
    hh: f64,
) -> Result<(), Error> {
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

    cx.draw_image_with_html_image_element_and_dw_and_dh(img, -dw / 2.0, -dh / 2.0, dw, dh)?;
    cx.restore();
    Ok(())
}
