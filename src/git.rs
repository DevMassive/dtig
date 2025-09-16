use git2::{Commit, Diff, DiffOptions, Error, ErrorCode, Oid, Repository, Status, StatusOptions};
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
                    let lines = content
                        .lines()
                        .map(|l| format!("+{}", l))
                        .collect::<Vec<_>>();
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
            // ヘッダ系はそのまま
            'H' | 'F' | 'B' => "".to_string(),
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

pub fn apply_patch_to_index(patch: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("git")
        .arg("apply")
        .arg("--cached")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn git apply command: {}", e))?;

    {
        // Scoped to ensure stdin is dropped and flushed before waiting
        let stdin = child.stdin.as_mut().ok_or("Failed to open stdin")?;
        stdin
            .write_all(patch.as_bytes())
            .map_err(|e| format!("Failed to write patch to stdin: {}", e))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to wait for git apply command: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git apply failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

pub struct ParsedDiff {
    pub header: String,
    pub hunks: Vec<String>,
}

pub fn create_patch_from_hunk(parsed_diff: &ParsedDiff, hunk_index: usize) -> Option<String> {
    if hunk_index < parsed_diff.hunks.len() {
        let mut patch = String::new();
        patch.push_str(&parsed_diff.header);
        patch.push_str("\n");
        patch.push_str(&parsed_diff.hunks[hunk_index]);
        patch.push_str("\n");
        Some(patch)
    } else {
        None
    }
}

pub fn parse_diff_output(diff_output: &str) -> ParsedDiff {
    let mut header = String::new();
    let mut hunks = Vec::new();
    let mut current_hunk = Vec::new();
    let mut in_hunk = false;

    let mut lines = diff_output.lines().peekable();

    // Collect header until the first hunk starts
    while let Some(line) = lines.peek() {
        if line.starts_with("@@") {
            break; // Hunk starts, stop collecting header
        }
        header.push_str(line);
        header.push('\n');
        lines.next(); // Consume the line
    }

    // Process hunks
    for line in lines {
        if line.starts_with("@@") {
            if in_hunk {
                hunks.push(current_hunk.join("\n"));
                current_hunk.clear();
            }
            in_hunk = true;
            current_hunk.push(line.to_string());
        } else if in_hunk {
            current_hunk.push(line.to_string());
        }
    }

    if in_hunk {
        hunks.push(current_hunk.join("\n"));
    }

    // Remove trailing newline from header if present
    if header.ends_with('\n') {
        header.pop();
    }

    ParsedDiff { header, hunks }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    fn setup_test_repo(test_name: &str) -> PathBuf {
        let repo_path = PathBuf::from(format!("./tmp_test_repo_{}", test_name));
        if repo_path.exists() {
            fs::remove_dir_all(&repo_path).unwrap();
        }
        fs::create_dir_all(&repo_path).unwrap();

        Command::new("git")
            .arg("init")
            .current_dir(&repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .arg("config")
            .arg("user.email")
            .arg("test@example.com")
            .current_dir(&repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .arg("config")
            .arg("user.name")
            .arg("Test User")
            .current_dir(&repo_path)
            .output()
            .unwrap();

        repo_path
    }

    fn teardown_test_repo(repo_path: &PathBuf) {
        if repo_path.exists() {
            fs::remove_dir_all(repo_path).unwrap();
        }
    }

    #[test]
    fn test_parse_diff_output() {
        let diff_output = r###"diff --git a/file.txt b/file.txt
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
+line 11 new"###;

        let parsed_diff = parse_diff_output(diff_output);

        assert_eq!(
            parsed_diff.header,
            "diff --git a/file.txt b/file.txt\nindex 1234567..abcdefg 100644\n---
+++ b/file.txt"
        );
        assert_eq!(parsed_diff.hunks.len(), 2);
        assert_eq!(
            parsed_diff.hunks[0],
            "@@ -1,3 +1,4 @@\n line 1\n-line 2\n+line 2 modified\n+line 3 new\n line 3"
        );
        assert_eq!(
            parsed_diff.hunks[1],
            "@@ -10,2 +11,2 @@\n line 10\n-line 11 old\n+line 11 new"
        );
    }

    #[test]
    fn test_create_patch_from_hunk() {
        let diff_output = r###"diff --git a/file.txt b/file.txt
index 1234567..abcdefg 100644
--- a/file.txt
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
+line 11 new"###;

        let parsed_diff = parse_diff_output(diff_output);

        // Test with the first hunk (index 0)
        let patch_hunk_0 = create_patch_from_hunk(&parsed_diff, 0).unwrap();
        let expected_patch_hunk_0 = r###"diff --git a/file.txt b/file.txt
index 1234567..abcdefg 100644
--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,4 @@
 line 1
-line 2
+line 2 modified
+line 3 new
 line 3"###;
        assert_eq!(patch_hunk_0, expected_patch_hunk_0);

        // Test with the second hunk (index 1)
        let patch_hunk_1 = create_patch_from_hunk(&parsed_diff, 1).unwrap();
        let expected_patch_hunk_1 = r###"diff --git a/file.txt b/file.txt
index 1234567..abcdefg 100644
--- a/file.txt
+++ b/file.txt
@@ -10,2 +11,2 @@
 line 10
-line 11 old
+line 11 new"###;
        assert_eq!(patch_hunk_1, expected_patch_hunk_1);

        // Test with an out-of-bounds index
        let patch_out_of_bounds = create_patch_from_hunk(&parsed_diff, 2);
        assert!(patch_out_of_bounds.is_none());
    }

    #[test]
    fn test_apply_patch_to_index() {
        let repo_path = setup_test_repo("apply_patch_to_index");
        let repo = Repository::open(&repo_path).unwrap();

        // 1. Create a file and commit it
        let file_path = repo_path.join("test_file.txt");
        fs::write(
            &file_path,
            "line 1
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
",
        )
        .unwrap();
        stage(&repo, "test_file.txt").unwrap();
        commit(&repo, "Initial commit").unwrap();

        // 2. Modify the file to create a diff with multiple hunks
        fs::write(
            &file_path,
            "line 1 modified
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
line 15 modified
",
        )
        .unwrap();

        // 3. Get the diff from workdir to index
        let diff_output = get_diff(&repo, "test_file.txt", FileType::NotStaged).unwrap();
        println!("Diff Output:\n{}", diff_output);
        let parsed_diff = parse_diff_output(&diff_output);
        println!("Number of hunks: {}", parsed_diff.hunks.len());

        // Ensure there are at least two hunks for testing
        assert!(
            parsed_diff.hunks.len() >= 2,
            "Expected at least two hunks for testing apply_patch_to_index"
        );

        // 4. Create a patch for the first hunk
        let patch_hunk_0 = create_patch_from_hunk(&parsed_diff, 0).unwrap();
        println!("Patch Hunk 0:\n```{}```", patch_hunk_0);

        // 5. Apply the patch to the index
        apply_patch_to_index(&patch_hunk_0).unwrap();

        // 6. Verify the index status
        let status_files = get_status(&repo);
        assert!(status_files.staged.contains(&"test_file.txt".to_string()));
        // The workdir still has the original changes, so it should still be not_staged
        assert!(
            status_files
                .not_staged
                .contains(&"test_file.txt".to_string())
        );

        // Verify the content of the staged file (index)
        let head_tree = repo.head().unwrap().peel_to_tree().unwrap();
        let diff_index_to_head = repo
            .diff_tree_to_index(Some(&head_tree), None, None)
            .unwrap();
        let mut staged_diff_str = String::new();
        diff_index_to_head
            .print(git2::DiffFormat::Patch, |_, _, line| {
                let prefix = match line.origin() {
                    '+' | '-' | '=' => line.origin().to_string(),
                    // ヘッダ系はそのまま
                    'H' | 'F' | 'B' => "".to_string(),
                    _ => " ".to_string(),
                };
                staged_diff_str.push_str(&format!(
                    "{}{}",
                    prefix,
                    String::from_utf8_lossy(line.content())
                ));
                true
            })
            .unwrap();

        // The staged_diff_str should contain only the first hunk's changes
        // Note: The index hash will be different, so we only compare the hunk part
        assert!(staged_diff_str.contains(&parsed_diff.hunks[0]));

        // Clean up
        teardown_test_repo(&repo_path);
    }
}
