use std::fmt::Write as _;

use serde_json::Value;

use crate::runtime::ProfilerRuntime;
use crate::util::escape_md;

use super::analysis::percent_of;

const MD_TOP_LIMIT: usize = 16;

impl ProfilerRuntime {
    pub(super) fn build_markdown_report(&self, report: &Value) -> String {
        let mut out = String::new();
        let summary = report.get("summary").unwrap_or(&Value::Null);
        let run = report.get("run").unwrap_or(&Value::Null);
        let analysis = report.get("analysis").unwrap_or(&Value::Null);

        let total_ms = summary
            .get("total_elapsed_ms")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let completed_count = summary
            .get("completed_jobs_kept")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let slow_count = summary
            .get("slow_or_over_budget_jobs")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let failed_count = summary
            .get("failed_jobs")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let active_count = summary
            .get("active_jobs")
            .and_then(Value::as_u64)
            .unwrap_or(0);

        let _ = writeln!(out, "# North Star Engine Profiler Report");
        let _ = writeln!(out);
        let _ = writeln!(out, "> [!INFO] INFO BLOCK — как читать отчёт");
        let _ = writeln!(out, "> **У нас сейчас:** отчёт показывает instrumented wall-clock time по job/service/plugin событиям. Главная строка для поиска виновника — `total_elapsed_ms` и `total_share_percent`; главная строка для бюджетов кадра — `load`, где `1.0` значит ровно бюджет, а `>1.0` значит перерасход.");
        let _ = writeln!(out, ">");
        let _ = writeln!(out, "> **Technical details (EN):** `load = elapsed_ms / budget_ms`; CSV files are emitted next to JSON/MD and duplicated in the timestamped ZIP archive when archive output is enabled.");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "- reason: `{}`",
            report
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
        let _ = writeln!(
            out,
            "- uptime_ms: `{:.3}`",
            run.get("uptime_ms").and_then(Value::as_f64).unwrap_or(0.0)
        );
        let _ = writeln!(
            out,
            "- events_seen: `{}`",
            run.get("events_seen").and_then(Value::as_u64).unwrap_or(0)
        );
        let _ = writeln!(
            out,
            "- malformed_events: `{}`",
            run.get("malformed_events")
                .and_then(Value::as_u64)
                .unwrap_or(0)
        );
        let _ = writeln!(out);

        let _ = writeln!(out, "## Quick answer — кто жрёт время");
        let _ = writeln!(out);
        if let Some(worst) = analysis.get("worst_offender").filter(|v| !v.is_null()) {
            let key = worst
                .get("key")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            let share = worst
                .get("total_share_percent")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let count = worst.get("count").and_then(Value::as_u64).unwrap_or(0);
            let max_load = worst.get("max_load").and_then(Value::as_f64).unwrap_or(0.0);
            let failed = worst.get("failed").and_then(Value::as_u64).unwrap_or(0);
            let slow = worst.get("slow").and_then(Value::as_u64).unwrap_or(0);
            let _ = writeln!(out, "**Worst offender:** `{}` — {:.3} ms total, {:.1}% of captured time, {} calls, max load {:.2}x, slow/over-budget {}, failed {}.",
                escape_md(key),
                worst.get("total_elapsed_ms").and_then(Value::as_f64).unwrap_or(0.0),
                share,
                count,
                max_load,
                slow,
                failed,
            );
            let _ = writeln!(out);
            let _ = writeln!(out, "```text");
            let _ = writeln!(
                out,
                "captured time share  [{}] {:>5.1}%",
                bar(share, 100.0, 32),
                share
            );
            let _ = writeln!(
                out,
                "max budget load      [{}] {:>5.2}x",
                bar(max_load.min(4.0), 4.0, 32),
                max_load
            );
            let _ = writeln!(out, "```");
        } else {
            let _ = writeln!(out, "No completed jobs were captured yet.");
        }
        let _ = writeln!(out);

        let _ = writeln!(out, "## Executive summary");
        let _ = writeln!(out);
        let _ = writeln!(out, "| Metric | Value | Meaning |");
        let _ = writeln!(out, "|---|---:|---|");
        let rows = [
            (
                "active_jobs",
                active_count.to_string(),
                "работа ещё не завершилась; если висит долго — смотреть `Active jobs`".to_owned(),
            ),
            (
                "completed_jobs_kept",
                completed_count.to_string(),
                "сколько завершённых записей осталось в ring buffer".to_owned(),
            ),
            (
                "failed_jobs",
                failed_count.to_string(),
                "ошибки, которые надо читать вместе с diagnostics".to_owned(),
            ),
            (
                "slow_or_over_budget_jobs",
                slow_count.to_string(),
                "slow threshold или `load >= 1.0`".to_owned(),
            ),
            (
                "total_elapsed_ms",
                format!("{total_ms:.3}"),
                "сумма captured wall-clock времени по завершённым jobs".to_owned(),
            ),
            (
                "average_elapsed_ms",
                format!(
                    "{:.3}",
                    summary
                        .get("average_elapsed_ms")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0)
                ),
                "среднее время одной завершённой job".to_owned(),
            ),
            (
                "max_elapsed_ms",
                format!(
                    "{:.3}",
                    summary
                        .get("max_elapsed_ms")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0)
                ),
                "самая дорогая одиночная job".to_owned(),
            ),
        ];
        for (metric, value, meaning) in rows {
            let _ = writeln!(out, "| `{metric}` | `{}` | {} |", value, meaning);
        }
        let _ = writeln!(out);

        let profiler_first = analysis
            .get("profiler_first")
            .or_else(|| summary.get("profiler_first"))
            .unwrap_or(&Value::Null);
        let _ = writeln!(out, "## Profiler-first telemetry view");
        let _ = writeln!(out);
        let _ = writeln!(out, "| Question | Count | Share | Meaning |");
        let _ = writeln!(out, "|---|---:|---:|---|");
        let profiler_rows = [
            (
                "what was scheduled",
                "scheduled_jobs",
                "",
                "jobs that entered the visible scheduling path",
            ),
            (
                "what was blocked",
                "blocked_jobs",
                "blocked_share_percent",
                "jobs that reported blocked/waiting/dependency/residency/barrier state",
            ),
            (
                "what was polling",
                "polling_jobs",
                "",
                "jobs/status events that stayed in a poll/ticket loop",
            ),
            (
                "what waited on GPU",
                "gpu_wait_jobs",
                "gpu_wait_share_percent",
                "jobs with gpu_wait_ms or GPU/fence/present/upload wait reason",
            ),
            (
                "what exceeded frame budget",
                "frame_budget_exceeded_jobs",
                "",
                "jobs where elapsed_ms exceeded explicit frame_budget_ms",
            ),
            (
                "what stayed async",
                "async_jobs",
                "async_share_percent",
                "jobs tagged as async/ticket/engine.threading/render-prep/streaming work",
            ),
        ];
        for (question, count_key, share_key, meaning) in profiler_rows {
            let count = profiler_first
                .get(count_key)
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let share = if share_key.is_empty() {
                None
            } else {
                profiler_first.get(share_key).and_then(Value::as_f64)
            };
            let share_text = share
                .map(|v| format!("{v:.1}%"))
                .unwrap_or_else(|| "-".to_owned());
            let _ = writeln!(
                out,
                "| `{question}` | `{count}` | `{share_text}` | {meaning} |"
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "> [!NOTE] REQUEST NOTE — profiler-first culture");
        let _ = writeln!(out, "> **У нас сейчас:** report теперь отделяет timing от scheduling/waiting/async facts, чтобы не путать `ms` с причиной тормоза.");
        let _ = writeln!(out, "> **Было бы здорово:** every heavy lane should emit `lane`, `priority`, `dependency_group`, `frame_id`, `frame_budget_ms`, `gpu_wait_ms`, `wait_reason`, and `async_mode` when known.");
        let _ = writeln!(out, "> **Technical details (EN):** StarProfiler report schema is `newengine.profiler.report.v3`; CSV consumers can read `profiler_first_latest.csv`, `profiler_lanes_latest.csv`, and `profiler_frame_budget_latest.csv`.");
        let _ = writeln!(out);

        let _ = writeln!(out, "## Flush and scheduling policy");
        let _ = writeln!(out);
        let scheduler = report.get("scheduler").unwrap_or(&Value::Null);
        let _ = writeln!(out, "| Setting | Value |");
        let _ = writeln!(out, "|---|---|");
        let _ = writeln!(
            out,
            "| `service_flush_mode` | `{}` |",
            scheduler
                .get("service_flush_mode")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
        let _ = writeln!(
            out,
            "| `shutdown_flush_mode` | `{}` |",
            scheduler
                .get("shutdown_flush_mode")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
        let _ = writeln!(
            out,
            "| `prefer_engine_threading` | `{}` |",
            scheduler
                .get("prefer_engine_threading")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        );
        let _ = writeln!(
            out,
            "| `require_engine_threading` | `{}` |",
            scheduler
                .get("require_engine_threading")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        );
        let _ = writeln!(
            out,
            "| `lock_policy` | `{}` |",
            scheduler
                .get("lock_policy")
                .and_then(Value::as_str)
                .unwrap_or("snapshot_then_build_and_write_outside_lock")
        );
        let _ = writeln!(out);
        let _ = writeln!(out, "> [!NOTE] REQUEST NOTE — profiler safety");
        let _ = writeln!(out, "> **У нас сейчас:** heavy report build/write is outside the runtime state lock; async flush is routed through `engine.threading` by default.");
        let _ = writeln!(out, "> **Было бы здорово:** keep every future heavy profiler export as a visible job/task, never as an invisible background load.");
        let _ = writeln!(out, "> **Technical details (EN):** `profiler.flush_report_v1` uses configured service flush mode; `profiler.flush_report_sync_v1` is the explicit synchronous worker entrypoint for `engine.threading` and shutdown-final flush.");
        let _ = writeln!(out);

        let elapsed_p = summary
            .get("elapsed_percentiles_ms")
            .unwrap_or(&Value::Null);
        let load_p = summary.get("load_percentiles").unwrap_or(&Value::Null);
        let _ = writeln!(out, "## Percentiles — latency and budget load");
        let _ = writeln!(out);
        let _ = writeln!(out, "| Metric | p50 | p90 | p95 | p99 |");
        let _ = writeln!(out, "|---|---:|---:|---:|---:|");
        let _ = writeln!(
            out,
            "| `elapsed_ms` | {:.3} | {:.3} | {:.3} | {:.3} |",
            elapsed_p.get("p50").and_then(Value::as_f64).unwrap_or(0.0),
            elapsed_p.get("p90").and_then(Value::as_f64).unwrap_or(0.0),
            elapsed_p.get("p95").and_then(Value::as_f64).unwrap_or(0.0),
            elapsed_p.get("p99").and_then(Value::as_f64).unwrap_or(0.0),
        );
        let _ = writeln!(
            out,
            "| `load` | {:.2}x | {:.2}x | {:.2}x | {:.2}x |",
            load_p.get("p50").and_then(Value::as_f64).unwrap_or(0.0),
            load_p.get("p90").and_then(Value::as_f64).unwrap_or(0.0),
            load_p.get("p95").and_then(Value::as_f64).unwrap_or(0.0),
            load_p.get("p99").and_then(Value::as_f64).unwrap_or(0.0),
        );
        let _ = writeln!(out);

        write_ranked_chart(
            &mut out,
            "## Load chart — категории по суммарному времени",
            analysis.get("by_category_ranked").and_then(Value::as_array),
            "category",
            total_ms,
        );
        write_ranked_chart(
            &mut out,
            "## Load chart — top offenders",
            analysis
                .get("top_offenders_by_total_elapsed")
                .and_then(Value::as_array),
            "key",
            total_ms,
        );
        write_ranked_chart(
            &mut out,
            "## Load chart — lanes",
            analysis.get("by_lane_ranked").and_then(Value::as_array),
            "key",
            total_ms,
        );

        let _ = writeln!(out, "## Top offenders by total elapsed time");
        let _ = writeln!(out);
        let _ = writeln!(out, "| Rank | Offender | Source | Category | Calls | Total ms | Share | Avg ms | Max ms | Max load | Slow | Failed |");
        let _ = writeln!(
            out,
            "|---:|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|"
        );
        if let Some(items) = analysis
            .get("top_offenders_by_total_elapsed")
            .and_then(Value::as_array)
        {
            for (idx, item) in items.iter().take(MD_TOP_LIMIT).enumerate() {
                let _ = writeln!(out,
                    "| {} | `{}` | `{}` | `{}` | {} | {:.3} | {:.1}% | {:.3} | {:.3} | {:.2}x | {} | {} |",
                    idx + 1,
                    escape_md(item.get("key").and_then(Value::as_str).unwrap_or("-")),
                    escape_md(item.get("source").and_then(Value::as_str).unwrap_or("-")),
                    escape_md(item.get("category").and_then(Value::as_str).unwrap_or("-")),
                    item.get("count").and_then(Value::as_u64).unwrap_or(0),
                    item.get("total_elapsed_ms").and_then(Value::as_f64).unwrap_or(0.0),
                    item.get("total_share_percent").and_then(Value::as_f64).unwrap_or(0.0),
                    item.get("average_elapsed_ms").and_then(Value::as_f64).unwrap_or(0.0),
                    item.get("max_elapsed_ms").and_then(Value::as_f64).unwrap_or(0.0),
                    item.get("max_load").and_then(Value::as_f64).unwrap_or(0.0),
                    item.get("slow").and_then(Value::as_u64).unwrap_or(0),
                    item.get("failed").and_then(Value::as_u64).unwrap_or(0),
                );
            }
        }
        let _ = writeln!(out);

        let _ = writeln!(out, "## Top methods by total elapsed time");
        let _ = writeln!(out);
        let _ = writeln!(out, "| Rank | Method | Source | Category | Calls | Total ms | Share | Avg ms | Max ms | Max load | Slow | Failed |");
        let _ = writeln!(
            out,
            "|---:|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|"
        );
        if let Some(items) = analysis.get("by_method_ranked").and_then(Value::as_array) {
            for (idx, item) in items.iter().take(MD_TOP_LIMIT).enumerate() {
                let _ = writeln!(out,
                    "| {} | `{}` | `{}` | `{}` | {} | {:.3} | {:.1}% | {:.3} | {:.3} | {:.2}x | {} | {} |",
                    idx + 1,
                    escape_md(item.get("key").and_then(Value::as_str).unwrap_or("-")),
                    escape_md(item.get("source").and_then(Value::as_str).unwrap_or("-")),
                    escape_md(item.get("category").and_then(Value::as_str).unwrap_or("-")),
                    item.get("count").and_then(Value::as_u64).unwrap_or(0),
                    item.get("total_elapsed_ms").and_then(Value::as_f64).unwrap_or(0.0),
                    item.get("total_share_percent").and_then(Value::as_f64).unwrap_or(0.0),
                    item.get("average_elapsed_ms").and_then(Value::as_f64).unwrap_or(0.0),
                    item.get("max_elapsed_ms").and_then(Value::as_f64).unwrap_or(0.0),
                    item.get("max_load").and_then(Value::as_f64).unwrap_or(0.0),
                    item.get("slow").and_then(Value::as_u64).unwrap_or(0),
                    item.get("failed").and_then(Value::as_u64).unwrap_or(0),
                );
            }
        }
        let _ = writeln!(out);

        let _ = writeln!(out, "## Budget violations — что пробило кадр/лимит");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Rank | Status | Category | Source | Name | Elapsed ms | Budget ms | Load | Detail |"
        );
        let _ = writeln!(out, "|---:|---|---|---|---|---:|---:|---:|---|");
        if let Some(jobs) = analysis.get("budget_violations").and_then(Value::as_array) {
            for (idx, job) in jobs.iter().take(MD_TOP_LIMIT).enumerate() {
                write_job_row(&mut out, idx + 1, job);
            }
        }
        let _ = writeln!(out);

        let _ = writeln!(
            out,
            "## Frame budget violations — explicit frame envelope misses"
        );
        let _ = writeln!(out);
        let _ = writeln!(out, "| Rank | Frame | Lane | Category | Source | Name | Elapsed ms | Frame budget ms | Over ms | GPU wait ms | Wait reason | Async | Detail |");
        let _ = writeln!(
            out,
            "|---:|---:|---|---|---|---|---:|---:|---:|---:|---|---|---|"
        );
        if let Some(jobs) = analysis
            .get("frame_budget_violations")
            .and_then(Value::as_array)
        {
            for (idx, job) in jobs.iter().take(MD_TOP_LIMIT).enumerate() {
                let elapsed = job.get("elapsed_ms").and_then(Value::as_f64).unwrap_or(0.0);
                let frame_budget = job
                    .get("frame_budget_ms")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                let over = (elapsed - frame_budget).max(0.0);
                let _ = writeln!(out,
                    "| {} | {} | `{}` | `{}` | `{}` | `{}` | {:.3} | {:.3} | {:.3} | {:.3} | {} | `{}` | {} |",
                    idx + 1,
                    job.get("frame_id").and_then(Value::as_u64).map(|v| v.to_string()).unwrap_or_else(|| "-".to_owned()),
                    escape_md(job.get("lane").and_then(Value::as_str).unwrap_or("-")),
                    escape_md(job.get("category").and_then(Value::as_str).unwrap_or("-")),
                    escape_md(job.get("source").and_then(Value::as_str).unwrap_or("-")),
                    escape_md(job.get("name").and_then(Value::as_str).unwrap_or("-")),
                    elapsed,
                    frame_budget,
                    over,
                    job.get("gpu_wait_ms").and_then(Value::as_f64).unwrap_or(0.0),
                    escape_md(job.get("wait_reason").and_then(Value::as_str).unwrap_or("")),
                    job.get("stayed_async").and_then(Value::as_bool).unwrap_or(false),
                    escape_md(job.get("detail").and_then(Value::as_str).unwrap_or("")),
                );
            }
        }
        let _ = writeln!(out);

        let _ = writeln!(out, "## Top single jobs by elapsed time");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Rank | Status | Category | Source | Name | Elapsed ms | Budget ms | Load | Detail |"
        );
        let _ = writeln!(out, "|---:|---|---|---|---|---:|---:|---:|---|");
        if let Some(jobs) = analysis
            .get("top_completed_jobs_by_elapsed")
            .and_then(Value::as_array)
        {
            for (idx, job) in jobs.iter().take(MD_TOP_LIMIT).enumerate() {
                write_job_row(&mut out, idx + 1, job);
            }
        }
        let _ = writeln!(out);

        let _ = writeln!(out, "## Top single jobs by budget load");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Rank | Status | Category | Source | Name | Elapsed ms | Budget ms | Load | Detail |"
        );
        let _ = writeln!(out, "|---:|---|---|---|---|---:|---:|---:|---|");
        if let Some(jobs) = analysis
            .get("top_completed_jobs_by_load")
            .and_then(Value::as_array)
        {
            for (idx, job) in jobs.iter().take(MD_TOP_LIMIT).enumerate() {
                write_job_row(&mut out, idx + 1, job);
            }
        }
        let _ = writeln!(out);

        let _ = writeln!(out, "## By category");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Category | Count | Failed | Slow | Total ms | Share | Avg ms | Max ms |"
        );
        let _ = writeln!(out, "|---|---:|---:|---:|---:|---:|---:|---:|");
        if let Some(cats) = analysis.get("by_category_ranked").and_then(Value::as_array) {
            for cat in cats {
                let _ = writeln!(
                    out,
                    "| `{}` | {} | {} | {} | {:.3} | {:.1}% | {:.3} | {:.3} |",
                    escape_md(cat.get("category").and_then(Value::as_str).unwrap_or("-")),
                    cat.get("count").and_then(Value::as_u64).unwrap_or(0),
                    cat.get("failed").and_then(Value::as_u64).unwrap_or(0),
                    cat.get("slow").and_then(Value::as_u64).unwrap_or(0),
                    cat.get("total_elapsed_ms")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                    cat.get("total_share_percent")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                    cat.get("average_elapsed_ms")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                    cat.get("max_elapsed_ms")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                );
            }
        }
        let _ = writeln!(out);

        let _ = writeln!(out, "## By source");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Source | Calls | Total ms | Share | Avg ms | Max ms | Max load | Slow | Failed |"
        );
        let _ = writeln!(out, "|---|---:|---:|---:|---:|---:|---:|---:|---:|");
        if let Some(items) = analysis.get("by_source_ranked").and_then(Value::as_array) {
            for item in items.iter().take(MD_TOP_LIMIT) {
                let _ = writeln!(
                    out,
                    "| `{}` | {} | {:.3} | {:.1}% | {:.3} | {:.3} | {:.2}x | {} | {} |",
                    escape_md(item.get("key").and_then(Value::as_str).unwrap_or("-")),
                    item.get("count").and_then(Value::as_u64).unwrap_or(0),
                    item.get("total_elapsed_ms")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                    item.get("total_share_percent")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                    item.get("average_elapsed_ms")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                    item.get("max_elapsed_ms")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                    item.get("max_load").and_then(Value::as_f64).unwrap_or(0.0),
                    item.get("slow").and_then(Value::as_u64).unwrap_or(0),
                    item.get("failed").and_then(Value::as_u64).unwrap_or(0),
                );
            }
        }
        let _ = writeln!(out);

        let _ = writeln!(out, "## Active jobs");
        let _ = writeln!(out);
        if let Some(active) = report.get("active_jobs").and_then(Value::as_array) {
            if active.is_empty() {
                let _ = writeln!(out, "No active jobs at report flush time.");
            } else {
                let _ = writeln!(out, "| Status | Category | Source | Name | Active ms | Budget ms | Current load | Progress | Detail |");
                let _ = writeln!(out, "|---|---|---|---|---:|---:|---:|---:|---|");
                for job in active.iter().take(MD_TOP_LIMIT) {
                    let _ = writeln!(
                        out,
                        "| `{}` | `{}` | `{}` | `{}` | {:.3} | {:.3} | {:.2}x | {:.1}% | {} |",
                        job.get("status").and_then(Value::as_str).unwrap_or("-"),
                        job.get("category").and_then(Value::as_str).unwrap_or("-"),
                        job.get("source").and_then(Value::as_str).unwrap_or("-"),
                        escape_md(job.get("name").and_then(Value::as_str).unwrap_or("-")),
                        job.get("active_elapsed_ms")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0),
                        job.get("budget_ms").and_then(Value::as_f64).unwrap_or(0.0),
                        job.get("current_load")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0),
                        job.get("progress").and_then(Value::as_f64).unwrap_or(0.0) * 100.0,
                        escape_md(job.get("detail").and_then(Value::as_str).unwrap_or("")),
                    );
                }
            }
        }
        let _ = writeln!(out);

        let _ = writeln!(out, "## CSV outputs");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "When enabled, the profiler writes these machine-readable tables:"
        );
        let _ = writeln!(out);
        let _ = writeln!(out, "| CSV | Purpose |");
        let _ = writeln!(out, "|---|---|");
        let _ = writeln!(
            out,
            "| `profiler_jobs_latest.csv` | all completed jobs with elapsed/budget/load columns |"
        );
        let _ = writeln!(out, "| `profiler_top_offenders_latest.csv` | grouped suspects sorted by total captured time |");
        let _ = writeln!(
            out,
            "| `profiler_categories_latest.csv` | category totals and share-of-time |"
        );
        let _ = writeln!(
            out,
            "| `profiler_sources_latest.csv` | source totals and share-of-time |"
        );
        let _ = writeln!(out, "| `profiler_active_jobs_latest.csv` | jobs still running at flush time with current load |");
        let _ = writeln!(out, "| `profiler_timeline_latest.csv` | completed jobs with run-relative start/end offsets |");
        let _ = writeln!(
            out,
            "| `profiler_methods_latest.csv` | method/service grouped timing totals |"
        );
        let _ = writeln!(out, "| `profiler_budget_violations_latest.csv` | jobs where `load >= 1.0` or slow threshold was crossed |");
        let _ = writeln!(out, "| `profiler_first_latest.csv` | scheduled/blocked/polling/GPU-wait/frame-budget/async counters |");
        let _ = writeln!(
            out,
            "| `profiler_lanes_latest.csv` | lane totals and share-of-time |"
        );
        let _ = writeln!(out, "| `profiler_frame_budget_latest.csv` | explicit frame-budget misses with lane/frame/wait fields |");
        let _ = writeln!(
            out,
            "| `profiler_diagnostics_latest.csv` | warnings/errors emitted by profiler analysis |"
        );
        let _ = writeln!(out);

        let _ = writeln!(out, "## Diagnostics");
        let _ = writeln!(out);
        if let Some(diags) = report.get("diagnostics").and_then(Value::as_array) {
            if diags.is_empty() {
                let _ = writeln!(out, "No diagnostics recorded.");
            } else {
                for d in diags.iter().rev().take(128) {
                    let _ = writeln!(
                        out,
                        "- `{}` `{}`: {}",
                        d.get("level").and_then(Value::as_str).unwrap_or("info"),
                        d.get("code")
                            .and_then(Value::as_str)
                            .unwrap_or("diagnostic"),
                        d.get("message").and_then(Value::as_str).unwrap_or("")
                    );
                }
            }
        }
        out
    }
}

fn bar(value: f64, max: f64, width: usize) -> String {
    let ratio = if max > 0.0 {
        (value / max).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = (ratio * width as f64).round() as usize;
    format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(width.saturating_sub(filled))
    )
}

fn write_ranked_chart(
    out: &mut String,
    title: &str,
    rows: Option<&Vec<Value>>,
    label_key: &str,
    total_ms: f64,
) {
    let _ = writeln!(out, "{title}");
    let _ = writeln!(out);
    let Some(rows) = rows else {
        let _ = writeln!(out, "No data.");
        let _ = writeln!(out);
        return;
    };
    if rows.is_empty() {
        let _ = writeln!(out, "No data.");
        let _ = writeln!(out);
        return;
    }
    let _ = writeln!(out, "```text");
    for row in rows.iter().take(10) {
        let label = row.get(label_key).and_then(Value::as_str).unwrap_or("-");
        let elapsed = row
            .get("total_elapsed_ms")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let share = row
            .get("total_share_percent")
            .and_then(Value::as_f64)
            .unwrap_or_else(|| percent_of(elapsed, total_ms));
        let short = shorten(label, 34);
        let _ = writeln!(
            out,
            "{short:<34} [{}] {:>7.3} ms {:>5.1}%",
            bar(share, 100.0, 28),
            elapsed,
            share
        );
    }
    let _ = writeln!(out, "```");
    let _ = writeln!(out);
}

fn write_job_row(out: &mut String, rank: usize, job: &Value) {
    let _ = writeln!(
        out,
        "| {} | `{}` | `{}` | `{}` | `{}` | {:.3} | {:.3} | {:.2}x | {} |",
        rank,
        job.get("status").and_then(Value::as_str).unwrap_or("-"),
        job.get("category").and_then(Value::as_str).unwrap_or("-"),
        job.get("source").and_then(Value::as_str).unwrap_or("-"),
        escape_md(job.get("name").and_then(Value::as_str).unwrap_or("-")),
        job.get("elapsed_ms").and_then(Value::as_f64).unwrap_or(0.0),
        job.get("budget_ms").and_then(Value::as_f64).unwrap_or(0.0),
        job.get("load").and_then(Value::as_f64).unwrap_or(0.0),
        escape_md(job.get("detail").and_then(Value::as_str).unwrap_or("")),
    );
}

fn shorten(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in s.chars().enumerate() {
        if idx >= max_chars.saturating_sub(1) {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}
