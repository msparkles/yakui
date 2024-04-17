//! This example shows yakui's progress in having the primitives needed to
//! implement a dropdown or combo box widget.

#![allow(clippy::collapsible_if)]

use yakui::widgets::{Layer, Scrollable};
use yakui::{button, column, reflow, use_state, Alignment, Dim2};
use yakui::{constrained, row, Constraints, Vec2};
use yakui_core::Pivot;

pub fn run() {
    let open = use_state(|| false);
    let options = ["Hello", "World", "Foobar", "Meow", "Woof"];
    let selected = use_state(|| 0);

    row(|| {
        constrained(Constraints::loose(Vec2::new(f32::INFINITY, 50.0)), || {
            Scrollable::vertical().show(|| {
                column(|| {
                    if button("Upper Button").clicked {
                        println!("Upper button clicked");
                    }

                    column(|| {
                        if button(options[selected.get()]).clicked {
                            open.modify(|x| !x);
                        }

                        if open.get() {
                            reflow(Alignment::BOTTOM_LEFT, Pivot::TOP_LEFT, Dim2::ZERO, || {
                                Layer::new().show(|| {
                                    constrained(
                                        Constraints::loose(Vec2::new(f32::INFINITY, 80.0)),
                                        || {
                                            Scrollable::vertical().show(|| {
                                                column(|| {
                                                    let current = selected.get();
                                                    for (i, option) in options.iter().enumerate() {
                                                        if i != current {
                                                            if button(*option).clicked {
                                                                selected.set(i);
                                                                open.set(false);
                                                            }
                                                        }
                                                    }
                                                });
                                            });
                                        },
                                    );
                                });
                            });
                        }
                    });
                });
            });
        });

        if button("Side Button").clicked {
            println!("Side button clicked");
        }
    });
}

fn main() {
    bootstrap::start(run as fn());
}
