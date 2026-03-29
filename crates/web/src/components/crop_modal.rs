use std::ops::{Add, Sub};

use web_sys::{HtmlImageElement, MouseEvent, PointerEvent};
use yew::prelude::*;

use crate::error::Error;

use super::Modal;

const HANDLES: &[(&str, Dir)] = &[
    ("nw", Dir::NW),
    ("n", Dir::N),
    ("ne", Dir::NE),
    ("w", Dir::W),
    ("e", Dir::E),
    ("sw", Dir::SW),
    ("s", Dir::S),
    ("se", Dir::SE),
];

#[derive(Clone, Copy, PartialEq)]
struct Vec2 {
    x: f64,
    y: f64,
}

impl Vec2 {
    const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[inline]
    fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    #[inline]
    fn offset(ev: &MouseEvent) -> Self {
        Self::new(ev.offset_x() as f64, ev.offset_y() as f64)
    }

    #[inline]
    fn client(ev: &MouseEvent) -> Self {
        Self::new(ev.client_x() as f64, ev.client_y() as f64)
    }

    fn clamp(self, min: Self, max: Self) -> Self {
        Self {
            x: self.x.clamp(min.x, max.x),
            y: self.y.clamp(min.y, max.y),
        }
    }
}

impl Sub for Vec2 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Add for Vec2 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

const MIN_SIZE: f64 = 2.0;

#[derive(Clone, Copy)]
enum Axis {
    Neg,
    Zero,
    Pos,
}

impl Axis {
    fn apply(self, v: f64) -> f64 {
        match self {
            Self::Pos => v,
            Self::Neg => -v,
            Self::Zero => 0.0,
        }
    }

    fn anchor(self, pos: f64, old: f64, new: f64, center: f64) -> f64 {
        match self {
            Self::Pos => pos,
            Self::Neg => pos + old - new,
            Self::Zero => center - new / 2.0,
        }
    }

    fn max_extent(self, pos: f64, old: f64, new_pos: f64, bound: f64) -> f64 {
        match self {
            Self::Neg => (pos + old) - new_pos,
            Self::Zero | Self::Pos => bound - new_pos,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Dir {
    x: Axis,
    y: Axis,
}

impl Dir {
    const N: Self = Self::new(Axis::Zero, Axis::Neg);
    const S: Self = Self::new(Axis::Zero, Axis::Pos);
    const E: Self = Self::new(Axis::Pos, Axis::Zero);
    const W: Self = Self::new(Axis::Neg, Axis::Zero);
    const NE: Self = Self::new(Axis::Pos, Axis::Neg);
    const NW: Self = Self::new(Axis::Neg, Axis::Neg);
    const SE: Self = Self::new(Axis::Pos, Axis::Pos);
    const SW: Self = Self::new(Axis::Neg, Axis::Pos);

    const fn new(x: Axis, y: Axis) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy)]
struct Selection {
    pos: Vec2,
    w: f64,
    h: f64,
}

impl Selection {
    fn max_centered(bounds: Vec2, ratio: Option<f64>) -> Selection {
        if let Some(r) = ratio {
            let h = (bounds.x / r).min(bounds.y);
            let w = h * r;
            Selection {
                pos: Vec2::new((bounds.x - w) / 2.0, (bounds.y - h) / 2.0),
                w,
                h,
            }
        } else {
            Selection {
                pos: Vec2::ZERO,
                w: bounds.x,
                h: bounds.y,
            }
        }
    }

    fn from_drag(a: Vec2, b: Vec2, ratio: Option<f64>) -> Self {
        let d = b - a;

        let (w, h) = if let Some(r) = ratio {
            let h = (d.x.abs() / r).min(d.y.abs());
            (h * r, h)
        } else {
            (d.x.abs(), d.y.abs())
        };

        let w = w.max(0.0);
        let h = h.max(0.0);

        let x = if d.x >= 0.0 { a.x } else { a.x - w };
        let y = if d.y >= 0.0 { a.y } else { a.y - h };

        Self {
            pos: Vec2::new(x, y),
            w,
            h,
        }
    }

    fn center(self) -> Vec2 {
        Vec2::new(self.pos.x + self.w / 2.0, self.pos.y + self.h / 2.0)
    }

    fn bottom_right(self) -> Vec2 {
        Vec2::new(self.pos.x + self.w, self.pos.y + self.h)
    }

    fn to_drag(self) -> (Vec2, Vec2) {
        (self.pos, self.bottom_right())
    }

    fn style(self) -> String {
        let Self {
            pos: Vec2 { x, y },
            w,
            h,
        } = self;

        format!("left:{x:.0}px;top:{y:.0}px;width:{w:.0}px;height:{h:.0}px")
    }

    fn resized(self, dir: Dir, delta: Vec2, bounds: Vec2, ratio: Option<f64>) -> Self {
        let (new_w, new_h) = if let Some(r) = ratio {
            let dh_x = dir.x.apply(delta.x) / r;
            let dh_y = dir.y.apply(delta.y);

            let new_h = match (dir.x, dir.y) {
                (Axis::Zero, _) => self.h + dh_y,
                (_, Axis::Zero) => self.h + dh_x,
                _ => (self.h + dh_x).min(self.h + dh_y),
            };

            let new_h = new_h.max(0.0);
            (new_h * r, new_h)
        } else {
            let new_w = match dir.x {
                Axis::Zero => self.w,
                _ => (self.w + dir.x.apply(delta.x)).max(0.0),
            };

            let new_h = match dir.y {
                Axis::Zero => self.h,
                _ => (self.h + dir.y.apply(delta.y)).max(0.0),
            };

            (new_w, new_h)
        };

        let c = self.center();
        let anchor_x = dir
            .x
            .anchor(self.pos.x, self.w, new_w, c.x)
            .clamp(0.0, bounds.x);
        let anchor_y = dir
            .y
            .anchor(self.pos.y, self.h, new_h, c.y)
            .clamp(0.0, bounds.y);
        let max_w = dir.x.max_extent(self.pos.x, self.w, anchor_x, bounds.x);
        let max_h = dir.y.max_extent(self.pos.y, self.h, anchor_y, bounds.y);

        let (final_w, final_h) = if ratio.is_some() {
            let scale = if new_w > 0.0 && new_h > 0.0 {
                1.0f64.min(max_w / new_w).min(max_h / new_h)
            } else {
                1.0
            };
            (new_w * scale, new_h * scale)
        } else {
            (new_w.min(max_w), new_h.min(max_h))
        };

        let final_x = dir
            .x
            .anchor(self.pos.x, self.w, final_w, c.x)
            .clamp(0.0, bounds.x);
        let final_y = dir
            .y
            .anchor(self.pos.y, self.h, final_h, c.y)
            .clamp(0.0, bounds.y);

        Self {
            pos: Vec2::new(final_x, final_y),
            w: final_w,
            h: final_h,
        }
    }

    fn moved(self, delta: Vec2, bounds: Vec2) -> Self {
        Self {
            pos: Vec2 {
                x: (self.pos.x + delta.x).clamp(0.0, (bounds.x - self.w).max(0.0)),
                y: (self.pos.y + delta.y).clamp(0.0, (bounds.y - self.h).max(0.0)),
            },
            ..self
        }
    }

    fn to_crop_region(self, client: Vec2, natural: Vec2) -> Option<api::CropRegion> {
        let scale_x = if client.x > 0.0 {
            natural.x / client.x
        } else {
            1.0
        };
        let scale_y = if client.y > 0.0 {
            natural.y / client.y
        } else {
            1.0
        };

        let x1 = (self.pos.x * scale_x).clamp(0.0, natural.x) as u32;
        let y1 = (self.pos.y * scale_y).clamp(0.0, natural.y) as u32;
        let x2 = ((self.pos.x + self.w) * scale_x).clamp(0.0, natural.x) as u32;
        let y2 = ((self.pos.y + self.h) * scale_y).clamp(0.0, natural.y) as u32;

        if x2 <= x1 || y2 <= y1 {
            return None;
        }

        Some(api::CropRegion { x1, y1, x2, y2 })
    }
}

struct MoveState {
    start: Vec2,
    selection: Selection,
}

struct ResizeState {
    dir: Dir,
    start: Vec2,
    selection: Selection,
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) source_url: AttrValue,
    pub(crate) onconfirm: Callback<api::CropRegion>,
    pub(crate) oncancel: Callback<()>,
    #[prop_or_default]
    pub(crate) onratio: Option<Callback<f64>>,
    #[prop_or_default]
    pub(crate) ratio: Option<f64>,
}

pub(crate) enum Msg {
    MoveStart(PointerEvent),
    ResizeStart(PointerEvent, Dir),
    PointerDown(PointerEvent),
    PointerMove(PointerEvent),
    PointerUp(PointerEvent),
    ClearSelection,
    SelectAll,
    Rescale,
    Confirm,
    Cancel,
}

pub(crate) struct CropModal {
    empty_crop_region: bool,
    drag: Option<(Vec2, Vec2)>,
    dragging: bool,
    move_state: Option<MoveState>,
    resize_state: Option<ResizeState>,
    image_ref: NodeRef,
    div_ref: NodeRef,
}

impl Component for CropModal {
    type Message = Msg;
    type Properties = Props;

    fn create(_: &Context<Self>) -> Self {
        Self {
            empty_crop_region: false,
            drag: None,
            dragging: false,
            move_state: None,
            resize_state: None,
            image_ref: NodeRef::default(),
            div_ref: NodeRef::default(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                tracing::error!(%error, "crop_modal::update");
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let selection = self.selection(ctx);

        let confirm_disabled = selection.is_none();

        let confirm_class = classes! {
            "btn",
            "primary",
            confirm_disabled.then_some("disabled"),
        };

        let confirm_onclick = (!confirm_disabled).then(|| ctx.link().callback(|_| Msg::Confirm));

        let rs = |dir: Dir| {
            ctx.link()
                .callback(move |ev: PointerEvent| Msg::ResizeStart(ev, dir))
        };

        let stop_propagation = |ev: MouseEvent| ev.stop_propagation();
        let selection_class = classes!("crop-selection", self.dragging.then_some("dragging"));

        html! {
            <Modal title="Crop Image" onclose={ctx.link().callback(|_| Msg::Cancel)} onclick={ctx.link().callback(|_| Msg::ClearSelection)} class="rows">
                <p class="hint">{"Drag on the image to select a crop region."}</p>

                <div class="crop-area" onclick={stop_propagation}>
                    <div
                        class="crop-inner"
                        ref={self.div_ref.clone()}
                        onpointerdown={ctx.link().callback(Msg::PointerDown)}
                        onpointermove={ctx.link().callback(Msg::PointerMove)}
                        onpointerup={ctx.link().callback(Msg::PointerUp)}
                    >
                        <img key="image" src={ctx.props().source_url.clone()} ref={self.image_ref.clone()} class="crop-source" draggable="false" />

                        if let Some(selection) = selection {
                            <div key="selection" onpointerdown={ctx.link().callback(Msg::MoveStart)} style={selection.style()} class={selection_class}>
                                for &(handle, dir) in HANDLES {
                                    <div class={classes!("crop-handle", handle)} onpointerdown={rs(dir)} />
                                }
                            </div>
                        }
                    </div>
                </div>

                if self.empty_crop_region {
                    <p class="hint error">{"Crop region cannot be empty."}</p>
                }

                <div class="control-group" onclick={stop_propagation}>
                    <button class={confirm_class} onclick={confirm_onclick}>
                        {"Upload"}
                    </button>

                    {if ctx.props().onratio.is_some() {
                        html! {
                            <button class="btn secondary" title="Rescale to original aspect ratio" onclick={ctx.link().callback(|_| Msg::Rescale)}>
                                {"Select All"}
                            </button>
                        }
                    } else {
                        html! {
                            <button class="btn secondary" title="Select the largest region" onclick={ctx.link().callback(|_| Msg::SelectAll)}>
                                {"Select All"}
                            </button>
                        }
                    }}

                    <section class="fill" />

                    <button class="btn danger right" onclick={ctx.link().callback(|_| Msg::Cancel)}>
                        {"Cancel"}
                    </button>
                </div>
            </Modal>
        }
    }
}

impl CropModal {
    fn selection(&self, ctx: &Context<Self>) -> Option<Selection> {
        let (a, b) = self.drag?;
        Some(Selection::from_drag(a, b, ctx.props().ratio))
    }

    fn image_bounds(&self) -> Vec2 {
        self.image_ref
            .cast::<HtmlImageElement>()
            .map(|image| Vec2::new(image.client_width() as f64, image.client_height() as f64))
            .unwrap_or(Vec2::ZERO)
    }

    fn capture_pointer(&self, ev: &PointerEvent) {
        if let Some(div) = self.div_ref.cast::<web_sys::Element>() {
            let _ = div.set_pointer_capture(ev.pointer_id());
        }
    }

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::MoveStart(e) => {
                self.move_start(ctx, e);
                Ok(false)
            }
            Msg::ResizeStart(e, dir) => {
                self.resize_start(ctx, e, dir);
                Ok(false)
            }
            Msg::PointerDown(e) => Ok(self.pointer_down(e)),
            Msg::PointerMove(e) => Ok(self.pointer_move(ctx, e)),
            Msg::PointerUp(e) => Ok(self.pointer_up(ctx, e)),
            Msg::ClearSelection => {
                self.drag = None;
                Ok(true)
            }
            Msg::SelectAll => {
                let selection = Selection::max_centered(self.image_bounds(), ctx.props().ratio);
                self.drag = Some(selection.to_drag());
                Ok(true)
            }
            Msg::Rescale => {
                let Some(onratio) = &ctx.props().onratio else {
                    return Ok(false);
                };

                let bounds = self.image_bounds();

                onratio.emit(bounds.x / bounds.y);

                let selection = Selection::max_centered(bounds, ctx.props().ratio);
                self.drag = Some(selection.to_drag());
                Ok(true)
            }
            Msg::Confirm => self.on_confirm(ctx),
            Msg::Cancel => {
                ctx.props().oncancel.emit(());
                Ok(false)
            }
        }
    }

    fn pointer_down(&mut self, ev: PointerEvent) -> bool {
        if ev.button() != 0 || self.dragging {
            return false;
        }

        self.capture_pointer(&ev);

        let p = Vec2::offset(&ev).clamp(Vec2::ZERO, self.image_bounds());
        self.drag = Some((p, p));
        self.move_state = None;
        self.dragging = true;
        true
    }

    fn move_start(&mut self, ctx: &Context<Self>, ev: PointerEvent) {
        if ev.button() != 0 || self.dragging {
            return;
        }

        ev.stop_propagation();

        let Some(selection) = self.selection(ctx) else {
            return;
        };

        self.capture_pointer(&ev);

        self.move_state = Some(MoveState {
            start: Vec2::client(&ev),
            selection,
        });

        self.resize_state = None;
        self.dragging = true;
    }

    fn resize_start(&mut self, ctx: &Context<Self>, ev: PointerEvent, dir: Dir) {
        if ev.button() != 0 {
            return;
        }

        ev.stop_propagation();

        let Some(selection) = self.selection(ctx) else {
            return;
        };

        self.capture_pointer(&ev);

        self.resize_state = Some(ResizeState {
            dir,
            start: Vec2::client(&ev),
            selection,
        });

        self.move_state = None;
        self.dragging = true;
    }

    fn pointer_move(&mut self, ctx: &Context<Self>, ev: PointerEvent) -> bool {
        if !self.dragging {
            return false;
        }

        let bounds = self.image_bounds();

        if let Some(rs) = &self.resize_state {
            let delta = Vec2::client(&ev) - rs.start;

            self.drag = Some(
                rs.selection
                    .resized(rs.dir, delta, bounds, ctx.props().ratio)
                    .to_drag(),
            );

            return true;
        }

        if let Some(ms) = &self.move_state {
            let delta = Vec2::client(&ev) - ms.start;

            if delta == Vec2::ZERO {
                self.drag = None;
            } else {
                self.drag = Some(ms.selection.moved(delta, bounds).to_drag());
            }

            return true;
        }

        if let Some((anchor, _)) = self.drag {
            let cursor = Vec2::offset(&ev).clamp(Vec2::ZERO, bounds);
            self.drag = Some((anchor, cursor));
        }

        true
    }

    fn pointer_up(&mut self, ctx: &Context<Self>, ev: PointerEvent) -> bool {
        if !self.dragging {
            return false;
        }

        let bounds = self.image_bounds();

        self.dragging = false;

        if let Some(rs) = self.resize_state.take() {
            let delta = Vec2::client(&ev) - rs.start;

            let new = rs
                .selection
                .resized(rs.dir, delta, bounds, ctx.props().ratio);

            self.drag = if new.h < MIN_SIZE {
                None
            } else {
                Some(new.to_drag())
            };

            return true;
        }

        if let Some(ms) = self.move_state.take() {
            let delta = Vec2::client(&ev) - ms.start;

            self.drag = Some(ms.selection.moved(delta, bounds).to_drag());
            return true;
        }

        if let Some((anchor, _)) = self.drag {
            let cursor = Vec2::offset(&ev).clamp(Vec2::ZERO, bounds);

            if anchor == cursor {
                self.drag = None;
            } else {
                self.drag = Some((anchor, cursor));
            }

            return true;
        }

        true
    }

    fn on_confirm(&mut self, ctx: &Context<Self>) -> Result<bool, Error> {
        let img = self
            .image_ref
            .cast::<HtmlImageElement>()
            .ok_or("no crop image")?;

        let Some(selection) = self.selection(ctx) else {
            return Ok(false);
        };

        let client = Vec2::new(img.client_width() as f64, img.client_height() as f64);
        let natural = Vec2::new(img.natural_width() as f64, img.natural_height() as f64);

        let Some(region) = selection.to_crop_region(client, natural) else {
            self.empty_crop_region = true;
            return Ok(true);
        };

        ctx.props().onconfirm.emit(region);
        Ok(false)
    }
}
