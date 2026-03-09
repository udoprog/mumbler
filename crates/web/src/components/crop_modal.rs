use std::ops::{Add, Sub};

use web_sys::{HtmlImageElement, MouseEvent, PointerEvent};
use yew::prelude::*;

use crate::error::Error;

use super::Icon;

#[derive(Clone, Copy)]
struct Vec2 {
    x: f64,
    y: f64,
}

impl Vec2 {
    fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn from_offset(e: &PointerEvent) -> Self {
        Self::new(e.offset_x() as f64, e.offset_y() as f64)
    }

    fn from_client(e: &PointerEvent) -> Self {
        Self::new(e.client_x() as f64, e.client_y() as f64)
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

const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
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
                pos: ZERO,
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
    sel: Selection,
}

struct ResizeState {
    dir: Dir,
    start: Vec2,
    sel: Selection,
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) source_url: AttrValue,
    pub(crate) on_confirm: Callback<api::CropRegion>,
    pub(crate) on_cancel: Callback<()>,
    #[prop_or_default]
    pub(crate) ratio: Option<f64>,
}

pub(crate) enum Msg {
    DragStart(PointerEvent),
    MoveStart(PointerEvent),
    ResizeStart(PointerEvent, Dir),
    DragMove(PointerEvent),
    DragEnd(PointerEvent),
    ClearSelection,
    SelectAll,
    Confirm,
    Cancel,
}

pub(crate) struct CropModal {
    ratio: Option<f64>,
    drag: Option<(Vec2, Vec2)>,
    dragging: bool,
    move_state: Option<MoveState>,
    resize_state: Option<ResizeState>,
    img_ref: NodeRef,
    div_ref: NodeRef,
}

impl Component for CropModal {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        Self {
            ratio: ctx.props().ratio,
            drag: None,
            dragging: false,
            move_state: None,
            resize_state: None,
            img_ref: NodeRef::default(),
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
        let sel = self.selection();
        let confirm_disabled = sel.is_none();
        let rs = |dir: Dir| {
            ctx.link()
                .callback(move |e: PointerEvent| Msg::ResizeStart(e, dir))
        };

        html! {
            <div class="modal-backdrop" onclick={ctx.link().callback(|_| Msg::Cancel)}>
                <div class="modal" onclick={|e: MouseEvent| e.stop_propagation()}>
                    <div class="modal-header">
                        <h2>{"Crop Image"}</h2>
                        <button class="btn sm square danger" title="Close"
                            onclick={ctx.link().callback(|_| Msg::Cancel)}>
                            <Icon name="x-mark" />
                        </button>
                    </div>
                    <div class="modal-body rows"
                        onclick={ctx.link().callback(|_| Msg::ClearSelection)}>
                        <p class="hint">{"Drag on the image to select a crop region."}</p>
                        <div class="crop-area"
                            onclick={|e: MouseEvent| e.stop_propagation()}>
                            <div class="crop-inner"
                                ref={self.div_ref.clone()}
                                onpointerdown={ctx.link().callback(Msg::DragStart)}
                                onpointermove={ctx.link().callback(Msg::DragMove)}
                                onpointerup={ctx.link().callback(Msg::DragEnd)}>
                                <img src={ctx.props().source_url.clone()}
                                    ref={self.img_ref.clone()}
                                    class="crop-source"
                                    draggable="false" />
                                if let Some(sel) = sel {
                                    <div class="crop-selection"
                                        onpointerdown={ctx.link().callback(Msg::MoveStart)}
                                        style={sel.style()}>
                                        <div class="crop-handle nw" onpointerdown={rs(Dir::NW)} />
                                        <div class="crop-handle n"  onpointerdown={rs(Dir::N)} />
                                        <div class="crop-handle ne" onpointerdown={rs(Dir::NE)} />
                                        <div class="crop-handle w"  onpointerdown={rs(Dir::W)} />
                                        <div class="crop-handle e"  onpointerdown={rs(Dir::E)} />
                                        <div class="crop-handle sw" onpointerdown={rs(Dir::SW)} />
                                        <div class="crop-handle s"  onpointerdown={rs(Dir::S)} />
                                        <div class="crop-handle se" onpointerdown={rs(Dir::SE)} />
                                    </div>
                                }
                            </div>
                        </div>
                        <div class="btn-group" onclick={|e: MouseEvent| e.stop_propagation()}>
                            <button class="btn primary" disabled={confirm_disabled}
                                onclick={ctx.link().callback(|_| Msg::Confirm)}>
                                {"Upload"}
                            </button>
                            <button class="btn" title="Select the largest region"
                                onclick={ctx.link().callback(|_| Msg::SelectAll)}>
                                {"Select All"}
                            </button>
                            <button class="btn danger"
                                onclick={ctx.link().callback(|_| Msg::Cancel)}>
                                {"Cancel"}
                            </button>
                        </div>
                    </div>
                </div>
            </div>
        }
    }
}

impl CropModal {
    fn selection(&self) -> Option<Selection> {
        let (a, b) = self.drag?;
        Some(Selection::from_drag(a, b, self.ratio))
    }

    fn image_bounds(&self) -> Vec2 {
        self.img_ref
            .cast::<HtmlImageElement>()
            .map(|img| Vec2::new(img.client_width() as f64, img.client_height() as f64))
            .unwrap_or(ZERO)
    }

    fn capture_pointer(&self, e: &PointerEvent) {
        if let Some(div) = self.div_ref.cast::<web_sys::Element>() {
            let _ = div.set_pointer_capture(e.pointer_id());
        }
    }

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::DragStart(e) => Ok(self.on_drag_start(e)),
            Msg::MoveStart(e) => {
                self.on_move_start(e);
                Ok(false)
            }
            Msg::ResizeStart(e, dir) => {
                self.on_resize_start(e, dir);
                Ok(false)
            }
            Msg::DragMove(e) => Ok(self.on_drag_move(e)),
            Msg::DragEnd(e) => Ok(self.on_drag_end(e)),
            Msg::ClearSelection => {
                self.drag = None;
                Ok(true)
            }
            Msg::SelectAll => {
                let sel = Selection::max_centered(self.image_bounds(), self.ratio);
                self.drag = Some(sel.to_drag());
                Ok(true)
            }
            Msg::Confirm => {
                self.on_confirm(ctx)?;
                Ok(false)
            }
            Msg::Cancel => {
                ctx.props().on_cancel.emit(());
                Ok(false)
            }
        }
    }

    fn on_drag_start(&mut self, e: PointerEvent) -> bool {
        if e.button() != 0 || self.dragging {
            return false;
        }

        self.capture_pointer(&e);
        let p = Vec2::from_offset(&e).clamp(ZERO, self.image_bounds());
        self.drag = Some((p, p));
        self.move_state = None;
        self.dragging = true;
        true
    }

    fn on_move_start(&mut self, e: PointerEvent) {
        if e.button() != 0 || self.dragging {
            return;
        }

        let Some(sel) = self.selection() else {
            return;
        };

        e.stop_propagation();
        self.capture_pointer(&e);
        self.move_state = Some(MoveState {
            start: Vec2::from_client(&e),
            sel,
        });
        self.resize_state = None;
        self.dragging = true;
    }

    fn on_resize_start(&mut self, e: PointerEvent, dir: Dir) {
        if e.button() != 0 {
            return;
        }

        let Some(sel) = self.selection() else {
            return;
        };

        e.stop_propagation();
        self.capture_pointer(&e);
        self.resize_state = Some(ResizeState {
            dir,
            start: Vec2::from_client(&e),
            sel,
        });
        self.move_state = None;
        self.dragging = true;
    }

    fn on_drag_move(&mut self, e: PointerEvent) -> bool {
        if !self.dragging {
            return false;
        }

        let bounds = self.image_bounds();

        if let Some(rs) = &self.resize_state {
            let delta = Vec2::from_client(&e) - rs.start;
            self.drag = Some(rs.sel.resized(rs.dir, delta, bounds, self.ratio).to_drag());
            return true;
        }

        if let Some(ms) = &self.move_state {
            let delta = Vec2::from_client(&e) - ms.start;
            self.drag = Some(ms.sel.moved(delta, bounds).to_drag());
            return true;
        }

        if let Some((anchor, _)) = self.drag {
            let cursor = Vec2::from_offset(&e).clamp(ZERO, bounds);
            self.drag = Some((anchor, cursor));
        }

        true
    }

    fn on_drag_end(&mut self, e: PointerEvent) -> bool {
        if !self.dragging {
            return false;
        }

        let bounds = self.image_bounds();

        if let Some(rs) = self.resize_state.take() {
            let delta = Vec2::from_client(&e) - rs.start;
            let new = rs.sel.resized(rs.dir, delta, bounds, self.ratio);
            self.drag = if new.h < MIN_SIZE {
                None
            } else {
                Some(new.to_drag())
            };
        } else if let Some(ms) = self.move_state.take() {
            let delta = Vec2::from_client(&e) - ms.start;
            self.drag = Some(ms.sel.moved(delta, bounds).to_drag());
        } else if let Some((anchor, _)) = self.drag {
            let cursor = Vec2::from_offset(&e).clamp(ZERO, bounds);
            self.drag = Some((anchor, cursor));
            if self.selection().is_none() {
                self.drag = None;
            }
        }

        self.dragging = false;
        true
    }

    fn on_confirm(&mut self, ctx: &Context<Self>) -> Result<(), Error> {
        let img = self
            .img_ref
            .cast::<HtmlImageElement>()
            .ok_or("no crop image")?;

        let sel = self.selection().ok_or("no crop selection")?;
        let client = Vec2::new(img.client_width() as f64, img.client_height() as f64);
        let natural = Vec2::new(img.natural_width() as f64, img.natural_height() as f64);
        let region = sel
            .to_crop_region(client, natural)
            .ok_or("crop region empty")?;
        ctx.props().on_confirm.emit(region);
        Ok(())
    }
}
