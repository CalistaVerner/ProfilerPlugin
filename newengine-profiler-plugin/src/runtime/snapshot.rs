use super::*;

impl ProfilerRuntime {
    pub(crate) fn snapshot(&self) -> Value {
        let state = self.lock_state();
        self.snapshot_locked(&state)
    }

    pub(crate) fn diagnostics(&self) -> Value {
        let state = self.lock_state();
        self.diagnostics_locked(&state)
    }

    pub(crate) fn snapshot_locked(&self, state: &ProfilerState) -> Value {
        let active_jobs: Vec<&JobRecord> = state.active.values().map(|j| &j.record).collect();
        let recent_completed: Vec<&JobRecord> = state.completed.iter().rev().take(256).collect();
        let diagnostics: Vec<&ProfilerDiagnostic> =
            state.diagnostics.iter().rev().take(256).collect();
        json!({
            "schema": "newengine.profiler.snapshot.v1",
            "service_id": PROFILER_SERVICE_ID,
            "gateway": ENGINE_PROFILER_GATEWAY_ID,
            "enabled": self.cfg.enabled,
            "run_started_unix_ms": state.run_started_unix_ms,
            "uptime_ms": duration_ms(state.run_started.elapsed()),
            "events_seen": state.events_seen,
            "malformed_events": state.malformed_events,
            "active_count": state.active.len(),
            "completed_count": state.completed.len(),
            "reports_written": state.reports_written,
            "reports_in_progress": state.reports_in_progress,
            "reports_scheduled": state.reports_scheduled,
            "reports_failed": state.reports_failed,
            "recent_flush_requests": state.flush_requests.iter().rev().take(64).collect::<Vec<_>>(),
            "active_jobs": active_jobs,
            "recent_completed": recent_completed,
            "diagnostics": diagnostics,
            "last_report_paths": state.last_report_paths.clone(),
        })
    }

    pub(crate) fn diagnostics_locked(&self, state: &ProfilerState) -> Value {
        let stale: Vec<Value> = state
            .active
            .values()
            .filter_map(|job| {
                let elapsed_ms = duration_ms(job.started_at.elapsed());
                if elapsed_ms >= self.cfg.diagnostics.stale_active_job_ms {
                    Some(json!({
                        "id": job.record.id.clone(),
                        "name": job.record.name.clone(),
                        "category": job.record.category.clone(),
                        "elapsed_ms": elapsed_ms,
                        "budget_ms": job.record.budget_ms,
                        "load": elapsed_ms / job.record.budget_ms.max(0.001),
                    }))
                } else {
                    None
                }
            })
            .collect();

        let failed_jobs = state
            .completed
            .iter()
            .filter(|job| job.status == "failed")
            .count();
        let slow_or_over_budget_jobs = state
            .completed
            .iter()
            .filter(|job| {
                job.elapsed_ms.unwrap_or_default() >= self.cfg.diagnostics.slow_job_warn_ms
                    || job.load.unwrap_or_default() >= 1.0
            })
            .count();

        let status = if state.malformed_events > 0 || !stale.is_empty() || failed_jobs > 0 {
            "warn"
        } else {
            "ok"
        };

        json!({
            "schema": "newengine.profiler.diagnostics.v1",
            "status": status,
            "enabled": self.cfg.enabled,
            "active_jobs": state.active.len(),
            "completed_jobs_kept": state.completed.len(),
            "failed_jobs": failed_jobs,
            "slow_or_over_budget_jobs": slow_or_over_budget_jobs,
            "events_seen": state.events_seen,
            "malformed_events": state.malformed_events,
            "stale_active_jobs": stale,
            "report_directory": self.cfg.report.directory.clone(),
            "reports_written": state.reports_written,
            "reports_in_progress": state.reports_in_progress,
            "reports_scheduled": state.reports_scheduled,
            "reports_failed": state.reports_failed,
            "scheduling": self.cfg.scheduling.clone(),
            "recent_flush_requests": state.flush_requests.iter().rev().take(64).collect::<Vec<_>>(),
            "recent_diagnostics": state.diagnostics.iter().rev().take(512).collect::<Vec<_>>(),
        })
    }

    pub(crate) fn push_diag_locked(
        cfg: &ProfilerConfig,
        state: &mut ProfilerState,
        level: &str,
        code: &str,
        message: String,
        task_id: Option<String>,
    ) {
        state.diagnostics.push_back(ProfilerDiagnostic {
            level: level.to_owned(),
            code: code.to_owned(),
            message,
            job_id: task_id,
            at_unix_ms: unix_ms(),
        });
        while state.diagnostics.len() > cfg.diagnostics.max_recent_diagnostics {
            state.diagnostics.pop_front();
        }
    }
}
