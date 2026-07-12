use super::*;

impl ProfilerRuntime {
    pub(crate) fn flush_report_service(&self, reason: &str) -> Result<Value, String> {
        if self
            .cfg
            .scheduling
            .service_flush_mode
            .eq_ignore_ascii_case("sync")
        {
            self.flush_report(reason)
        } else {
            self.flush_report_async(reason)
        }
    }

    fn flush_request_record(
        request_id: &str,
        job_id: &str,
        reason: &str,
        scheduling_mode: impl Into<String>,
        status: &str,
        requested_unix_ms: u128,
        completed_unix_ms: Option<u128>,
        engine_jobs_response: Option<Value>,
        error: Option<String>,
    ) -> FlushRequestRecord {
        FlushRequestRecord {
            request_id: request_id.to_owned(),
            job_id: job_id.to_owned(),
            reason: reason.to_owned(),
            scheduling_mode: scheduling_mode.into(),
            status: status.to_owned(),
            requested_unix_ms,
            completed_unix_ms,
            engine_jobs_response,
            error,
        }
    }

    pub(crate) fn flush_report_async(&self, reason: &str) -> Result<Value, String> {
        let (request_id, job_id, requested_unix_ms) = {
            let mut state = self.lock_state();
            let request_id = state.local_id();
            let job_id = format!("{request_id}.engine-jobs-flush");
            (request_id, job_id, unix_ms())
        };

        let scheduling_mode = format!("{ENGINE_THREADING_GATEWAY_ID}/{TASK_INVOKE_SERVICE_V1}");
        let job_reason = format!("{reason}.engine_threading");
        let job_request = json!({
            "schema": "newengine.threading.service_call.request.v1",
            "task_id": job_id.clone(),
            "name": "North Star Profiler report flush",
            "owner": PROFILER_SERVICE_ID,
            "category": "profiler.report.flush",
            "lane": "plugin",
            "priority": "background",
            "can_pause": false,
            "can_cancel": true,
            "target": {
                "gateway": ENGINE_PROFILER_GATEWAY_ID,
                "method": METHOD_FLUSH_REPORT_SYNC_V1,
                "payload_json": {
                    "schema": "newengine.profiler.flush_report.request.v1",
                    "reason": job_reason,
                    "request_id": request_id.clone()
                }
            }
        });

        self.record_flush_request(Self::flush_request_record(
            &request_id,
            &job_id,
            reason,
            scheduling_mode.clone(),
            "scheduling",
            requested_unix_ms,
            None,
            None,
            None,
        ));

        if self.cfg.scheduling.prefer_engine_jobs {
            if let Some(scheduler) = self.scheduler {
                match scheduler.invoke_service_job(job_request.clone()) {
                    Ok(response) => {
                        let accepted = response
                            .get("accepted")
                            .and_then(Value::as_bool)
                            .unwrap_or(true);
                        if accepted {
                            self.record_flush_request(Self::flush_request_record(
                                &request_id,
                                &job_id,
                                reason,
                                scheduling_mode.clone(),
                                "scheduled",
                                requested_unix_ms,
                                None,
                                Some(response.clone()),
                                None,
                            ));
                        } else {
                            let error = response
                                .get("detail")
                                .and_then(Value::as_str)
                                .unwrap_or("engine.threading rejected profiler service-call job")
                                .to_owned();
                            self.record_flush_request(Self::flush_request_record(
                                &request_id,
                                &job_id,
                                reason,
                                scheduling_mode.clone(),
                                "rejected",
                                requested_unix_ms,
                                Some(unix_ms()),
                                Some(response.clone()),
                                Some(error),
                            ));
                        }
                        return Ok(json!({
                            "schema": "newengine.profiler.flush_report.async_result.v1",
                            "accepted": accepted,
                            "mode": "engine_threading",
                            "engine_threading_gateway": ENGINE_THREADING_GATEWAY_ID,
                            "engine_task_method": TASK_INVOKE_SERVICE_V1,
                            "request_id": request_id,
                            "job_id": job_id,
                            "response": response,
                        }));
                    }
                    Err(e) => {
                        self.record_flush_request(Self::flush_request_record(
                            &request_id,
                            &job_id,
                            reason,
                            scheduling_mode.clone(),
                            "rejected",
                            requested_unix_ms,
                            Some(unix_ms()),
                            None,
                            Some(e.clone()),
                        ));
                        return Ok(json!({
                            "schema": "newengine.profiler.flush_report.async_result.v1",
                            "accepted": false,
                            "mode": "engine_threading_required",
                            "engine_threading_gateway": ENGINE_THREADING_GATEWAY_ID,
                            "engine_task_method": TASK_INVOKE_SERVICE_V1,
                            "request_id": request_id,
                            "job_id": job_id,
                            "error": e,
                        }));
                    }
                }
            } else if self.cfg.scheduling.require_engine_jobs {
                let error = "engine.threading scheduler is unavailable; profiler-owned background fallback is not allowed".to_owned();
                self.record_flush_request(Self::flush_request_record(
                    &request_id,
                    &job_id,
                    reason,
                    "engine_threading_unavailable",
                    "rejected",
                    requested_unix_ms,
                    Some(unix_ms()),
                    None,
                    Some(error.clone()),
                ));
                return Ok(json!({
                    "schema": "newengine.profiler.flush_report.async_result.v1",
                    "accepted": false,
                    "mode": "engine_threading_required",
                    "engine_threading_gateway": ENGINE_THREADING_GATEWAY_ID,
                    "engine_task_method": TASK_INVOKE_SERVICE_V1,
                    "request_id": request_id,
                    "job_id": job_id,
                    "error": error,
                }));
            }
        }

        let error = "async profiler flush requires engine.threading; no profiler-owned background fallback is allowed".to_owned();
        self.record_flush_request(Self::flush_request_record(
            &request_id,
            &job_id,
            reason,
            "engine_threading_required",
            "rejected",
            requested_unix_ms,
            Some(unix_ms()),
            None,
            Some(error.clone()),
        ));
        Ok(json!({
            "schema": "newengine.profiler.flush_report.async_result.v1",
            "accepted": false,
            "mode": "engine_threading_required",
            "engine_threading_gateway": ENGINE_THREADING_GATEWAY_ID,
            "engine_task_method": TASK_INVOKE_SERVICE_V1,
            "request_id": request_id,
            "job_id": job_id,
            "error": error,
        }))
    }

    pub(crate) fn flush_status(&self) -> Value {
        let state = self.lock_state();
        json!({
            "schema": "newengine.profiler.flush_status.v1",
            "reports_written": state.reports_written,
            "reports_in_progress": state.reports_in_progress,
            "reports_scheduled": state.reports_scheduled,
            "reports_failed": state.reports_failed,
            "last_report_paths": state.last_report_paths.clone(),
            "recent_flush_requests": state.flush_requests.iter().rev().take(128).collect::<Vec<_>>(),
            "scheduling": self.cfg.scheduling.clone(),
        })
    }

    fn record_flush_request(&self, mut record: FlushRequestRecord) {
        let mut state = self.lock_state();
        let status = record.status.clone();
        let diagnostic = record.error.as_ref().map(|error| {
            (
                if status == "rejected" {
                    "warn"
                } else {
                    "error"
                },
                format!(
                    "profiler report flush request '{}' status='{}': {}",
                    record.request_id, status, error
                ),
                record.job_id.clone(),
            )
        });

        let mut should_count_status = true;
        if let Some(existing) = state
            .flush_requests
            .iter_mut()
            .rev()
            .find(|item| item.request_id == record.request_id)
        {
            // A fast engine.threading worker may finish before the scheduling call returns.
            // In that case, keep the terminal state and only attach the scheduler response.
            if matches!(existing.status.as_str(), "completed" | "failed") && status == "scheduled" {
                if existing.engine_jobs_response.is_none() {
                    existing.engine_jobs_response = record.engine_jobs_response.take();
                }
                existing.scheduling_mode = std::mem::take(&mut record.scheduling_mode);
                should_count_status = false;
            } else {
                should_count_status = existing.status != status;
                *existing = record;
            }
        } else {
            state.flush_requests.push_back(record);
            while state.flush_requests.len() > 256 {
                state.flush_requests.pop_front();
            }
        }

        if should_count_status {
            match status.as_str() {
                "scheduled" => state.reports_scheduled = state.reports_scheduled.saturating_add(1),
                "failed" | "rejected" => {
                    state.reports_failed = state.reports_failed.saturating_add(1)
                }
                _ => {}
            }
        }
        if let Some((level, message, job_id)) = diagnostic {
            Self::push_diag_locked(
                &self.cfg,
                &mut state,
                level,
                "profiler_flush_schedule_status",
                message,
                Some(job_id),
            );
        }
    }

    pub(crate) fn mark_flush_request_completed(&self, request_id: &str, error: Option<String>) {
        let mut state = self.lock_state();
        let mut failed = false;
        if let Some(record) = state
            .flush_requests
            .iter_mut()
            .rev()
            .find(|it| it.request_id == request_id)
        {
            record.completed_unix_ms = Some(unix_ms());
            if let Some(error) = error {
                record.status = "failed".to_owned();
                record.error = Some(error);
                failed = true;
            } else {
                record.status = "completed".to_owned();
            }
        }
        if failed {
            state.reports_failed = state.reports_failed.saturating_add(1);
        }
    }
}
