use git2::{
    Commit, Diff, DiffOptions, Error, ErrorCode, Oid, Repository, Status, StatusOptions,
};
use std::path::Path;

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
            Status::WT_MODIFIED
                | Status::WT_DELETED
                | Status::WT_RENAMED
                | Status::WT_TYPECHANGE,
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
                    let lines = content.lines().map(|l| format!("+{}", l)).collect::<Vec<_>>();
                    Ok(lines.join("\n"))
                }
                Err(e) => Err(e.to_string()),
            }
        }
        FileType::Staged => {
            let mut diff_opts = DiffOptions::new();
            diff_opts.pathspec(path);
            let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
            repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut diff_opts))
                .map_err(|e| e.to_string())
                .and_then(format_diff)
        }
        FileType::NotStaged => {
            let mut diff_opts = DiffOptions::new();
            diff_opts.pathspec(path);
            repo.diff_index_to_workdir(None, Some(&mut diff_opts))
                .map_err(|e| e.to_string())
                .and_then(format_diff)
        }
    }
}

fn format_diff(diff: Diff) -> Result<String, String> {
    let mut diff_str = String::new();
    diff.print(git2::DiffFormat::Patch, |_, _, line| {
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
    })
    .map_err(|e| e.to_string())?;
    Ok(diff_str)
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

pub struct ParsedDiff {
    pub header: String,
    pub hunks: Vec<String>,
}

pub fn parse_diff_output(diff_output: &str) -> ParsedDiff {
    let mut header_lines = Vec::new();
    let mut hunks = Vec::new();
    let mut current_hunk = Vec::new();
    let mut in_hunk = false;

    for line in diff_output.lines() {
        if line.starts_with("diff --git")
            || line.starts_with("index")
            || line.starts_with("---")
            || line.starts_with("+++")
        {
            if in_hunk {
                hunks.push(current_hunk.join("\n"));
                current_hunk.clear();
                in_hunk = false;
            }
            header_lines.push(line.to_string());
        } else if line.starts_with("@@") {
            if in_hunk {
                hunks.push(current_hunk.join("\n"));
                current_hunk.clear();
            }
            in_hunk = true;
            current_hunk.push(line.to_string());
        } else if in_hunk {
            current_hunk.push(line.to_string());
        } else {
            // This case should ideally not be reached if the diff format is consistent,
            // but it's here for robustness.
            header_lines.push(line.to_string());
        }
    }

    if in_hunk {
        hunks.push(current_hunk.join("\n"));
    }

    ParsedDiff {
        header: header_lines.join("\n"),
        hunks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_diff_output() {
        let diff_output = r#"diff --git a/file.txt b/file.txt
index 1234567..abcdefg 100644
---
+++ b/file.txt
@@ -1,3 +1,4 @@
 line 1
-line 2
+line 2 modified
+line 3 new
 line 3
@@ -10,2 +11,2 @@
 line 10
-line 11 old
+line 11 new"#;

        let parsed_diff = parse_diff_output(diff_output);

        assert_eq!(parsed_diff.header, "diff --git a/file.txt b/file.txt\nindex 1234567..abcdefg 100644\n---
+++ b/file.txt");
        assert_eq!(parsed_diff.hunks.len(), 2);
        assert_eq!(parsed_diff.hunks[0], "@@ -1,3 +1,4 @@\n line 1\n-line 2\n+line 2 modified\n+line 3 new\n line 3");
        assert_eq!(parsed_diff.hunks[1], "@@ -10,2 +11,2 @@\n line 10\n-line 11 old\n+line 11 new");
    }
}

