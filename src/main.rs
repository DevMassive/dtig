mod app;
mod event;
mod git;
mod tui;
mod ui;

use crate::app::App;
use crate::event::handle_key_event;
use crate::tui::{init, restore};
use git2::Repository;
use ratatui::crossterm::event::{read, Event, KeyEventKind};
use std::io;

fn main() -> io::Result<()> {
    let mut terminal = init()?;

    let repo = match Repository::open(".") {
        Ok(repo) => repo,
        Err(e) => {
            restore()?;
            eprintln!("Failed to open repository: {}", e);
            // Return Ok because we've handled the error gracefully by printing a message.
            return Ok(());
        }
    };

    let mut app = App::new(&repo);

    // The main loop
    while !app.should_quit {
        // Render the UI
        terminal.draw(|f| ui::ui(f, &app))?;

        // Calculate a dynamic value based on frame size
        let frame_size = terminal.get_frame().area();
        let diff_view_height = frame_size.height.saturating_sub(2);

        // Handle events
        if let Event::Key(key) = read()? {
            if key.kind == KeyEventKind::Press {
                handle_key_event(&mut app, key.code, diff_view_height);
            }
        }
    }

    restore()?;
    Ok(())
}
