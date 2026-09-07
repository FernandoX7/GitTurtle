//! Pure branch hierarchy construction for the virtualized navigation list.
//! Folder expansion belongs to the caller, so rebuilding cannot reopen a folder
//! the user collapsed. Search temporarily reveals all matching descendants.

use gitturtle_core::Branch;
use std::collections::{BTreeMap, HashSet};

/// At most sixteen folder levels precede a leaf. Deeper name components remain
/// together in the leaf label rather than being lost or recursively expanded.
const MAX_FOLDER_DEPTH: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Folder {
        key: String,
        label: String,
        depth: usize,
        count: usize,
        expanded: bool,
    },
    Branch {
        /// Index in the original, unfiltered branch slice supplied by the caller.
        index: usize,
        depth: usize,
    },
}

#[derive(Default)]
struct Node<'a> {
    folders: BTreeMap<&'a str, Node<'a>>,
    branches: Vec<(usize, &'a str)>,
    count: usize,
}

/// Build folders first, then branch leaves sorted by their displayed suffix.
/// Counts include all matching descendant branches, including collapsed ones.
/// Query matching is case-insensitive against the complete branch name.
pub fn branch_rows(
    branches: &[Branch],
    remote: bool,
    query: &str,
    expanded: &HashSet<String>,
) -> Vec<Row> {
    let query = query.trim().to_lowercase();
    let searching = !query.is_empty();
    let mut root = Node::default();
    for (index, branch) in branches.iter().enumerate() {
        if branch.remote != remote || (searching && !branch.name.to_lowercase().contains(&query)) {
            continue;
        }
        let parts: Vec<_> = name_parts(&branch.name).collect();
        let (leaf, folders) = parts.split_last().expect("splitn always yields a part");
        root.count += 1;
        let mut node = &mut root;
        for folder in folders {
            node = node.folders.entry(folder).or_default();
            node.count += 1;
        }
        node.branches.push((index, leaf));
    }

    let mut rows = Vec::new();
    append_rows(&mut root, prefix(remote), 0, searching, expanded, &mut rows);
    rows
}

/// Seed expansion once when a repository is opened. Do not merge this on every
/// rebuild, because doing so would undo the user's explicit collapse actions.
pub fn current_ancestors(branches: &[Branch]) -> HashSet<String> {
    let mut keys = HashSet::new();
    for branch in branches.iter().filter(|branch| branch.current) {
        let parts: Vec<_> = name_parts(&branch.name).collect();
        let mut key = prefix(branch.remote).to_owned();
        for (depth, folder) in parts[..parts.len().saturating_sub(1)].iter().enumerate() {
            if depth > 0 {
                key.push('/');
            }
            key.push_str(folder);
            keys.insert(key.clone());
        }
    }
    keys
}

/// Display the branch suffix corresponding to a `Row::Branch` depth. This also
/// preserves the full remaining suffix when a branch exceeds the depth budget.
pub fn branch_label(name: &str, depth: usize) -> &str {
    name.splitn(depth.min(MAX_FOLDER_DEPTH) + 1, '/')
        .nth(depth.min(MAX_FOLDER_DEPTH))
        .unwrap_or(name)
}

fn name_parts(name: &str) -> impl Iterator<Item = &str> {
    name.splitn(MAX_FOLDER_DEPTH + 1, '/')
}

fn prefix(remote: bool) -> &'static str {
    if remote { "remote:" } else { "local:" }
}

fn append_rows(
    node: &mut Node<'_>,
    parent_key: &str,
    depth: usize,
    searching: bool,
    expanded: &HashSet<String>,
    rows: &mut Vec<Row>,
) {
    for (label, child) in &mut node.folders {
        let key = if depth == 0 {
            format!("{parent_key}{label}")
        } else {
            format!("{parent_key}/{label}")
        };
        let is_expanded = searching || expanded.contains(&key);
        rows.push(Row::Folder {
            key: key.clone(),
            label: (*label).into(),
            depth,
            count: child.count,
            expanded: is_expanded,
        });
        if is_expanded {
            append_rows(child, &key, depth + 1, searching, expanded, rows);
        }
    }
    node.branches
        .sort_by(|(left_index, left), (right_index, right)| {
            left.cmp(right).then(left_index.cmp(right_index))
        });
    rows.extend(node.branches.iter().map(|(index, _)| Row::Branch {
        index: *index,
        depth,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(name: &str, remote: bool, current: bool) -> Branch {
        Branch {
            name: name.into(),
            oid: "1".repeat(40),
            remote,
            current,
        }
    }

    fn folder(key: &str, label: &str, depth: usize, count: usize, expanded: bool) -> Row {
        Row::Folder {
            key: key.into(),
            label: label.into(),
            depth,
            count,
            expanded,
        }
    }

    fn fixture() -> Vec<Branch> {
        vec![
            branch("feature/zeta", false, false),
            branch("feature/team/cache", false, true),
            branch("main", false, false),
            branch("feature/alpha", false, false),
            branch("release/v1", false, false),
            branch("origin/main", true, false),
        ]
    }

    #[test]
    fn folders_are_grouped_and_count_collapsed_descendants() {
        let branches = fixture();
        assert_eq!(
            branch_rows(&branches, false, "", &HashSet::new()),
            vec![
                folder("local:feature", "feature", 0, 3, false),
                folder("local:release", "release", 0, 1, false),
                Row::Branch { index: 2, depth: 0 },
            ]
        );
        let expanded = HashSet::from(["local:feature".into()]);
        assert_eq!(
            branch_rows(&branches, false, "", &expanded),
            vec![
                folder("local:feature", "feature", 0, 3, true),
                folder("local:feature/team", "team", 1, 1, false),
                Row::Branch { index: 3, depth: 1 },
                Row::Branch { index: 0, depth: 1 },
                folder("local:release", "release", 0, 1, false),
                Row::Branch { index: 2, depth: 0 },
            ]
        );
    }

    #[test]
    fn search_reveals_only_matching_paths_without_mutating_expansion() {
        let branches = fixture();
        let expanded = HashSet::new();
        assert_eq!(
            branch_rows(&branches, false, "  TEAM/CA  ", &expanded),
            vec![
                folder("local:feature", "feature", 0, 1, true),
                folder("local:feature/team", "team", 1, 1, true),
                Row::Branch { index: 1, depth: 2 },
            ]
        );
        assert!(expanded.is_empty());
        assert_eq!(
            branch_rows(&branches, false, " \t ", &expanded),
            branch_rows(&branches, false, "", &expanded)
        );
        assert!(branch_rows(&branches, false, "no-matching-branch", &expanded).is_empty());
    }

    #[test]
    fn current_ancestor_seed_does_not_override_later_manual_collapse() {
        let branches = fixture();
        let mut expanded = current_ancestors(&branches);
        assert_eq!(
            expanded,
            HashSet::from(["local:feature".into(), "local:feature/team".into()])
        );
        assert!(
            branch_rows(&branches, false, "", &expanded)
                .contains(&Row::Branch { index: 1, depth: 2 })
        );
        expanded.remove("local:feature/team");
        let rows = branch_rows(&branches, false, "", &expanded);
        assert!(
            !rows
                .iter()
                .any(|row| matches!(row, Row::Branch { index: 1, .. }))
        );
        assert!(rows.contains(&folder("local:feature/team", "team", 1, 1, false)));
    }

    #[test]
    fn local_and_remote_folder_keys_and_original_indices_are_independent() {
        let branches = vec![
            branch("origin/team/zeta", true, false),
            branch("origin/team/local", false, true),
            branch("origin/team/alpha", true, false),
            branch("upstream/main", true, false),
        ];
        let local_expanded = current_ancestors(&branches);
        assert_eq!(
            branch_rows(&branches, true, "", &local_expanded),
            vec![
                folder("remote:origin", "origin", 0, 2, false),
                folder("remote:upstream", "upstream", 0, 1, false),
            ]
        );
        let rows = branch_rows(&branches, true, "origin", &local_expanded);
        assert_eq!(
            rows,
            vec![
                folder("remote:origin", "origin", 0, 2, true),
                folder("remote:origin/team", "team", 1, 2, true),
                Row::Branch { index: 2, depth: 2 },
                Row::Branch { index: 0, depth: 2 },
            ]
        );
        for row in rows {
            if let Row::Branch { index, depth } = row {
                assert!(branches[index].remote);
                assert_eq!(
                    branch_label(&branches[index].name, depth),
                    if index == 2 { "alpha" } else { "zeta" }
                );
            }
        }
    }

    #[test]
    fn depth_budget_preserves_long_branch_suffix_and_current_identity() {
        let parts: Vec<_> = (0..40).map(|i| format!("part{i}")).collect();
        let name = parts.join("/");
        let branches = vec![branch(&name, false, true)];
        let expanded = current_ancestors(&branches);
        assert_eq!(expanded.len(), MAX_FOLDER_DEPTH);
        let rows = branch_rows(&branches, false, "", &expanded);
        assert_eq!(rows.len(), MAX_FOLDER_DEPTH + 1);
        assert_eq!(
            rows.last(),
            Some(&Row::Branch {
                index: 0,
                depth: MAX_FOLDER_DEPTH
            })
        );
        assert_eq!(
            branch_label(&name, MAX_FOLDER_DEPTH),
            parts[MAX_FOLDER_DEPTH..].join("/")
        );
        assert_eq!(
            branch_rows(&branches, false, "part39", &HashSet::new()),
            rows
        );
    }

    #[test]
    fn leaves_sort_by_display_name_and_empty_inputs_stay_empty() {
        let branches = vec![
            branch("zeta", false, false),
            branch("alpha", false, true),
            branch("beta", false, false),
        ];
        assert_eq!(
            branch_rows(&branches, false, "", &HashSet::new()),
            vec![
                Row::Branch { index: 1, depth: 0 },
                Row::Branch { index: 2, depth: 0 },
                Row::Branch { index: 0, depth: 0 },
            ]
        );
        assert!(current_ancestors(&branches).is_empty());
        assert!(branch_rows(&[], false, "", &HashSet::new()).is_empty());
        assert_eq!(branch_label("main", 0), "main");
    }
}
