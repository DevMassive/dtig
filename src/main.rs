use std::io::{self, stdout};
use ratatui::{
    crossterm::{
        event::{self, Event, KeyCode},
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
        ExecutableCommand,
    },
    prelude::*,
    widgets::Paragraph,
};
use git2::{Repository, StatusOptions};
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
            return Ok(())
        }
    };

    let mut app = App::new(repo);
    app.update_status();

    while !app.should_quit {
        terminal.draw(|f| ui(f, &app))?;
        if let Event::Key(key) = event::read()? {
            if key.code == KeyCode::Char('q') {
                app.should_quit = true;
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

struct App {
    repo: Repository,
    status: Vec<String>,
    should_quit: bool,
}

impl App {
    fn new(repo: Repository) -> Self {
        Self {
            repo,
            status: Vec::new(),
            should_quit: false,
        }
    }

    fn update_status(&mut self) {
        let mut status_opts = StatusOptions::new();
        status_opts.include_untracked(true);
        let statuses = match self.repo.statuses(Some(&mut status_opts)) {
            Ok(statuses) => statuses,
            Err(_) => return,
        };

        let mut status_list = Vec::new();
        for entry in statuses.iter() {
            let status = entry.status();
            let path = entry.path().unwrap_or("[invalid utf-8]");
            let status_str = if status.is_wt_modified() {
                "modified:"
            } else if status.is_wt_new() {
                "new file:"
            } else if status.is_wt_deleted() {
                "deleted:"
            } else if status.is_wt_renamed() {
                "renamed:"
            } else if status.is_wt_typechange() {
                "typechange:"
            } else if status.is_index_new() {
                "new file:"
            } else if status.is_index_modified() {
                "modified:"
            } else if status.is_index_deleted() {
                "deleted:"
            } else if status.is_index_renamed() {
                "renamed:"
            } else if status.is_index_typechange() {
                "typechange:"
            } else {
                ""
            };
            if !status_str.is_empty() {
                status_list.push(format!("\t{} {}", status_str, path));
            }
        }
        self.status = status_list;
    }
}

fn ui(frame: &mut Frame, app: &App) {
    let status_text: String = app.status.join("\n");
    frame.render_widget(
        Paragraph::new(status_text),
        frame.area(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;
    use git2::{Repository, Signature, StatusOptions};

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
        repo.commit(Some("HEAD"), &signature, &signature, "initial commit", &tree, &[]).unwrap();
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

        assert_eq!(app.status.len(), 1);
        assert_eq!(app.status[0], "\tnew file: new_file.txt");
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
        repo.commit(Some("HEAD"), &signature, &signature, "add modified_file.txt", &tree, &[]).unwrap();

        let mut file = File::options().append(true).open(&file_path).unwrap();
        writeln!(file, " world").unwrap();

        let repo_for_app = Repository::open(temp_dir.path()).unwrap();

        {
            let mut status_opts = StatusOptions::new();
            status_opts.include_untracked(true);
            let statuses = repo_for_app.statuses(Some(&mut status_opts)).unwrap();
            let entry = statuses.iter().find(|e| e.path() == Some("modified_file.txt")).unwrap();
            let status = entry.status();
            
            assert!(status.is_wt_modified(), "WT_MODIFIED should be true. Status was: {:?}", status);
        }

        let mut app = App::new(repo_for_app);
        app.update_status();

        assert_eq!(app.status.len(), 1);
        assert_eq!(app.status[0], "\tmodified: modified_file.txt");
    }
}