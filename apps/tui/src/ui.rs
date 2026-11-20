//! Drawing. Reads `App`, writes widgets, and records clickable regions in `app.hits`.

use crate::app::{App, Derived, HideKind, Hit, HitKind, LogKind, Screen, SCREENS};
use crate::browser::Row;
use crate::form::{Field, Id, Kind, PathFilter};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Gauge, Paragraph};
use ratatui::Frame;
use std::sync::atomic::{AtomicBool, Ordering};
use stegosaur::analysis::{Report, Risk};
use stegosaur::app::files::FileKind;
use stegosaur::app::text;

pub const MIN_W: u16 = 96;
pub const MIN_H: u16 = 28;

const GREEN: Color = Color::Rgb(169, 207, 122);
const ORANGE: Color = Color::Rgb(240, 136, 62);
const SKY: Color = Color::Rgb(143, 203, 233);
const DIM: Color = Color::Rgb(127, 154, 134);
const RED: Color = Color::Rgb(229, 104, 79);
const INK: Color = Color::Rgb(232, 239, 217);
const DARK: Color = Color::Rgb(16, 32, 15);

static ASCII: AtomicBool = AtomicBool::new(false);

/// Use plain ASCII for arrows, bullets and the spinner (for terminals without those glyphs).
pub fn set_ascii(on: bool) {
    ASCII.store(on, Ordering::Relaxed);
}

fn ascii() -> bool {
    ASCII.load(Ordering::Relaxed)
}

fn spinner(n: usize) -> &'static str {
    const U: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    const A: [&str; 4] = ["|", "/", "-", "\\"];
    if ascii() { A[(n / 2) % 4] } else { U[(n / 2) % 10] }
}

fn st(c: Color) -> Style {
    Style::new().fg(c)
}

fn bold(c: Color) -> Style {
    Style::new().fg(c).add_modifier(Modifier::BOLD)
}

fn fmt_bytes(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1} MB", n as f64 / 1e6)
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

/// Keeps the end of a long string (the file name matters more than the folder).
fn tail(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n <= width {
        s.to_string()
    } else {
        let keep = width.saturating_sub(1);
        format!("…{}", s.chars().skip(n - keep).collect::<String>())
    }
}

fn clip(s: &str, width: usize) -> String {
    if s.chars().count() <= width { s.to_string() } else { format!("{}…", s.chars().take(width.saturating_sub(1)).collect::<String>()) }
}

fn risk_color(r: Risk) -> Color {
    match r {
        Risk::Low => GREEN,
        Risk::Medium => ORANGE,
        Risk::High => RED,
    }
}

fn risk_name(r: Risk) -> &'static str {
    match r {
        Risk::Low => "LOW",
        Risk::Medium => "MEDIUM",
        Risk::High => "HIGH",
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    app.hits.clear();
    if area.width < MIN_W || area.height < MIN_H {
        let msg = vec![
            Line::from(Span::styled("Please enlarge the window", bold(ORANGE))),
            Line::from(format!("Stegosaur needs at least {MIN_W}x{MIN_H}; this window is {}x{}.", area.width, area.height)),
        ];
        let h = 2.min(area.height);
        let r = Rect::new(area.x, area.y + area.height.saturating_sub(h) / 2, area.width, h);
        f.render_widget(Paragraph::new(msg).centered(), r);
        return;
    }
    let [tabs, main, cmd, log, foot] = Layout::vertical([Constraint::Length(3), Constraint::Min(10), Constraint::Length(1), Constraint::Length(8), Constraint::Length(1)]).areas(area);
    draw_tabs(f, app, tabs);
    let [left, right] = Layout::horizontal([Constraint::Percentage(54), Constraint::Percentage(46)]).areas(main);
    draw_form(f, app, left);
    draw_preview(f, app, right);
    draw_command(f, app, cmd);
    draw_log(f, app, log);
    draw_footer(f, app, foot);
    if app.browser.is_some() {
        draw_browser(f, app, area);
    }
    if app.help {
        draw_help(f, area);
    }
}

fn draw_tabs(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::bordered().border_style(st(GREEN)).title(Span::styled(" Stegosaur ", bold(ORANGE)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut spans = Vec::new();
    let mut x = inner.x;
    for (i, s) in SCREENS.iter().enumerate() {
        let label = format!(" {} {} ", i + 1, s.name());
        let w = label.chars().count() as u16;
        let on = *s == app.screen;
        spans.push(Span::styled(label, if on { Style::new().fg(DARK).bg(GREEN).add_modifier(Modifier::BOLD) } else { st(DIM) }));
        spans.push(Span::raw(" "));
        app.hits.push(Hit { rect: Rect::new(x, inner.y, w, 1), kind: HitKind::Tab(i) });
        x += w + 1;
    }
    f.render_widget(Paragraph::new(Line::from(spans)), inner);
}

fn field_height(field: &Field) -> u16 {
    match &field.kind {
        Kind::Paths { items, .. } => 1 + items.len().clamp(1, 4) as u16,
        _ => 1,
    }
}

fn value_spans(app: &App, field: &Field, focused: bool, width: usize) -> Vec<Span<'static>> {
    let hl = Style::new().fg(INK).bg(Color::Rgb(43, 66, 49));
    let (l, r) = if ascii() { ("<", ">") } else { ("◀", "▶") };
    match &field.kind {
        Kind::Path { value, .. } => match value {
            Some(p) => vec![Span::styled(tail(&p.display().to_string(), width), if focused { hl } else { Style::new() })],
            None => vec![Span::styled(if focused { "press Enter to choose".to_string() } else { "(none)".to_string() }, st(DIM))],
        },
        Kind::Text { value, masked, hint, .. } => {
            let shown: String = if *masked { (if ascii() { "*" } else { "•" }).repeat(value.chars().count()) } else { value.clone() };
            let mut s = tail(&shown, width.saturating_sub(1));
            if shown.is_empty() && !focused {
                return vec![Span::styled(clip(hint, width), st(DIM))];
            }
            if focused {
                s.push(if ascii() { '_' } else { '▌' });
            }
            let style = if *masked { st(ORANGE) } else { Style::new() };
            vec![Span::styled(s, if focused { style.bg(Color::Rgb(43, 66, 49)) } else { style })]
        }
        Kind::Choice { options, idx } => vec![Span::styled(l, st(SKY)), Span::styled(format!(" {} ", options[*idx]), if focused { hl.add_modifier(Modifier::BOLD) } else { bold(INK) }), Span::styled(r, st(SKY))],
        Kind::Step { value, unit, .. } => {
            let t = if *value == 0 && *unit == "bytes" { "off".to_string() } else if unit.is_empty() { value.to_string() } else { format!("{value} {unit}") };
            vec![Span::styled(l, st(SKY)), Span::styled(format!(" {t} "), if focused { hl.add_modifier(Modifier::BOLD) } else { bold(INK) }), Span::styled(r, st(SKY))]
        }
        Kind::Paths { items, .. } => vec![Span::styled(if items.is_empty() { "(none yet)".to_string() } else { format!("{} selected", items.len()) }, st(if items.is_empty() { DIM } else { INK })), Span::styled(if focused { "   Enter add  d remove" } else { "" }, st(DIM))],
        Kind::Button => {
            let _ = app;
            vec![]
        }
    }
}

fn draw_form(f: &mut Frame, app: &mut App, area: Rect) {
    let focus_border = app.form().focused().id != Id::Run;
    let block = Block::bordered().border_style(st(if focus_border { GREEN } else { ORANGE })).title(Span::styled(format!(" {} ", app.screen.name()), bold(GREEN)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let label_w: u16 = 25;
    let mut y = inner.y;
    let ready = app.derived.problems.is_empty();
    let busy = app.busy();
    let focus = app.form().focus;
    let fields: Vec<(usize, Field)> = app.form().fields.iter().cloned().enumerate().filter(|(_, f)| !f.hidden).collect();
    for (i, field) in fields {
        let h = field_height(&field);
        if y + h > inner.y + inner.height {
            break;
        }
        let row = Rect::new(inner.x, y, inner.width, h);
        app.hits.push(Hit { rect: row, kind: HitKind::Field(i) });
        let focused = i == focus;
        if let Kind::Button = field.kind {
            let label = format!("[ {} ]", field.label);
            let style = if busy { Style::new().fg(DARK).bg(DIM) } else if focused && ready { Style::new().fg(DARK).bg(ORANGE).add_modifier(Modifier::BOLD) } else if ready { bold(GREEN) } else { st(DIM) };
            let extra = if busy { format!("  {} working…", spinner(app.spinner)) } else if !ready && focused { "  (see Preview for what is missing)".to_string() } else { String::new() };
            f.render_widget(Paragraph::new(Line::from(vec![Span::styled(label, style), Span::styled(extra, st(ORANGE))])), Rect::new(row.x + 1, row.y, row.width.saturating_sub(1), 1));
            y += h;
            continue;
        }
        let label_style = if focused { bold(ORANGE) } else { st(DIM) };
        let marker = if focused { if ascii() { ">" } else { "▸" } } else { " " };
        f.render_widget(Paragraph::new(Line::from(vec![Span::styled(marker, st(ORANGE)), Span::styled(format!(" {}", field.label), label_style)])), Rect::new(row.x, row.y, label_w.min(row.width), 1));
        let vx = row.x + label_w;
        let vw = row.width.saturating_sub(label_w + 1);
        f.render_widget(Paragraph::new(Line::from(value_spans(app, &field, focused, vw as usize))), Rect::new(vx, row.y, vw + 1, 1));
        if let Kind::Paths { items, cursor, .. } = &field.kind {
            let start = if items.len() > 4 { (*cursor).saturating_sub(3).min(items.len() - 4) } else { 0 };
            for (n, p) in items.iter().enumerate().skip(start).take(4) {
                let sel = n == *cursor && focused;
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("?");
                let line = format!("{} {}", if sel { if ascii() { ">" } else { "▸" } } else { " " }, name);
                f.render_widget(Paragraph::new(Span::styled(clip(&line, vw as usize), if sel { Style::new().fg(DARK).bg(GREEN) } else { Style::new() })), Rect::new(vx, row.y + 1 + (n - start) as u16, vw, 1));
            }
        }
        y += h;
    }
}

struct Col {
    area: Rect,
    y: u16,
}

impl Col {
    fn new(area: Rect) -> Self {
        Self { area, y: area.y }
    }

    fn room(&self) -> bool {
        self.y < self.area.y + self.area.height
    }

    fn line(&mut self, f: &mut Frame, line: Line<'static>) {
        if self.room() {
            f.render_widget(Paragraph::new(line), Rect::new(self.area.x, self.y, self.area.width, 1));
            self.y += 1;
        }
    }

    fn text(&mut self, f: &mut Frame, s: impl Into<String>, style: Style) {
        let w = self.area.width as usize;
        self.line(f, Line::from(Span::styled(clip(&s.into(), w), style)));
    }

    fn blank(&mut self) {
        self.y += 1;
    }

    fn gauge(&mut self, f: &mut Frame, ratio: f64, label: String, color: Color) {
        if self.room() {
            let g = Gauge::default().ratio(ratio.clamp(0.0, 1.0)).label(Span::styled(label, Style::new().fg(INK))).gauge_style(Style::new().fg(color).bg(Color::Rgb(30, 44, 34))).use_unicode(!ascii());
            f.render_widget(g, Rect::new(self.area.x, self.y, self.area.width, 1));
            self.y += 1;
        }
    }
}

fn report_block(c: &mut Col, f: &mut Frame, r: &Report, title: &str) {
    let color = risk_color(r.risk);
    c.text(f, title, bold(INK));
    c.gauge(f, r.rs.clamp(0.0, 1.0), format!("RS   {:.3}", r.rs), color);
    c.gauge(f, r.spa.clamp(0.0, 1.0), format!("SPA  {:.3}", r.spa), color);
    c.line(f, Line::from(vec![Span::styled("score ", st(DIM)), Span::styled(format!("{:.0} / 100  {}", r.score, risk_name(r.risk)), bold(color))]));
}

fn detect_section(c: &mut Col, f: &mut Frame, app: &App) {
    c.text(f, "Detectability (measured on a copy)", bold(INK));
    if app.measuring {
        c.text(f, format!("{} measuring…", spinner(app.spinner)), st(ORANGE));
    } else if let Some(m) = &app.measured {
        let color = risk_color(m.after.risk);
        c.gauge(f, m.after.rs.clamp(0.0, 1.0), format!("RS   {:.3}", m.after.rs), color);
        c.gauge(f, m.after.spa.clamp(0.0, 1.0), format!("SPA  {:.3}", m.after.spa), color);
        c.line(f, Line::from(vec![Span::styled("score ", st(DIM)), Span::styled(format!("{:.0} / 100  {}", m.after.score, risk_name(m.after.risk)), bold(color)), Span::styled(format!("   (untouched image: {:.0})", m.before.score), st(DIM))]));
        c.text(f, "Classical detectors only: a low score is not proof of safety.", st(DIM));
    } else if let Some(n) = &app.measure_note {
        c.text(f, n.clone(), st(DIM));
    } else {
        c.text(f, "(complete the form to see a measurement)", st(DIM));
    }
}

fn problems_section(c: &mut Col, f: &mut Frame, app: &App) {
    if app.derived.problems.is_empty() {
        c.text(f, "Ready.", bold(GREEN));
    } else {
        for p in app.derived.problems.iter().take(3) {
            c.text(f, format!("! {p}"), st(ORANGE));
        }
    }
}

fn draw_preview(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::bordered().border_style(st(GREEN)).title(Span::styled(" Preview ", bold(GREEN)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let inner = Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(2), inner.height);
    let mut c = Col::new(inner);
    let d = app.derived.clone();
    match app.screen {
        Screen::Hide => match app.hide_kind() {
            HideKind::Text => text_preview(&mut c, f, app, &d, inner, false),
            HideKind::Embed => {
                match &d.info {
                    Some(Ok(i)) => c.text(f, format!("Cover: {} ({} samples)", i.summary, i.samples), st(INK)),
                    Some(Err(e)) => c.text(f, e.clone(), st(RED)),
                    None => c.text(f, "Choose a cover image to begin.", st(DIM)),
                }
                c.blank();
                if let Some(p) = &d.embed {
                    c.text(f, "Capacity", bold(INK));
                    c.gauge(f, if p.capacity == 0 { 1.0 } else { p.need as f64 / p.capacity as f64 }, format!("{} of {}  {:.0}%", fmt_bytes(p.need), fmt_bytes(p.capacity), 100.0 * p.need as f64 / p.capacity.max(1) as f64), if p.fits { GREEN } else { RED });
                    c.text(f, if p.fits { format!("fits - {} to spare", fmt_bytes(p.capacity - p.need)) } else { p.error.clone().unwrap_or_default() }, st(if p.fits { DIM } else { RED }));
                    c.blank();
                    detect_section(&mut c, f, app);
                }
                c.blank();
                problems_section(&mut c, f, app);
            }
            HideKind::Hidden => {
                match &d.info {
                    Some(Ok(i)) => c.text(f, format!("Cover: {} ({} samples)", i.summary, i.samples), st(INK)),
                    Some(Err(e)) => c.text(f, e.clone(), st(RED)),
                    None => c.text(f, "Choose a cover image to begin.", st(DIM)),
                }
                c.blank();
                if let Some(p) = &d.hidden {
                    c.text(f, format!("Each slot holds up to {}", fmt_bytes(p.slot_capacity)), bold(INK));
                    c.text(f, format!("first file:   {}", if p.public_fits { "fits" } else { "too big" }), st(if p.public_fits { GREEN } else { RED }));
                    match p.hidden_fits {
                        Some(ok) => c.text(f, format!("second file:  {}", if ok { "fits" } else { "too big" }), st(if ok { GREEN } else { RED })),
                        None => c.text(f, "second slot: filled with random bytes", st(DIM)),
                    }
                    c.text(f, "Two-slot layout: unused space is always random, so the image looks the", st(DIM));
                    c.text(f, "same whether it holds one file, two files, or none.", st(DIM));
                    c.blank();
                    detect_section(&mut c, f, app);
                }
                c.blank();
                problems_section(&mut c, f, app);
            }
        },
        Screen::Reveal => {
            if d.reveal_text {
                text_preview(&mut c, f, app, &d, inner, true);
            } else {
                match &d.info {
                    Some(Ok(i)) => c.text(f, format!("File: {} ({} samples)", i.summary, i.samples), st(INK)),
                    Some(Err(e)) => c.text(f, e.clone(), st(RED)),
                    None => c.text(f, "Choose the image, audio or text file.", st(DIM)),
                }
                c.blank();
                c.text(f, "Tries the normal layout first, then the two-passphrase", st(DIM));
                c.text(f, "layout. A wrong passphrase looks exactly like", st(DIM));
                c.text(f, "an image with nothing hidden in it - by design.", st(DIM));
                c.blank();
                c.text(f, "Use the same error-correction and slot size as when hiding.", st(DIM));
                c.blank();
                problems_section(&mut c, f, app);
            }
        }
        Screen::Analyze => {
            match &app.analysis[0] {
                Some(a) => {
                    report_block(&mut c, f, a, "Image");
                    c.text(f, format!("chi-square p = {:.2}  (unreliable on smooth histograms)", a.chi_p), st(DIM));
                    if let Some(b) = &app.analysis[1] {
                        c.blank();
                        report_block(&mut c, f, b, "Original");
                        c.text(f, format!("change: RS {:+.3}   SPA {:+.3}", a.rs - b.rs, a.spa - b.spa), bold(INK));
                    }
                    c.blank();
                    c.text(f, "Classical detectors only: a low score is not proof of safety.", st(DIM));
                }
                None => {
                    c.text(f, "Estimates how much of an image carries a hidden message", st(DIM));
                    c.text(f, "using chi-square, RS and SPA analysis.", st(DIM));
                    c.blank();
                    if app.busy() {
                        c.text(f, format!("{} scanning…", spinner(app.spinner)), st(ORANGE));
                    } else {
                        problems_section(&mut c, f, app);
                    }
                }
            }
        }
        Screen::Split => {
            if app.form().choice(Id::Direction) == 1 {
                c.text(f, "Add every image that might hold a share, plus the", st(DIM));
                c.text(f, "passphrase. Images without a share are skipped.", st(DIM));
            } else if let Some(p) = &d.split {
                c.text(f, format!("Each share is {}", fmt_bytes(p.share_len)), bold(INK));
                for (i, cr) in p.carriers.iter().enumerate().take(6) {
                    c.line(f, Line::from(vec![Span::styled(format!("  image {}: ", i + 1), st(DIM)), Span::styled(format!("holds {}  ", fmt_bytes(cr.capacity)), st(INK)), Span::styled(if cr.fits { "fits" } else { "too small" }, st(if cr.fits { GREEN } else { RED }))]));
                }
                if p.n > 0 {
                    c.blank();
                    let k = app.form().step(Id::K);
                    c.text(f, format!("Any {} of the {} images rebuild the secret.", k.min(p.n as i64), p.n), st(INK));
                    c.text(f, "Fewer than that reveal nothing at all.", st(DIM));
                }
            } else {
                c.text(f, "Splits a secret into shares, one per image.", st(DIM));
            }
            c.blank();
            problems_section(&mut c, f, app);
        }
    }
}

/// Preview for cover text: how much a style carries, what a message becomes, and the generated or recovered text.
fn text_preview(c: &mut Col, f: &mut Frame, app: &App, d: &Derived, inner: Rect, recover: bool) {
    let style = text::all_styles()[app.form().choice(Id::Style).min(4)];
    let order = if text::adjustable_order(style) { app.form().step(Id::Order) as usize } else { style.default_order() };
    if !recover {
        let s = text::stats(style, order);
        c.line(f, Line::from(vec![Span::styled("capacity ", st(DIM)), Span::styled(format!("{:.2} bits/word", s.bits_per_word), bold(ORANGE)), Span::styled("   well-formed lines ", st(DIM)), Span::styled(s.well_formed_percent.map(|p| format!("{p}%")).unwrap_or_else(|| "n/a".into()), bold(if s.well_formed_percent.unwrap_or(100) >= 95 { GREEN } else { ORANGE }))]));
        if let Some((w, l)) = d.words_lines {
            c.text(f, format!("this message becomes about {w} words / {l} lines"), st(INK));
        }
    } else {
        c.text(f, "Use the same style and realism that were used to hide it.", st(DIM));
    }
    c.blank();
    let shown = if recover { app.recovered.as_ref().map(|t| ("Recovered message", t)) } else { app.text_out.as_ref().map(|t| ("Generated cover text", t)) };
    match shown {
        Some((title, t)) => {
            c.text(f, title, bold(INK));
            let w = c.area.width as usize;
            for l in t.lines().take(inner.height.saturating_sub(c.y - inner.y) as usize) {
                c.text(f, clip(l, w), st(DIM));
            }
        }
        None => problems_section(c, f, app),
    }
}

fn draw_command(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::styled("$ ", st(DIM))];
    let max = area.width.saturating_sub(3) as usize;
    for w in clip(&app.derived.command, max).split(' ') {
        let style = if w.starts_with("--") { st(ORANGE) } else { st(SKY) };
        spans.push(Span::styled(format!("{w} "), style));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_log(f: &mut Frame, app: &App, area: Rect) {
    let title = if app.busy() { format!(" Log  {} working ", spinner(app.spinner)) } else { " Log ".to_string() };
    let block = Block::bordered().border_style(st(GREEN)).title(Span::styled(title, bold(GREEN)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if app.log.is_empty() {
        f.render_widget(Paragraph::new(Span::styled("(nothing yet - fill in the form and press the button)", st(DIM))), Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(1), 1));
        return;
    }
    let rows = inner.height as usize;
    let start = app.log.len().saturating_sub(rows);
    for (i, l) in app.log[start..].iter().enumerate() {
        let color = match l.kind {
            LogKind::Info => INK,
            LogKind::Ok => GREEN,
            LogKind::Err => RED,
            LogKind::Dim => DIM,
        };
        let line = format!("+{:>5.1}s  {}", l.secs, l.text);
        f.render_widget(Paragraph::new(Span::styled(clip(&line, inner.width.saturating_sub(2) as usize), st(color))), Rect::new(inner.x + 1, inner.y + i as u16, inner.width.saturating_sub(1), 1));
    }
}

fn hint(k: &str, what: &str) -> Vec<Span<'static>> {
    vec![Span::styled(format!(" {k} "), Style::new().fg(INK).bg(Color::Rgb(61, 90, 69))), Span::styled(format!(" {what}  "), st(DIM))]
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = Vec::new();
    spans.extend(hint("Tab", "next"));
    match &app.form().focused().kind {
        Kind::Text { .. } => {
            spans.extend(hint("type", "edit"));
            spans.extend(hint("Ctrl+U", "clear"));
        }
        Kind::Path { .. } => {
            spans.extend(hint("Enter", "browse"));
            spans.extend(hint("Del", "clear"));
        }
        Kind::Paths { .. } => {
            spans.extend(hint("Enter", "add"));
            spans.extend(hint("d", "remove"));
        }
        Kind::Choice { .. } | Kind::Step { .. } => spans.extend(hint("←→", "change")),
        Kind::Button => spans.extend(hint("Enter", "run")),
    }
    spans.extend(hint("1-4", "screens"));
    spans.extend(hint("?", "help"));
    spans.extend(hint("Ctrl+Q", "quit"));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h)
}

fn draw_browser(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(b) = &app.browser else { return };
    let r = centered(area, 78, 22.min(area.height.saturating_sub(2)));
    f.render_widget(Clear, r);
    let filter = match b.filter {
        PathFilter::Carrier => "images and audio that can carry data",
        PathFilter::Readable => "images, audio or text files",
        PathFilter::Any => "any file",
        PathFilter::Dir => "folders - press s to choose the open folder",
    };
    let title = format!(" {} - {} ", b.title, filter);
    let block = Block::bordered().border_style(st(ORANGE)).title(Span::styled(clip(&title, r.width.saturating_sub(4) as usize), bold(ORANGE)));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let shown = b.dir.as_ref().map(|d| d.display().to_string()).unwrap_or_else(|| "This computer".into());
    f.render_widget(Paragraph::new(Span::styled(tail(&shown, inner.width as usize), st(SKY))), Rect::new(inner.x, inner.y, inner.width, 1));
    let list_h = inner.height.saturating_sub(3) as usize;
    let top = b.sel.saturating_sub(list_h / 2).min(b.rows.len().saturating_sub(list_h));
    let mut hits = Vec::new();
    for (n, row) in b.rows.iter().enumerate().skip(top).take(list_h) {
        let y = inner.y + 1 + (n - top) as u16;
        let sel = n == b.sel;
        let (text, color) = match row {
            Row::Parent => ("[..]  (up)".to_string(), SKY),
            Row::Entry(e) => match e.kind {
                FileKind::Dir => (format!("[{}]", e.name), SKY),
                FileKind::Carrier(_) => (format!("{:<34} {:<14} {}", clip(&e.name, 34), e.info.as_ref().map(|i| i.summary.clone()).unwrap_or_default(), fmt_bytes(e.size as usize)), GREEN),
                FileKind::Unusable => (format!("{:<34} {}", clip(&e.name, 34), e.note.clone().unwrap_or_default()), DIM),
                FileKind::Other => (format!("{:<34} {}", clip(&e.name, 34), fmt_bytes(e.size as usize)), if b.filter == PathFilter::Carrier { DIM } else { INK }),
            },
        };
        let style = if sel { Style::new().fg(DARK).bg(GREEN) } else { st(color) };
        f.render_widget(Paragraph::new(Span::styled(clip(&text, inner.width as usize), style)), Rect::new(inner.x, y, inner.width, 1));
        hits.push(Hit { rect: Rect::new(inner.x, y, inner.width, 1), kind: HitKind::BrowserRow(n) });
    }
    if let Some(note) = &b.note {
        f.render_widget(Paragraph::new(Span::styled(clip(note, inner.width as usize), st(ORANGE))), Rect::new(inner.x, inner.y + inner.height - 2, inner.width, 1));
    }
    f.render_widget(Paragraph::new(Span::styled("Enter open/select   Backspace up   s choose folder   Esc cancel", st(DIM))), Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1));
    hits.push(Hit { rect: Rect::new(r.x, r.y, r.width, 1), kind: HitKind::CloseBrowser });
    app.hits.extend(hits);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let r = centered(area, 64, 18);
    f.render_widget(Clear, r);
    let block = Block::default().borders(Borders::ALL).border_style(st(ORANGE)).title(Span::styled(" Keys ", bold(ORANGE)));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let rows = [
        ("Tab / Shift+Tab", "next / previous field"),
        ("Up / Down", "move between fields (or inside a list)"),
        ("Left / Right", "change a choice or number"),
        ("Enter", "open the file browser, or run the button"),
        ("Delete", "clear a file field"),
        ("a / d", "add / remove an image in a list"),
        ("1-4, F1-F4, Alt+1-4", "switch screens"),
        ("Ctrl+U", "clear the text field"),
        ("Ctrl+Q or Ctrl+C", "quit"),
        ("mouse", "click tabs, fields and buttons"),
    ];
    let lines: Vec<Line> = rows.iter().map(|(k, v)| Line::from(vec![Span::styled(format!("{k:<22}"), bold(GREEN)), Span::styled(*v, st(INK))])).chain([Line::from(""), Line::from(Span::styled("Press any key to close", st(DIM)))]).collect();
    f.render_widget(Paragraph::new(lines), Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(2), inner.height));
}
