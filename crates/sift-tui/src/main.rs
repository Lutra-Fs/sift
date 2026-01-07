//! Sift TUI - Terminal User Interface
//!
//! Ratatui-based terminal interface for managing MCP servers and skills.

use ratatui::{
    crossterm::{
        event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    widgets::{Block, Paragraph},
    Frame, Terminal,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sift_tui=debug,info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the TUI
    let res = run_tui(&mut terminal);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("{err:?}");
    }

    Ok(())
}

fn run_tui(terminal: &mut Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>) -> anyhow::Result<()> {
    loop {
        terminal.draw(|f| ui(f))?;

        if let Event::Key(key) = event::read()? {
            if key.code == KeyCode::Char('q') {
                return Ok(());
            }
        }
    }
}

fn ui(f: &mut Frame) {
    let area = f.area();
    let paragraph = Paragraph::new("Sift - MCP & Skills Manager\n\nPress 'q' to quit")
        .block(Block::bordered().title("Sift TUI"))
        .centered();
    f.render_widget(paragraph, area);
}
