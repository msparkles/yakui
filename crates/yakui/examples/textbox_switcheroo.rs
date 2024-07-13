use yakui::{button, column, textbox, use_state};

#[derive(Debug, Clone, Copy)]
enum Page {
    A,
    B,
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
            Page::A => {
                textbox("a", None);
            }
            Page::B => {
                textbox("b", None);
            }
        };
    });
}

fn main() {
    bootstrap::start(run as fn());
}
