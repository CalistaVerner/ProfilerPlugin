use serde_json::Value;

use crate::util::format_json_scalar;

pub(super) fn csv_completed_jobs(report: &Value) -> String {
    let mut out = csv_header(&[
        "id",
        "status",
        "category",
        "source",
        "lane",
        "priority",
        "dependency_group",
        "frame_id",
        "task_domain",
        "task_pass",
        "executor",
        "name",
        "elapsed_ms",
        "budget_ms",
        "frame_budget_ms",
        "load",
        "load_percent",
        "over_budget",
        "exceeded_frame_budget",
        "gpu_wait_ms",
        "wait_reason",
        "scheduled",
        "blocked",
        "polling",
        "waited_on_gpu",
        "stayed_async",
        "async_mode",
        "started_unix_ms",
        "ended_unix_ms",
        "payload_bytes",
        "output_bytes",
        "service_id",
        "method",
        "plugin_id",
        "gateway",
        "error",
        "detail",
    ]);
    if let Some(jobs) = report.get("completed_jobs").and_then(Value::as_array) {
        for job in jobs {
            let load = f(job, "load");
            csv_push(
                &mut out,
                &[
                    s(job, "id"),
                    s(job, "status"),
                    s(job, "category"),
                    s(job, "source"),
                    direct_or_metadata(
                        job,
                        "lane",
                        &[
                            "/metadata/lane",
                            "/metadata/event/lane",
                            "/metadata/metadata/lane",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "priority",
                        &[
                            "/metadata/priority",
                            "/metadata/event/priority",
                            "/metadata/metadata/priority",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "dependency_group",
                        &[
                            "/metadata/dependency_group",
                            "/metadata/event/dependency_group",
                            "/metadata/metadata/dependency_group",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "frame_id",
                        &[
                            "/metadata/frame_id",
                            "/metadata/event/frame_id",
                            "/metadata/metadata/frame_id",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "task_domain",
                        &[
                            "/metadata/task_domain",
                            "/metadata/event/task_domain",
                            "/metadata/metadata/task_domain",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "task_pass",
                        &[
                            "/metadata/task_pass",
                            "/metadata/event/task_pass",
                            "/metadata/metadata/task_pass",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "executor",
                        &[
                            "/metadata/executor",
                            "/metadata/event/executor",
                            "/metadata/metadata/executor",
                        ],
                    ),
                    s(job, "name"),
                    f(job, "elapsed_ms"),
                    f(job, "budget_ms"),
                    f(job, "frame_budget_ms"),
                    load.clone(),
                    format!("{:.3}", load.parse::<f64>().unwrap_or(0.0) * 100.0),
                    (load.parse::<f64>().unwrap_or(0.0) >= 1.0).to_string(),
                    b(job, "exceeded_frame_budget"),
                    f(job, "gpu_wait_ms"),
                    s(job, "wait_reason"),
                    b(job, "scheduled"),
                    b(job, "blocked"),
                    b(job, "polling"),
                    b(job, "waited_on_gpu"),
                    b(job, "stayed_async"),
                    s(job, "async_mode"),
                    scalar(job.get("started_unix_ms")),
                    scalar(job.get("ended_unix_ms")),
                    scalar(job.get("payload_bytes")),
                    scalar(job.get("output_bytes")),
                    metadata_csv(
                        job,
                        &["/metadata/service_id", "/metadata/metadata/service_id"],
                    ),
                    metadata_csv(
                        job,
                        &[
                            "/metadata/method",
                            "/metadata/method_name",
                            "/metadata/metadata/method",
                        ],
                    ),
                    metadata_csv(
                        job,
                        &["/metadata/plugin_id", "/metadata/metadata/plugin_id"],
                    ),
                    metadata_csv(
                        job,
                        &[
                            "/metadata/gateway",
                            "/metadata/engine_gateway",
                            "/metadata/metadata/gateway",
                        ],
                    ),
                    s(job, "error"),
                    s(job, "detail"),
                ],
            );
        }
    }
    out
}

pub(super) fn csv_category_summary(report: &Value) -> String {
    let mut out = csv_header(&[
        "rank",
        "category",
        "count",
        "failed",
        "slow",
        "total_elapsed_ms",
        "total_share_percent",
        "average_elapsed_ms",
        "max_elapsed_ms",
    ]);
    if let Some(rows) = report
        .pointer("/analysis/by_category_ranked")
        .and_then(Value::as_array)
    {
        for (idx, row) in rows.iter().enumerate() {
            csv_push(
                &mut out,
                &[
                    rank(idx),
                    s(row, "category"),
                    u(row, "count"),
                    u(row, "failed"),
                    u(row, "slow"),
                    f(row, "total_elapsed_ms"),
                    f(row, "total_share_percent"),
                    f(row, "average_elapsed_ms"),
                    f(row, "max_elapsed_ms"),
                ],
            );
        }
    }
    out
}

pub(super) fn csv_source_summary(report: &Value) -> String {
    csv_aggregate(
        report
            .pointer("/analysis/by_source_ranked")
            .and_then(Value::as_array),
    )
}

pub(super) fn csv_top_offenders(report: &Value) -> String {
    csv_aggregate(
        report
            .pointer("/analysis/top_offenders_by_total_elapsed")
            .and_then(Value::as_array),
    )
}

pub(super) fn csv_methods(report: &Value) -> String {
    csv_aggregate(
        report
            .pointer("/analysis/by_method_ranked")
            .and_then(Value::as_array),
    )
}

pub(super) fn csv_lanes(report: &Value) -> String {
    csv_aggregate(
        report
            .pointer("/analysis/by_lane_ranked")
            .and_then(Value::as_array),
    )
}

pub(super) fn csv_profiler_first(report: &Value) -> String {
    let mut out = csv_header(&[
        "scheduled_jobs",
        "blocked_jobs",
        "blocked_share_percent",
        "polling_jobs",
        "gpu_wait_jobs",
        "gpu_wait_share_percent",
        "frame_budget_exceeded_jobs",
        "async_jobs",
        "async_share_percent",
    ]);
    let row = report
        .pointer("/analysis/profiler_first")
        .or_else(|| report.pointer("/summary/profiler_first"))
        .unwrap_or(&Value::Null);
    csv_push(
        &mut out,
        &[
            u(row, "scheduled_jobs"),
            u(row, "blocked_jobs"),
            f(row, "blocked_share_percent"),
            u(row, "polling_jobs"),
            u(row, "gpu_wait_jobs"),
            f(row, "gpu_wait_share_percent"),
            u(row, "frame_budget_exceeded_jobs"),
            u(row, "async_jobs"),
            f(row, "async_share_percent"),
        ],
    );
    out
}

pub(super) fn csv_frame_budget(report: &Value) -> String {
    let mut out = csv_header(&[
        "rank",
        "id",
        "frame_id",
        "status",
        "lane",
        "priority",
        "dependency_group",
        "category",
        "source",
        "name",
        "elapsed_ms",
        "frame_budget_ms",
        "over_frame_budget_ms",
        "gpu_wait_ms",
        "wait_reason",
        "stayed_async",
        "async_mode",
        "detail",
    ]);
    if let Some(jobs) = report
        .pointer("/analysis/frame_budget_violations")
        .and_then(Value::as_array)
    {
        for (idx, job) in jobs.iter().enumerate() {
            let elapsed = job.get("elapsed_ms").and_then(Value::as_f64).unwrap_or(0.0);
            let frame_budget = job
                .get("frame_budget_ms")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            csv_push(
                &mut out,
                &[
                    rank(idx),
                    s(job, "id"),
                    scalar(job.get("frame_id")),
                    s(job, "status"),
                    direct_or_metadata(job, "lane", &["/metadata/lane", "/metadata/event/lane"]),
                    direct_or_metadata(
                        job,
                        "priority",
                        &["/metadata/priority", "/metadata/event/priority"],
                    ),
                    direct_or_metadata(
                        job,
                        "dependency_group",
                        &[
                            "/metadata/dependency_group",
                            "/metadata/event/dependency_group",
                        ],
                    ),
                    s(job, "category"),
                    s(job, "source"),
                    s(job, "name"),
                    f(job, "elapsed_ms"),
                    f(job, "frame_budget_ms"),
                    format!("{:.6}", (elapsed - frame_budget).max(0.0)),
                    f(job, "gpu_wait_ms"),
                    s(job, "wait_reason"),
                    b(job, "stayed_async"),
                    s(job, "async_mode"),
                    s(job, "detail"),
                ],
            );
        }
    }
    out
}

pub(super) fn csv_budget_violations(report: &Value) -> String {
    let mut out = csv_header(&[
        "rank",
        "id",
        "status",
        "category",
        "source",
        "lane",
        "priority",
        "dependency_group",
        "frame_id",
        "task_domain",
        "task_pass",
        "executor",
        "name",
        "elapsed_ms",
        "budget_ms",
        "frame_budget_ms",
        "load",
        "load_percent",
        "exceeded_frame_budget",
        "gpu_wait_ms",
        "wait_reason",
        "stayed_async",
        "async_mode",
        "started_unix_ms",
        "ended_unix_ms",
        "service_id",
        "method",
        "plugin_id",
        "gateway",
        "error",
        "detail",
    ]);
    if let Some(jobs) = report
        .pointer("/analysis/budget_violations")
        .and_then(Value::as_array)
    {
        for (idx, job) in jobs.iter().enumerate() {
            let load = f(job, "load");
            csv_push(
                &mut out,
                &[
                    rank(idx),
                    s(job, "id"),
                    s(job, "status"),
                    s(job, "category"),
                    s(job, "source"),
                    direct_or_metadata(
                        job,
                        "lane",
                        &[
                            "/metadata/lane",
                            "/metadata/event/lane",
                            "/metadata/metadata/lane",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "priority",
                        &[
                            "/metadata/priority",
                            "/metadata/event/priority",
                            "/metadata/metadata/priority",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "dependency_group",
                        &[
                            "/metadata/dependency_group",
                            "/metadata/event/dependency_group",
                            "/metadata/metadata/dependency_group",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "frame_id",
                        &[
                            "/metadata/frame_id",
                            "/metadata/event/frame_id",
                            "/metadata/metadata/frame_id",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "task_domain",
                        &[
                            "/metadata/task_domain",
                            "/metadata/event/task_domain",
                            "/metadata/metadata/task_domain",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "task_pass",
                        &[
                            "/metadata/task_pass",
                            "/metadata/event/task_pass",
                            "/metadata/metadata/task_pass",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "executor",
                        &[
                            "/metadata/executor",
                            "/metadata/event/executor",
                            "/metadata/metadata/executor",
                        ],
                    ),
                    s(job, "name"),
                    f(job, "elapsed_ms"),
                    f(job, "budget_ms"),
                    f(job, "frame_budget_ms"),
                    load.clone(),
                    format!("{:.3}", load.parse::<f64>().unwrap_or(0.0) * 100.0),
                    b(job, "exceeded_frame_budget"),
                    f(job, "gpu_wait_ms"),
                    s(job, "wait_reason"),
                    b(job, "stayed_async"),
                    s(job, "async_mode"),
                    scalar(job.get("started_unix_ms")),
                    scalar(job.get("ended_unix_ms")),
                    metadata_csv(
                        job,
                        &["/metadata/service_id", "/metadata/metadata/service_id"],
                    ),
                    metadata_csv(
                        job,
                        &[
                            "/metadata/method",
                            "/metadata/method_name",
                            "/metadata/metadata/method",
                        ],
                    ),
                    metadata_csv(
                        job,
                        &["/metadata/plugin_id", "/metadata/metadata/plugin_id"],
                    ),
                    metadata_csv(
                        job,
                        &[
                            "/metadata/gateway",
                            "/metadata/engine_gateway",
                            "/metadata/metadata/gateway",
                        ],
                    ),
                    s(job, "error"),
                    s(job, "detail"),
                ],
            );
        }
    }
    out
}

pub(super) fn csv_aggregate(rows: Option<&Vec<Value>>) -> String {
    let mut out = csv_header(&[
        "rank",
        "key",
        "category",
        "source",
        "sample_name",
        "count",
        "failed",
        "slow",
        "total_elapsed_ms",
        "total_share_percent",
        "average_elapsed_ms",
        "max_elapsed_ms",
        "max_load",
        "total_payload_bytes",
        "total_output_bytes",
    ]);
    if let Some(rows) = rows {
        for (idx, row) in rows.iter().enumerate() {
            csv_push(
                &mut out,
                &[
                    rank(idx),
                    s(row, "key"),
                    s(row, "category"),
                    s(row, "source"),
                    s(row, "sample_name"),
                    u(row, "count"),
                    u(row, "failed"),
                    u(row, "slow"),
                    f(row, "total_elapsed_ms"),
                    f(row, "total_share_percent"),
                    f(row, "average_elapsed_ms"),
                    f(row, "max_elapsed_ms"),
                    f(row, "max_load"),
                    u(row, "total_payload_bytes"),
                    u(row, "total_output_bytes"),
                ],
            );
        }
    }
    out
}

pub(super) fn csv_active_jobs(report: &Value) -> String {
    let mut out = csv_header(&[
        "id",
        "status",
        "category",
        "source",
        "lane",
        "priority",
        "dependency_group",
        "frame_id",
        "task_domain",
        "task_pass",
        "executor",
        "name",
        "active_elapsed_ms",
        "budget_ms",
        "frame_budget_ms",
        "current_load",
        "current_load_percent",
        "current_over_budget",
        "progress",
        "blocked",
        "polling",
        "waited_on_gpu",
        "stayed_async",
        "gpu_wait_ms",
        "wait_reason",
        "async_mode",
        "started_unix_ms",
        "detail",
    ]);
    if let Some(jobs) = report.get("active_jobs").and_then(Value::as_array) {
        for job in jobs {
            let load = job
                .get("current_load")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            csv_push(
                &mut out,
                &[
                    s(job, "id"),
                    s(job, "status"),
                    s(job, "category"),
                    s(job, "source"),
                    direct_or_metadata(
                        job,
                        "lane",
                        &[
                            "/metadata/lane",
                            "/metadata/event/lane",
                            "/metadata/metadata/lane",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "priority",
                        &[
                            "/metadata/priority",
                            "/metadata/event/priority",
                            "/metadata/metadata/priority",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "dependency_group",
                        &[
                            "/metadata/dependency_group",
                            "/metadata/event/dependency_group",
                            "/metadata/metadata/dependency_group",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "frame_id",
                        &[
                            "/metadata/frame_id",
                            "/metadata/event/frame_id",
                            "/metadata/metadata/frame_id",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "task_domain",
                        &[
                            "/metadata/task_domain",
                            "/metadata/event/task_domain",
                            "/metadata/metadata/task_domain",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "task_pass",
                        &[
                            "/metadata/task_pass",
                            "/metadata/event/task_pass",
                            "/metadata/metadata/task_pass",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "executor",
                        &[
                            "/metadata/executor",
                            "/metadata/event/executor",
                            "/metadata/metadata/executor",
                        ],
                    ),
                    s(job, "name"),
                    f(job, "active_elapsed_ms"),
                    f(job, "budget_ms"),
                    f(job, "frame_budget_ms"),
                    format!("{load:.6}"),
                    format!("{:.3}", load * 100.0),
                    (load >= 1.0).to_string(),
                    f(job, "progress"),
                    b(job, "blocked"),
                    b(job, "polling"),
                    b(job, "waited_on_gpu"),
                    b(job, "stayed_async"),
                    f(job, "gpu_wait_ms"),
                    s(job, "wait_reason"),
                    s(job, "async_mode"),
                    scalar(job.get("started_unix_ms")),
                    s(job, "detail"),
                ],
            );
        }
    }
    out
}

pub(super) fn csv_diagnostics(report: &Value) -> String {
    let mut out = csv_header(&["at_unix_ms", "level", "code", "job_id", "message"]);
    if let Some(rows) = report.get("diagnostics").and_then(Value::as_array) {
        for row in rows {
            csv_push(
                &mut out,
                &[
                    scalar(row.get("at_unix_ms")),
                    s(row, "level"),
                    s(row, "code"),
                    s(row, "job_id"),
                    s(row, "message"),
                ],
            );
        }
    }
    out
}

pub(super) fn csv_timeline(report: &Value) -> String {
    let run_start = report
        .pointer("/run/started_unix_ms")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut out = csv_header(&[
        "id",
        "category",
        "source",
        "name",
        "status",
        "start_offset_ms",
        "end_offset_ms",
        "elapsed_ms",
        "budget_ms",
        "load",
        "frame_id",
        "dependency_group",
        "task_domain",
        "task_pass",
        "lane",
        "priority",
        "executor",
        "detail",
    ]);
    if let Some(jobs) = report.get("completed_jobs").and_then(Value::as_array) {
        for job in jobs {
            let start = job
                .get("started_unix_ms")
                .and_then(Value::as_u64)
                .unwrap_or(run_start)
                .saturating_sub(run_start);
            let end = job
                .get("ended_unix_ms")
                .and_then(Value::as_u64)
                .unwrap_or(run_start)
                .saturating_sub(run_start);
            csv_push(
                &mut out,
                &[
                    s(job, "id"),
                    s(job, "category"),
                    s(job, "source"),
                    s(job, "name"),
                    s(job, "status"),
                    start.to_string(),
                    end.to_string(),
                    f(job, "elapsed_ms"),
                    f(job, "budget_ms"),
                    f(job, "load"),
                    direct_or_metadata(
                        job,
                        "frame_id",
                        &[
                            "/metadata/frame_id",
                            "/metadata/event/frame_id",
                            "/metadata/metadata/frame_id",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "dependency_group",
                        &[
                            "/metadata/dependency_group",
                            "/metadata/event/dependency_group",
                            "/metadata/metadata/dependency_group",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "task_domain",
                        &[
                            "/metadata/task_domain",
                            "/metadata/event/task_domain",
                            "/metadata/metadata/task_domain",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "task_pass",
                        &[
                            "/metadata/task_pass",
                            "/metadata/event/task_pass",
                            "/metadata/metadata/task_pass",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "lane",
                        &[
                            "/metadata/lane",
                            "/metadata/event/lane",
                            "/metadata/metadata/lane",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "priority",
                        &[
                            "/metadata/priority",
                            "/metadata/event/priority",
                            "/metadata/metadata/priority",
                        ],
                    ),
                    direct_or_metadata(
                        job,
                        "executor",
                        &[
                            "/metadata/executor",
                            "/metadata/event/executor",
                            "/metadata/metadata/executor",
                        ],
                    ),
                    s(job, "detail"),
                ],
            );
        }
    }
    out
}

fn csv_header(cols: &[&str]) -> String {
    let mut out = String::with_capacity(cols.iter().map(|col| col.len() + 1).sum());
    for (index, column) in cols.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(column);
    }
    out.push('\n');
    out
}

fn csv_push(out: &mut String, cells: &[String]) {
    for (idx, cell) in cells.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&csv_escape(cell));
    }
    out.push('\n');
}

fn csv_escape(cell: &str) -> String {
    if cell.contains(',') || cell.contains('"') || cell.contains('\n') || cell.contains('\r') {
        format!("\"{}\"", cell.replace('"', "\"\""))
    } else {
        cell.to_owned()
    }
}

fn rank(idx: usize) -> String {
    (idx + 1).to_string()
}
fn s(row: &Value, key: &str) -> String {
    row.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}
fn u(row: &Value, key: &str) -> String {
    row.get(key)
        .and_then(Value::as_u64)
        .map(|v| v.to_string())
        .unwrap_or_default()
}
fn f(row: &Value, key: &str) -> String {
    row.get(key)
        .and_then(Value::as_f64)
        .map(|v| format!("{v:.6}"))
        .unwrap_or_default()
}
fn b(row: &Value, key: &str) -> String {
    row.get(key)
        .and_then(Value::as_bool)
        .unwrap_or(false)
        .to_string()
}

fn direct_or_metadata(row: &Value, key: &str, paths: &[&str]) -> String {
    row.get(key)
        .map(format_json_scalar)
        .filter(|value| !value.trim().is_empty() && value != "null")
        .unwrap_or_else(|| metadata_csv(row, paths))
}

fn scalar(value: Option<&Value>) -> String {
    value.map(format_json_scalar).unwrap_or_default()
}

fn metadata_csv(row: &Value, paths: &[&str]) -> String {
    paths
        .iter()
        .find_map(|path| row.pointer(path).map(format_json_scalar))
        .unwrap_or_default()
}
