use crate::git::{self, FileType, StatusFiles};
use git2::Repository;

pub enum FocusArea {
    Commit,
    Files,
    Diff,
}

pub struct App<'a> {
    pub repo: &'a Repository,
    pub status: StatusFiles,
    pub selected_file_type: FileType,
    pub selected_file_index: usize,
    pub should_quit: bool,
    pub commit_message: String,
    pub focus: FocusArea,
    pub diff: String,
    pub parsed_diff: Option<git::ParsedDiff>,
    pub diff_scroll: u16,
    pub diff_selected_line: usize,
}

impl<'a> App<'a> {
    pub fn new(repo: &'a Repository) -> Self {
        let mut app = Self {
            repo,
            status: StatusFiles::default(),
            selected_file_type: FileType::Staged,
            selected_file_index: 0,
            should_quit: false,
            commit_message: String::new(),
            focus: FocusArea::Files,
            diff: String::new(),
            parsed_diff: None,
            diff_scroll: 0,
            diff_selected_line: 0,
        };
        app.update_status();
        app
    }

    pub fn update_status(&mut self) {
        self.status = git::get_status(self.repo);
        let current_section_len = match self.selected_file_type {
            FileType::Staged => self.status.staged.len(),
            FileType::NotStaged => self.status.not_staged.len(),
            FileType::Untracked => self.status.untracked.len(),
        };

        if current_section_len == 0 {
            self.selected_file_index = 0;
        } else if self.selected_file_index >= current_section_len {
            self.selected_file_index = current_section_len - 1;
        }
        self.update_diff();
    }

    fn get_selected_file(&self) -> Option<(String, FileType)> {
        match self.selected_file_type {
            FileType::Staged => self
                .status
                .staged
                .get(self.selected_file_index)
                .map(|s| (s.clone(), FileType::Staged)),
            FileType::NotStaged => self
                .status
                .not_staged
                .get(self.selected_file_index)
                .map(|s| (s.clone(), FileType::NotStaged)),
            FileType::Untracked => self
                .status
                .untracked
                .get(self.selected_file_index)
                .map(|s| (s.clone(), FileType::Untracked)),
        }
    }

    pub fn update_diff(&mut self) {
        let diff_text = if let Some((path, file_type)) = self.get_selected_file() {
            match git::get_diff(self.repo, &path, file_type) {
                Ok(text) => {
                    self.parsed_diff = Some(git::parse_diff_output(&text));
                    text
                }
                Err(e) => {
                    self.parsed_diff = None;
                    format!("Failed to generate diff: {e}")
                }
            }
        } else {
            self.parsed_diff = None;
            String::new()
        };

        self.diff = diff_text;
        self.diff_scroll = 0;
        self.diff_selected_line = 0;
    }

    pub fn select_next(&mut self) {
        let (staged_len, not_staged_len, untracked_len) = (
            self.status.staged.len(),
            self.status.not_staged.len(),
            self.status.untracked.len(),
        );

        match self.selected_file_type {
            FileType::Staged => {
                if self.selected_file_index + 1 < staged_len {
                    self.selected_file_index += 1;
                } else if not_staged_len > 0 {
                    self.selected_file_type = FileType::NotStaged;
                    self.selected_file_index = 0;
                } else if untracked_len > 0 {
                    self.selected_file_type = FileType::Untracked;
                    self.selected_file_index = 0;
                } else {
                    self.selected_file_index = 0;
                }
            }
            FileType::NotStaged => {
                if self.selected_file_index + 1 < not_staged_len {
                    self.selected_file_index += 1;
                } else if untracked_len > 0 {
                    self.selected_file_type = FileType::Untracked;
                    self.selected_file_index = 0;
                } else if staged_len > 0 {
                    self.selected_file_type = FileType::Staged;
                    self.selected_file_index = 0;
                } else {
                    self.selected_file_index = 0;
                }
            }
            FileType::Untracked => {
                if self.selected_file_index + 1 < untracked_len {
                    self.selected_file_index += 1;
                } else if staged_len > 0 {
                    self.selected_file_type = FileType::Staged;
                    self.selected_file_index = 0;
                } else if not_staged_len > 0 {
                    self.selected_file_type = FileType::NotStaged;
                    self.selected_file_index = 0;
                } else {
                    self.selected_file_index = 0;
                }
            }
        }
        self.update_diff();
    }

    pub fn select_previous(&mut self) {
        let (staged_len, not_staged_len, untracked_len) = (
            self.status.staged.len(),
            self.status.not_staged.len(),
            self.status.untracked.len(),
        );

        match self.selected_file_type {
            FileType::Staged => {
                if self.selected_file_index > 0 {
                    self.selected_file_index -= 1;
                } else if untracked_len > 0 {
                    self.selected_file_type = FileType::Untracked;
                    self.selected_file_index = untracked_len - 1;
                } else if not_staged_len > 0 {
                    self.selected_file_type = FileType::NotStaged;
                    self.selected_file_index = not_staged_len - 1;
                } else {
                    self.selected_file_index = 0;
                }
            }
            FileType::NotStaged => {
                if self.selected_file_index > 0 {
                    self.selected_file_index -= 1;
                } else if staged_len > 0 {
                    self.selected_file_type = FileType::Staged;
                    self.selected_file_index = staged_len - 1;
                } else if untracked_len > 0 {
                    self.selected_file_type = FileType::Untracked;
                    self.selected_file_index = untracked_len - 1;
                } else {
                    self.selected_file_index = 0;
                }
            }
            FileType::Untracked => {
                if self.selected_file_index > 0 {
                    self.selected_file_index -= 1;
                } else if not_staged_len > 0 {
                    self.selected_file_type = FileType::NotStaged;
                    self.selected_file_index = not_staged_len - 1;
                } else if staged_len > 0 {
                    self.selected_file_type = FileType::Staged;
                    self.selected_file_index = staged_len - 1;
                } else {
                    self.selected_file_index = 0;
                }
            }
        }
        self.update_diff();
    }

    pub fn toggle_selection(&mut self) {
        if let Some((path, file_type)) = self.get_selected_file() {
            let result = match file_type {
                FileType::Staged => git::unstage(self.repo, &path),
                FileType::NotStaged | FileType::Untracked => git::stage(self.repo, &path),
            };
            if result.is_ok() {
                self.update_status();
            }
        }
    }

    pub fn commit(&mut self) {
        if !self.commit_message.is_empty()
            && git::commit(self.repo, &self.commit_message.clone()).is_ok()
        {
            self.commit_message.clear();
            self.update_status();
        }
    }

    pub fn apply_hunk(&mut self) {
        if let Some(parsed_diff) = &self.parsed_diff {
            if let Some(hunk_index) = git::get_hunk_index_from_line(
                parsed_diff,
                self.diff_selected_line
                    .saturating_sub(self.diff_scroll as usize),
            ) {
                if let Some(patch) = git::create_patch_from_hunk(parsed_diff, hunk_index) {
                    let repo_path = self.repo.path().parent().unwrap();
                    if git::apply_patch_to_index(repo_path, &patch).is_ok() {
                        self.update_status();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Repository, Signature};
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;
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

        let app = App::new(&repo);

        assert_eq!(app.status.untracked.len(), 1);
        assert_eq!(app.status.untracked[0], "new_file.txt");
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

        let app = App::new(&repo);

        assert_eq!(app.status.not_staged.len(), 1);
        assert_eq!(app.status.not_staged[0], "modified_file.txt");
    }

    #[test]
    fn test_toggle_selection_stage_untracked_file() {
        let temp_dir = TempDir::new().unwrap();
        let repo = setup_repo(&temp_dir);
        commit_initial(&repo);

        let file_path = temp_dir.path().join("new_file.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "hello").unwrap();

        let mut app = App::new(&repo);

        assert_eq!(app.status.untracked.len(), 1);
        assert_eq!(app.status.staged.len(), 0);
        app.selected_file_type = FileType::Untracked;
        app.selected_file_index = 0;

        app.toggle_selection();

        assert_eq!(app.status.untracked.len(), 0);
        assert_eq!(app.status.staged.len(), 1);
        assert_eq!(app.status.staged[0], "new_file.txt");
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

        let mut app = App::new(&repo);

        assert_eq!(app.status.staged.len(), 1);
        assert_eq!(app.status.untracked.len(), 0);
        app.selected_file_type = FileType::Staged;
        app.selected_file_index = 0;

        app.toggle_selection();

        assert_eq!(app.status.staged.len(), 0);
        assert_eq!(app.status.untracked.len(), 1);
        assert_eq!(app.status.untracked[0], "new_file.txt");
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
        let mut app = App::new(&repo);
        assert_eq!(app.status.staged.len(), 1, "File should be staged");
        assert_eq!(app.status.staged[0], "a.txt");
        assert_eq!(
            app.status.not_staged.len(),
            0,
            "Should be no not-staged files yet"
        );

        // 5. Select the file and unstage it
        app.selected_file_type = FileType::Staged;
        app.selected_file_index = 0;
        app.toggle_selection();

        // 6. Assert final state (file is now not-staged)
        assert_eq!(app.status.staged.len(), 0, "File should be unstaged");
        assert_eq!(
            app.status.not_staged.len(),
            1,
            "File should be in not-staged list"
        );
        assert_eq!(app.status.not_staged[0], "a.txt");
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
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "add staged.txt",
            &tree,
            &[&parent_commit],
        )
        .unwrap();

        // 2. Create and commit a file for the not-staged test
        let not_staged_path = temp_dir.path().join("not_staged.txt");
        let mut not_staged_file = File::create(&not_staged_path).unwrap();
        writeln!(not_staged_file, "abc").unwrap();
        index.add_path(Path::new("not_staged.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let parent_commit = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "add not_staged.txt",
            &tree,
            &[&parent_commit],
        )
        .unwrap();

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
        let mut app = App::new(&repo);

        // Test diff for staged file
        let staged_index_in_vec = app
            .status
            .staged
            .iter()
            .position(|r| r == "staged.txt")
            .unwrap();
        app.selected_file_type = FileType::Staged;
        app.selected_file_index = staged_index_in_vec;
        app.update_diff();
        assert!(
            app.diff.contains("\n+line2"),
            "Diff for staged was: '{}'",
            app.diff
        );

        // Test diff for not-staged file
        let not_staged_index_in_vec = app
            .status
            .not_staged
            .iter()
            .position(|r| r == "not_staged.txt")
            .unwrap();
        app.selected_file_type = FileType::NotStaged;
        app.selected_file_index = not_staged_index_in_vec;
        app.update_diff();
        assert!(
            app.diff.contains("\n+def"),
            "Diff for not-staged was: '{}'",
            app.diff
        );

        // Test diff for untracked file
        let untracked_index_in_vec = app
            .status
            .untracked
            .iter()
            .position(|r| r == "untracked.txt")
            .unwrap();
        app.selected_file_type = FileType::Untracked;
        app.selected_file_index = untracked_index_in_vec;
        app.update_diff();
        assert!(
            app.diff.contains("+new"),
            "Diff for untracked was: '{}'",
            app.diff
        );
    }
}
