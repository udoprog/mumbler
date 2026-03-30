use core::ops::{Add, Div, Sub};

use web_sys::{HtmlImageElement, MouseEvent, PointerEvent};
use yew::prelude::*;

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

const MIN_SIZE: f64 = 2.0;

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

    #[inline]
    const fn new(x: Axis, y: Axis) -> Self {
        Self { x, y }
    }

    #[inline]
    fn anchor(self, pos: Vec2, old: Vec2, new: Vec2, center: Vec2) -> Vec2 {
        let x = self.x.anchor(pos.x, old.x, new.x, center.x);
        let y = self.y.anchor(pos.y, old.y, new.y, center.y);
        Vec2::new(x, y)
    }

    #[inline]
    fn extent(self, pos: Vec2, old: Vec2, new_pos: Vec2, bound: Vec2) -> Vec2 {
        let x = self.x.extent(pos.x, old.x, new_pos.x, bound.x);
        let y = self.y.extent(pos.y, old.y, new_pos.y, bound.y);
        Vec2::new(x, y)
    }
}

#[derive(Clone, Copy)]
struct Selection {
    pos: Vec2,
    size: Vec2,
}

impl Selection {
    fn max_centered(bounds: Vec2, ratio: Option<f64>) -> Selection {
        if let Some(ratio) = ratio {
            let height = (bounds.x / ratio).min(bounds.y);
            let width = height * ratio;

            Selection {
                pos: Vec2::new((bounds.x - width) / 2.0, (bounds.y - height) / 2.0),
                size: Vec2::new(width, height),
            }
        } else {
            Selection {
                pos: Vec2::ZERO,
                size: bounds,
            }
        }
    }

    fn from_extent(extent: &Extent, ratio: Option<f64>) -> Self {
        let delta = extent.p2 - extent.p1;

        let size = if let Some(ratio) = ratio {
            let height = (delta.x.abs() / ratio).min(delta.y.abs());
            let width = height * ratio;
            Vec2::new(width, height)
        } else {
            Vec2::new(delta.x.abs(), delta.y.abs())
        };

        let size = size.max(Vec2::ZERO);

        let x = if delta.x >= 0.0 {
            extent.p1.x
        } else {
            extent.p1.x - size.x
        };
        let y = if delta.y >= 0.0 {
            extent.p1.y
        } else {
            extent.p1.y - size.y
        };

        Self {
            pos: Vec2::new(x, y),
            size,
        }
    }

    #[inline]
    fn center(self) -> Vec2 {
        self.pos + self.size / 2.0
    }

    #[inline]
    fn bottom_right(self) -> Vec2 {
        self.pos + self.size
    }

    #[inline]
    fn to_extent(self) -> Extent {
        Extent {
            p1: self.pos,
            p2: self.bottom_right(),
        }
    }

    #[inline]
    fn style(self) -> String {
        let Self {
            pos: Vec2 { x, y },
            size: Vec2 {
                x: width,
                y: height,
            },
        } = self;

        format!("left:{x:.0}px; top:{y:.0}px; width:{width:.0}px; height:{height:.0}px;")
    }

    fn resized(self, dir: Dir, delta: Vec2, bounds: Vec2, ratio: Option<f64>) -> Self {
        let new = if let Some(r) = ratio {
            let dh_x = dir.x.apply(delta.x) / r;
            let dh_y = dir.y.apply(delta.y);

            let new_h = match (dir.x, dir.y) {
                (Axis::Zero, _) => self.size.y + dh_y,
                (_, Axis::Zero) => self.size.y + dh_x,
                _ => (self.size.y + dh_x).min(self.size.y + dh_y),
            };

            let new_h = new_h.max(0.0);
            Vec2::new(new_h * r, new_h)
        } else {
            let new_w = match dir.x {
                Axis::Zero => self.size.x,
                _ => (self.size.x + dir.x.apply(delta.x)).max(0.0),
            };

            let new_h = match dir.y {
                Axis::Zero => self.size.y,
                _ => (self.size.y + dir.y.apply(delta.y)).max(0.0),
            };

            Vec2::new(new_w, new_h)
        };

        let c = self.center();

        let anchor = dir
            .anchor(self.pos, self.size, new, c)
            .clamp(Vec2::ZERO, bounds);

        let max = dir.extent(self.pos, self.size, anchor, bounds);

        let size = if ratio.is_some() {
            let scale = if new.x > 0.0 && new.y > 0.0 {
                1.0f64.min(max.x / new.x).min(max.y / new.y)
            } else {
                1.0
            };

            Vec2::new(new.x * scale, new.y * scale)
        } else {
            new.min(max)
        };

        let pos = dir
            .anchor(self.pos, self.size, size, c)
            .clamp(Vec2::ZERO, bounds);

        Self { pos, size }
    }

    #[inline]
    fn moved(self, delta: Vec2, bounds: Vec2) -> Self {
        Self {
            pos: (self.pos + delta).clamp(Vec2::ZERO, bounds - self.size),
            ..self
        }
    }

    fn to_crop_region(self, client: Vec2, natural: Vec2) -> Option<api::CropRegion> {
        if client.x <= 0.0 || client.y <= 0.0 {
            return None;
        }

        let scale_x = natural.x / client.x;
        let scale_y = natural.y / client.y;

        let x1 = to_u32((self.pos.x * scale_x).clamp(0.0, natural.x))?;
        let y1 = to_u32((self.pos.y * scale_y).clamp(0.0, natural.y))?;
        let x2 = to_u32(((self.pos.x + self.size.x) * scale_x).clamp(0.0, natural.x))?;
        let y2 = to_u32(((self.pos.y + self.size.y) * scale_y).clamp(0.0, natural.y))?;

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
    pub(crate) drag: Option<Extent>,
    pub(crate) ondrag: Callback<Option<Extent>>,
    pub(crate) source_url: AttrValue,
    pub(crate) onconfirm: Callback<api::CropRegion>,
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
    SelectAll,
    Rescale,
    Confirm,
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub(crate) struct Extent {
    p1: Vec2,
    p2: Vec2,
}

pub(crate) struct Crop {
    empty_crop_region: bool,
    dragging: bool,
    move_state: Option<MoveState>,
    resize_state: Option<ResizeState>,
    image_ref: NodeRef,
    div_ref: NodeRef,
}

impl Component for Crop {
    type Message = Msg;
    type Properties = Props;

    fn create(_: &Context<Self>) -> Self {
        Self {
            empty_crop_region: false,
            dragging: false,
            move_state: None,
            resize_state: None,
            image_ref: NodeRef::default(),
            div_ref: NodeRef::default(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::MoveStart(e) => {
                self.move_start(ctx, e);
                false
            }
            Msg::ResizeStart(e, dir) => {
                self.resize_start(ctx, e, dir);
                false
            }
            Msg::PointerDown(e) => self.pointer_down(ctx, e),
            Msg::PointerMove(e) => self.pointer_move(ctx, e),
            Msg::PointerUp(e) => self.pointer_up(ctx, e),
            Msg::SelectAll => {
                let selection = Selection::max_centered(self.image_bounds(), ctx.props().ratio);
                ctx.props().ondrag.emit(Some(selection.to_extent()));
                false
            }
            Msg::Rescale => {
                let Some(onratio) = &ctx.props().onratio else {
                    return false;
                };

                let bounds = self.image_bounds();

                tracing::warn!(
                    ?bounds,
                    ratio = bounds.x / bounds.y,
                    "rescaling crop selection"
                );

                onratio.emit(bounds.x / bounds.y);
                ctx.props()
                    .ondrag
                    .emit(Some(Selection::max_centered(bounds, None).to_extent()));
                false
            }
            Msg::Confirm => self.on_confirm(ctx),
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
            <>
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
                </div>
            </>
        }
    }
}

impl Crop {
    fn selection(&self, ctx: &Context<Self>) -> Option<Selection> {
        let extent = ctx.props().drag.as_ref()?;
        Some(Selection::from_extent(extent, ctx.props().ratio))
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

    fn pointer_down(&mut self, ctx: &Context<Self>, ev: PointerEvent) -> bool {
        if ev.button() != 0 || self.dragging {
            return false;
        }

        self.dragging = true;

        self.capture_pointer(&ev);

        let p = Vec2::offset(&ev).clamp(Vec2::ZERO, self.image_bounds());
        ctx.props().ondrag.emit(Some(Extent { p1: p, p2: p }));
        self.move_state = None;
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

            let drag = Some(
                rs.selection
                    .resized(rs.dir, delta, bounds, ctx.props().ratio)
                    .to_extent(),
            );

            ctx.props().ondrag.emit(drag);
            return false;
        }

        if let Some(ms) = &self.move_state {
            let delta = Vec2::client(&ev) - ms.start;

            ctx.props()
                .ondrag
                .emit(Some(ms.selection.moved(delta, bounds).to_extent()));

            return false;
        }

        if let Some(Extent { p1, .. }) = ctx.props().drag {
            let cursor = Vec2::offset(&ev).clamp(Vec2::ZERO, bounds);
            ctx.props().ondrag.emit(Some(Extent { p1, p2: cursor }));
        }

        false
    }

    fn pointer_up(&mut self, ctx: &Context<Self>, ev: PointerEvent) -> bool {
        if !self.dragging {
            return false;
        }

        self.dragging = false;

        let bounds = self.image_bounds();

        if let Some(rs) = self.resize_state.take() {
            let delta = Vec2::client(&ev) - rs.start;

            let new = rs
                .selection
                .resized(rs.dir, delta, bounds, ctx.props().ratio);

            ctx.props().ondrag.emit(if new.size.y < MIN_SIZE {
                None
            } else {
                Some(new.to_extent())
            });

            return true;
        }

        if let Some(ms) = self.move_state.take() {
            let delta = Vec2::client(&ev) - ms.start;
            ctx.props()
                .ondrag
                .emit(Some(ms.selection.moved(delta, bounds).to_extent()));
            return true;
        }

        if let Some(Extent { p1, .. }) = ctx.props().drag {
            let p2 = Vec2::offset(&ev).clamp(Vec2::ZERO, bounds);

            let drag = if p2.manhattan_distance(p1) < MIN_SIZE {
                None
            } else {
                Some(Extent { p1, p2 })
            };

            ctx.props().ondrag.emit(drag);
            return true;
        }

        true
    }

    fn on_confirm(&mut self, ctx: &Context<Self>) -> bool {
        let Some(img) = self.image_ref.cast::<HtmlImageElement>() else {
            return false;
        };

        let Some(selection) = self.selection(ctx) else {
            return false;
        };

        let client = Vec2::new(img.client_width() as f64, img.client_height() as f64);
        let natural = Vec2::new(img.natural_width() as f64, img.natural_height() as f64);

        let Some(region) = selection.to_crop_region(client, natural) else {
            self.empty_crop_region = true;
            return true;
        };

        ctx.props().onconfirm.emit(region);
        false
    }
}

#[derive(Clone, Copy)]
enum Axis {
    Neg,
    Zero,
    Pos,
}

impl Axis {
    #[inline]
    fn apply(self, v: f64) -> f64 {
        match self {
            Self::Neg => -v,
            Self::Zero => 0.0,
            Self::Pos => v,
        }
    }

    #[inline]
    fn anchor(self, pos: f64, old: f64, new: f64, center: f64) -> f64 {
        match self {
            Self::Neg => pos + old - new,
            Self::Zero => center - new / 2.0,
            Self::Pos => pos,
        }
    }

    #[inline]
    fn extent(self, pos: f64, old: f64, new_pos: f64, bound: f64) -> f64 {
        match self {
            Self::Neg => (pos + old) - new_pos,
            Self::Zero | Self::Pos => bound - new_pos,
        }
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
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
    fn manhattan(self) -> f64 {
        self.x.abs() + self.y.abs()
    }

    #[inline]
    fn manhattan_distance(self, other: Self) -> f64 {
        (self - other).manhattan()
    }

    #[inline]
    fn offset(ev: &MouseEvent) -> Self {
        Self::new(ev.offset_x() as f64, ev.offset_y() as f64)
    }

    #[inline]
    fn client(ev: &MouseEvent) -> Self {
        Self::new(ev.client_x() as f64, ev.client_y() as f64)
    }

    #[inline]
    fn clamp(self, min: Self, max: Self) -> Self {
        Self::new(self.x.clamp(min.x, max.x), self.y.clamp(min.y, max.y))
    }

    #[inline]
    fn max(self, other: Self) -> Self {
        Self::new(self.x.max(other.x), self.y.max(other.y))
    }

    #[inline]
    fn min(self, other: Self) -> Self {
        Self::new(self.x.min(other.x), self.y.min(other.y))
    }
}

impl Sub for Vec2 {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Add for Vec2 {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Div<f64> for Vec2 {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f64) -> Self {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

#[inline]
fn to_u32(v: f64) -> Option<u32> {
    let v = v.round();

    if v >= 0.0 && v <= u32::MAX as f64 {
        return Some(v as u32);
    }

    None
}
