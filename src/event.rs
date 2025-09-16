use crate::app::{App, FocusArea};
use crate::git::FileType;
use ratatui::crossterm::event::KeyCode;

pub fn handle_key_event(app: &mut App, key_code: KeyCode, diff_view_height: u16) {
    match app.focus {
        FocusArea::Commit => match key_code {
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Char(c) => app.commit_message.push(c),
            KeyCode::Backspace => {
                app.commit_message.pop();
            }
            KeyCode::Enter => {
                app.commit();
            }
            KeyCode::Down => app.focus = FocusArea::Files,
            _ => {}
        },
        FocusArea::Files => match key_code {
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Down => app.select_next(),
            KeyCode::Up => {
                if app.selected_file_index == 0
                    && matches!(app.selected_file_type, FileType::Staged)
                {
                    app.focus = FocusArea::Commit;
                } else {
                    app.select_previous();
                }
            }
            KeyCode::Enter => {
                app.toggle_selection();
            }
            KeyCode::Right => app.focus = FocusArea::Diff,
            _ => {}
        },
        FocusArea::Diff => match key_code {
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Left => app.focus = FocusArea::Files,
            KeyCode::Down => {
                let diff_lines = app.diff.lines().count();
                if diff_lines > 0 {
                    app.diff_selected_line = (app.diff_selected_line + 1).min(diff_lines - 1);
                    if app.diff_selected_line
                        >= (app.diff_scroll as usize + diff_view_height as usize)
                    {
                        app.diff_scroll = app.diff_scroll.saturating_add(1);
                    }
                }
            }
            KeyCode::Up => {
                if app.diff_selected_line > 0 {
                    app.diff_selected_line -= 1;
                    if app.diff_selected_line < app.diff_scroll as usize {
                        app.diff_scroll = app.diff_scroll.saturating_sub(1);
                    }
                }
            }
            KeyCode::Enter => {
                app.apply_hunk();
            }
            _ => {}
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, FocusArea};
    use git2::Repository;
    use ratatui::crossterm::event::KeyCode;
    use std::fs::File;
    use tempfile::TempDir;

    // Helper functions duplicated from app.rs tests
    fn setup_repo(temp_dir: &TempDir) -> Repository {
        let repo = Repository::init(temp_dir.path()).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test User").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();
        repo
    }

    #[test]
    fn test_focus_movement() {
        let temp_dir = TempDir::new().unwrap();
        let repo = setup_repo(&temp_dir);

        // With a file in the list
        File::create(temp_dir.path().join("new_file.txt")).unwrap();

        let mut app = App::new(&repo);

        // Initial state: Files focus
        assert!(matches!(app.focus, FocusArea::Files));
        assert!(matches!(app.selected_file_type, FileType::Staged));
        assert_eq!(app.selected_file_index, 0);

        // Press Up at index 0 -> focus moves to Commit
        handle_key_event(&mut app, KeyCode::Up, 10);
        assert!(matches!(app.focus, FocusArea::Commit));

        // Press Down -> focus moves back to Files
        handle_key_event(&mut app, KeyCode::Down, 10);
        assert!(matches!(app.focus, FocusArea::Files));
        assert!(matches!(app.selected_file_type, FileType::Staged));
        assert_eq!(app.selected_file_index, 0);
    }

    #[test]
    fn test_commit_input() {
        let temp_dir = TempDir::new().unwrap();
        let repo = setup_repo(&temp_dir);
        let mut app = App::new(&repo);
        app.focus = FocusArea::Commit;

        // Type a message
        handle_key_event(&mut app, KeyCode::Char('t'), 10);
        handle_key_event(&mut app, KeyCode::Char('e'), 10);
        handle_key_event(&mut app, KeyCode::Char('s'), 10);
        handle_key_event(&mut app, KeyCode::Char('t'), 10);
        assert_eq!(app.commit_message, "test");

        // Backspace
        handle_key_event(&mut app, KeyCode::Backspace, 10);
        assert_eq!(app.commit_message, "tes");

        // Quit
        handle_key_event(&mut app, KeyCode::Char('q'), 10);
        assert!(app.should_quit)
    }

    #[test]
    fn test_file_navigation_does_not_switch_focus() {
        let temp_dir = TempDir::new().unwrap();
        let repo = setup_repo(&temp_dir);

        // Add two files
        File::create(temp_dir.path().join("file1.txt")).unwrap();
        File::create(temp_dir.path().join("file2.txt")).unwrap();

        let mut app = App::new(&repo);

        assert_eq!(app.status.total_files(), 2);
        app.selected_file_type = FileType::Staged;
        app.selected_file_index = 1; // Start at the second file

        // Press Up, should not change focus
        handle_key_event(&mut app, KeyCode::Up, 10);
        assert!(matches!(app.focus, FocusArea::Files));
        assert_eq!(app.selected_file_index, 0);
    }
}
