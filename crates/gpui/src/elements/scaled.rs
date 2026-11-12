use crate::{
    AnyElement, App, Bounds, Element, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    Pixels, Point, Window, point, px,
};

/// Builds a [`Scaled`] element, which paints its child scaled about a point of its own
/// bounds, such as a popover zooming in from its anchor.
pub fn scaled(child: impl IntoElement) -> Scaled {
    Scaled {
        child: child.into_any_element(),
        scale: 1.0,
        origin: point(0.5, 0.5),
    }
}

/// Paints its child scaled, like CSS `transform: scale()`: layout is unchanged, and
/// text, SVGs and images are rasterized at the scaled size, so they stay sharp while
/// a scale animates. Hitboxes stay where the unscaled child would be, and views
/// cached with `.cached()` inside it aren't supported.
pub struct Scaled {
    child: AnyElement,
    scale: f32,
    origin: Point<f32>,
}

impl Scaled {
    /// The scale factor (1.0 paints the child as is).
    pub fn scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// The fixed point, as a fraction of the element's bounds: `point(0.5, 0.5)`
    /// (the default) is its center, `point(1.0, 0.0)` its top-right corner.
    pub fn origin(mut self, origin: Point<f32>) -> Self {
        self.origin = origin;
        self
    }

    fn active(&self) -> bool {
        self.scale > 0.0 && self.scale != 1.0
    }
}

impl Element for Scaled {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<crate::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if !self.active() {
            self.child.prepaint(window, cx);
            return;
        }
        // Painting at `scale` times the scale factor scales about the window's
        // origin; shifting the child first by o·(1 − s)/s moves the fixed point to o.
        let s = self.scale;
        let o = point(
            f32::from(bounds.origin.x) + f32::from(bounds.size.width) * self.origin.x,
            f32::from(bounds.origin.y) + f32::from(bounds.size.height) * self.origin.y,
        );
        let shift = point(px(o.x * (1.0 - s) / s), px(o.y * (1.0 - s) / s));
        window.with_element_offset(shift, |window| self.child.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if !self.active() {
            self.child.paint(window, cx);
            return;
        }
        window.with_paint_scale(self.scale, |window| self.child.paint(window, cx));
    }
}

impl IntoElement for Scaled {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}
