use yakui::{button, column, row, textbox, use_state};

#[derive(Debug, Clone, Copy)]
enum Page {
    A,
    B,
}

#[track_caller]
fn rowded_textbox(initial_text: &str) {
    row(|| {
        textbox(initial_text, None);
    });
}

pub fn run() {
    let page = use_state(|| Page::A);

    column(|| {
        if button("page a").clicked {
            page.set(Page::A);
        }

        if button("page b").clicked {
            page.set(Page::B);
        }

        match page.get() {
            Page::A => rowded_textbox("a"),
            Page::B => rowded_textbox("b"),
        };
    });
}

fn main() {
    bootstrap::start(run as fn());
}
