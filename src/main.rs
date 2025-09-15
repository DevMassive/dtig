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
                handle_key_event(&mut app, key.code);
            }
        }
    }

    cleanup_terminal()?;
    Ok(())
}

fn handle_key_event(app: &mut App, key_code: KeyCode) {
    match app.focus {
        FocusArea::Commit => match key_code {
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
        FocusArea::Files => match key_code {
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
            KeyCode::Char('j') => app.diff_scroll = app.diff_scroll.saturating_add(1),
            KeyCode::Char('k') => app.diff_scroll = app.diff_scroll.saturating_sub(1),
            _ => {}
        },
    }
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
    diff: String,
    diff_scroll: u16,
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
            diff: String::new(),
            diff_scroll: 0,
        }
    }

    fn update_diff(&mut self) {
        let selected_path_str = {
            let total_staged = self.staged_files.len();
            let total_not_staged = self.not_staged_files.len();

            if self.selected_index < total_staged {
                Some(self.staged_files[self.selected_index].clone())
            } else if self.selected_index < total_staged + total_not_staged {
                Some(self.not_staged_files[self.selected_index - total_staged].clone())
            } else if self.selected_index < self.total_files() {
                Some(self.untracked_files[self.selected_index - total_staged - total_not_staged].clone())
            } else {
                None
            }
        };

        let diff_text = if let Some(path_str) = selected_path_str {
            let path = Path::new(&path_str);
            let is_staged = self.staged_files.contains(&path_str);
            let is_untracked = self.untracked_files.contains(&path_str);

            let diff_result = if is_untracked {
                // For untracked files, show the whole file as an addition
                let full_path = self.repo.workdir().unwrap().join(path);
                match std::fs::read_to_string(full_path) {
                    Ok(content) => {
                        let lines = content.lines().map(|l| format!("+{}", l)).collect::<Vec<_>>();
                        Ok(lines.join("\n"))
                    }
                    Err(e) => Err(e.to_string()),
                }
            } else {
                let diff_opts = &mut git2::DiffOptions::new();
                diff_opts.pathspec(path);

                let diff = if is_staged {
                    let head_tree = self.repo.head().ok().and_then(|h| h.peel_to_tree().ok());
                    self.repo.diff_tree_to_index(head_tree.as_ref(), None, Some(diff_opts))
                } else {
                    self.repo.diff_index_to_workdir(None, Some(diff_opts))
                };

                match diff {
                    Ok(diff) => {
                        let mut diff_str = String::new();
                        diff.print(git2::DiffFormat::Patch, |_, _, line| {
                            let prefix = match line.origin() {
                                '+' | '-' | '=' => line.origin().to_string(),
                                _ => " ".to_string(),
                            };
                            diff_str.push_str(&format!("{}{}", prefix, String::from_utf8_lossy(line.content())));
                            true
                        }).unwrap();
                        Ok(diff_str)
                    }
                    Err(e) => Err(e.to_string()),
                }
            };

            match diff_result {
                Ok(text) => text,
                Err(e) => format!("Failed to generate diff: {}", e),
            }
        } else {
            String::new()
        };

        self.diff = diff_text;
        self.diff_scroll = 0;
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

        {
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
        }

        let total_files = self.total_files();
        if total_files == 0 {
            self.selected_index = 0;
        } else if self.selected_index >= total_files {
            self.selected_index = total_files - 1;
        }
        self.update_diff();
    }

    fn total_files(&self) -> usize {
        self.staged_files.len() + self.not_staged_files.len() + self.untracked_files.len()
    }

    fn select_next(&mut self) {
        let total = self.total_files();
        if total > 0 {
            self.selected_index = (self.selected_index + 1) % total;
        }
        self.update_diff();
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
        self.update_diff();
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
    let screen_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(frame.area());

    let left_chunks = Layout::default()
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(screen_chunks[0]);

    let input_block = Block::default().borders(Borders::ALL).title("Commit Message");
    let input = Paragraph::new(app.commit_message.as_str())
        .style(match app.focus {
            FocusArea::Commit => Style::default().fg(Color::Yellow),
            _ => Style::default(),
        })
        .block(input_block);
    frame.render_widget(input, left_chunks[0]);

    if let FocusArea::Commit = app.focus {
        frame.set_cursor_position((
            left_chunks[0].x + app.commit_message.len() as u16 + 1,
            left_chunks[0].y + 1,
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
        .split(left_chunks[1]);

    let mut current_index = 0;

    let staged_items: Vec<ListItem> = app
        .staged_files
        .iter()
        .map(|file| {
            let mut style = Style::default();
            if let FocusArea::Files = app.focus {
                if app.selected_index == current_index {
                    style = style.add_modifier(Modifier::REVERSED);
                }
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
            if let FocusArea::Files = app.focus {
                if app.selected_index == current_index {
                    style = style.add_modifier(Modifier::REVERSED);
                }
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
            if let FocusArea::Files = app.focus {
                if app.selected_index == current_index {
                    style = style.add_modifier(Modifier::REVERSED);
                }
            }
            current_index += 1;
            ListItem::new(file.as_str()).style(style)
        })
        .collect();
    let untracked_list =
        List::new(untracked_items).block(Block::default().borders(Borders::ALL).title("Untracked"));
    frame.render_widget(untracked_list, file_chunks[2]);

    let diff_view = Paragraph::new(app.diff.as_str())
        .block(Block::default().borders(Borders::ALL).title("Diff"))
        .scroll((app.diff_scroll, 0));
    frame.render_widget(diff_view, screen_chunks[1]);
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

    #[test]
    fn test_focus_movement() {
        let temp_dir = TempDir::new().unwrap();
        let repo = setup_repo(&temp_dir);
        let mut app = App::new(repo);

        // With a file in the list
        let file_path = temp_dir.path().join("new_file.txt");
        File::create(&file_path).unwrap();
        app.update_status();

        // Initial state: Files focus
        assert!(matches!(app.focus, FocusArea::Files));
        assert_eq!(app.selected_index, 0);

        // Press Up at index 0 -> focus moves to Commit
        handle_key_event(&mut app, KeyCode::Up);
        assert!(matches!(app.focus, FocusArea::Commit));

        // Press Down -> focus moves back to Files
        handle_key_event(&mut app, KeyCode::Down);
        assert!(matches!(app.focus, FocusArea::Files));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_commit_input() {
        let temp_dir = TempDir::new().unwrap();
        let repo = setup_repo(&temp_dir);
        let mut app = App::new(repo);
        app.focus = FocusArea::Commit;

        // Type a message
        handle_key_event(&mut app, KeyCode::Char('t'));
        handle_key_event(&mut app, KeyCode::Char('e'));
        handle_key_event(&mut app, KeyCode::Char('s'));
        handle_key_event(&mut app, KeyCode::Char('t'));
        assert_eq!(app.commit_message, "test");

        // Backspace
        handle_key_event(&mut app, KeyCode::Backspace);
        assert_eq!(app.commit_message, "tes");

        // Quit
        handle_key_event(&mut app, KeyCode::Char('q'));
        assert!(app.should_quit)
    }

    #[test]
    fn test_file_navigation_does_not_switch_focus() {
        let temp_dir = TempDir::new().unwrap();
        let repo = setup_repo(&temp_dir);
        
        // Add two files
        File::create(temp_dir.path().join("file1.txt")).unwrap();
        File::create(temp_dir.path().join("file2.txt")).unwrap();

        let mut app = App::new(repo);
        app.update_status();

        assert_eq!(app.total_files(), 2);
        app.selected_index = 1; // Start at the second file

        // Press Up, should not change focus
        handle_key_event(&mut app, KeyCode::Up);
        assert!(matches!(app.focus, FocusArea::Files));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_diff_generation() {
        let temp_dir = TempDir::new().unwrap();
        let repo = setup_repo(&temp_dir);
        commit_initial(&repo);

        // 1. Create and commit a file for the staged test
        let staged_path = temp_dir.path().join("staged.txt");
        let mut staged_file = File::create(&staged_path).unwrap();
        writeln!(staged_file, "line1").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("staged.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let signature = repo.signature().unwrap();
        let parent_commit = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "add staged.txt", &tree, &[&parent_commit]).unwrap();

        // 2. Create and commit a file for the not-staged test
        let not_staged_path = temp_dir.path().join("not_staged.txt");
        let mut not_staged_file = File::create(&not_staged_path).unwrap();
        writeln!(not_staged_file, "abc").unwrap();
        index.add_path(Path::new("not_staged.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let parent_commit = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "add not_staged.txt", &tree, &[&parent_commit]).unwrap();

        // 3. Modify the files
        writeln!(staged_file, "line2").unwrap(); // Modified
        index.add_path(Path::new("staged.txt")).unwrap(); // Staged
        index.write().unwrap();

        writeln!(not_staged_file, "def").unwrap(); // Modified, not staged

        // 4. Create an untracked file
        let untracked_path = temp_dir.path().join("untracked.txt");
        let mut untracked_file = File::create(&untracked_path).unwrap();
        writeln!(untracked_file, "new").unwrap();

        // 5. Create app and test
        let repo_for_app = Repository::open(temp_dir.path()).unwrap();
        let mut app = App::new(repo_for_app);
        app.update_status();

        // Test diff for staged file
        let staged_index = app.staged_files.iter().position(|r| r == "staged.txt").unwrap();
        app.selected_index = staged_index;
        app.update_diff();
        assert!(app.diff.contains("\n+line2"), "Diff for staged was: '{}'", app.diff);

        // Test diff for not-staged file
        let not_staged_index = app.not_staged_files.iter().position(|r| r == "not_staged.txt").unwrap();
        app.selected_index = app.staged_files.len() + not_staged_index;
        app.update_diff();
        assert!(app.diff.contains("\n+def"), "Diff for not-staged was: '{}'", app.diff);

        // Test diff for untracked file
        let untracked_index = app.untracked_files.iter().position(|r| r == "untracked.txt").unwrap();
        app.selected_index = app.staged_files.len() + app.not_staged_files.len() + untracked_index;
        app.update_diff();
        assert!(app.diff.contains("+new"), "Diff for untracked was: '{}'", app.diff);
    }
}
