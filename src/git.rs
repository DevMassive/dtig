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
