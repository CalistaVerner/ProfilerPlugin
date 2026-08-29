use std::collections::{BTreeMap, VecDeque};

use serde::Serialize;

use crate::records::JobRecord;

/// Hierarchical profiler read-model for runtime budget pressure.
///
/// This module is intentionally diagnostic-only. It never throttles engine work and
/// therefore cannot become a second scheduler. Runtime systems remain responsible
/// for their own admission policies; StarProfiler only aggregates the evidence.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub(crate) struct WorkBudgetNode {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) sample_count: u64,
    pub(crate) over_budget_samples: u64,
    pub(crate) measured_ms: f64,
    pub(crate) allowance_ms: f64,
    pub(crate) pressure: f64,
    pub(crate) peak_sample_ms: f64,
    pub(crate) peak_sample_load: f64,
    pub(crate) children: Vec<WorkBudgetNode>,
}

#[derive(Debug, Clone, Default)]
struct MutableBudgetNode {
    name: String,
    sample_count: u64,
    over_budget_samples: u64,
    measured_ms: f64,
    allowance_ms: f64,
    peak_sample_ms: f64,
    peak_sample_load: f64,
    children: BTreeMap<String, MutableBudgetNode>,
}

impl MutableBudgetNode {
    fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    fn observe(&mut self, job: &JobRecord) {
        let elapsed = finite_non_negative(job.elapsed_ms.unwrap_or_default());
        let allowance = finite_non_negative(job.budget_ms);
        let load = if allowance > 0.0 {
            elapsed / allowance
        } else {
            finite_non_negative(job.load.unwrap_or_default())
        };

        self.sample_count = self.sample_count.saturating_add(1);
        self.over_budget_samples = self
            .over_budget_samples
            .saturating_add(u64::from(load >= 1.0));
        self.measured_ms += elapsed;
        self.allowance_ms += allowance;
        self.peak_sample_ms = self.peak_sample_ms.max(elapsed);
        self.peak_sample_load = self.peak_sample_load.max(load);
    }

    fn freeze(self, id: String) -> WorkBudgetNode {
        let pressure = ratio(self.measured_ms, self.allowance_ms);
        let children = self
            .children
            .into_iter()
            .map(|(child_id, child)| child.freeze(format!("{id}/{child_id}")))
            .collect();
        WorkBudgetNode {
            id,
            name: self.name,
            sample_count: self.sample_count,
            over_budget_samples: self.over_budget_samples,
            measured_ms: self.measured_ms,
            allowance_ms: self.allowance_ms,
            pressure,
            peak_sample_ms: self.peak_sample_ms,
            peak_sample_load: self.peak_sample_load,
            children,
        }
    }
}

/// Build a deterministic `runtime -> lane -> category` pressure tree.
///
/// Every sample contributes exactly once to each level of its ancestry. The tree
/// therefore provides a dimensionally valid view: both measured and allowance
/// values are milliseconds, and pressure is their ratio. Unknown/empty lanes and
/// categories are normalized rather than silently dropped.
pub(crate) fn build_work_budget_tree(jobs: &VecDeque<JobRecord>) -> WorkBudgetNode {
    let mut root = MutableBudgetNode::named("Runtime captured work");

    for job in jobs {
        root.observe(job);

        let lane_id = normalized_key(&job.lane, "unspecified");
        let lane_name = normalized_label(&job.lane, "Unspecified lane");
        let lane = root
            .children
            .entry(lane_id)
            .or_insert_with(|| MutableBudgetNode::named(lane_name));
        lane.observe(job);

        let category_id = normalized_key(&job.category, "uncategorized");
        let category_name = normalized_label(&job.category, "Uncategorized");
        lane.children
            .entry(category_id)
            .or_insert_with(|| MutableBudgetNode::named(category_name))
            .observe(job);
    }

    root.freeze("runtime".to_owned())
}

fn normalized_key(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return fallback.to_owned();
    }

    let mut out = String::with_capacity(value.len());
    let mut previous_separator = false;
    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            previous_separator = false;
            Some(ch.to_ascii_lowercase())
        } else if previous_separator {
            None
        } else {
            previous_separator = true;
            Some('-')
        };
        if let Some(ch) = normalized {
            out.push(ch);
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        fallback.to_owned()
    } else {
        out.to_owned()
    }
}

fn normalized_label(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

#[inline]
fn finite_non_negative(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

#[inline]
fn ratio(value: f64, allowance: f64) -> f64 {
    if allowance > 0.0 {
        value / allowance
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn job(name: &str, lane: &str, category: &str, elapsed_ms: f64, budget_ms: f64) -> JobRecord {
        JobRecord {
            id: name.to_owned(),
            name: name.to_owned(),
            category: category.to_owned(),
            source: "test".to_owned(),
            lane: lane.to_owned(),
            priority: "normal".to_owned(),
            dependency_group: String::new(),
            frame_id: Some(1),
            status: "completed".to_owned(),
            detail: String::new(),
            scheduled: true,
            blocked: false,
            polling: false,
            waited_on_gpu: false,
            stayed_async: true,
            exceeded_frame_budget: false,
            frame_budget_ms: None,
            gpu_wait_ms: None,
            wait_reason: None,
            async_mode: Some("engine.threading".to_owned()),
            started_unix_ms: 0,
            ended_unix_ms: Some(1),
            elapsed_ms: Some(elapsed_ms),
            budget_ms,
            load: Some(if budget_ms > 0.0 {
                elapsed_ms / budget_ms
            } else {
                0.0
            }),
            progress: Some(1.0),
            payload_bytes: None,
            output_bytes: None,
            error: None,
            metadata: Value::Null,
        }
    }

    #[test]
    fn aggregates_runtime_lane_and_category_without_double_counting_root() {
        let jobs = VecDeque::from([
            job("a", "asset-io", "streaming", 2.0, 4.0),
            job("b", "asset-io", "streaming", 6.0, 4.0),
            job("c", "simulation", "ai", 1.0, 2.0),
        ]);

        let tree = build_work_budget_tree(&jobs);
        assert_eq!(tree.sample_count, 3);
        assert_eq!(tree.measured_ms, 9.0);
        assert_eq!(tree.allowance_ms, 10.0);
        assert!((tree.pressure - 0.9).abs() < 1.0e-9);
        assert_eq!(tree.children.len(), 2);

        let asset_io = tree
            .children
            .iter()
            .find(|node| node.id == "runtime/asset-io")
            .unwrap();
        assert_eq!(asset_io.sample_count, 2);
        assert_eq!(asset_io.measured_ms, 8.0);
        assert_eq!(asset_io.allowance_ms, 8.0);
        assert_eq!(asset_io.children[0].id, "runtime/asset-io/streaming");
    }

    #[test]
    fn reports_peak_and_over_budget_samples() {
        let jobs = VecDeque::from([
            job("good", "simulation", "ai", 1.0, 2.0),
            job("bad", "simulation", "ai", 7.0, 2.0),
        ]);
        let tree = build_work_budget_tree(&jobs);
        assert_eq!(tree.over_budget_samples, 1);
        assert_eq!(tree.peak_sample_ms, 7.0);
        assert!((tree.peak_sample_load - 3.5).abs() < 1.0e-9);
    }

    #[test]
    fn empty_labels_are_retained_under_explicit_fallback_nodes() {
        let jobs = VecDeque::from([job("unknown", "  ", "", 1.0, 2.0)]);
        let tree = build_work_budget_tree(&jobs);
        assert_eq!(tree.children[0].id, "runtime/unspecified");
        assert_eq!(tree.children[0].name, "Unspecified lane");
        assert_eq!(
            tree.children[0].children[0].id,
            "runtime/unspecified/uncategorized"
        );
    }

    #[test]
    fn non_finite_measurements_do_not_poison_report() {
        let jobs = VecDeque::from([
            job("nan", "simulation", "ai", f64::NAN, 2.0),
            job("inf", "simulation", "ai", f64::INFINITY, f64::INFINITY),
        ]);
        let tree = build_work_budget_tree(&jobs);
        assert!(tree.measured_ms.is_finite());
        assert!(tree.allowance_ms.is_finite());
        assert!(tree.pressure.is_finite());
    }

    #[test]
    fn ids_are_normalized_deterministically() {
        let jobs = VecDeque::from([job(
            "normalize",
            "Render Prep / Main",
            "Mesh Extraction",
            1.0,
            2.0,
        )]);
        let tree = build_work_budget_tree(&jobs);
        assert_eq!(tree.children[0].id, "runtime/render-prep-main");
        assert_eq!(
            tree.children[0].children[0].id,
            "runtime/render-prep-main/mesh-extraction"
        );
    }
}
