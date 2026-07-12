use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{json, Value};

use crate::constants::{
    ENGINE_PROFILER_GATEWAY_ID, PROFILER_PLUGIN_ID, PROFILER_PLUGIN_NAME, PROFILER_SERVICE_ID,
};
use crate::records::{CategoryStats, JobRecord, ProfilerState};
use crate::runtime::ProfilerRuntime;
use crate::util::{duration_ms, unix_ms};

const JSON_TOP_LIMIT: usize = 64;

#[derive(Debug, Default, Clone, Serialize)]
struct AggregateStats {
    key: String,
    category: String,
    source: String,
    sample_name: String,
    count: u64,
    failed: u64,
    slow: u64,
    total_elapsed_ms: f64,
    average_elapsed_ms: f64,
    max_elapsed_ms: f64,
    max_load: f64,
    total_share_percent: f64,
    total_payload_bytes: u64,
    total_output_bytes: u64,
}

impl ProfilerRuntime {
    pub(super) fn build_report_from_state(&self, state: &ProfilerState, reason: &str) -> Value {
        let mut by_category: BTreeMap<String, CategoryStats> = BTreeMap::new();
        let mut by_status: BTreeMap<String, u64> = BTreeMap::new();
        let mut by_source: BTreeMap<String, AggregateStats> = BTreeMap::new();
        let mut by_owner: BTreeMap<String, AggregateStats> = BTreeMap::new();
        let mut by_offender: BTreeMap<String, AggregateStats> = BTreeMap::new();
        let mut by_method: BTreeMap<String, AggregateStats> = BTreeMap::new();
        let mut by_lane: BTreeMap<String, AggregateStats> = BTreeMap::new();
        let mut scheduled_jobs = 0u64;
        let mut blocked_jobs = 0u64;
        let mut polling_jobs = 0u64;
        let mut gpu_wait_jobs = 0u64;
        let mut async_jobs = 0u64;
        let mut frame_budget_exceeded_jobs = 0u64;
        let mut elapsed_values = Vec::new();
        let mut load_values = Vec::new();
        let mut failed = 0u64;
        let mut slow = 0u64;
        let mut over_budget = 0u64;
        let mut total_elapsed_ms = 0.0f64;
        let mut max_elapsed_ms = 0.0f64;
        let mut total_payload_bytes = 0u64;
        let mut total_output_bytes = 0u64;

        for job in &state.completed {
            let elapsed = job.elapsed_ms.unwrap_or_default();
            let load = job.load.unwrap_or_default();
            let is_failed = job.status == "failed";
            let is_slow = elapsed >= self.cfg.diagnostics.slow_job_warn_ms || load >= 1.0;
            let is_over_budget = load >= 1.0;

            total_elapsed_ms += elapsed;
            max_elapsed_ms = max_elapsed_ms.max(elapsed);
            elapsed_values.push(elapsed);
            load_values.push(load);
            total_payload_bytes =
                total_payload_bytes.saturating_add(job.payload_bytes.unwrap_or_default());
            total_output_bytes =
                total_output_bytes.saturating_add(job.output_bytes.unwrap_or_default());
            *by_status.entry(job.status.clone()).or_insert(0) += 1;
            if job.scheduled {
                scheduled_jobs = scheduled_jobs.saturating_add(1);
            }
            if job.blocked {
                blocked_jobs = blocked_jobs.saturating_add(1);
            }
            if job.polling {
                polling_jobs = polling_jobs.saturating_add(1);
            }
            if job.waited_on_gpu {
                gpu_wait_jobs = gpu_wait_jobs.saturating_add(1);
            }
            if job.stayed_async {
                async_jobs = async_jobs.saturating_add(1);
            }
            if job.exceeded_frame_budget {
                frame_budget_exceeded_jobs = frame_budget_exceeded_jobs.saturating_add(1);
            }

            let lane = if job.lane.trim().is_empty() {
                "unspecified"
            } else {
                job.lane.as_str()
            };
            accumulate(
                by_lane
                    .entry(lane.to_owned())
                    .or_insert_with(|| AggregateStats {
                        key: lane.to_owned(),
                        category: job.category.clone(),
                        source: job.source.clone(),
                        sample_name: job.name.clone(),
                        ..AggregateStats::default()
                    }),
                job,
                is_failed,
                is_slow,
            );

            let cat = by_category.entry(job.category.clone()).or_default();
            cat.count = cat.count.saturating_add(1);
            cat.total_elapsed_ms += elapsed;
            cat.max_elapsed_ms = cat.max_elapsed_ms.max(elapsed);
            if is_failed {
                failed = failed.saturating_add(1);
                cat.failed = cat.failed.saturating_add(1);
            }
            if is_slow {
                slow = slow.saturating_add(1);
                cat.slow = cat.slow.saturating_add(1);
            }
            if is_over_budget {
                over_budget = over_budget.saturating_add(1);
            }

            accumulate(
                by_source
                    .entry(job.source.clone())
                    .or_insert_with(|| AggregateStats {
                        key: job.source.clone(),
                        category: "*".to_owned(),
                        source: job.source.clone(),
                        sample_name: job.name.clone(),
                        ..AggregateStats::default()
                    }),
                job,
                is_failed,
                is_slow,
            );

            let owner = job_owner_key(job);
            accumulate(
                by_owner
                    .entry(owner.clone())
                    .or_insert_with(|| AggregateStats {
                        key: owner,
                        category: job.category.clone(),
                        source: job.source.clone(),
                        sample_name: job.name.clone(),
                        ..AggregateStats::default()
                    }),
                job,
                is_failed,
                is_slow,
            );

            let offender = job_offender_key(job);
            accumulate(
                by_offender
                    .entry(offender.clone())
                    .or_insert_with(|| AggregateStats {
                        key: offender,
                        category: job.category.clone(),
                        source: job.source.clone(),
                        sample_name: job.name.clone(),
                        ..AggregateStats::default()
                    }),
                job,
                is_failed,
                is_slow,
            );

            let method = job_method_key(job);
            accumulate(
                by_method
                    .entry(method.clone())
                    .or_insert_with(|| AggregateStats {
                        key: method,
                        category: job.category.clone(),
                        source: job.source.clone(),
                        sample_name: job.name.clone(),
                        ..AggregateStats::default()
                    }),
                job,
                is_failed,
                is_slow,
            );
        }

        let active_jobs: Vec<Value> = state
            .active
            .values()
            .map(|job| {
                let active_elapsed_ms = duration_ms(job.started_at.elapsed());
                let current_load = active_elapsed_ms / job.record.budget_ms.max(0.001);
                let mut value = serde_json::to_value(&job.record).unwrap_or(Value::Null);
                if let Value::Object(obj) = &mut value {
                    obj.insert("active_elapsed_ms".to_owned(), json!(active_elapsed_ms));
                    obj.insert("current_load".to_owned(), json!(current_load));
                    obj.insert("current_over_budget".to_owned(), json!(current_load >= 1.0));
                }
                value
            })
            .collect();
        let completed_count = state.completed.len() as f64;

        let mut category_ranked = Vec::new();
        for (category, st) in &by_category {
            let avg = if st.count > 0 {
                st.total_elapsed_ms / st.count as f64
            } else {
                0.0
            };
            category_ranked.push(json!({
                "category": category,
                "count": st.count,
                "failed": st.failed,
                "slow": st.slow,
                "total_elapsed_ms": st.total_elapsed_ms,
                "average_elapsed_ms": avg,
                "max_elapsed_ms": st.max_elapsed_ms,
                "total_share_percent": percent_of(st.total_elapsed_ms, total_elapsed_ms),
            }));
        }
        sort_objects_desc(&mut category_ranked, "total_elapsed_ms");

        finalize_aggregates(&mut by_source, total_elapsed_ms);
        finalize_aggregates(&mut by_owner, total_elapsed_ms);
        finalize_aggregates(&mut by_offender, total_elapsed_ms);
        finalize_aggregates(&mut by_method, total_elapsed_ms);
        finalize_aggregates(&mut by_lane, total_elapsed_ms);

        let source_ranked = ranked_aggregates(by_source, JSON_TOP_LIMIT);
        let owner_ranked = ranked_aggregates(by_owner, JSON_TOP_LIMIT);
        let offender_ranked = ranked_aggregates(by_offender, JSON_TOP_LIMIT);
        let method_ranked = ranked_aggregates(by_method, JSON_TOP_LIMIT);
        let lane_ranked = ranked_aggregates(by_lane, JSON_TOP_LIMIT);
        let top_elapsed_jobs = ranked_jobs_by(&state.completed, "elapsed", JSON_TOP_LIMIT);
        let top_load_jobs = ranked_jobs_by(&state.completed, "load", JSON_TOP_LIMIT);
        let budget_violations = ranked_budget_violations(
            &state.completed,
            self.cfg.diagnostics.slow_job_warn_ms,
            JSON_TOP_LIMIT,
        );
        let frame_budget_violations =
            ranked_frame_budget_violations(&state.completed, JSON_TOP_LIMIT);
        let profiler_first = json!({
            "scheduled_jobs": scheduled_jobs,
            "blocked_jobs": blocked_jobs,
            "polling_jobs": polling_jobs,
            "gpu_wait_jobs": gpu_wait_jobs,
            "async_jobs": async_jobs,
            "frame_budget_exceeded_jobs": frame_budget_exceeded_jobs,
            "async_share_percent": percent_of(async_jobs as f64, state.completed.len() as f64),
            "blocked_share_percent": percent_of(blocked_jobs as f64, state.completed.len() as f64),
            "gpu_wait_share_percent": percent_of(gpu_wait_jobs as f64, state.completed.len() as f64),
        });
        let elapsed_percentiles = percentiles_json(elapsed_values);
        let load_percentiles = percentiles_json(load_values);

        json!({
            "schema": "newengine.profiler.report.v3",
            "reason": reason,
            "generated_unix_ms": unix_ms(),
            "plugin": {
                "id": PROFILER_PLUGIN_ID,
                "name": PROFILER_PLUGIN_NAME,
                "version": env!("CARGO_PKG_VERSION"),
                "service_id": PROFILER_SERVICE_ID,
                "gateway": ENGINE_PROFILER_GATEWAY_ID
            },
            "run": {
                "started_unix_ms": state.run_started_unix_ms,
                "uptime_ms": duration_ms(state.run_started.elapsed()),
                "events_seen": state.events_seen,
                "malformed_events": state.malformed_events,
            },
            "summary": {
                "active_jobs": state.active.len(),
                "completed_jobs_kept": state.completed.len(),
                "failed_jobs": failed,
                "slow_or_over_budget_jobs": slow,
                "over_budget_jobs": over_budget,
                "total_elapsed_ms": total_elapsed_ms,
                "average_elapsed_ms": if completed_count > 0.0 { total_elapsed_ms / completed_count } else { 0.0 },
                "max_elapsed_ms": max_elapsed_ms,
                "total_payload_bytes": total_payload_bytes,
                "total_output_bytes": total_output_bytes,
                "elapsed_percentiles_ms": elapsed_percentiles,
                "load_percentiles": load_percentiles,
                "reports_written": state.reports_written,
                "reports_in_progress": state.reports_in_progress,
                "reports_scheduled": state.reports_scheduled,
                "reports_failed": state.reports_failed,
                "by_status": by_status,
                "by_category": by_category,
                "profiler_first": profiler_first.clone(),
            },
            "analysis": {
                "human_reading_order": [
                    "worst_offender",
                    "top_offenders_by_total_elapsed",
                    "top_completed_jobs_by_elapsed",
                    "top_completed_jobs_by_load",
                    "by_category_ranked",
                    "by_source_ranked",
                    "by_method_ranked",
                    "by_lane_ranked",
                    "profiler_first",
                    "budget_violations",
                    "frame_budget_violations",
                    "active_jobs"
                ],
                "interpretation": "elapsed_ms is observed wall-clock time captured by profiler events; load = elapsed_ms / budget_ms. It identifies CPU-time suspects inside instrumented engine/plugin work, not OS-level sampled CPU cycles.",
                "worst_offender": offender_ranked.first().cloned().unwrap_or(Value::Null),
                "by_category_ranked": category_ranked,
                "by_source_ranked": source_ranked,
                "by_owner_ranked": owner_ranked,
                "by_method_ranked": method_ranked,
                "by_lane_ranked": lane_ranked,
                "profiler_first": profiler_first,
                "top_offenders_by_total_elapsed": offender_ranked,
                "top_completed_jobs_by_elapsed": top_elapsed_jobs,
                "top_completed_jobs_by_load": top_load_jobs,
                "budget_violations": budget_violations,
                "frame_budget_violations": frame_budget_violations,
            },
            "active_jobs": active_jobs,
            "completed_jobs": &state.completed,
            "diagnostics": &state.diagnostics,
            "flush_requests": &state.flush_requests,
            "scheduler": {
                "service_flush_mode": self.cfg.scheduling.service_flush_mode.clone(),
                "shutdown_flush_mode": self.cfg.scheduling.shutdown_flush_mode.clone(),
                "prefer_engine_threading": self.cfg.scheduling.prefer_engine_jobs,
                "require_engine_threading": self.cfg.scheduling.require_engine_jobs,
                "lock_policy": "snapshot_then_build_and_write_outside_lock",
                "hidden_load_policy": "engine.threading required for async flush; profiler-owned background fallback is not allowed"
            },
            "config": self.cfg.clone(),
        })
    }
}

fn accumulate(stats: &mut AggregateStats, job: &JobRecord, failed: bool, slow: bool) {
    let elapsed = job.elapsed_ms.unwrap_or_default();
    let load = job.load.unwrap_or_default();
    stats.count = stats.count.saturating_add(1);
    stats.total_elapsed_ms += elapsed;
    stats.max_elapsed_ms = stats.max_elapsed_ms.max(elapsed);
    stats.max_load = stats.max_load.max(load);
    stats.total_payload_bytes = stats
        .total_payload_bytes
        .saturating_add(job.payload_bytes.unwrap_or_default());
    stats.total_output_bytes = stats
        .total_output_bytes
        .saturating_add(job.output_bytes.unwrap_or_default());
    if failed {
        stats.failed = stats.failed.saturating_add(1);
    }
    if slow {
        stats.slow = stats.slow.saturating_add(1);
    }
}

fn finalize_aggregates(map: &mut BTreeMap<String, AggregateStats>, total_elapsed_ms: f64) {
    for value in map.values_mut() {
        value.average_elapsed_ms = if value.count > 0 {
            value.total_elapsed_ms / value.count as f64
        } else {
            0.0
        };
        value.total_share_percent = percent_of(value.total_elapsed_ms, total_elapsed_ms);
    }
}

fn ranked_aggregates(map: BTreeMap<String, AggregateStats>, limit: usize) -> Vec<Value> {
    let mut values = map.into_values().collect::<Vec<_>>();
    values.sort_by(|a, b| {
        cmp_f64_desc(a.total_elapsed_ms, b.total_elapsed_ms).then_with(|| a.key.cmp(&b.key))
    });
    values
        .into_iter()
        .take(limit)
        .map(|v| serde_json::to_value(v).unwrap_or(Value::Null))
        .collect()
}

fn ranked_jobs_by(
    jobs: &std::collections::VecDeque<JobRecord>,
    by: &str,
    limit: usize,
) -> Vec<Value> {
    let mut values = jobs.iter().collect::<Vec<_>>();
    values.sort_by(|a, b| {
        let av = if by == "load" {
            a.load.unwrap_or_default()
        } else {
            a.elapsed_ms.unwrap_or_default()
        };
        let bv = if by == "load" {
            b.load.unwrap_or_default()
        } else {
            b.elapsed_ms.unwrap_or_default()
        };
        cmp_f64_desc(av, bv).then_with(|| a.name.cmp(&b.name))
    });
    values
        .into_iter()
        .take(limit)
        .map(|job| serde_json::to_value(job).unwrap_or(Value::Null))
        .collect()
}

fn ranked_budget_violations(
    jobs: &std::collections::VecDeque<JobRecord>,
    slow_job_warn_ms: f64,
    limit: usize,
) -> Vec<Value> {
    let mut values = jobs
        .iter()
        .filter(|job| {
            job.load.unwrap_or_default() >= 1.0
                || job.elapsed_ms.unwrap_or_default() >= slow_job_warn_ms
        })
        .collect::<Vec<_>>();
    values.sort_by(|a, b| {
        cmp_f64_desc(a.load.unwrap_or_default(), b.load.unwrap_or_default())
            .then_with(|| {
                cmp_f64_desc(
                    a.elapsed_ms.unwrap_or_default(),
                    b.elapsed_ms.unwrap_or_default(),
                )
            })
            .then_with(|| a.name.cmp(&b.name))
    });
    values
        .into_iter()
        .take(limit)
        .map(|job| serde_json::to_value(job).unwrap_or(Value::Null))
        .collect()
}

fn ranked_frame_budget_violations(
    jobs: &std::collections::VecDeque<JobRecord>,
    limit: usize,
) -> Vec<Value> {
    let mut values = jobs
        .iter()
        .filter(|job| job.exceeded_frame_budget)
        .collect::<Vec<_>>();
    values.sort_by(|a, b| {
        let a_over =
            (a.elapsed_ms.unwrap_or_default() - a.frame_budget_ms.unwrap_or_default()).max(0.0);
        let b_over =
            (b.elapsed_ms.unwrap_or_default() - b.frame_budget_ms.unwrap_or_default()).max(0.0);
        cmp_f64_desc(a_over, b_over)
            .then_with(|| {
                cmp_f64_desc(
                    a.elapsed_ms.unwrap_or_default(),
                    b.elapsed_ms.unwrap_or_default(),
                )
            })
            .then_with(|| a.name.cmp(&b.name))
    });
    values
        .into_iter()
        .take(limit)
        .map(|job| serde_json::to_value(job).unwrap_or(Value::Null))
        .collect()
}

fn percentiles_json(mut values: Vec<f64>) -> Value {
    values.retain(|value| value.is_finite());
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    json!({
        "p50": percentile_sorted(&values, 0.50),
        "p90": percentile_sorted(&values, 0.90),
        "p95": percentile_sorted(&values, 0.95),
        "p99": percentile_sorted(&values, 0.99),
    })
}

fn percentile_sorted(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let last = values.len().saturating_sub(1);
    let idx = ((last as f64) * q.clamp(0.0, 1.0)).round() as usize;
    values[idx.min(last)]
}

fn sort_objects_desc(values: &mut [Value], key: &str) {
    values.sort_by(|a, b| {
        let av = a.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        let bv = b.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        cmp_f64_desc(av, bv)
    });
}

fn cmp_f64_desc(a: f64, b: f64) -> std::cmp::Ordering {
    b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal)
}

pub(super) fn percent_of(value: f64, total: f64) -> f64 {
    if total > 0.0 {
        (value / total) * 100.0
    } else {
        0.0
    }
}

fn job_owner_key(job: &JobRecord) -> String {
    first_metadata_str(
        &job.metadata,
        &[
            "/service_id",
            "/metadata/service_id",
            "/provider_service_id",
            "/metadata/provider_service_id",
        ],
    )
    .or_else(|| {
        first_metadata_str(
            &job.metadata,
            &[
                "/plugin_id",
                "/metadata/plugin_id",
                "/owner_plugin_id",
                "/metadata/owner_plugin_id",
            ],
        )
        .map(|v| format!("plugin:{v}"))
    })
    .or_else(|| {
        first_metadata_str(
            &job.metadata,
            &[
                "/gateway",
                "/engine_gateway",
                "/metadata/gateway",
                "/metadata/engine_gateway",
            ],
        )
        .map(|v| format!("gateway:{v}"))
    })
    .unwrap_or_else(|| format!("{}:{}", job.source, job.category))
}

fn job_method_key(job: &JobRecord) -> String {
    first_metadata_str(
        &job.metadata,
        &[
            "/method",
            "/method_name",
            "/metadata/method",
            "/metadata/method_name",
        ],
    )
    .map(|method| format!("{}::{method}", job_owner_key(job)))
    .unwrap_or_else(|| format!("{}::<no-method>", job_owner_key(job)))
}

fn job_offender_key(job: &JobRecord) -> String {
    let owner = job_owner_key(job);
    if let Some(method) = first_metadata_str(
        &job.metadata,
        &[
            "/method",
            "/method_name",
            "/metadata/method",
            "/metadata/method_name",
        ],
    ) {
        format!("{owner}::{method}")
    } else {
        format!("{owner}::{}", job.name)
    }
}

fn first_metadata_str(value: &Value, paths: &[&str]) -> Option<String> {
    paths
        .iter()
        .filter_map(|path| value.pointer(path).and_then(Value::as_str))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
        .next()
}
