use yakui::{align, colored_box, column, label, widgets::Absolute, Alignment, Color, Dim2, Pivot};

use bootstrap::ExampleState;

pub fn run(state: &mut ExampleState) {
    const ALIGNMENTS: &[Alignment] = &[
        Alignment::TOP_LEFT,
        Alignment::TOP_CENTER,
        Alignment::TOP_RIGHT,
        Alignment::CENTER_LEFT,
        Alignment::CENTER,
        Alignment::CENTER_RIGHT,
        Alignment::BOTTOM_LEFT,
        Alignment::BOTTOM_CENTER,
        Alignment::BOTTOM_RIGHT,
    ];

    let index = (state.time as usize) % ALIGNMENTS.len();
    let alignment = ALIGNMENTS[index];

    align(alignment, || {
        colored_box(Color::REBECCA_PURPLE, [100.0, 100.0]);
        label("this should be inside of the aligned box");

        column(|| {
            Absolute::new(Alignment::TOP_CENTER, Pivot::TOP_CENTER, Dim2::ZERO).show(|| {
                label("this should always be on the top center");
            });
            Absolute::new(Alignment::BOTTOM_RIGHT, Pivot::BOTTOM_RIGHT, Dim2::ZERO).show(|| {
                label("this should always be on the bottom right");
            });
        });
    });
}

fn main() {
    bootstrap::start(run as fn(&mut ExampleState));
}
