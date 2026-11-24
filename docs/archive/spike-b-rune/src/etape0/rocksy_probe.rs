//! Rocksy step-0 probe: a 319 KB Markdown-like document in a TextArea with Live-Preview-style
//! decorations (bigger bold headings, bold runs, dimmed/shrunk markers), typed into headlessly.
//! PROBE_KB sets the document size (default 319).

use rune::{AppSpec, Cx, El, Span, TextEdit, col, text};
use rune::tok::type_;
use rune_kit::{Kit, TextArea};

struct State {
    edit: TextEdit,
}

fn doc(kb: usize) -> String {
    let mut s = String::new();
    let mut i = 0;
    while s.len() < kb * 1024 {
        if i % 12 == 0 {
            s.push_str(&format!("## Section {i} du journal\n"));
        } else if i % 12 == 5 {
            s.push_str(&format!("- [ ] tache {i} avec un **mot en gras** et du `code` et [[Lien {i}]]\n"));
        } else {
            s.push_str(&format!("Ligne {i} : texte courant avec **gras**, *italique* et quelques mots de plus.\n"));
        }
        i += 1;
    }
    s
}

impl Default for State {
    fn default() -> Self {
        let kb = std::env::var("PROBE_KB").ok().and_then(|v| v.parse().ok()).unwrap_or(319);
        let mut edit = TextEdit::new(doc(kb));
        edit.focused = true;
        edit.set_caret(30);
        Self { edit }
    }
}

/// Naive full-document "Markdown" decoration pass (what a Live Preview would do, unvirtualized).
fn spans(src: &str, caret: usize) -> Vec<Span> {
    let dim = Kit::neutral(true).c.text_accent;
    let mut out = Vec::new();
    let mut at = 0;
    for line in src.split_inclusive('\n') {
        let end = at + line.trim_end_matches('\n').len();
        let active = caret >= at && caret <= end;
        if line.starts_with("## ") {
            // Marker: shown dimmed on the caret's line, shrunk to (almost) nothing elsewhere.
            out.push(if active { Span::new(at, at + 3).color(dim) } else { Span::new(at, at + 3).size(0.1) });
            out.push(Span::new(at + 3, end).size(22.0).bold());
        } else {
            let mut from = 0;
            while let Some(a) = line[from..].find("**") {
                let a = from + a;
                let Some(b) = line[a + 2..].find("**") else { break };
                let b = a + 2 + b;
                let m = |r: std::ops::Range<usize>| {
                    if active { Span::new(at + r.start, at + r.end).color(dim) } else { Span::new(at + r.start, at + r.end).size(0.1) }
                };
                out.push(m(a..a + 2));
                out.push(Span::new(at + a + 2, at + b).bold());
                out.push(m(b..b + 2));
                from = b + 2;
            }
        }
        at += line.len();
    }
    out
}

fn view(s: &State, _: &mut Cx<State>) -> El {
    let k = Kit::neutral(true);
    let t0 = std::time::Instant::now();
    let deco = if std::env::var("PROBE_NO_SPANS").is_ok() { Vec::new() } else { spans(&s.edit.text, s.edit.range().0) };
    let n = deco.len();
    let el = col()
        .w_full()
        .h_full()
        .fill(k.c.page)
        .padding(16.0)
        .gap(8.0)
        .child(text(format!("Rocksy probe: {} KB, {} spans", s.edit.text.len() / 1024, n)).font(type_::TITLE).color(k.c.text_base))
        .child(TextArea::new(&s.edit).width(None).rows(22).decorations(deco).on_edit(|s: &mut State| &mut s.edit).build(&k));
    eprintln!("probe: view built in {:.2} ms ({} spans)", t0.elapsed().as_secs_f64() * 1e3, n);
    el
}

fn main() {
    rune::run(AppSpec {
        fonts: &[
            include_bytes!("../../rune/assets/Inter-Regular.ttf"),
            include_bytes!("../../rune/assets/Inter-Medium.ttf"),
            include_bytes!("../../rune/assets/Inter-SemiBold.ttf"),
            include_bytes!("../../rune/assets/IBMPlexMono-Regular.ttf"),
        ],
        families: ("Inter", "IBM Plex Mono"),
        ..AppSpec::new("Rocksy probe", (900.0, 640.0), State::default, view)
    });
}
