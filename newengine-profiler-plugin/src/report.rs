use std::time::Instant;

use serde_json::{json, Value};

use crate::runtime::ProfilerRuntime;
use crate::util::duration_ms;

mod analysis;
mod csv;
mod markdown;
mod output;

impl ProfilerRuntime {
    pub(crate) fn flush_report(&self, reason: &str) -> Result<Value, String> {
        let shutdown_report = is_shutdown_report_reason(reason);
        let flush_started = Instant::now();

        let snapshot = {
            let mut state = self.lock_state();
            if shutdown_report && state.shutdown_report_written {
                return Ok(json!({
                    "schema": "newengine.profiler.flush_report.result.v1",
                    "reason": reason,
                    "paths": state.last_report_paths.clone(),
                    "json_bytes": 0,
                    "markdown_bytes": 0,
                    "csv_bytes": 0,
                    "skipped_duplicate_shutdown_report": true,
                    "lock_policy": "snapshot_only",
                }));
            }
            state.reports_in_progress = state.reports_in_progress.saturating_add(1);
            state.clone()
        };

        let report = self.build_report_from_state(&snapshot, reason);
        let markdown = self.build_markdown_report(&report);
        let json_len = serde_json::to_vec(&report).map(|v| v.len()).unwrap_or(0);
        let markdown_len = markdown.len();
        let write_result = self.write_report_files(&report, &markdown);
        let flush_elapsed_ms = duration_ms(flush_started.elapsed());

        match write_result {
            Ok((paths, csv_bytes)) => {
                let mut state = self.lock_state();
                state.reports_in_progress = state.reports_in_progress.saturating_sub(1);
                state.reports_written = state.reports_written.saturating_add(1);
                if shutdown_report {
                    state.shutdown_report_written = true;
                }
                state.last_report_paths = Some(paths.clone());
                Self::push_diag_locked(
                    &self.cfg,
                    &mut state,
                    "info",
                    "profiler_report_flushed",
                    format!(
                        "profiler report flushed reason='{}' elapsed_ms={:.3} policy='snapshot_then_write_outside_lock'",
                        reason, flush_elapsed_ms,
                    ),
                    None,
                );
                Ok(json!({
                    "schema": "newengine.profiler.flush_report.result.v1",
                    "reason": reason,
                    "paths": paths,
                    "json_bytes": json_len,
                    "markdown_bytes": markdown_len,
                    "csv_bytes": csv_bytes,
                    "flush_elapsed_ms": flush_elapsed_ms,
                    "skipped_duplicate_shutdown_report": false,
                    "lock_policy": "snapshot_then_build_and_write_outside_lock",
                }))
            }
            Err(e) => {
                let mut state = self.lock_state();
                state.reports_in_progress = state.reports_in_progress.saturating_sub(1);
                state.reports_failed = state.reports_failed.saturating_add(1);
                Self::push_diag_locked(
                    &self.cfg,
                    &mut state,
                    "error",
                    "profiler_report_flush_failed",
                    format!(
                        "profiler report flush failed reason='{}' elapsed_ms={:.3}: {}",
                        reason, flush_elapsed_ms, e
                    ),
                    None,
                );
                Err(e)
            }
        }
    }
}

fn is_shutdown_report_reason(reason: &str) -> bool {
    matches!(reason, "service.shutdown_v1" | "plugin.shutdown")
}
