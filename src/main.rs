use git2::{Repository, StatusOptions};
use ratatui::{
    crossterm::{
        ExecutableCommand,
        event::{self, Event, KeyCode, KeyEventKind},
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
    layout::{Constraint, Layout},
    prelude::*,
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use std::io::{self, stdout};
use std::path::Path;

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let repo = match Repository::open(".") {
        Ok(repo) => repo,
        Err(e) => {
            cleanup_terminal()?;
            eprintln!("Failed to open repository: {}", e);
            return Ok(());
        }
    };

    let mut app = App::new(repo);
    app.update_status();

    while !app.should_quit {
        terminal.draw(|f| ui(f, &app))?;
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match app.focus {
                    FocusArea::Commit => match key.code {
                        KeyCode::Char('q') => app.should_quit = true,
                        KeyCode::Char(c) => app.commit_message.push(c),
                        KeyCode::Backspace => {
                            app.commit_message.pop();
                        }
                        KeyCode::Enter => {
                            if !app.commit_message.is_empty() {
                                if app.commit(&app.commit_message.clone()).is_ok() {
                                    app.commit_message.clear();
                                    app.update_status();
                                }
                            }
                        }
                        KeyCode::Down => app.focus = FocusArea::Files,
                        _ => {}
                    },
                    FocusArea::Files => match key.code {
                        KeyCode::Char('q') => app.should_quit = true,
                        KeyCode::Down => app.select_next(),
                        KeyCode::Up => {
                            if app.selected_index == 0 {
                                app.focus = FocusArea::Commit;
                            } else {
                                app.select_previous();
                            }
                        }
                        KeyCode::Enter => {
                            app.toggle_selection();
                            app.update_status();
                        }
                        _ => {}
                    },
                }
            }
        }
    }

    cleanup_terminal()?;
    Ok(())
}

fn cleanup_terminal() -> io::Result<()> {
    stdout().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}

enum FocusArea { Commit, Files }

struct App {
    repo: Repository,
    staged_files: Vec<String>,
    not_staged_files: Vec<String>,
    untracked_files: Vec<String>,
    selected_index: usize,
    should_quit: bool,
    commit_message: String,
    focus: FocusArea,
}

impl App {
    fn new(repo: Repository) -> Self {
        Self {
            repo,
            staged_files: Vec::new(),
            not_staged_files: Vec::new(),
            untracked_files: Vec::new(),
            selected_index: 0,
            should_quit: false,
            commit_message: String::new(),
            focus: FocusArea::Files,
        }
    }

    fn update_status(&mut self) {
        // Force the index to be re-read from disk. This is crucial because
        // operations like `reset_default` modify the index, but `repo.statuses()`
        // might read from a stale in-memory cache.
        if let Ok(mut index) = self.repo.index() {
            let _ = index.read(true);
        }

        self.staged_files.clear();
        self.not_staged_files.clear();
        self.untracked_files.clear();

        let mut status_opts = StatusOptions::new();
        status_opts.include_untracked(true);
        let statuses = match self.repo.statuses(Some(&mut status_opts)) {
            Ok(statuses) => statuses,
            Err(_) => return,
        };

        for entry in statuses.iter() {
            let path = match entry.path() {
                Some(p) => p.to_string(),
                None => continue,
            };
            let status = entry.status();

            if status.intersects(
                git2::Status::INDEX_MODIFIED
                    | git2::Status::INDEX_NEW
                    | git2::Status::INDEX_DELETED
                    | git2::Status::INDEX_RENAMED
                    | git2::Status::INDEX_TYPECHANGE,
            ) {
                self.staged_files.push(path.clone());
            }

            if status.intersects(
                git2::Status::WT_MODIFIED
                    | git2::Status::WT_DELETED
                    | git2::Status::WT_RENAMED
                    | git2::Status::WT_TYPECHANGE,
            ) {
                self.not_staged_files.push(path.clone());
            }

            if status.is_wt_new() {
                self.untracked_files.push(path);
            }
        }

        let total_files = self.total_files();
        if total_files == 0 {
            self.selected_index = 0;
        } else if self.selected_index >= total_files {
            self.selected_index = total_files - 1;
        }
    }

    fn total_files(&self) -> usize {
        self.staged_files.len() + self.not_staged_files.len() + self.untracked_files.len()
    }

    fn select_next(&mut self) {
        let total = self.total_files();
        if total > 0 {
            self.selected_index = (self.selected_index + 1) % total;
        }
    }

    fn select_previous(&mut self) {
        let total = self.total_files();
        if total > 0 {
            if self.selected_index == 0 {
                self.selected_index = total - 1;
            } else {
                self.selected_index -= 1;
            }
        }
    }

    fn toggle_selection(&mut self) {
        let total_staged = self.staged_files.len();
        let total_not_staged = self.not_staged_files.len();

        if self.selected_index >= self.total_files() {
            return;
        }

        if self.selected_index < total_staged {
            // Unstage the file. This is equivalent to `git reset HEAD <path>`.
            let path = &self.staged_files[self.selected_index];
            match self.repo.head() {
                Ok(head) => {
                    // HEAD exists. Peel it to a commit, get its tree, and reset the index from there.
                    if let Ok(commit) = head.peel_to_commit() {
                        self.repo
                            .reset_default(Some(commit.as_object()), &[path])
                            .unwrap();
                    }
                }
                Err(_) => {
                    // No HEAD exists (e.g. initial commit). Unstaging means removing from the index.
                    if let Ok(mut index) = self.repo.index() {
                        index.remove_path(Path::new(path)).unwrap();
                        index.write().unwrap();
                    }
                }
            }
        } else {
            // Stage
            let path_str = if self.selected_index < total_staged + total_not_staged {
                &self.not_staged_files[self.selected_index - total_staged]
            } else {
                &self.untracked_files[self.selected_index - total_staged - total_not_staged]
            };
            let mut index = self.repo.index().unwrap();
            index.add_path(Path::new(path_str)).unwrap();
            index.write().unwrap();
        }
    }

    fn commit(&mut self, message: &str) -> Result<git2::Oid, git2::Error> {
        let mut index = self.repo.index()?;
        let tree_oid = index.write_tree()?;
        let tree = self.repo.find_tree(tree_oid)?;

        let signature = self.repo.signature()?;

        let parent_commit = self.find_head_commit()?;
        let parents = if let Some(parent) = &parent_commit {
            vec![parent]
        } else {
            vec![]
        };

        self.repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents,
        )
    }

    fn find_head_commit(&self) -> Result<Option<git2::Commit>, git2::Error> {
        match self.repo.head() {
            Ok(head) => head.peel_to_commit().map(Some),
            Err(e) if e.code() == git2::ErrorCode::UnbornBranch => Ok(None),
            Err(e) => Err(e),
        }
    }
}

fn ui(frame: &mut Frame, app: &App) {
    let main_chunks = Layout::default()
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(frame.area());

    let input_block = Block::default().borders(Borders::ALL).title("Commit Message");
    let input = Paragraph::new(app.commit_message.as_str())
        .style(match app.focus {
            FocusArea::Commit => Style::default().fg(Color::Yellow),
            _ => Style::default(),
        })
        .block(input_block);
    frame.render_widget(input, main_chunks[0]);

    if let FocusArea::Commit = app.focus {
        frame.set_cursor_position((
            main_chunks[0].x + app.commit_message.len() as u16 + 1,
            main_chunks[0].y + 1,
        ));
    }

    let file_chunks = Layout::default()
        .constraints(
            [
                Constraint::Percentage(33),
                Constraint::Percentage(33),
                Constraint::Percentage(34),
            ]
            .as_ref(),
        )
        .split(main_chunks[1]);

    let mut current_index = 0;

    let staged_items: Vec<ListItem> = app
        .staged_files
        .iter()
        .map(|file| {
            let mut style = Style::default();
            if app.selected_index == current_index {
                style = style.add_modifier(Modifier::REVERSED);
            }
            current_index += 1;
            ListItem::new(file.as_str()).style(style)
        })
        .collect();
    let staged_list =
        List::new(staged_items).block(Block::default().borders(Borders::ALL).title("Staged"));
    frame.render_widget(staged_list, file_chunks[0]);

    let not_staged_items: Vec<ListItem> = app
        .not_staged_files
        .iter()
        .map(|file| {
            let mut style = Style::default();
            if app.selected_index == current_index {
                style = style.add_modifier(Modifier::REVERSED);
            }
            current_index += 1;
            ListItem::new(file.as_str()).style(style)
        })
        .collect();
    let not_staged_list = List::new(not_staged_items)
        .block(Block::default().borders(Borders::ALL).title("Not Staged"));
    frame.render_widget(not_staged_list, file_chunks[1]);

    let untracked_items: Vec<ListItem> = app
        .untracked_files
        .iter()
        .map(|file| {
            let mut style = Style::default();
            if app.selected_index == current_index {
                style = style.add_modifier(Modifier::REVERSED);
            }
            current_index += 1;
            ListItem::new(file.as_str()).style(style)
        })
        .collect();
    let untracked_list =
        List::new(untracked_items).block(Block::default().borders(Borders::ALL).title("Untracked"));
    frame.render_widget(untracked_list, file_chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Repository, Signature};
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    fn setup_repo(temp_dir: &TempDir) -> Repository {
        let repo = Repository::init(temp_dir.path()).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test User").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();
        repo
    }

    fn commit_initial(repo: &Repository) {
        let mut index = repo.index().unwrap();
        let oid = index.write_tree().unwrap();
        let tree = repo.find_tree(oid).unwrap();
        let signature = Signature::now("Test User", "test@example.com").unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "initial commit",
            &tree,
            &[],
        )
        .unwrap();
    }

    #[test]
    fn test_update_status_new_file() {
        let temp_dir = TempDir::new().unwrap();
        let repo = setup_repo(&temp_dir);
        commit_initial(&repo);

        let file_path = temp_dir.path().join("new_file.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "hello").unwrap();

        let repo_for_app = Repository::open(temp_dir.path()).unwrap();
        let mut app = App::new(repo_for_app);
        app.update_status();

        assert_eq!(app.untracked_files.len(), 1);
        assert_eq!(app.untracked_files[0], "new_file.txt");
    }

    #[test]
    fn test_update_status_modified_file() {
        let temp_dir = TempDir::new().unwrap();
        let repo = setup_repo(&temp_dir);

        let file_path = temp_dir.path().join("modified_file.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "hello").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("modified_file.txt")).unwrap();
        index.write().unwrap();
        let oid = index.write_tree().unwrap();
        let tree = repo.find_tree(oid).unwrap();
        let signature = Signature::now("Test User", "test@example.com").unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "add modified_file.txt",
            &tree,
            &[],
        )
        .unwrap();

        let mut file = File::options().append(true).open(&file_path).unwrap();
        writeln!(file, " world").unwrap();

        let repo_for_app = Repository::open(temp_dir.path()).unwrap();

        let mut app = App::new(repo_for_app);
        app.update_status();

        assert_eq!(app.not_staged_files.len(), 1);
        assert_eq!(app.not_staged_files[0], "modified_file.txt");
    }

    #[test]
    fn test_toggle_selection_stage_untracked_file() {
        let temp_dir = TempDir::new().unwrap();
        let repo = setup_repo(&temp_dir);
        commit_initial(&repo);

        let file_path = temp_dir.path().join("new_file.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "hello").unwrap();

        let repo_for_app = Repository::open(temp_dir.path()).unwrap();
        let mut app = App::new(repo_for_app);
        app.update_status();

        assert_eq!(app.untracked_files.len(), 1);
        assert_eq!(app.staged_files.len(), 0);
        app.selected_index = 0; // The only file is untracked

        app.toggle_selection();
        app.update_status();

        assert_eq!(app.untracked_files.len(), 0);
        assert_eq!(app.staged_files.len(), 1);
        assert_eq!(app.staged_files[0], "new_file.txt");
    }

    #[test]
    fn test_toggle_selection_unstage_staged_file() {
        let temp_dir = TempDir::new().unwrap();
        let repo = setup_repo(&temp_dir);
        commit_initial(&repo);

        let file_path = temp_dir.path().join("new_file.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "hello").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("new_file.txt")).unwrap();
        index.write().unwrap();

        let repo_for_app = Repository::open(temp_dir.path()).unwrap();
        let mut app = App::new(repo_for_app);
        app.update_status();

        assert_eq!(app.staged_files.len(), 1);
        assert_eq!(app.untracked_files.len(), 0);
        app.selected_index = 0; // The only file is staged

        app.toggle_selection();
        app.update_status();

        assert_eq!(app.staged_files.len(), 0);
        assert_eq!(app.untracked_files.len(), 1);
        assert_eq!(app.untracked_files[0], "new_file.txt");
    }

    #[test]
    fn test_unstage_modified_file_scenario() {
        // 1. Setup repo and commit a file
        let temp_dir = TempDir::new().unwrap();
        let repo = setup_repo(&temp_dir);
        let file_path = temp_dir.path().join("a.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "v1").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let oid = index.write_tree().unwrap();
        let tree = repo.find_tree(oid).unwrap();
        let signature = Signature::now("Test User", "test@example.com").unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "commit v1",
            &tree,
            &[],
        )
        .unwrap();

        // 2. Modify the file
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&file_path)
            .unwrap();
        writeln!(file, "v2").unwrap();

        // 3. Stage the modification
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();

        // 4. Create app and assert initial state (file is staged)
        let repo_for_app = Repository::open(temp_dir.path()).unwrap();
        let mut app = App::new(repo_for_app);
        app.update_status();
        assert_eq!(app.staged_files.len(), 1, "File should be staged");
        assert_eq!(app.staged_files[0], "a.txt");
        assert_eq!(
            app.not_staged_files.len(),
            0,
            "Should be no not-staged files yet"
        );

        // 5. Select the file and unstage it
        app.selected_index = 0;
        app.toggle_selection();
        app.update_status();

        // 6. Assert final state (file is now not-staged)
        assert_eq!(app.staged_files.len(), 0, "File should be unstaged");
        assert_eq!(
            app.not_staged_files.len(),
            1,
            "File should be in not-staged list"
        );
        assert_eq!(app.not_staged_files[0], "a.txt");
    }
}
