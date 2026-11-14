//! Native backdrops: each backdrop blur in a frame's scene repeated as a Core Animation
//! backdrop layer under the window's Metal layer.
//!
//! gpui's own backdrop blur samples what gpui drew. Where it drew nothing its layer stays
//! clear, so whatever is under the window's view (views an application placed below it, the
//! window's background) would show through unblurred. A `CABackdropLayer` under the Metal
//! layer, cut to the same rounded rect, blurs that part, the way WebKit builds CSS
//! `backdrop-filter`:
//!
//! ```text
//! view's backing layer (the container)
//! ├── host: one clip and backdrop layer per backdrop in the scene
//! └── CAMetalLayer
//! ```
//!
//! The backdrops are set in the Core Animation transaction that presents their frame
//! (`MacWindow::draw`), so they move with gpui's content rather than a frame before or after.
//!
//! `CABackdropLayer` and `CAFilter` are private API. Each piece is looked up and checked before
//! use; without them, gpui's blur still covers what gpui drew.

use gpui::{BackdropFilter, ScaledPixels};
use objc2::{
    class, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Bool, Sel},
    sel,
};
use objc2_foundation::{NSArray, NSNumber, NSPoint, NSRect, NSSize, NSString};

/// `kCALayerWidthSizable | kCALayerHeightSizable`
const FILL_SUPERLAYER: u32 = (1 << 1) | (1 << 4);

/// A rect in points, from its parent's top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn ns(self) -> NSRect {
        NSRect::new(
            NSPoint::new(self.x, self.y),
            NSSize::new(self.width, self.height),
        )
    }
}

/// Where one backdrop goes and how it looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    /// The content mask's part of the backdrop, in points from the view's top left
    pub clip: Rect,
    /// The backdrop's bounds, in points from the clip's top left
    pub frame: Rect,
    pub corner_radius: f64,
    /// The rounded corners, as `CALayer.maskedCorners` bits
    pub corners: u64,
    /// Smoothed corners, drawn as Core Animation's continuous curve
    pub continuous: bool,
    /// The gaussian blur's radius, in points
    pub blur_radius: f64,
    pub opacity: f32,
}

impl Placement {
    /// `filter`'s placement in a window drawn at `scale`, or None if it shows nothing.
    pub(crate) fn of(filter: &BackdropFilter, scale: f32) -> Option<Self> {
        let blur_radius = filter.max_blur_radius();
        if blur_radius <= 0. || filter.opacity <= 0. || scale <= 0. {
            return None;
        }
        let bounds = filter.bounds;
        let clip = filter.content_mask.bounds.intersect(&bounds);
        if clip.is_empty() {
            return None;
        }
        let points = |value: ScaledPixels| f64::from(value.0 / scale);
        let radii = filter.corner_radii;
        // `maskedCorners` names corners by their coordinates; the host is flipped, so min y is
        // the top
        let corners = [
            (radii.top_left, 1),
            (radii.top_right, 2),
            (radii.bottom_left, 4),
            (radii.bottom_right, 8),
        ];
        let corner_radius = corners
            .iter()
            .map(|(radius, _)| points(*radius))
            .fold(0., f64::max)
            .min(points(bounds.size.width.min(bounds.size.height)) / 2.);
        // A layer has one radius: the corners rounded by the largest, or all of them by it if
        // they differ
        let mut mask = corners
            .iter()
            .filter(|(radius, _)| radius.0 > 0.)
            .fold(0, |mask, (_, bit)| mask | bit);
        if corners
            .iter()
            .any(|(radius, _)| radius.0 > 0. && points(*radius) < corner_radius)
        {
            mask = 15;
        }
        Some(Self {
            clip: Rect {
                x: points(clip.origin.x),
                y: points(clip.origin.y),
                width: points(clip.size.width),
                height: points(clip.size.height),
            },
            frame: Rect {
                x: points(bounds.origin.x - clip.origin.x),
                y: points(bounds.origin.y - clip.origin.y),
                width: points(bounds.size.width),
                height: points(bounds.size.height),
            },
            corner_radius,
            corners: if corner_radius > 0. { mask } else { 15 },
            continuous: filter.corner_smoothing > 0.,
            blur_radius: f64::from(blur_radius / scale),
            opacity: filter.opacity,
        })
    }
}

/// The private classes, found and checked once.
struct Kit {
    backdrop: &'static AnyClass,
    filter: &'static AnyClass,
    normalize_edges: bool,
    corner_curve: bool,
}

impl Kit {
    fn find() -> Option<Self> {
        let backdrop = AnyClass::get(c"CABackdropLayer")?;
        let filter = AnyClass::get(c"CAFilter")?;
        unsafe {
            if !responds(backdrop, sel!(layer)) || !responds(filter, sel!(filterWithType:)) {
                return None;
            }
            let sample: Option<Retained<AnyObject>> =
                msg_send![filter, filterWithType: &*NSString::from_str("gaussianBlur")];
            let sample = sample?;
            // An unknown key would throw (`setValue:forUndefinedKey:`)
            let keys: Option<Retained<NSArray<NSString>>> = msg_send![&*sample, inputKeys];
            let has = |key: &str| {
                keys.as_ref()
                    .is_some_and(|keys| keys.containsObject(&NSString::from_str(key)))
            };
            if !has("inputRadius") {
                return None;
            }
            let layer: Option<Retained<AnyObject>> = msg_send![backdrop, layer];
            let layer = layer?;
            if !responds_instance(&layer, sel!(setFilters:)) {
                return None;
            }
            Some(Self {
                backdrop,
                filter,
                normalize_edges: has("inputNormalizeEdges"),
                corner_curve: responds_instance(&layer, sel!(setCornerCurve:)),
            })
        }
    }
}

unsafe fn responds(class: &AnyClass, selector: Sel) -> bool {
    let responds: Bool = unsafe { msg_send![class, respondsToSelector: selector] };
    responds.as_bool()
}

unsafe fn responds_instance(object: &AnyObject, selector: Sel) -> bool {
    let responds: Bool = unsafe { msg_send![object, respondsToSelector: selector] };
    responds.as_bool()
}

/// One backdrop's layers: a clip for the content mask, holding the blur.
struct Backdrop {
    clip: Retained<AnyObject>,
    blur: Retained<AnyObject>,
    shown: Option<Placement>,
}

/// A window view's backing layer and the backdrops under its Metal layer.
pub(crate) struct Backdrops {
    container: Retained<AnyObject>,
    host: Retained<AnyObject>,
    metal: *mut AnyObject,
    kit: Option<Kit>,
    pool: Vec<Backdrop>,
    shown: Vec<Placement>,
}

impl Backdrops {
    /// The container for `metal_layer`, the view's Metal layer.
    pub(crate) fn new(metal_layer: *mut AnyObject) -> Self {
        let kit = Kit::find();
        if kit.is_none() {
            log::warn!("CABackdropLayer unavailable: backdrop blurs cover gpui's content only");
        }
        unsafe {
            let container: Retained<AnyObject> = msg_send![class!(CALayer), layer];
            let host: Retained<AnyObject> = msg_send![class!(CALayer), layer];
            // Placements are measured from the top left, as gpui's are
            let () = msg_send![&*host, setGeometryFlipped: true];
            let () = msg_send![&*host, setAutoresizingMask: FILL_SUPERLAYER];
            let () = msg_send![&*container, addSublayer: &*host];
            let () = msg_send![&*container, addSublayer: metal_layer];
            Self {
                container,
                host,
                metal: metal_layer,
                kit,
                pool: Vec::new(),
                shown: Vec::new(),
            }
        }
    }

    /// The view's backing layer.
    pub(crate) fn container(&self) -> *mut AnyObject {
        Retained::as_ptr(&self.container).cast_mut()
    }

    /// Sizes the host and the Metal layer to the view's.
    pub(crate) fn fit(&self, size: NSSize) {
        let frame = NSRect::new(NSPoint::new(0., 0.), size);
        unsafe {
            let () = msg_send![class!(CATransaction), begin];
            let () = msg_send![class!(CATransaction), setDisableActions: true];
            let () = msg_send![&*self.host, setFrame: frame];
            let () = msg_send![self.metal, setFrame: frame];
            let () = msg_send![class!(CATransaction), commit];
        }
    }

    /// Whether `placements` differ from the backdrops shown.
    pub(crate) fn differ(&self, placements: &[Placement]) -> bool {
        self.kit.is_some() && self.shown != placements
    }

    /// Shows `placements`, hiding the rest. Call inside a transaction with actions disabled.
    pub(crate) fn show(&mut self, placements: Vec<Placement>) {
        let Some(kit) = &self.kit else {
            return;
        };
        while self.pool.len() < placements.len() {
            match backdrop(kit, &self.host) {
                Some(backdrop) => self.pool.push(backdrop),
                None => break,
            }
        }
        for (index, backdrop) in self.pool.iter_mut().enumerate() {
            let placement = placements.get(index).copied();
            if backdrop.shown == placement {
                continue;
            }
            unsafe { place(kit, backdrop, placement) };
            backdrop.shown = placement;
        }
        self.shown = placements;
    }
}

/// A new, hidden backdrop in `host`.
fn backdrop(kit: &Kit, host: &AnyObject) -> Option<Backdrop> {
    unsafe {
        let clip: Retained<AnyObject> = msg_send![class!(CALayer), layer];
        let () = msg_send![&*clip, setMasksToBounds: true];
        let () = msg_send![&*clip, setHidden: true];
        let blur: Option<Retained<AnyObject>> = msg_send![kit.backdrop, layer];
        let blur = blur?;
        let filter: Option<Retained<AnyObject>> =
            msg_send![kit.filter, filterWithType: &*NSString::from_str("gaussianBlur")];
        let filter = filter?;
        // Edges sample as if the backdrop went on past them, rather than darkening
        if kit.normalize_edges {
            let () = msg_send![
                &*filter,
                setValue: &*NSNumber::new_bool(true),
                forKey: &*NSString::from_str("inputNormalizeEdges")
            ];
        }
        let filters = NSArray::from_retained_slice(&[filter]);
        let () = msg_send![&*blur, setFilters: &*filters];
        let () = msg_send![&*blur, setMasksToBounds: true];
        let () = msg_send![&*clip, addSublayer: &*blur];
        let () = msg_send![host, addSublayer: &*clip];
        Some(Backdrop {
            clip,
            blur,
            shown: None,
        })
    }
}

/// Moves `backdrop` to `placement`, or hides it.
unsafe fn place(kit: &Kit, backdrop: &Backdrop, placement: Option<Placement>) {
    let (clip, blur) = (&*backdrop.clip, &*backdrop.blur);
    let Some(placement) = placement else {
        let () = unsafe { msg_send![clip, setHidden: true] };
        return;
    };
    unsafe {
        let () = msg_send![clip, setFrame: placement.clip.ns()];
        let () = msg_send![blur, setFrame: placement.frame.ns()];
        let () = msg_send![blur, setCornerRadius: placement.corner_radius];
        let () = msg_send![blur, setMaskedCorners: placement.corners];
        if kit.corner_curve {
            let curve = if placement.continuous {
                "continuous"
            } else {
                "circular"
            };
            let () = msg_send![blur, setCornerCurve: &*NSString::from_str(curve)];
        }
        // The filter is named by its type; one set on the layer is changed through it
        let () = msg_send![
            blur,
            setValue: &*NSNumber::new_f64(placement.blur_radius),
            forKeyPath: &*NSString::from_str("filters.gaussianBlur.inputRadius")
        ];
        let () = msg_send![clip, setOpacity: placement.opacity];
        let () = msg_send![clip, setHidden: false];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Bounds, ContentMask, Corners, ScaledFilter, point, size};
    use smallvec::smallvec;

    fn filter(bounds: Bounds<ScaledPixels>, mask: Bounds<ScaledPixels>) -> BackdropFilter {
        BackdropFilter {
            order: 0,
            bounds,
            content_mask: ContentMask {
                bounds: mask,
                ..Default::default()
            },
            corner_radii: Corners::all(ScaledPixels(16.)),
            corner_smoothing: 0.,
            filters: smallvec![ScaledFilter::Blur(ScaledPixels(32.))],
            opacity: 1.,
        }
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
        Bounds {
            origin: point(ScaledPixels(x), ScaledPixels(y)),
            size: size(ScaledPixels(width), ScaledPixels(height)),
        }
    }

    #[test]
    fn placed_in_points() {
        let placement = Placement::of(
            &filter(rect(100., 40., 200., 120.), rect(0., 0., 1000., 1000.)),
            2.,
        )
        .unwrap();
        assert_eq!(
            placement.clip,
            Rect {
                x: 50.,
                y: 20.,
                width: 100.,
                height: 60.
            }
        );
        assert_eq!(
            placement.frame,
            Rect {
                x: 0.,
                y: 0.,
                width: 100.,
                height: 60.
            }
        );
        assert_eq!(placement.corner_radius, 8.);
        assert_eq!(placement.corners, 15);
        assert_eq!(placement.blur_radius, 16.);
    }

    #[test]
    fn clipped_by_its_content_mask() {
        let placement = Placement::of(
            &filter(rect(100., 40., 200., 120.), rect(150., 0., 1000., 100.)),
            1.,
        )
        .unwrap();
        assert_eq!(
            placement.clip,
            Rect {
                x: 150.,
                y: 40.,
                width: 150.,
                height: 60.
            }
        );
        assert_eq!(placement.frame.x, -50.);
        assert_eq!(placement.frame.y, 0.);
        assert_eq!(placement.frame.width, 200.);
    }

    #[test]
    fn nothing_when_hidden() {
        let mut hidden = filter(rect(0., 0., 10., 10.), rect(20., 20., 10., 10.));
        assert_eq!(Placement::of(&hidden, 1.), None);
        hidden.content_mask.bounds = rect(0., 0., 10., 10.);
        hidden.opacity = 0.;
        assert_eq!(Placement::of(&hidden, 1.), None);
    }

    #[test]
    fn rounds_only_the_rounded_corners() {
        let mut tab = filter(rect(0., 0., 100., 30.), rect(0., 0., 100., 30.));
        tab.corner_radii = Corners {
            top_left: ScaledPixels(6.),
            top_right: ScaledPixels(6.),
            bottom_right: ScaledPixels(0.),
            bottom_left: ScaledPixels(0.),
        };
        let placement = Placement::of(&tab, 1.).unwrap();
        assert_eq!(placement.corner_radius, 6.);
        assert_eq!(placement.corners, 1 | 2);
        // Different radii round every corner by the largest
        tab.corner_radii.bottom_left = ScaledPixels(2.);
        assert_eq!(Placement::of(&tab, 1.).unwrap().corners, 15);
    }

    #[test]
    fn radius_at_most_half_the_short_side() {
        let mut pill = filter(rect(0., 0., 100., 20.), rect(0., 0., 100., 20.));
        pill.corner_radii = Corners::all(ScaledPixels(50.));
        assert_eq!(Placement::of(&pill, 1.).unwrap().corner_radius, 10.);
    }
}
