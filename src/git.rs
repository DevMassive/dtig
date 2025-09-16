use git2::{Commit, Diff, DiffOptions, Error, ErrorCode, Oid, Repository, Status, StatusOptions};
use std::cell::RefCell;
use std::{path::Path};

#[derive(Default, Clone)]
pub struct StatusFiles {
    pub staged: Vec<String>,
    pub not_staged: Vec<String>,
    pub untracked: Vec<String>,
}

impl StatusFiles {
    pub fn total_files(&self) -> usize {
        self.staged.len() + self.not_staged.len() + self.untracked.len()
    }
}

pub fn get_status(repo: &Repository) -> StatusFiles {
    if let Ok(mut index) = repo.index() {
        let _ = index.read(true);
    }
    let mut status_files = StatusFiles::default();
    let mut status_opts = StatusOptions::new();
    status_opts.include_untracked(true);
    let statuses = match repo.statuses(Some(&mut status_opts)) {
        Ok(statuses) => statuses,
        Err(_) => return status_files,
    };
    for entry in statuses.iter() {
        let path = match entry.path() {
            Some(p) => p.to_string(),
            None => continue,
        };
        let status = entry.status();
        if status.intersects(
            Status::INDEX_MODIFIED
                | Status::INDEX_NEW
                | Status::INDEX_DELETED
                | Status::INDEX_RENAMED
                | Status::INDEX_TYPECHANGE,
        ) {
            status_files.staged.push(path.clone());
        }
        if status.intersects(
            Status::WT_MODIFIED | Status::WT_DELETED | Status::WT_RENAMED | Status::WT_TYPECHANGE,
        ) {
            status_files.not_staged.push(path.clone());
        }
        if status.is_wt_new() {
            status_files.untracked.push(path);
        }
    }
    status_files
}

#[derive(Clone, Copy)]
pub enum FileType {
    Staged,
    NotStaged,
    Untracked,
}

pub fn get_diff(repo: &Repository, path_str: &str, file_type: FileType) -> Result<String, String> {
    let path = Path::new(path_str);
    match file_type {
        FileType::Untracked => {
            let full_path = repo.workdir().unwrap().join(path);
            match std::fs::read_to_string(full_path) {
                Ok(content) => {
                    let lines = content.lines().map(|l| format!("+{l}")).collect::<Vec<_>>();
                    Ok(lines.join("\n"))
                }
                Err(e) => Err(e.to_string()),
            }
        }
        FileType::Staged => {
            let mut diff_opts = DiffOptions::new();
            diff_opts.pathspec(path);
            let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
            let diff = repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut diff_opts))
                .map_err(|e| e.to_string())?;
            format_diff(diff, None)
        }
        FileType::NotStaged => {
            let mut diff_opts = DiffOptions::new();
            diff_opts.pathspec(path);
            let diff = repo.diff_index_to_workdir(None, Some(&mut diff_opts))
                .map_err(|e| e.to_string())?;
            format_diff(diff, None)
        }
    }
}

fn format_diff(diff: Diff, line_number: Option<usize>) -> Result<String, String> {
    let diff_str = RefCell::new(String::new());
    diff.foreach(
        &mut |delta, _| {
            let mut diff_str = diff_str.borrow_mut();
            let old_path = delta
                .old_file()
                .path()
                .map_or("".to_string(), |p| p.to_string_lossy().to_string());
            let new_path = delta
                .new_file()
                .path()
                .map_or("".to_string(), |p| p.to_string_lossy().to_string());

            if !old_path.is_empty() && !new_path.is_empty() && old_path != new_path {
                diff_str.push_str(&format!("diff --git a/{old_path} b/{new_path}\n"));
            }
            if !old_path.is_empty() {
                diff_str.push_str(&format!("--- a/{old_path}\n"));
            } else {
                diff_str.push_str("--- /dev/null\n");
            }
            if !new_path.is_empty() {
                diff_str.push_str(&format!("+++ b/{new_path}\n"));
            } else {
                diff_str.push_str("+++ /dev/null\n");
            }
            true
        },
        None,
        Some(&mut |_delta, hunk| {
            if let Some(line) = line_number {
                let hunk_start_line = hunk.new_start() as usize;
                let hunk_end_line = (hunk.new_start() + hunk.new_lines()) as usize;
                if !(hunk_start_line <= line && line < hunk_end_line) {
                    return true;
                }
            }
            let mut diff_str = diff_str.borrow_mut();
            diff_str.push_str(&String::from_utf8_lossy(hunk.header()));
            true
        }),
        Some(&mut |_delta, _hunk, line| {
            let mut diff_str = diff_str.borrow_mut();
            let prefix = match line.origin() {
                '+' | '-' | '=' => line.origin().to_string(),
                _ => " ".to_string(),
            };
            diff_str.push_str(&format!(
                "{}{}",
                prefix,
                String::from_utf8_lossy(line.content())
            ));
            true
        }),
    )
    .map_err(|e| e.to_string())?;

    Ok(diff_str.into_inner())
}

pub fn stage(repo: &Repository, path_str: &str) -> Result<(), Error> {
    let mut index = repo.index()?;
    index.add_path(Path::new(path_str))?;
    index.write()
}

pub fn unstage(repo: &Repository, path: &str) -> Result<(), Error> {
    match repo.head() {
        Ok(head) => {
            if let Ok(commit) = head.peel_to_commit() {
                repo.reset_default(Some(commit.as_object()), [path])
            } else {
                Err(Error::from_str("Could not peel head to commit"))
            }
        }
        Err(_) => {
            let mut index = repo.index()?;
            index.remove_path(Path::new(path))?;
            index.write()
        }
    }
}

fn find_head_commit(repo: &Repository) -> Result<Option<Commit>, Error> {
    match repo.head() {
        Ok(head) => head.peel_to_commit().map(Some),
        Err(e) if e.code() == ErrorCode::UnbornBranch => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn commit(repo: &Repository, message: &str) -> Result<Oid, Error> {
    let mut index = repo.index()?;
    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;
    let signature = repo.signature()?;
    let parent_commit = find_head_commit(repo)?;
    let parents = if let Some(parent) = &parent_commit {
        vec![parent]
    } else {
        vec![]
    };
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &parents,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Repository;
    use std::fs;
    
    use tempfile::TempDir;

    fn setup_repo() -> (TempDir, Repository) {
        let temp_dir = TempDir::new().unwrap();
        let repo = Repository::init(temp_dir.path()).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "test").unwrap();
            config.set_str("user.email", "test@example.com").unwrap();
        }
        (temp_dir, repo)
    }

    fn create_and_commit_file(repo: &Repository, file_name: &str, content: &str) {
        let file_path = repo.workdir().unwrap().join(file_name);
        fs::write(&file_path, content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(file_name)).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = repo.signature().unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "Initial commit",
            &tree,
            &[],
        )
        .unwrap();
    }

    #[test]
    fn test_format_diff_single_hunk_extraction() {
        let (_temp_dir, repo) = setup_repo();
        let file_name = "test_file.txt";
        let initial_content = "line 1
line 2
line 3
line 4
line 5
line 6
line 7
line 8
line 9
line 10
line 11
line 12
line 13
line 14
line 15
";
        create_and_commit_file(&repo, file_name, initial_content);

        let modified_content = "line 1
MODIFIED LINE 2
line 3
line 4
line 5
line 6
line 7
line 8
line 9
line 10
line 11
MODIFIED LINE 12
line 13
line 14
line 15
";
        let file_path = repo.workdir().unwrap().join(file_name);
        fs::write(&file_path, modified_content).unwrap();

        let mut diff_opts = DiffOptions::new();
        diff_opts.pathspec(Path::new(file_name));
        let diff = repo.diff_index_to_workdir(None, Some(&mut diff_opts)).unwrap();

        // Test extracting the first hunk (line 2)
        let diff_for_hunk1 = repo.diff_index_to_workdir(None, Some(&mut diff_opts)).unwrap();
        let hunk1_diff = format_diff(diff_for_hunk1, Some(2)).unwrap();
        println!("Hunk 1 Diff:
{}", hunk1_diff);
        assert!(hunk1_diff.contains("-line 2"));
        assert!(hunk1_diff.contains("+MODIFIED LINE 2"));
        assert!(!hunk1_diff.contains("-line 12"));
        assert!(!hunk1_diff.contains("+MODIFIED LINE 12"));
        assert!(hunk1_diff.contains("@@ -2,7 +2,7 @@")); // Check for hunk header, context lines might vary

        // Test extracting the second hunk (line 12)
        let diff_for_hunk2 = repo.diff_index_to_workdir(None, Some(&mut diff_opts)).unwrap();
        let hunk2_diff = format_diff(diff_for_hunk2, Some(12)).unwrap();
        println!("Hunk 2 Diff:
{}", hunk2_diff);
        assert!(!hunk2_diff.contains("-line 2"));
        assert!(!hunk2_diff.contains("+MODIFIED LINE 2"));
        assert!(hunk2_diff.contains("-line 12"));
        assert!(hunk2_diff.contains("+MODIFIED LINE 12"));
        assert!(hunk2_diff.contains("@@ -12,4 +12,4 @@")); // Check for hunk header, context lines might vary
    }
}