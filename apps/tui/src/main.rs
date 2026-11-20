use ratatui::crossterm::event::{self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, KeyEventKind};
use ratatui::crossterm::execute;
use std::io::stdout;
use std::time::Duration;
use stegosaur_tui::app::{App, Msg};
use stegosaur_tui::ui;

fn wants_ascii() -> bool {
    std::env::var("STEGOSAUR_ASCII").map(|v| v == "1").unwrap_or(false) || std::env::var("TERM").map(|t| t == "dumb").unwrap_or(false)
}

fn main() -> std::io::Result<()> {
    ui::set_ascii(wants_ascii());
    let start = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let mut app = App::new(start);

    // `ratatui::init` enters raw mode and the alternate screen and installs a panic hook that restores them.
    // The extra terminal modes below need the same care, so the hook is extended to undo them too.
    let mut terminal = ratatui::init();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste);
        previous(info);
    }));
    execute!(stdout(), EnableMouseCapture, EnableBracketedPaste)?;

    let result = run(&mut terminal, &mut app);

    let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    while !app.quit {
        terminal.draw(|f| ui::draw(f, app))?;
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                // Windows reports both key presses and releases; only presses (and repeats) count.
                Event::Key(k) if k.kind != KeyEventKind::Release => app.update(Msg::Key(k)),
                Event::Mouse(m) => app.update(Msg::Mouse(m)),
                Event::Paste(s) => app.update(Msg::Paste(s)),
                _ => {}
            }
        }
        app.update(Msg::Tick);
    }
    Ok(())
}
