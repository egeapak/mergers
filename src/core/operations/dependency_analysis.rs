//! PR dependency analysis for merge workflows.
//!
//! This module analyzes file changes across pull requests to determine
//! dependencies between them. PRs are categorized as:
//!
//! - **Independent**: No files edited in common with preceding PRs
//! - **Partially Dependent**: Common files, but no overlapping line ranges
//! - **Dependent**: Common files with overlapping edited line ranges
//!
//! The analysis builds a Directed Acyclic Graph (DAG) representing the
//! dependency relationships, enabling validation such as detecting when
//! a selected PR depends on an unselected one.

use std::collections::{HashMap, HashSet};

use rayon::prelude::*;
use roaring::RoaringBitmap;
use serde::{Deserialize, Serialize};

/// A range of lines in a file.
///
/// Represents a contiguous range of lines that were modified.
/// Both `start` and `end` are inclusive (1-indexed, matching git output).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRange {
    /// Starting line number (1-indexed, inclusive).
    pub start: u32,
    /// Ending line number (1-indexed, inclusive).
    pub end: u32,
}

impl LineRange {
    /// Creates a new line range.
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// Creates a line range for a single line.
    pub fn single(line: u32) -> Self {
        Self {
            start: line,
            end: line,
        }
    }

    /// Checks if this range overlaps with another.
    ///
    /// Two ranges overlap if they share at least one line number.
    pub fn overlaps(&self, other: &LineRange) -> bool {
        self.start <= other.end && other.start <= self.end
    }

    /// Returns the number of lines in this range.
    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start) + 1
    }

    /// Returns true if the range is empty (which shouldn't happen in practice).
    pub fn is_empty(&self) -> bool {
        self.end < self.start
    }
}

/// The type of change made to a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    /// File was added.
    Add,
    /// File was modified.
    Modify,
    /// File was deleted.
    Delete,
    /// File was renamed.
    Rename,
    /// File was copied.
    Copy,
}

impl ChangeType {
    /// Parses a change type from git's single-letter status code.
    pub fn from_git_status(status: &str) -> Option<Self> {
        match status.chars().next() {
            Some('A') => Some(ChangeType::Add),
            Some('M') => Some(ChangeType::Modify),
            Some('D') => Some(ChangeType::Delete),
            Some('R') => Some(ChangeType::Rename),
            Some('C') => Some(ChangeType::Copy),
            _ => None,
        }
    }
}

/// A file change within a commit, including affected line ranges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    /// Path to the file (relative to repository root).
    pub path: String,
    /// Original path if the file was renamed.
    pub original_path: Option<String>,
    /// Type of change.
    pub change_type: ChangeType,
    /// Line ranges that were modified in this file.
    /// Empty for additions/deletions of entire files.
    pub line_ranges: Vec<LineRange>,
}

impl FileChange {
    /// Creates a new file change.
    pub fn new(path: String, change_type: ChangeType) -> Self {
        Self {
            path,
            original_path: None,
            change_type,
            line_ranges: Vec::new(),
        }
    }

    /// Creates a file change with line ranges.
    pub fn with_ranges(path: String, change_type: ChangeType, line_ranges: Vec<LineRange>) -> Self {
        Self {
            path,
            original_path: None,
            change_type,
            line_ranges,
        }
    }

    /// Checks if this file change overlaps with another on the same file.
    ///
    /// Returns true if any line ranges overlap.
    pub fn has_overlapping_lines(&self, other: &FileChange) -> bool {
        // Must be the same file (considering renames)
        if !self.same_file(other) {
            return false;
        }

        // Check for line range overlaps
        for range1 in &self.line_ranges {
            for range2 in &other.line_ranges {
                if range1.overlaps(range2) {
                    return true;
                }
            }
        }

        false
    }

    /// Checks if this change and another affect the same file.
    fn same_file(&self, other: &FileChange) -> bool {
        self.path == other.path
            || self.original_path.as_ref() == Some(&other.path)
            || other.original_path.as_ref() == Some(&self.path)
    }

    /// Gets the overlapping line ranges between this change and another.
    pub fn get_overlapping_ranges(&self, other: &FileChange) -> Vec<LineRange> {
        if !self.same_file(other) {
            return Vec::new();
        }

        let mut overlaps = Vec::new();
        for range1 in &self.line_ranges {
            for range2 in &other.line_ranges {
                if range1.overlaps(range2) {
                    // Return the intersection
                    let start = range1.start.max(range2.start);
                    let end = range1.end.min(range2.end);
                    overlaps.push(LineRange::new(start, end));
                }
            }
        }
        overlaps
    }
}

/// The category of dependency between two PRs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum DependencyCategory {
    /// No files in common between the PRs.
    Independent,

    /// Some files are edited by both PRs, but the edited lines don't overlap.
    PartiallyDependent {
        /// Files that are modified by both PRs.
        shared_files: Vec<String>,
    },

    /// Files are edited by both PRs and the edited line ranges overlap.
    Dependent {
        /// Files that are modified by both PRs.
        shared_files: Vec<String>,
        /// Files with overlapping line ranges and the specific overlapping ranges.
        overlapping_files: Vec<OverlappingFile>,
    },
}

impl DependencyCategory {
    /// Returns true if this is an independent (no dependency) category.
    pub fn is_independent(&self) -> bool {
        matches!(self, DependencyCategory::Independent)
    }

    /// Returns the shared files if any.
    pub fn shared_files(&self) -> &[String] {
        match self {
            DependencyCategory::Independent => &[],
            DependencyCategory::PartiallyDependent { shared_files } => shared_files,
            DependencyCategory::Dependent { shared_files, .. } => shared_files,
        }
    }
}

/// Information about a file with overlapping line ranges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlappingFile {
    /// Path to the file.
    pub path: String,
    /// The overlapping line ranges.
    pub overlapping_ranges: Vec<LineRange>,
}

/// A dependency relationship from one PR to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PRDependency {
    /// The PR that has the dependency (the "dependent" PR).
    pub from_pr_id: i32,
    /// The PR that is depended upon (the "dependency").
    pub to_pr_id: i32,
    /// The category/strength of the dependency.
    pub category: DependencyCategory,
}

/// A node in the PR dependency graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PRDependencyNode {
    /// The PR ID.
    pub pr_id: i32,
    /// The PR title for display.
    pub pr_title: String,
    /// Whether this PR is selected for merging.
    pub is_selected: bool,
    /// PRs that this PR depends on (edges going out).
    pub dependencies: Vec<PRDependency>,
    /// PRs that depend on this PR (edges coming in).
    pub dependents: Vec<i32>,
}

impl PRDependencyNode {
    /// Creates a new dependency node.
    pub fn new(pr_id: i32, pr_title: String, is_selected: bool) -> Self {
        Self {
            pr_id,
            pr_title,
            is_selected,
            dependencies: Vec::new(),
            dependents: Vec::new(),
        }
    }

    /// Returns true if this PR has any dependencies (is not fully independent).
    pub fn has_dependencies(&self) -> bool {
        self.dependencies
            .iter()
            .any(|d| !d.category.is_independent())
    }

    /// Returns the count of dependencies by category.
    pub fn dependency_counts(&self) -> DependencyCounts {
        let mut counts = DependencyCounts::default();
        for dep in &self.dependencies {
            match &dep.category {
                DependencyCategory::Independent => counts.independent += 1,
                DependencyCategory::PartiallyDependent { .. } => counts.partial += 1,
                DependencyCategory::Dependent { .. } => counts.dependent += 1,
            }
        }
        counts
    }
}

/// Counts of dependencies by category.
#[derive(Debug, Default, Clone)]
pub struct DependencyCounts {
    /// Number of independent relationships.
    pub independent: usize,
    /// Number of partially dependent relationships.
    pub partial: usize,
    /// Number of fully dependent relationships.
    pub dependent: usize,
}

/// The complete dependency graph for a set of PRs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PRDependencyGraph {
    /// Nodes in the graph, keyed by PR ID.
    pub nodes: HashMap<i32, PRDependencyNode>,
    /// PRs in topological order (dependencies before dependents).
    pub topological_order: Vec<i32>,
}

impl PRDependencyGraph {
    /// Creates a new empty graph.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            topological_order: Vec::new(),
        }
    }

    /// Adds a node to the graph.
    pub fn add_node(&mut self, node: PRDependencyNode) {
        self.nodes.insert(node.pr_id, node);
    }

    /// Gets a node by PR ID.
    pub fn get_node(&self, pr_id: i32) -> Option<&PRDependencyNode> {
        self.nodes.get(&pr_id)
    }

    /// Gets a mutable node by PR ID.
    pub fn get_node_mut(&mut self, pr_id: i32) -> Option<&mut PRDependencyNode> {
        self.nodes.get_mut(&pr_id)
    }

    /// Computes the topological order of nodes.
    ///
    /// This ensures dependencies come before dependents in the order.
    pub fn compute_topological_order(&mut self) {
        // Use Kahn's algorithm
        let mut in_degree: HashMap<i32, usize> = HashMap::new();
        let mut queue: Vec<i32> = Vec::new();

        // Initialize in-degrees
        for (pr_id, node) in &self.nodes {
            let degree = node
                .dependencies
                .iter()
                .filter(|d| !d.category.is_independent())
                .count();
            in_degree.insert(*pr_id, degree);
            if degree == 0 {
                queue.push(*pr_id);
            }
        }

        // Sort initial queue for deterministic output
        queue.sort();

        let mut result = Vec::new();
        while let Some(pr_id) = queue.pop() {
            result.push(pr_id);

            if let Some(node) = self.nodes.get(&pr_id) {
                for dependent_id in &node.dependents {
                    if let Some(degree) = in_degree.get_mut(dependent_id) {
                        *degree = degree.saturating_sub(1);
                        if *degree == 0 {
                            queue.push(*dependent_id);
                            queue.sort(); // Keep sorted for determinism
                        }
                    }
                }
            }
        }

        self.topological_order = result;
    }

    /// Returns summary statistics for the graph.
    pub fn summary(&self) -> GraphSummary {
        let mut selected_prs = 0;
        let mut independent_relationships = 0;
        let mut partial_relationships = 0;
        let mut dependent_relationships = 0;

        for node in self.nodes.values() {
            if node.is_selected {
                selected_prs += 1;
            }

            let counts = node.dependency_counts();
            independent_relationships += counts.independent;
            partial_relationships += counts.partial;
            dependent_relationships += counts.dependent;
        }

        GraphSummary {
            total_prs: self.nodes.len(),
            selected_prs,
            independent_relationships,
            partial_relationships,
            dependent_relationships,
        }
    }
}

impl Default for PRDependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary statistics for a dependency graph.
#[derive(Debug, Default, Clone)]
pub struct GraphSummary {
    /// Total number of PRs in the graph.
    pub total_prs: usize,
    /// Number of selected PRs.
    pub selected_prs: usize,
    /// Number of independent relationships.
    pub independent_relationships: usize,
    /// Number of partially dependent relationships.
    pub partial_relationships: usize,
    /// Number of fully dependent relationships.
    pub dependent_relationships: usize,
}

/// A warning generated during dependency analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum DependencyWarning {
    /// A selected PR depends on an unselected PR.
    UnselectedDependency {
        /// The selected PR that has the dependency.
        selected_pr_id: i32,
        /// Title of the selected PR.
        selected_pr_title: String,
        /// The unselected PR that is depended upon.
        unselected_pr_id: i32,
        /// Title of the unselected PR.
        unselected_pr_title: String,
        /// The category of dependency.
        category: DependencyCategory,
    },

    /// A circular dependency was detected (should be rare for PRs).
    CircularDependency {
        /// The PR IDs forming the cycle.
        cycle: Vec<i32>,
    },
}

impl DependencyWarning {
    /// Returns a human-readable message for this warning.
    pub fn message(&self) -> String {
        match self {
            DependencyWarning::UnselectedDependency {
                selected_pr_id,
                selected_pr_title,
                unselected_pr_id,
                unselected_pr_title,
                category,
            } => {
                let category_str = match category {
                    DependencyCategory::Independent => "independent of",
                    DependencyCategory::PartiallyDependent { .. } => "partially depends on",
                    DependencyCategory::Dependent { .. } => "depends on",
                };
                format!(
                    "PR #{} ({}) {} unselected PR #{} ({})",
                    selected_pr_id,
                    truncate_title(selected_pr_title, 30),
                    category_str,
                    unselected_pr_id,
                    truncate_title(unselected_pr_title, 30),
                )
            }
            DependencyWarning::CircularDependency { cycle } => {
                format!("Circular dependency detected: {:?}", cycle)
            }
        }
    }

    /// Returns true if this is a critical warning (Dependent on unselected).
    pub fn is_critical(&self) -> bool {
        matches!(
            self,
            DependencyWarning::UnselectedDependency {
                category: DependencyCategory::Dependent { .. },
                ..
            } | DependencyWarning::CircularDependency { .. }
        )
    }
}

/// Truncates a title to a maximum length, adding ellipsis if needed.
fn truncate_title(title: &str, max_len: usize) -> String {
    if title.len() <= max_len {
        title.to_string()
    } else {
        format!("{}...", &title[..max_len.saturating_sub(3)])
    }
}

/// The result of a dependency analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyAnalysisResult {
    /// The dependency graph.
    pub graph: PRDependencyGraph,
    /// Warnings generated during analysis.
    pub warnings: Vec<DependencyWarning>,
}

impl DependencyAnalysisResult {
    /// Returns true if there are any critical warnings.
    pub fn has_critical_warnings(&self) -> bool {
        self.warnings.iter().any(|w| w.is_critical())
    }

    /// Returns the count of warnings by severity.
    pub fn warning_counts(&self) -> (usize, usize) {
        let critical = self.warnings.iter().filter(|w| w.is_critical()).count();
        let non_critical = self.warnings.len() - critical;
        (critical, non_critical)
    }
}

/// Configuration for dependency analysis.
#[derive(Debug, Clone)]
pub struct DependencyAnalysisConfig {
    /// Whether to include partially-dependent relationships in warnings.
    pub warn_on_partial: bool,
    /// Whether to fail on any unselected dependencies.
    pub fail_on_unselected: bool,
}

impl Default for DependencyAnalysisConfig {
    fn default() -> Self {
        Self {
            warn_on_partial: true,
            fail_on_unselected: false,
        }
    }
}

// ==================== Bitmap Index for Optimized Analysis ====================

/// Pre-computed bitmap index for fast dependency analysis.
///
/// Uses roaring bitmaps to enable O(1) file overlap detection and
/// fast line range intersection checks. This significantly speeds up
/// analysis for large PR sets (100+ PRs).
#[derive(Debug)]
pub struct PRBitmapIndex {
    /// Map file path -> unique integer ID for bitmap indexing
    file_dict: HashMap<String, u32>,
    /// Reverse map: file ID -> file path
    file_dict_reverse: HashMap<u32, String>,
    /// Map PR ID -> bitmap of file IDs it touches
    pr_file_bitmaps: HashMap<i32, RoaringBitmap>,
    /// Map (PR ID, file ID) -> bitmap of line numbers touched
    pr_line_bitmaps: HashMap<(i32, u32), RoaringBitmap>,
}

impl PRBitmapIndex {
    /// Builds a bitmap index from PR file changes.
    ///
    /// This is a three-pass algorithm:
    /// 1. Build file path -> integer dictionary
    /// 2. Build file bitmaps per PR (parallelized)
    /// 3. Build line bitmaps per (PR, file) (parallelized)
    pub fn build(pr_changes: &HashMap<i32, Vec<FileChange>>) -> Self {
        // Pass 1: Build file dictionary (sequential - needs unique IDs)
        let mut file_dict = HashMap::new();
        let mut file_dict_reverse = HashMap::new();
        let mut next_id = 0u32;

        for changes in pr_changes.values() {
            for change in changes {
                if !file_dict.contains_key(&change.path) {
                    file_dict.insert(change.path.clone(), next_id);
                    file_dict_reverse.insert(next_id, change.path.clone());
                    next_id += 1;
                }
                // Also handle original_path for renames
                if let Some(ref orig) = change.original_path
                    && !file_dict.contains_key(orig)
                {
                    file_dict.insert(orig.clone(), next_id);
                    file_dict_reverse.insert(next_id, orig.clone());
                    next_id += 1;
                }
            }
        }

        // Pass 2: Build file bitmaps per PR (parallel)
        let pr_file_bitmaps: HashMap<i32, RoaringBitmap> = pr_changes
            .par_iter()
            .map(|(pr_id, changes)| {
                let mut bitmap = RoaringBitmap::new();
                for change in changes {
                    if let Some(&file_id) = file_dict.get(&change.path) {
                        bitmap.insert(file_id);
                    }
                    if let Some(ref orig) = change.original_path
                        && let Some(&file_id) = file_dict.get(orig)
                    {
                        bitmap.insert(file_id);
                    }
                }
                (*pr_id, bitmap)
            })
            .collect();

        // Pass 3: Build line bitmaps per (PR, file) (parallel)
        // We need to collect into a Vec first to avoid borrow issues, then convert to HashMap
        let file_dict_ref = &file_dict;
        let pr_line_bitmaps: HashMap<(i32, u32), RoaringBitmap> = pr_changes
            .par_iter()
            .flat_map_iter(|(pr_id, changes)| {
                let pr_id = *pr_id;
                changes.iter().filter_map(move |change| {
                    let file_id = file_dict_ref.get(&change.path)?;
                    let mut bitmap = RoaringBitmap::new();
                    for range in &change.line_ranges {
                        // Insert all line numbers in the range
                        for line in range.start..=range.end {
                            bitmap.insert(line);
                        }
                    }
                    // Only store if there are line ranges
                    if bitmap.is_empty() {
                        None
                    } else {
                        Some(((pr_id, *file_id), bitmap))
                    }
                })
            })
            .collect();

        Self {
            file_dict,
            file_dict_reverse,
            pr_file_bitmaps,
            pr_line_bitmaps,
        }
    }

    /// Returns the file bitmap for a PR, if it exists.
    pub fn get_file_bitmap(&self, pr_id: i32) -> Option<&RoaringBitmap> {
        self.pr_file_bitmaps.get(&pr_id)
    }

    /// Returns the line bitmap for a PR and file, if it exists.
    pub fn get_line_bitmap(&self, pr_id: i32, file_id: u32) -> Option<&RoaringBitmap> {
        self.pr_line_bitmaps.get(&(pr_id, file_id))
    }

    /// Returns the file path for a file ID.
    pub fn get_file_path(&self, file_id: u32) -> Option<&String> {
        self.file_dict_reverse.get(&file_id)
    }

    /// Returns the file ID for a file path.
    pub fn get_file_id(&self, path: &str) -> Option<u32> {
        self.file_dict.get(path).copied()
    }
}

/// Converts a roaring bitmap of line numbers back to a vector of LineRange.
///
/// Consecutive line numbers are merged into ranges.
fn bitmap_to_ranges(bitmap: &RoaringBitmap) -> Vec<LineRange> {
    let mut ranges = Vec::new();
    let mut iter = bitmap.iter().peekable();

    while let Some(start) = iter.next() {
        let mut end = start;
        while let Some(&next) = iter.peek() {
            if next == end + 1 {
                end = next;
                iter.next();
            } else {
                break;
            }
        }
        ranges.push(LineRange::new(start, end));
    }
    ranges
}

// ==================== Dependency Analyzer ====================

/// Analyzes dependencies between pull requests based on file changes.
///
/// The analyzer compares each PR against all preceding PRs (in chronological order)
/// to determine if they modify the same files and whether the line ranges overlap.
pub struct DependencyAnalyzer {
    config: DependencyAnalysisConfig,
}

impl DependencyAnalyzer {
    /// Creates a new analyzer with default configuration.
    pub fn new() -> Self {
        Self {
            config: DependencyAnalysisConfig::default(),
        }
    }

    /// Creates a new analyzer with custom configuration.
    pub fn with_config(config: DependencyAnalysisConfig) -> Self {
        Self { config }
    }

    /// Analyzes dependencies between PRs based on their file changes.
    ///
    /// # Arguments
    ///
    /// * `prs` - List of PRs with their metadata, in chronological order
    /// * `pr_changes` - Map from PR ID to its file changes
    ///
    /// # Returns
    ///
    /// A `DependencyAnalysisResult` containing the dependency graph and any warnings.
    pub fn analyze(
        &self,
        prs: &[PRInfo],
        pr_changes: &HashMap<i32, Vec<FileChange>>,
    ) -> DependencyAnalysisResult {
        let mut graph = PRDependencyGraph::new();
        let mut warnings = Vec::new();

        // Build nodes for all PRs
        for pr in prs {
            let node = PRDependencyNode::new(pr.id, pr.title.clone(), pr.is_selected);
            graph.add_node(node);
        }

        // Compare each PR against all preceding PRs
        for (idx, pr) in prs.iter().enumerate() {
            let current_changes = pr_changes.get(&pr.id);

            for prev_pr in prs.iter().take(idx) {
                let prev_changes = pr_changes.get(&prev_pr.id);

                let category = Self::categorize_dependency(current_changes, prev_changes);

                // Only record non-independent dependencies
                if !category.is_independent() {
                    let dependency = PRDependency {
                        from_pr_id: pr.id,
                        to_pr_id: prev_pr.id,
                        category: category.clone(),
                    };

                    // Add dependency to current node
                    if let Some(node) = graph.get_node_mut(pr.id) {
                        node.dependencies.push(dependency);
                    }

                    // Add as dependent to previous node
                    if let Some(prev_node) = graph.get_node_mut(prev_pr.id) {
                        prev_node.dependents.push(pr.id);
                    }

                    // Check for unselected dependency warning
                    if pr.is_selected && !prev_pr.is_selected {
                        let should_warn = match &category {
                            DependencyCategory::Dependent { .. } => true,
                            DependencyCategory::PartiallyDependent { .. } => {
                                self.config.warn_on_partial
                            }
                            DependencyCategory::Independent => false,
                        };

                        if should_warn {
                            warnings.push(DependencyWarning::UnselectedDependency {
                                selected_pr_id: pr.id,
                                selected_pr_title: pr.title.clone(),
                                unselected_pr_id: prev_pr.id,
                                unselected_pr_title: prev_pr.title.clone(),
                                category,
                            });
                        }
                    }
                }
            }
        }

        // Compute topological order
        graph.compute_topological_order();

        DependencyAnalysisResult { graph, warnings }
    }

    /// Analyzes dependencies between PRs using parallel processing.
    ///
    /// Uses rayon for parallel pairwise comparison, which can significantly
    /// speed up analysis for large PR sets. The O(n^2) comparisons are
    /// distributed across available CPU cores.
    ///
    /// # Arguments
    ///
    /// * `prs` - List of PRs with their metadata, in chronological order
    /// * `pr_changes` - Map from PR ID to its file changes
    ///
    /// # Returns
    ///
    /// A `DependencyAnalysisResult` containing the dependency graph and any warnings.
    pub fn analyze_parallel(
        &self,
        prs: &[PRInfo],
        pr_changes: &HashMap<i32, Vec<FileChange>>,
    ) -> DependencyAnalysisResult {
        // Build bitmap index for fast comparison (parallelized internally)
        let index = PRBitmapIndex::build(pr_changes);

        // Build nodes for all PRs (sequential - fast)
        let mut graph = PRDependencyGraph::new();
        for pr in prs {
            let node = PRDependencyNode::new(pr.id, pr.title.clone(), pr.is_selected);
            graph.add_node(node);
        }

        // Generate all pairs (i, j) where j < i for parallel processing
        let pairs: Vec<(usize, usize)> = (0..prs.len())
            .flat_map(|i| (0..i).map(move |j| (i, j)))
            .collect();

        // Parallel pairwise comparison using bitmap optimization
        let dependencies: Vec<(i32, i32, DependencyCategory)> = pairs
            .par_iter()
            .filter_map(|&(i, j)| {
                let current_pr = &prs[i];
                let prev_pr = &prs[j];

                // Fast file overlap check via bitmap AND
                let bitmap1 = index.get_file_bitmap(current_pr.id)?;
                let bitmap2 = index.get_file_bitmap(prev_pr.id)?;
                let file_overlap = bitmap1 & bitmap2;

                if file_overlap.is_empty() {
                    return None; // Independent - no shared files
                }

                // Get shared file paths from the overlap bitmap
                let shared_files: Vec<String> = file_overlap
                    .iter()
                    .filter_map(|file_id| index.get_file_path(file_id).cloned())
                    .collect();

                // Check line overlaps for each shared file using line bitmaps
                let mut overlapping_files = Vec::new();
                for file_id in file_overlap.iter() {
                    let lines1 = index.get_line_bitmap(current_pr.id, file_id);
                    let lines2 = index.get_line_bitmap(prev_pr.id, file_id);

                    if let (Some(l1), Some(l2)) = (lines1, lines2) {
                        let line_overlap = l1 & l2;
                        if !line_overlap.is_empty() {
                            // Convert bitmap back to ranges for storage
                            let ranges = bitmap_to_ranges(&line_overlap);
                            if let Some(path) = index.get_file_path(file_id) {
                                overlapping_files.push(OverlappingFile {
                                    path: path.clone(),
                                    overlapping_ranges: ranges,
                                });
                            }
                        }
                    }
                }

                let category = if overlapping_files.is_empty() {
                    DependencyCategory::PartiallyDependent { shared_files }
                } else {
                    DependencyCategory::Dependent {
                        shared_files,
                        overlapping_files,
                    }
                };

                Some((current_pr.id, prev_pr.id, category))
            })
            .collect();

        // Build graph from collected dependencies (sequential - needs mutable access)
        let mut warnings = Vec::new();
        for (from_id, to_id, category) in dependencies {
            let from_pr = prs.iter().find(|p| p.id == from_id).unwrap();
            let to_pr = prs.iter().find(|p| p.id == to_id).unwrap();

            let dependency = PRDependency {
                from_pr_id: from_id,
                to_pr_id: to_id,
                category: category.clone(),
            };

            // Add dependency to current node
            if let Some(node) = graph.get_node_mut(from_id) {
                node.dependencies.push(dependency);
            }

            // Add as dependent to previous node
            if let Some(prev_node) = graph.get_node_mut(to_id) {
                prev_node.dependents.push(from_id);
            }

            // Check for unselected dependency warning
            if from_pr.is_selected && !to_pr.is_selected {
                let should_warn = match &category {
                    DependencyCategory::Dependent { .. } => true,
                    DependencyCategory::PartiallyDependent { .. } => self.config.warn_on_partial,
                    DependencyCategory::Independent => false,
                };

                if should_warn {
                    warnings.push(DependencyWarning::UnselectedDependency {
                        selected_pr_id: from_id,
                        selected_pr_title: from_pr.title.clone(),
                        unselected_pr_id: to_id,
                        unselected_pr_title: to_pr.title.clone(),
                        category,
                    });
                }
            }
        }

        // Compute topological order
        graph.compute_topological_order();

        DependencyAnalysisResult { graph, warnings }
    }

    /// Categorizes the dependency between two sets of file changes.
    fn categorize_dependency(
        current: Option<&Vec<FileChange>>,
        previous: Option<&Vec<FileChange>>,
    ) -> DependencyCategory {
        let current = match current {
            Some(c) if !c.is_empty() => c,
            _ => return DependencyCategory::Independent,
        };

        let previous = match previous {
            Some(p) if !p.is_empty() => p,
            _ => return DependencyCategory::Independent,
        };

        // Find files that appear in both changesets
        let current_files: HashSet<&str> = current
            .iter()
            .flat_map(|c| {
                let mut files = vec![c.path.as_str()];
                if let Some(ref orig) = c.original_path {
                    files.push(orig.as_str());
                }
                files
            })
            .collect();

        let previous_files: HashSet<&str> = previous
            .iter()
            .flat_map(|c| {
                let mut files = vec![c.path.as_str()];
                if let Some(ref orig) = c.original_path {
                    files.push(orig.as_str());
                }
                files
            })
            .collect();

        let shared_files: Vec<String> = current_files
            .intersection(&previous_files)
            .map(|s| s.to_string())
            .collect();

        if shared_files.is_empty() {
            return DependencyCategory::Independent;
        }

        // Check for overlapping line ranges in shared files
        let mut overlapping_files = Vec::new();

        for shared_file in &shared_files {
            let current_change = current.iter().find(|c| {
                c.path == *shared_file || c.original_path.as_deref() == Some(shared_file)
            });
            let previous_change = previous.iter().find(|c| {
                c.path == *shared_file || c.original_path.as_deref() == Some(shared_file)
            });

            if let (Some(curr), Some(prev)) = (current_change, previous_change) {
                let overlapping_ranges = curr.get_overlapping_ranges(prev);
                if !overlapping_ranges.is_empty() {
                    overlapping_files.push(OverlappingFile {
                        path: shared_file.clone(),
                        overlapping_ranges,
                    });
                }
            }
        }

        if overlapping_files.is_empty() {
            DependencyCategory::PartiallyDependent { shared_files }
        } else {
            DependencyCategory::Dependent {
                shared_files,
                overlapping_files,
            }
        }
    }
}

impl Default for DependencyAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Minimal PR information needed for dependency analysis.
#[derive(Debug, Clone)]
pub struct PRInfo {
    /// PR ID.
    pub id: i32,
    /// PR title.
    pub title: String,
    /// Whether the PR is selected for merging.
    pub is_selected: bool,
    /// The merge commit ID.
    pub commit_id: Option<String>,
}

impl PRInfo {
    /// Creates a new PRInfo.
    pub fn new(id: i32, title: String, is_selected: bool, commit_id: Option<String>) -> Self {
        Self {
            id,
            title,
            is_selected,
            commit_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # LineRange Overlap Detection
    ///
    /// Tests that overlapping line ranges are correctly detected.
    ///
    /// ## Test Scenario
    /// - Creates pairs of ranges with various overlap conditions
    ///
    /// ## Expected Outcome
    /// - Overlapping ranges return true, non-overlapping return false
    #[test]
    fn test_line_range_overlap() {
        // Exact overlap
        assert!(LineRange::new(1, 10).overlaps(&LineRange::new(1, 10)));

        // Partial overlap
        assert!(LineRange::new(1, 10).overlaps(&LineRange::new(5, 15)));
        assert!(LineRange::new(5, 15).overlaps(&LineRange::new(1, 10)));

        // Adjacent (touching) - should overlap
        assert!(LineRange::new(1, 5).overlaps(&LineRange::new(5, 10)));

        // No overlap
        assert!(!LineRange::new(1, 5).overlaps(&LineRange::new(6, 10)));
        assert!(!LineRange::new(10, 20).overlaps(&LineRange::new(1, 5)));

        // Single line overlaps
        assert!(LineRange::single(5).overlaps(&LineRange::new(1, 10)));
        assert!(!LineRange::single(5).overlaps(&LineRange::new(6, 10)));
    }

    /// # LineRange Length Calculation
    ///
    /// Tests that line range length is correctly calculated.
    #[test]
    fn test_line_range_len() {
        assert_eq!(LineRange::new(1, 1).len(), 1);
        assert_eq!(LineRange::new(1, 10).len(), 10);
        assert_eq!(LineRange::new(5, 15).len(), 11);
        assert_eq!(LineRange::single(42).len(), 1);
    }

    /// # ChangeType Parsing from Git Status
    ///
    /// Tests parsing of git status codes.
    #[test]
    fn test_change_type_from_git_status() {
        assert_eq!(ChangeType::from_git_status("A"), Some(ChangeType::Add));
        assert_eq!(ChangeType::from_git_status("M"), Some(ChangeType::Modify));
        assert_eq!(ChangeType::from_git_status("D"), Some(ChangeType::Delete));
        assert_eq!(
            ChangeType::from_git_status("R100"),
            Some(ChangeType::Rename)
        );
        assert_eq!(ChangeType::from_git_status("C"), Some(ChangeType::Copy));
        assert_eq!(ChangeType::from_git_status("X"), None);
        assert_eq!(ChangeType::from_git_status(""), None);
    }

    /// # FileChange Overlapping Lines Detection
    ///
    /// Tests detection of overlapping line changes in files.
    #[test]
    fn test_file_change_overlapping_lines() {
        let change1 = FileChange::with_ranges(
            "src/main.rs".to_string(),
            ChangeType::Modify,
            vec![LineRange::new(10, 20), LineRange::new(50, 60)],
        );

        let change2 = FileChange::with_ranges(
            "src/main.rs".to_string(),
            ChangeType::Modify,
            vec![LineRange::new(15, 25)], // Overlaps with 10-20
        );

        let change3 = FileChange::with_ranges(
            "src/main.rs".to_string(),
            ChangeType::Modify,
            vec![LineRange::new(30, 40)], // No overlap
        );

        let change4 = FileChange::with_ranges(
            "src/other.rs".to_string(),
            ChangeType::Modify,
            vec![LineRange::new(10, 20)], // Different file
        );

        assert!(change1.has_overlapping_lines(&change2));
        assert!(!change1.has_overlapping_lines(&change3));
        assert!(!change1.has_overlapping_lines(&change4));
    }

    /// # DependencyCategory Serialization
    ///
    /// Tests that dependency categories serialize correctly to JSON.
    #[test]
    fn test_dependency_category_serialization() {
        let independent = DependencyCategory::Independent;
        let json = serde_json::to_string(&independent).unwrap();
        assert!(json.contains("\"type\":\"independent\""));

        let partial = DependencyCategory::PartiallyDependent {
            shared_files: vec!["file1.rs".to_string()],
        };
        let json = serde_json::to_string(&partial).unwrap();
        assert!(json.contains("\"type\":\"partially_dependent\""));

        let dependent = DependencyCategory::Dependent {
            shared_files: vec!["file1.rs".to_string()],
            overlapping_files: vec![OverlappingFile {
                path: "file1.rs".to_string(),
                overlapping_ranges: vec![LineRange::new(10, 20)],
            }],
        };
        let json = serde_json::to_string(&dependent).unwrap();
        assert!(json.contains("\"type\":\"dependent\""));
    }

    /// # PRDependencyNode Dependency Counts
    ///
    /// Tests counting of dependencies by category.
    #[test]
    fn test_node_dependency_counts() {
        let mut node = PRDependencyNode::new(1, "Test PR".to_string(), true);
        node.dependencies.push(PRDependency {
            from_pr_id: 1,
            to_pr_id: 2,
            category: DependencyCategory::Independent,
        });
        node.dependencies.push(PRDependency {
            from_pr_id: 1,
            to_pr_id: 3,
            category: DependencyCategory::PartiallyDependent {
                shared_files: vec!["file.rs".to_string()],
            },
        });
        node.dependencies.push(PRDependency {
            from_pr_id: 1,
            to_pr_id: 4,
            category: DependencyCategory::Dependent {
                shared_files: vec!["file.rs".to_string()],
                overlapping_files: vec![],
            },
        });

        let counts = node.dependency_counts();
        assert_eq!(counts.independent, 1);
        assert_eq!(counts.partial, 1);
        assert_eq!(counts.dependent, 1);
    }

    /// # DependencyWarning Message Generation
    ///
    /// Tests that warning messages are correctly formatted.
    #[test]
    fn test_warning_message() {
        let warning = DependencyWarning::UnselectedDependency {
            selected_pr_id: 123,
            selected_pr_title: "Add feature X".to_string(),
            unselected_pr_id: 100,
            unselected_pr_title: "Refactor base".to_string(),
            category: DependencyCategory::Dependent {
                shared_files: vec!["src/lib.rs".to_string()],
                overlapping_files: vec![],
            },
        };

        let msg = warning.message();
        assert!(msg.contains("PR #123"));
        assert!(msg.contains("depends on"));
        assert!(msg.contains("PR #100"));
        assert!(warning.is_critical());
    }

    /// # Truncate Title Function
    ///
    /// Tests title truncation for display.
    #[test]
    fn test_truncate_title() {
        assert_eq!(truncate_title("Short", 10), "Short");
        assert_eq!(
            truncate_title("This is a very long title", 10),
            "This is..."
        );
        assert_eq!(truncate_title("Exactly10!", 10), "Exactly10!");
    }

    /// # DependencyAnalyzer Independent PRs
    ///
    /// Tests analysis of PRs that don't share any files.
    ///
    /// ## Test Scenario
    /// - Creates two PRs that modify different files
    ///
    /// ## Expected Outcome
    /// - Both PRs should be categorized as independent
    #[test]
    fn test_analyzer_independent_prs() {
        let prs = vec![
            PRInfo::new(1, "PR 1".to_string(), true, Some("abc123".to_string())),
            PRInfo::new(2, "PR 2".to_string(), true, Some("def456".to_string())),
        ];

        let mut pr_changes = HashMap::new();
        pr_changes.insert(
            1,
            vec![FileChange::with_ranges(
                "src/module_a.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(10, 20)],
            )],
        );
        pr_changes.insert(
            2,
            vec![FileChange::with_ranges(
                "src/module_b.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(10, 20)],
            )],
        );

        let analyzer = DependencyAnalyzer::new();
        let result = analyzer.analyze(&prs, &pr_changes);

        // PR 2 should have no dependencies on PR 1
        let node2 = result.graph.get_node(2).unwrap();
        assert!(
            node2.dependencies.is_empty(),
            "Independent PRs should have no dependencies"
        );
        assert!(result.warnings.is_empty());
    }

    /// # DependencyAnalyzer Partially Dependent PRs
    ///
    /// Tests analysis of PRs that share files but not overlapping lines.
    ///
    /// ## Test Scenario
    /// - Creates two PRs that modify the same file but different lines
    ///
    /// ## Expected Outcome
    /// - Second PR should be partially dependent on first
    #[test]
    fn test_analyzer_partially_dependent_prs() {
        let prs = vec![
            PRInfo::new(1, "PR 1".to_string(), true, Some("abc123".to_string())),
            PRInfo::new(2, "PR 2".to_string(), true, Some("def456".to_string())),
        ];

        let mut pr_changes = HashMap::new();
        pr_changes.insert(
            1,
            vec![FileChange::with_ranges(
                "src/shared.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(10, 20)],
            )],
        );
        pr_changes.insert(
            2,
            vec![FileChange::with_ranges(
                "src/shared.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(50, 60)], // Different lines
            )],
        );

        let analyzer = DependencyAnalyzer::new();
        let result = analyzer.analyze(&prs, &pr_changes);

        let node2 = result.graph.get_node(2).unwrap();
        assert_eq!(node2.dependencies.len(), 1);
        assert!(matches!(
            node2.dependencies[0].category,
            DependencyCategory::PartiallyDependent { .. }
        ));
    }

    /// # DependencyAnalyzer Dependent PRs
    ///
    /// Tests analysis of PRs with overlapping line changes.
    ///
    /// ## Test Scenario
    /// - Creates two PRs that modify overlapping lines in the same file
    ///
    /// ## Expected Outcome
    /// - Second PR should be fully dependent on first
    #[test]
    fn test_analyzer_dependent_prs() {
        let prs = vec![
            PRInfo::new(1, "PR 1".to_string(), true, Some("abc123".to_string())),
            PRInfo::new(2, "PR 2".to_string(), true, Some("def456".to_string())),
        ];

        let mut pr_changes = HashMap::new();
        pr_changes.insert(
            1,
            vec![FileChange::with_ranges(
                "src/shared.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(10, 30)],
            )],
        );
        pr_changes.insert(
            2,
            vec![FileChange::with_ranges(
                "src/shared.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(25, 40)], // Overlaps with 10-30
            )],
        );

        let analyzer = DependencyAnalyzer::new();
        let result = analyzer.analyze(&prs, &pr_changes);

        let node2 = result.graph.get_node(2).unwrap();
        assert_eq!(node2.dependencies.len(), 1);
        assert!(matches!(
            node2.dependencies[0].category,
            DependencyCategory::Dependent { .. }
        ));
    }

    /// # DependencyAnalyzer Unselected Dependency Warning
    ///
    /// Tests that warnings are generated when a selected PR depends on unselected PR.
    ///
    /// ## Test Scenario
    /// - Creates a selected PR that depends on an unselected PR
    ///
    /// ## Expected Outcome
    /// - Warning should be generated for the unselected dependency
    #[test]
    fn test_analyzer_unselected_dependency_warning() {
        let prs = vec![
            PRInfo::new(
                1,
                "Base refactor".to_string(),
                false,
                Some("abc123".to_string()),
            ), // Not selected
            PRInfo::new(2, "Feature".to_string(), true, Some("def456".to_string())), // Selected
        ];

        let mut pr_changes = HashMap::new();
        pr_changes.insert(
            1,
            vec![FileChange::with_ranges(
                "src/shared.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(10, 30)],
            )],
        );
        pr_changes.insert(
            2,
            vec![FileChange::with_ranges(
                "src/shared.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(25, 40)], // Overlaps
            )],
        );

        let analyzer = DependencyAnalyzer::new();
        let result = analyzer.analyze(&prs, &pr_changes);

        // Should have a warning about unselected dependency
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].is_critical());

        match &result.warnings[0] {
            DependencyWarning::UnselectedDependency {
                selected_pr_id,
                unselected_pr_id,
                ..
            } => {
                assert_eq!(*selected_pr_id, 2);
                assert_eq!(*unselected_pr_id, 1);
            }
            _ => panic!("Expected UnselectedDependency warning"),
        }
    }

    /// # DependencyAnalyzer Multiple Dependencies
    ///
    /// Tests analysis with multiple PRs and various dependency types.
    ///
    /// ## Test Scenario
    /// - Creates three PRs with mixed dependency relationships
    ///
    /// ## Expected Outcome
    /// - Correct dependencies are recorded for each PR
    #[test]
    fn test_analyzer_multiple_dependencies() {
        let prs = vec![
            PRInfo::new(1, "PR 1".to_string(), true, Some("abc".to_string())),
            PRInfo::new(2, "PR 2".to_string(), true, Some("def".to_string())),
            PRInfo::new(3, "PR 3".to_string(), true, Some("ghi".to_string())),
        ];

        let mut pr_changes = HashMap::new();
        // PR 1: modifies file A
        pr_changes.insert(
            1,
            vec![FileChange::with_ranges(
                "src/a.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(10, 20)],
            )],
        );
        // PR 2: modifies file A (different lines) and file B
        pr_changes.insert(
            2,
            vec![
                FileChange::with_ranges(
                    "src/a.rs".to_string(),
                    ChangeType::Modify,
                    vec![LineRange::new(50, 60)],
                ),
                FileChange::with_ranges(
                    "src/b.rs".to_string(),
                    ChangeType::Modify,
                    vec![LineRange::new(10, 20)],
                ),
            ],
        );
        // PR 3: modifies file A (overlapping with PR 1) and file B (overlapping with PR 2)
        pr_changes.insert(
            3,
            vec![
                FileChange::with_ranges(
                    "src/a.rs".to_string(),
                    ChangeType::Modify,
                    vec![LineRange::new(15, 25)], // Overlaps PR 1
                ),
                FileChange::with_ranges(
                    "src/b.rs".to_string(),
                    ChangeType::Modify,
                    vec![LineRange::new(15, 25)], // Overlaps PR 2
                ),
            ],
        );

        let analyzer = DependencyAnalyzer::new();
        let result = analyzer.analyze(&prs, &pr_changes);

        // PR 1: no dependencies (first)
        let node1 = result.graph.get_node(1).unwrap();
        assert!(node1.dependencies.is_empty());

        // PR 2: partial dependency on PR 1 (same file, different lines)
        let node2 = result.graph.get_node(2).unwrap();
        assert_eq!(node2.dependencies.len(), 1);
        assert!(matches!(
            node2.dependencies[0].category,
            DependencyCategory::PartiallyDependent { .. }
        ));

        // PR 3: dependencies on both PR 1 and PR 2
        let node3 = result.graph.get_node(3).unwrap();
        assert_eq!(node3.dependencies.len(), 2);
    }

    /// # DependencyAnalyzer Empty Changes
    ///
    /// Tests handling of PRs with no file changes.
    ///
    /// ## Test Scenario
    /// - Creates PRs where some have no changes recorded
    ///
    /// ## Expected Outcome
    /// - PRs with no changes are treated as independent
    #[test]
    fn test_analyzer_empty_changes() {
        let prs = vec![
            PRInfo::new(1, "PR 1".to_string(), true, None), // No commit
            PRInfo::new(2, "PR 2".to_string(), true, Some("def".to_string())),
        ];

        let mut pr_changes = HashMap::new();
        pr_changes.insert(
            2,
            vec![FileChange::with_ranges(
                "src/a.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(10, 20)],
            )],
        );
        // PR 1 has no changes recorded

        let analyzer = DependencyAnalyzer::new();
        let result = analyzer.analyze(&prs, &pr_changes);

        // Both PRs should exist in graph but PR 2 should be independent of PR 1
        let node2 = result.graph.get_node(2).unwrap();
        assert!(node2.dependencies.is_empty());
    }

    /// # DependencyGraph Topological Order
    ///
    /// Tests that the topological order is computed correctly.
    ///
    /// ## Test Scenario
    /// - Creates PRs with dependencies
    ///
    /// ## Expected Outcome
    /// - Dependencies appear before dependents in order
    #[test]
    fn test_graph_topological_order() {
        let prs = vec![
            PRInfo::new(1, "Base".to_string(), true, Some("a".to_string())),
            PRInfo::new(2, "Middle".to_string(), true, Some("b".to_string())),
            PRInfo::new(3, "Top".to_string(), true, Some("c".to_string())),
        ];

        let mut pr_changes = HashMap::new();
        pr_changes.insert(
            1,
            vec![FileChange::with_ranges(
                "src/shared.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(10, 20)],
            )],
        );
        pr_changes.insert(
            2,
            vec![FileChange::with_ranges(
                "src/shared.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(15, 25)], // Depends on 1
            )],
        );
        pr_changes.insert(
            3,
            vec![FileChange::with_ranges(
                "src/shared.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(20, 30)], // Depends on 1 and 2
            )],
        );

        let analyzer = DependencyAnalyzer::new();
        let result = analyzer.analyze(&prs, &pr_changes);

        // In topological order, 1 should come before 2, and 2 before 3
        let order = &result.graph.topological_order;
        let pos1 = order.iter().position(|&id| id == 1).unwrap();
        let pos2 = order.iter().position(|&id| id == 2).unwrap();
        let pos3 = order.iter().position(|&id| id == 3).unwrap();

        assert!(pos1 < pos2, "PR 1 should come before PR 2");
        assert!(pos2 < pos3, "PR 2 should come before PR 3");
    }

    /// # Parallel Analysis Produces Same Results as Sequential
    ///
    /// Tests that the parallel analysis method produces identical results
    /// to the sequential analysis method.
    ///
    /// ## Test Scenario
    /// - Creates a set of PRs with various dependency relationships
    /// - Runs both sequential and parallel analysis
    /// - Compares the results
    ///
    /// ## Expected Outcome
    /// - Both methods should produce identical dependency graphs
    /// - Same warnings should be generated
    #[test]
    fn test_parallel_analysis_equivalence() {
        let prs = vec![
            PRInfo::new(1, "Base".to_string(), true, Some("abc".to_string())),
            PRInfo::new(2, "Feature A".to_string(), true, Some("def".to_string())),
            PRInfo::new(3, "Feature B".to_string(), false, Some("ghi".to_string())),
            PRInfo::new(4, "Integration".to_string(), true, Some("jkl".to_string())),
        ];

        let mut pr_changes = HashMap::new();
        pr_changes.insert(
            1,
            vec![FileChange::with_ranges(
                "src/core.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(10, 30)],
            )],
        );
        pr_changes.insert(
            2,
            vec![FileChange::with_ranges(
                "src/core.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(25, 45)], // Overlaps with 1
            )],
        );
        pr_changes.insert(
            3,
            vec![FileChange::with_ranges(
                "src/util.rs".to_string(),
                ChangeType::Modify,
                vec![LineRange::new(5, 15)], // Different file
            )],
        );
        pr_changes.insert(
            4,
            vec![
                FileChange::with_ranges(
                    "src/core.rs".to_string(),
                    ChangeType::Modify,
                    vec![LineRange::new(40, 55)], // Overlaps with 2
                ),
                FileChange::with_ranges(
                    "src/util.rs".to_string(),
                    ChangeType::Modify,
                    vec![LineRange::new(10, 20)], // Overlaps with 3
                ),
            ],
        );

        let analyzer = DependencyAnalyzer::new();
        let sequential_result = analyzer.analyze(&prs, &pr_changes);
        let parallel_result = analyzer.analyze_parallel(&prs, &pr_changes);

        // Compare graph node counts
        assert_eq!(
            sequential_result.graph.nodes.len(),
            parallel_result.graph.nodes.len(),
            "Node counts should match"
        );

        // Compare each node's dependencies
        for pr in &prs {
            let seq_node = sequential_result.graph.get_node(pr.id).unwrap();
            let par_node = parallel_result.graph.get_node(pr.id).unwrap();

            assert_eq!(
                seq_node.dependencies.len(),
                par_node.dependencies.len(),
                "Dependency count should match for PR {}",
                pr.id
            );

            // Check that same dependencies exist (order may differ)
            for seq_dep in &seq_node.dependencies {
                let found = par_node.dependencies.iter().any(|par_dep| {
                    par_dep.to_pr_id == seq_dep.to_pr_id
                        && std::mem::discriminant(&par_dep.category)
                            == std::mem::discriminant(&seq_dep.category)
                });
                assert!(
                    found,
                    "Parallel result should have same dependency from {} to {}",
                    pr.id, seq_dep.to_pr_id
                );
            }
        }

        // Compare warning counts
        assert_eq!(
            sequential_result.warnings.len(),
            parallel_result.warnings.len(),
            "Warning counts should match"
        );
    }

    /// # Parallel Analysis with Many PRs
    ///
    /// Tests that parallel analysis handles larger datasets correctly.
    ///
    /// ## Test Scenario
    /// - Creates 20 PRs with chained dependencies
    /// - Runs parallel analysis
    ///
    /// ## Expected Outcome
    /// - Analysis should complete without errors
    /// - Topological order should respect dependencies
    #[test]
    fn test_parallel_analysis_many_prs() {
        let prs: Vec<PRInfo> = (1..=20)
            .map(|i| {
                PRInfo::new(
                    i,
                    format!("PR {}", i),
                    i % 2 == 0, // Alternate selection
                    Some(format!("commit{}", i)),
                )
            })
            .collect();

        // Each PR modifies the same file with slightly overlapping ranges
        let pr_changes: HashMap<i32, Vec<FileChange>> = (1..=20)
            .map(|i| {
                let start = (i as u32 - 1) * 5 + 1;
                let end = start + 10; // 10 line range, overlaps with adjacent PRs
                (
                    i,
                    vec![FileChange::with_ranges(
                        "src/main.rs".to_string(),
                        ChangeType::Modify,
                        vec![LineRange::new(start, end)],
                    )],
                )
            })
            .collect();

        let analyzer = DependencyAnalyzer::new();
        let result = analyzer.analyze_parallel(&prs, &pr_changes);

        // Should have nodes for all 20 PRs
        assert_eq!(result.graph.nodes.len(), 20);

        // Topological order should be valid (each PR should come after its dependencies)
        for (idx, &pr_id) in result.graph.topological_order.iter().enumerate() {
            if let Some(node) = result.graph.get_node(pr_id) {
                for dep in &node.dependencies {
                    let dep_idx = result
                        .graph
                        .topological_order
                        .iter()
                        .position(|&id| id == dep.to_pr_id);
                    if let Some(dep_idx) = dep_idx {
                        assert!(
                            dep_idx < idx,
                            "PR {} depends on {}, but {} comes later in topological order",
                            pr_id,
                            dep.to_pr_id,
                            dep.to_pr_id
                        );
                    }
                }
            }
        }
    }
}
