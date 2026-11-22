//! The project list a user curates: known projects, named groups and the
//! nested order between them. This module owns the in-memory model and its
//! bounds only. Persistence belongs to `preferences`, and presentation to
//! `project_pane`; a folder on disk is never moved, created or deleted here.

use crate::preferences::{MAX_PROJECT_NAME_BYTES, validate_project_name};
use std::path::{Path, PathBuf};

/// Bounds keep one settings file small and one pane readable. Reaching a
/// limit refuses the new entry instead of discarding something the user made.
pub const MAX_GROUPS: usize = 64;
pub const MAX_PROJECTS: usize = 256;
/// Counted from a root group at depth one, so five levels can nest.
pub const MAX_DEPTH: usize = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectNode {
    Group(ProjectGroup),
    Project(PathBuf),
}

/// A user-named folder in the project list. `collapsed` is view state, and it
/// is saved with the group so the pane looks the same after a restart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectGroup {
    pub id: u32,
    pub name: String,
    pub collapsed: bool,
    pub nodes: Vec<ProjectNode>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectLibrary {
    pub nodes: Vec<ProjectNode>,
}

/// One visible line of the pane. Collapsed groups hide their descendants, so
/// a row list is shorter than the tree it comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryRow {
    Group {
        id: u32,
        name: String,
        depth: usize,
        collapsed: bool,
        projects: usize,
    },
    Project {
        path: PathBuf,
        depth: usize,
        parent: Option<u32>,
    },
}

/// A destination offered by a "Move to…" menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupEntry {
    pub id: u32,
    pub name: String,
    pub depth: usize,
}

impl ProjectLibrary {
    pub fn group_count(&self) -> usize {
        count_groups(&self.nodes)
    }

    pub fn project_count(&self) -> usize {
        count_projects(&self.nodes)
    }

    pub fn contains_project(&self, path: &Path) -> bool {
        contains_project(&self.nodes, path)
    }

    pub fn group(&self, id: u32) -> Option<&ProjectGroup> {
        find_group(&self.nodes, id)
    }

    /// Every group, in list order, with its indentation depth. A move menu and
    /// the accessibility labels both read this.
    pub fn groups(&self) -> Vec<GroupEntry> {
        let mut entries = Vec::new();
        collect_groups(&self.nodes, 0, &mut entries);
        entries
    }

    /// Add a project the user opened. The project joins the end of the top
    /// level, because the application cannot know which group it belongs to.
    /// A full list drops its oldest ungrouped project; grouped projects stay.
    pub fn remember(&mut self, path: &Path) -> bool {
        if !path.is_absolute() || self.contains_project(path) {
            return false;
        }
        while self.project_count() >= MAX_PROJECTS {
            let Some(index) = self
                .nodes
                .iter()
                .position(|node| matches!(node, ProjectNode::Project(_)))
            else {
                return false;
            };
            self.nodes.remove(index);
        }
        self.nodes.push(ProjectNode::Project(path.to_owned()));
        true
    }

    pub fn forget_project(&mut self, path: &Path) -> bool {
        detach_project(&mut self.nodes, path).is_some()
    }

    pub fn create_group(&mut self, parent: Option<u32>, name: &str) -> Result<u32, String> {
        let name = trimmed_group_name(name)?;
        if self.group_count() >= MAX_GROUPS {
            return Err(format!(
                "Up to {MAX_GROUPS} groups can be saved. Remove a group before adding another."
            ));
        }
        let depth = match parent {
            Some(id) => self
                .depth_of(id)
                .ok_or_else(|| "That group is no longer in the project list.".to_owned())?,
            None => 0,
        };
        if depth + 1 > MAX_DEPTH {
            return Err(format!("Groups can nest up to {MAX_DEPTH} levels deep."));
        }
        let id = self.next_group_id();
        let group = ProjectNode::Group(ProjectGroup {
            id,
            name,
            collapsed: false,
            nodes: Vec::new(),
        });
        match parent {
            Some(parent) => {
                let target = find_group_mut(&mut self.nodes, parent)
                    .ok_or_else(|| "That group is no longer in the project list.".to_owned())?;
                target.nodes.push(group);
            }
            None => self.nodes.push(group),
        }
        Ok(id)
    }

    pub fn rename_group(&mut self, id: u32, name: &str) -> Result<(), String> {
        let name = trimmed_group_name(name)?;
        let group = find_group_mut(&mut self.nodes, id)
            .ok_or_else(|| "That group is no longer in the project list.".to_owned())?;
        group.name = name;
        Ok(())
    }

    /// Remove the group only. Its projects and subgroups take its place, so a
    /// removal never hides a project the user still opens.
    pub fn remove_group(&mut self, id: u32) -> bool {
        remove_group(&mut self.nodes, id)
    }

    pub fn set_collapsed(&mut self, id: u32, collapsed: bool) -> bool {
        match find_group_mut(&mut self.nodes, id) {
            Some(group) if group.collapsed != collapsed => {
                group.collapsed = collapsed;
                true
            }
            _ => false,
        }
    }

    /// Move a known project into a group, or to the top level with `None`.
    pub fn move_project(&mut self, path: &Path, into: Option<u32>) -> Result<(), String> {
        if into.is_some_and(|id| self.depth_of(id).is_none()) {
            return Err("That group is no longer in the project list.".into());
        }
        let node = detach_project(&mut self.nodes, path)
            .ok_or_else(|| "That project is no longer in the project list.".to_owned())?;
        self.attach(node, into);
        Ok(())
    }

    /// Move a group, with everything inside it, into another group or to the
    /// top level. A group can never become its own descendant.
    pub fn move_group(&mut self, id: u32, into: Option<u32>) -> Result<(), String> {
        if into == Some(id) {
            return Err("Choose a different group.".into());
        }
        let target_depth = match into {
            Some(target) => {
                if self.group_contains(id, target) {
                    return Err("A group cannot move inside one of its own groups.".into());
                }
                self.depth_of(target)
                    .ok_or_else(|| "That group is no longer in the project list.".to_owned())?
            }
            None => 0,
        };
        let height = self
            .group(id)
            .map(subtree_height)
            .ok_or_else(|| "That group is no longer in the project list.".to_owned())?;
        if target_depth + height > MAX_DEPTH {
            return Err(format!("Groups can nest up to {MAX_DEPTH} levels deep."));
        }
        let node = detach_group(&mut self.nodes, id)
            .ok_or_else(|| "That group is no longer in the project list.".to_owned())?;
        self.attach(node, into);
        Ok(())
    }

    /// Reject a stored list the application cannot present faithfully. A
    /// caller refuses the file rather than dropping the user's groups when an
    /// unrelated settings or draft write rewrites it.
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = Vec::new();
        let mut paths = Vec::new();
        validate_nodes(&self.nodes, 0, &mut ids, &mut paths)?;
        if ids.len() > MAX_GROUPS {
            return Err(format!("Up to {MAX_GROUPS} groups can be saved."));
        }
        if paths.len() > MAX_PROJECTS {
            return Err(format!("Up to {MAX_PROJECTS} projects can be saved."));
        }
        ids.sort_unstable();
        let unique = ids.len();
        ids.dedup();
        if ids.len() != unique {
            return Err("Two groups share an identifier.".into());
        }
        paths.sort();
        let unique = paths.len();
        paths.dedup();
        if paths.len() != unique {
            return Err("A project appears twice in the project list.".into());
        }
        Ok(())
    }

    /// Rows in saved order, without presentation sorting. Fixtures inspect
    /// this to check what the file holds; the pane shows `sorted_rows`.
    #[cfg(test)]
    pub fn rows(&self) -> Vec<LibraryRow> {
        let mut rows = Vec::new();
        collect_rows(&self.nodes, 0, None, &mut rows);
        rows
    }

    /// The same rows with each level sorted: groups by name, then projects by
    /// the name the caller displays, both case-insensitively. Saved order
    /// breaks ties, so the list is stable while it grows.
    pub fn sorted_rows(&self, name_of: &dyn Fn(&Path) -> String) -> Vec<LibraryRow> {
        let mut rows = Vec::new();
        collect_sorted_rows(&self.nodes, 0, None, name_of, &mut rows);
        rows
    }

    /// Every group in the sorted presentation order, with its depth.
    pub fn sorted_groups(&self) -> Vec<GroupEntry> {
        let mut entries = Vec::new();
        collect_sorted_groups(&self.nodes, 0, &mut entries);
        entries
    }

    /// Every saved project path, in saved order.
    pub fn projects(&self) -> Vec<&Path> {
        let mut paths = Vec::new();
        collect_projects(&self.nodes, &mut paths);
        paths
    }

    /// `None` when the group is missing; `Some(None)` at the top level.
    pub fn parent_of_group(&self, id: u32) -> Option<Option<u32>> {
        parent_of(
            &self.nodes,
            None,
            &|node| matches!(node, ProjectNode::Group(group) if group.id == id),
        )
    }

    /// The group holding a project, or `None` at the top level or when the
    /// project is not in the list.
    pub fn parent_of_project(&self, path: &Path) -> Option<u32> {
        parent_of(
            &self.nodes,
            None,
            &|node| matches!(node, ProjectNode::Project(project) if project == path),
        )
        .flatten()
    }

    /// Whether `candidate` sits anywhere inside `ancestor`.
    pub fn group_contains(&self, ancestor: u32, candidate: u32) -> bool {
        self.group(ancestor)
            .is_some_and(|group| find_group(&group.nodes, candidate).is_some())
    }

    /// Levels a group occupies, counting itself; `None` when it is missing.
    pub fn group_height(&self, id: u32) -> Option<usize> {
        self.group(id).map(subtree_height)
    }

    /// Group names from the top level down to this group. Empty when missing.
    pub fn group_path(&self, id: u32) -> Vec<String> {
        let mut path = Vec::new();
        group_path(&self.nodes, id, &mut path);
        path
    }

    fn next_group_id(&self) -> u32 {
        self.groups()
            .iter()
            .map(|entry| entry.id)
            .max()
            .map_or(1, |id| id.saturating_add(1))
    }

    /// One-based: a top-level group has depth one.
    fn depth_of(&self, id: u32) -> Option<usize> {
        depth_of(&self.nodes, id, 1)
    }

    fn attach(&mut self, node: ProjectNode, into: Option<u32>) {
        match into.and_then(|id| find_group_mut(&mut self.nodes, id)) {
            Some(group) => group.nodes.push(node),
            None => self.nodes.push(node),
        }
    }
}

fn trimmed_group_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a name for this group.".into());
    }
    if name.len() > MAX_PROJECT_NAME_BYTES {
        return Err(format!(
            "Use a group name of at most {MAX_PROJECT_NAME_BYTES} bytes."
        ));
    }
    // Groups and projects share one text rule, so the pane can present either
    // label on a single line.
    validate_project_name(name).map(|()| name.to_owned())
}

fn count_groups(nodes: &[ProjectNode]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            ProjectNode::Group(group) => 1 + count_groups(&group.nodes),
            ProjectNode::Project(_) => 0,
        })
        .sum()
}

fn count_projects(nodes: &[ProjectNode]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            ProjectNode::Group(group) => count_projects(&group.nodes),
            ProjectNode::Project(_) => 1,
        })
        .sum()
}

fn contains_project(nodes: &[ProjectNode], path: &Path) -> bool {
    nodes.iter().any(|node| match node {
        ProjectNode::Group(group) => contains_project(&group.nodes, path),
        ProjectNode::Project(project) => project == path,
    })
}

fn find_group(nodes: &[ProjectNode], id: u32) -> Option<&ProjectGroup> {
    for node in nodes {
        if let ProjectNode::Group(group) = node {
            if group.id == id {
                return Some(group);
            }
            if let Some(found) = find_group(&group.nodes, id) {
                return Some(found);
            }
        }
    }
    None
}

fn find_group_mut(nodes: &mut [ProjectNode], id: u32) -> Option<&mut ProjectGroup> {
    for node in nodes {
        if let ProjectNode::Group(group) = node {
            if group.id == id {
                return Some(group);
            }
            if let Some(found) = find_group_mut(&mut group.nodes, id) {
                return Some(found);
            }
        }
    }
    None
}

fn collect_groups(nodes: &[ProjectNode], depth: usize, entries: &mut Vec<GroupEntry>) {
    for node in nodes {
        if let ProjectNode::Group(group) = node {
            entries.push(GroupEntry {
                id: group.id,
                name: group.name.clone(),
                depth,
            });
            collect_groups(&group.nodes, depth + 1, entries);
        }
    }
}

/// Groups come before projects at each level, like a folder listing. Saved
/// order decides the rest, so a new group or project joins the end of its kind.
#[cfg(test)]
fn collect_rows(
    nodes: &[ProjectNode],
    depth: usize,
    parent: Option<u32>,
    rows: &mut Vec<LibraryRow>,
) {
    for node in nodes {
        let ProjectNode::Group(group) = node else {
            continue;
        };
        rows.push(LibraryRow::Group {
            id: group.id,
            name: group.name.clone(),
            depth,
            collapsed: group.collapsed,
            projects: count_projects(&group.nodes),
        });
        if !group.collapsed {
            collect_rows(&group.nodes, depth + 1, Some(group.id), rows);
        }
    }
    for node in nodes {
        if let ProjectNode::Project(path) = node {
            rows.push(LibraryRow::Project {
                path: path.clone(),
                depth,
                parent,
            });
        }
    }
}

/// Groups first, then projects, each sorted by lowercase name with the saved
/// position as the tiebreak.
fn sorted_children<'a>(
    nodes: &'a [ProjectNode],
    name_of: &dyn Fn(&Path) -> String,
) -> (Vec<&'a ProjectGroup>, Vec<&'a PathBuf>) {
    let mut groups: Vec<(String, usize, &ProjectGroup)> = Vec::new();
    let mut projects: Vec<(String, usize, &PathBuf)> = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        match node {
            ProjectNode::Group(group) => groups.push((group.name.to_lowercase(), index, group)),
            ProjectNode::Project(path) => {
                projects.push((name_of(path).to_lowercase(), index, path))
            }
        }
    }
    groups.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    projects.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    (
        groups.into_iter().map(|(_, _, group)| group).collect(),
        projects.into_iter().map(|(_, _, path)| path).collect(),
    )
}

fn collect_sorted_rows(
    nodes: &[ProjectNode],
    depth: usize,
    parent: Option<u32>,
    name_of: &dyn Fn(&Path) -> String,
    rows: &mut Vec<LibraryRow>,
) {
    let (groups, projects) = sorted_children(nodes, name_of);
    for group in groups {
        rows.push(LibraryRow::Group {
            id: group.id,
            name: group.name.clone(),
            depth,
            collapsed: group.collapsed,
            projects: count_projects(&group.nodes),
        });
        if !group.collapsed {
            collect_sorted_rows(&group.nodes, depth + 1, Some(group.id), name_of, rows);
        }
    }
    for path in projects {
        rows.push(LibraryRow::Project {
            path: path.clone(),
            depth,
            parent,
        });
    }
}

fn collect_sorted_groups(nodes: &[ProjectNode], depth: usize, entries: &mut Vec<GroupEntry>) {
    let (groups, _) = sorted_children(nodes, &|_| String::new());
    for group in groups {
        entries.push(GroupEntry {
            id: group.id,
            name: group.name.clone(),
            depth,
        });
        collect_sorted_groups(&group.nodes, depth + 1, entries);
    }
}

fn collect_projects<'a>(nodes: &'a [ProjectNode], paths: &mut Vec<&'a Path>) {
    for node in nodes {
        match node {
            ProjectNode::Group(group) => collect_projects(&group.nodes, paths),
            ProjectNode::Project(path) => paths.push(path),
        }
    }
}

/// The parent group of the first node matching `is_target`, wrapped so a
/// missing node and a top-level node stay distinguishable.
fn parent_of(
    nodes: &[ProjectNode],
    parent: Option<u32>,
    is_target: &dyn Fn(&ProjectNode) -> bool,
) -> Option<Option<u32>> {
    for node in nodes {
        if is_target(node) {
            return Some(parent);
        }
        if let ProjectNode::Group(group) = node
            && let Some(found) = parent_of(&group.nodes, Some(group.id), is_target)
        {
            return Some(found);
        }
    }
    None
}

fn group_path(nodes: &[ProjectNode], id: u32, path: &mut Vec<String>) -> bool {
    for node in nodes {
        if let ProjectNode::Group(group) = node {
            path.push(group.name.clone());
            if group.id == id || group_path(&group.nodes, id, path) {
                return true;
            }
            path.pop();
        }
    }
    false
}

fn depth_of(nodes: &[ProjectNode], id: u32, depth: usize) -> Option<usize> {
    for node in nodes {
        if let ProjectNode::Group(group) = node {
            if group.id == id {
                return Some(depth);
            }
            if let Some(found) = depth_of(&group.nodes, id, depth + 1) {
                return Some(found);
            }
        }
    }
    None
}

/// Levels this group occupies, counting itself. An empty group measures one.
fn subtree_height(group: &ProjectGroup) -> usize {
    1 + group
        .nodes
        .iter()
        .filter_map(|node| match node {
            ProjectNode::Group(child) => Some(subtree_height(child)),
            ProjectNode::Project(_) => None,
        })
        .max()
        .unwrap_or(0)
}

fn detach_project(nodes: &mut Vec<ProjectNode>, path: &Path) -> Option<ProjectNode> {
    if let Some(index) = nodes
        .iter()
        .position(|node| matches!(node, ProjectNode::Project(project) if project == path))
    {
        return Some(nodes.remove(index));
    }
    for node in nodes {
        if let ProjectNode::Group(group) = node
            && let Some(found) = detach_project(&mut group.nodes, path)
        {
            return Some(found);
        }
    }
    None
}

fn detach_group(nodes: &mut Vec<ProjectNode>, id: u32) -> Option<ProjectNode> {
    if let Some(index) = nodes
        .iter()
        .position(|node| matches!(node, ProjectNode::Group(group) if group.id == id))
    {
        return Some(nodes.remove(index));
    }
    for node in nodes {
        if let ProjectNode::Group(group) = node
            && let Some(found) = detach_group(&mut group.nodes, id)
        {
            return Some(found);
        }
    }
    None
}

fn remove_group(nodes: &mut Vec<ProjectNode>, id: u32) -> bool {
    if let Some(index) = nodes
        .iter()
        .position(|node| matches!(node, ProjectNode::Group(group) if group.id == id))
    {
        let ProjectNode::Group(group) = nodes.remove(index) else {
            return false;
        };
        for (offset, child) in group.nodes.into_iter().enumerate() {
            nodes.insert(index + offset, child);
        }
        return true;
    }
    for node in nodes {
        if let ProjectNode::Group(group) = node
            && remove_group(&mut group.nodes, id)
        {
            return true;
        }
    }
    false
}

fn validate_nodes(
    nodes: &[ProjectNode],
    depth: usize,
    ids: &mut Vec<u32>,
    paths: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for node in nodes {
        match node {
            ProjectNode::Group(group) => {
                if depth + 1 > MAX_DEPTH {
                    return Err(format!("Groups can nest up to {MAX_DEPTH} levels deep."));
                }
                trimmed_group_name(&group.name)?;
                ids.push(group.id);
                validate_nodes(&group.nodes, depth + 1, ids, paths)?;
            }
            ProjectNode::Project(path) => {
                if !path.is_absolute() {
                    return Err("A saved project path must be absolute.".into());
                }
                paths.push(path.clone());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn project(name: &str) -> PathBuf {
        PathBuf::from("/projects").join(name)
    }

    fn library() -> ProjectLibrary {
        let mut library = ProjectLibrary::default();
        for name in ["alpha", "beta", "gamma"] {
            assert!(library.remember(&project(name)));
        }
        library
    }

    #[test]
    fn remembering_a_project_is_idempotent_and_needs_an_absolute_path() {
        let mut library = library();
        assert!(!library.remember(&project("alpha")));
        assert!(!library.remember(Path::new("relative/path")));
        assert_eq!(library.project_count(), 3);
        assert_eq!(library.rows().len(), 3);
    }

    #[test]
    fn a_collapsed_group_hides_its_projects_and_keeps_that_view() {
        let mut library = library();
        let work = library.create_group(None, "Work").unwrap();
        library.move_project(&project("alpha"), Some(work)).unwrap();
        library.move_project(&project("beta"), Some(work)).unwrap();
        assert_eq!(library.rows().len(), 4);

        assert!(library.set_collapsed(work, true));
        assert!(!library.set_collapsed(work, true));
        let rows = library.rows();
        assert_eq!(rows.len(), 2);
        assert!(matches!(
            rows.first(),
            Some(LibraryRow::Group {
                projects: 2,
                collapsed: true,
                ..
            })
        ));

        // The saved tree carries the collapsed flag, so a restart restores it.
        let restored = ProjectLibrary {
            nodes: library.nodes.clone(),
        };
        assert_eq!(restored.rows(), rows);
    }

    #[test]
    fn groups_nest_and_report_the_depth_of_each_row() {
        let mut library = library();
        let outer = library.create_group(None, "Work").unwrap();
        let inner = library.create_group(Some(outer), "Clients").unwrap();
        library
            .move_project(&project("alpha"), Some(inner))
            .unwrap();
        let rows = library.rows();
        assert!(matches!(rows[0], LibraryRow::Group { depth: 0, .. }));
        assert!(matches!(rows[1], LibraryRow::Group { depth: 1, .. }));
        assert!(matches!(
            &rows[2],
            LibraryRow::Project { depth: 2, parent: Some(id), .. } if *id == inner
        ));
        assert_eq!(
            library
                .groups()
                .iter()
                .map(|entry| (entry.id, entry.depth))
                .collect::<Vec<_>>(),
            vec![(outer, 0), (inner, 1)]
        );
    }

    #[test]
    fn removing_a_group_keeps_its_projects_and_subgroups_in_place() {
        let mut library = library();
        let outer = library.create_group(None, "Work").unwrap();
        let inner = library.create_group(Some(outer), "Clients").unwrap();
        library
            .move_project(&project("alpha"), Some(outer))
            .unwrap();
        library.move_project(&project("beta"), Some(inner)).unwrap();

        assert!(library.remove_group(outer));
        assert!(library.group(outer).is_none());
        assert_eq!(library.project_count(), 3);
        assert!(library.contains_project(&project("alpha")));
        assert_eq!(
            library
                .group(inner)
                .map(|group| group.nodes.len())
                .unwrap_or_default(),
            1
        );
        assert!(library.validate().is_ok());
    }

    #[test]
    fn a_group_cannot_move_inside_itself_or_past_the_depth_limit() {
        let mut library = ProjectLibrary::default();
        let mut parent = None;
        let mut ids = Vec::new();
        for level in 0..MAX_DEPTH {
            let id = library
                .create_group(parent, &format!("Level {level}"))
                .unwrap();
            ids.push(id);
            parent = Some(id);
        }
        assert!(library.create_group(parent, "Too deep").is_err());

        let branch = library.create_group(None, "Branch").unwrap();
        assert!(library.move_group(ids[0], Some(ids[1])).is_err());
        assert!(library.move_group(ids[0], Some(ids[0])).is_err());
        // The moved group carries five levels, so no group can accept it.
        assert!(library.move_group(ids[0], Some(branch)).is_err());
        assert!(library.move_group(branch, Some(ids[0])).is_ok());
        assert_eq!(library.group_count(), MAX_DEPTH + 1);
        assert!(library.validate().is_ok());
    }

    #[test]
    fn a_full_list_drops_an_ungrouped_project_and_keeps_grouped_ones() {
        let mut library = ProjectLibrary::default();
        let kept = library.create_group(None, "Keep").unwrap();
        for index in 0..MAX_PROJECTS {
            assert!(library.remember(&project(&format!("p{index}"))));
        }
        library.move_project(&project("p0"), Some(kept)).unwrap();
        assert!(library.remember(&project("newest")));
        assert_eq!(library.project_count(), MAX_PROJECTS);
        assert!(library.contains_project(&project("p0")));
        assert!(library.contains_project(&project("newest")));
        assert!(!library.contains_project(&project("p1")));
    }

    #[test]
    fn sorted_rows_order_each_level_by_name_and_keep_saved_order_for_ties() {
        let mut library = ProjectLibrary::default();
        for name in ["zeta", "Alpha", "beta"] {
            assert!(library.remember(&project(name)));
        }
        let work = library.create_group(None, "work").unwrap();
        let archive = library.create_group(None, "Archive").unwrap();
        let clients = library.create_group(Some(work), "Clients").unwrap();
        library.move_project(&project("zeta"), Some(work)).unwrap();
        library
            .move_project(&project("beta"), Some(clients))
            .unwrap();

        let names = |row: &LibraryRow| match row {
            LibraryRow::Group { name, depth, .. } => format!("{depth}:{name}/"),
            LibraryRow::Project { path, depth, .. } => {
                format!("{depth}:{}", path.file_name().unwrap().to_string_lossy())
            }
        };
        let name_of = |path: &Path| path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            library
                .sorted_rows(&name_of)
                .iter()
                .map(names)
                .collect::<Vec<_>>(),
            vec![
                "0:Archive/",
                "0:work/",
                "1:Clients/",
                "2:beta",
                "1:zeta",
                "0:Alpha"
            ]
        );
        // The saved order still puts the group the user made first on disk.
        assert!(matches!(
            library.rows().first(),
            Some(LibraryRow::Group { id, .. }) if *id == work
        ));
        assert_eq!(
            library
                .sorted_groups()
                .iter()
                .map(|entry| (entry.id, entry.depth))
                .collect::<Vec<_>>(),
            vec![(archive, 0), (work, 0), (clients, 1)]
        );
        assert_eq!(library.group_path(clients), vec!["work", "Clients"]);
        assert_eq!(library.parent_of_group(clients), Some(Some(work)));
        assert_eq!(library.parent_of_group(work), Some(None));
        assert_eq!(library.parent_of_group(99), None);
        assert_eq!(library.parent_of_project(&project("beta")), Some(clients));
        assert_eq!(library.parent_of_project(&project("Alpha")), None);
        assert!(library.group_contains(work, clients));
        assert!(!library.group_contains(clients, work));
        assert_eq!(library.group_height(work), Some(2));
        assert_eq!(library.projects().len(), 3);
    }

    #[test]
    fn saved_lists_with_unusable_names_paths_or_identifiers_are_refused() {
        let mut library = library();
        let group = library.create_group(None, "Work").unwrap();
        assert!(library.validate().is_ok());
        assert!(library.create_group(None, "  ").is_err());
        assert!(library.create_group(None, "Two\nlines").is_err());
        assert!(library.rename_group(group + 100, "Missing").is_err());

        let duplicate = ProjectLibrary {
            nodes: vec![
                ProjectNode::Project(project("alpha")),
                ProjectNode::Project(project("alpha")),
            ],
        };
        assert!(duplicate.validate().is_err());
        let relative = ProjectLibrary {
            nodes: vec![ProjectNode::Project(PathBuf::from("relative"))],
        };
        assert!(relative.validate().is_err());
        let shared = ProjectLibrary {
            nodes: vec![
                ProjectNode::Group(ProjectGroup {
                    id: 1,
                    name: "One".into(),
                    collapsed: false,
                    nodes: Vec::new(),
                }),
                ProjectNode::Group(ProjectGroup {
                    id: 1,
                    name: "Two".into(),
                    collapsed: false,
                    nodes: Vec::new(),
                }),
            ],
        };
        assert!(shared.validate().is_err());
    }
}
