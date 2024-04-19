use yakui_core::{
    geometry::{Constraints, Dim2, Vec2},
    widget::{LayoutContext, Widget},
    Alignment, Flow, Pivot, Response,
};

use crate::util::widget_children;

/**
Changes the flow behavior a widget tree, allowing it to break out of any layouts, and be positioned in relation to the screen instead.
*/
#[derive(Debug, Clone)]
pub struct Absolute {
    pub anchor: Alignment,
    pub pivot: Pivot,
    pub offset: Dim2,
}

impl Absolute {
    pub fn new(anchor: Alignment, pivot: Pivot, offset: Dim2) -> Self {
        Self {
            anchor,
            pivot,
            offset,
        }
    }

    pub fn show<F: FnOnce()>(self, children: F) -> Response<AbsoluteResponse> {
        widget_children::<AbsoluteWidget, F>(children, self)
    }
}

#[derive(Debug)]
pub struct AbsoluteWidget {
    props: Absolute,
}

pub type AbsoluteResponse = ();

impl Widget for AbsoluteWidget {
    type Props<'a> = Absolute;
    type Response = AbsoluteResponse;

    fn new() -> Self {
        Self {
            props: Absolute {
                anchor: Alignment::TOP_LEFT,
                pivot: Pivot::TOP_LEFT,
                offset: Dim2::ZERO,
            },
        }
    }

    fn update(&mut self, props: Self::Props<'_>) -> Self::Response {
        self.props = props;
    }

    fn flow(&self) -> Flow {
        Flow::Absolute {
            anchor: self.props.anchor,
            offset: self.props.offset,
        }
    }

    fn layout(&self, mut ctx: LayoutContext<'_>, constraints: Constraints) -> Vec2 {
        let node = ctx.dom.get_current();
        let mut size = Vec2::ZERO;
        for &child in &node.children {
            size = size.max(ctx.calculate_layout(child, Constraints::none()));
        }

        let pivot_offset = -size * self.props.pivot.as_vec2();
        for &child in &node.children {
            ctx.layout.set_pos(child, pivot_offset);
        }

        constraints.constrain_min(size)
    }
}
