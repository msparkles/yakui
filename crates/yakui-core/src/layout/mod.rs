//! Defines yakui's layout protocol and Layout DOM.

use std::collections::VecDeque;

use glam::Vec2;
use thunderdome::{Arena, Index};

use crate::dom::Dom;
use crate::event::EventInterest;
use crate::geometry::{Constraints, Rect};
use crate::id::WidgetId;
use crate::input::{InputState, Interests};
use crate::widget::LayoutContext;
use crate::Flow;

/// Contains information on how each widget in the DOM is laid out and what
/// events they're interested in.
#[derive(Debug)]
pub struct LayoutDom {
    nodes: Arena<LayoutDomNode>,
    clip_stack: Arena<Vec<WidgetId>>,
    current_clip_stack: Option<Index>,

    unscaled_viewport: Rect,
    scale_factor: f32,

    pub(crate) interests: Interests,
}

/// A node in a [`LayoutDom`].
#[derive(Debug)]
#[non_exhaustive]
pub struct LayoutDomNode {
    /// The bounding rectangle of the node in logical pixels.
    pub rect: Rect,

    /// This node will clip its descendants to its bounding rectangle.
    pub clipping_enabled: bool,

    /// This node is the beginning of a new layer, and all of its descendants
    /// should be hit tested and painted with higher priority.
    pub new_layer: bool,

    /// This node is clipped to the region defined by the given node.
    pub clipped_by: Option<WidgetId>,

    /// What events the widget reported interest in.
    pub event_interest: EventInterest,

    /// Which clip stack the widget belongs in.
    pub clip_stack_id: Index,
}

impl LayoutDom {
    /// Create an empty `LayoutDom`.
    pub fn new() -> Self {
        Self {
            nodes: Arena::new(),
            clip_stack: Arena::new(),
            current_clip_stack: None,

            unscaled_viewport: Rect::ONE,
            scale_factor: 1.0,

            interests: Interests::new(),
        }
    }

    pub(crate) fn sync_removals(&mut self, removals: &[WidgetId]) {
        for id in removals {
            self.nodes.remove(id.index());
        }
    }

    /// Get a widget's layout information.
    pub fn get(&self, id: WidgetId) -> Option<&LayoutDomNode> {
        self.nodes.get(id.index())
    }

    /// Get a mutable reference to a widget's layout information.
    pub fn get_mut(&mut self, id: WidgetId) -> Option<&mut LayoutDomNode> {
        self.nodes.get_mut(id.index())
    }

    /// Set the viewport of the DOM in unscaled units.
    pub fn set_unscaled_viewport(&mut self, view: Rect) {
        self.unscaled_viewport = view;
    }

    /// Set the scale factor to use for layout.
    pub fn set_scale_factor(&mut self, scale: f32) {
        self.scale_factor = scale;
    }

    /// Get the currently active scale factor.
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Get the viewport in scaled units.
    pub fn viewport(&self) -> Rect {
        Rect::from_pos_size(
            self.unscaled_viewport.pos() / self.scale_factor,
            self.unscaled_viewport.size() / self.scale_factor,
        )
    }

    /// Get the viewport in unscaled units.
    pub fn unscaled_viewport(&self) -> Rect {
        self.unscaled_viewport
    }

    /// Tells how many nodes are currently in the `LayoutDom`.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Tells whether the `LayoutDom` is currently empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Calculate the layout of all elements in the given DOM.
    pub fn calculate_all(&mut self, dom: &Dom, input: &InputState) {
        profiling::scope!("LayoutDom::calculate_all");
        log::debug!("LayoutDom::calculate_all()");

        self.clip_stack.clear();
        self.current_clip_stack = None;
        self.interests.clear();

        let constraints = Constraints::tight(self.viewport().size());

        self.calculate(dom, input, dom.root(), constraints);
        self.resolve_positions(dom);
    }

    /// Calculate the layout of a specific widget.
    ///
    /// This function must only be called from
    /// [`Widget::layout`][crate::widget::Widget::layout] and should only be
    /// called once per widget per layout pass.
    pub fn calculate(
        &mut self,
        dom: &Dom,
        input: &InputState,
        id: WidgetId,
        constraints: Constraints,
    ) -> Vec2 {
        dom.enter(id);

        let dom_node = dom.get(id).unwrap();

        let context = LayoutContext {
            dom,
            input,
            layout: self,
        };

        let size = dom_node.widget.layout(context, constraints);

        // If the widget called new_layer() during layout, it will be on top of
        // the mouse interest layer stack.
        let new_layer = self.interests.current_layer_root() == Some(id);

        // Mouse interest will be registered into the layout created by the
        // widget if there is one.
        let event_interest = dom_node.widget.event_interest();
        if event_interest.intersects(EventInterest::MOUSE_ALL)
            || event_interest.intersects(EventInterest::CAPTURE_KEYS)
        {
            self.interests.insert(id, event_interest);
        }

        // If the widget created a new layer, we're done with it now, so it's
        // time to clean it up.
        if new_layer {
            self.interests.pop_layer();
        }

        if self.current_clip_stack.is_none() {
            self.enable_clipping(dom);
        }

        // There should always be a currently active clip stack.
        let clip_stack_id = self.current_clip_stack.unwrap();
        let clip_stack = self.clip_stack.get_mut(clip_stack_id).unwrap();

        // If the widget called enable_clipping() during layout, it will be on
        // top of the clip stack at this point.
        let clipping_enabled = clip_stack.last() == Some(&id);

        // If this node enabled clipping, the next node under that is the node
        // that clips this one.
        let clipped_by = if clipping_enabled {
            clip_stack.iter().nth_back(2).copied()
        } else {
            clip_stack.last().copied()
        };

        self.nodes.insert_at(
            id.index(),
            LayoutDomNode {
                rect: Rect::from_pos_size(Vec2::ZERO, size),
                clipping_enabled,
                new_layer,
                clipped_by,
                event_interest,
                clip_stack_id,
            },
        );

        if clipping_enabled {
            clip_stack.pop();
        }

        dom.exit(id);
        size
    }

    /// Enables clipping for the currently active widget.
    pub fn enable_clipping(&mut self, dom: &Dom) {
        if self.current_clip_stack.is_none() {
            self.current_clip_stack = Some(dom.current().index());
            self.clip_stack.insert_at(dom.current().index(), Vec::new());
        }

        self.clip_stack
            .get_mut(self.current_clip_stack.unwrap())
            .unwrap()
            .push(dom.current());
    }

    /// Create a new clip stack for the currently active widget.
    pub fn new_clip_stack(&mut self, dom: &Dom) {
        self.current_clip_stack = Some(dom.current().index());
        let old = self.clip_stack.insert_at(dom.current().index(), Vec::new());
        debug_assert!(old.is_none(), "clip_stack id clashed");
        self.enable_clipping(dom);
    }

    /// Put this widget and its children into a new layer.
    pub fn new_layer(&mut self, dom: &Dom) {
        self.interests.push_layer(dom.current());
    }

    /// Set the position of a widget.
    pub fn set_pos(&mut self, id: WidgetId, pos: Vec2) {
        if let Some(node) = self.nodes.get_mut(id.index()) {
            node.rect.set_pos(pos);
        }
    }

    fn resolve_positions(&mut self, dom: &Dom) {
        let viewport = self.viewport();
        let mut queue = VecDeque::new();

        queue.push_back((dom.root(), Vec2::ZERO));

        while let Some((id, parent_pos)) = queue.pop_front() {
            if let Some(layout_node) = self.nodes.get_mut(id.index()) {
                let node = dom.get(id).unwrap();

                if let Flow::Absolute { anchor, offset } = node.widget.flow() {
                    let anchor = viewport.size() * anchor.as_vec2();
                    let offset = offset.resolve(viewport.size());

                    layout_node.rect.set_pos(anchor + offset);
                } else {
                    layout_node
                        .rect
                        .set_pos(layout_node.rect.pos() + parent_pos);
                }

                queue.extend(node.children.iter().map(|&id| (id, layout_node.rect.pos())));
            }
        }
    }
}
