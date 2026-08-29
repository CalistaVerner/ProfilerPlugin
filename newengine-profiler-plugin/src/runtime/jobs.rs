use super::*;

impl ProfilerRuntime {
    pub(crate) fn record_begin_value(&self, payload: &[u8]) -> Result<Value, String> {
        let value = serde_json::from_slice::<Value>(payload).map_err(|e| e.to_string())?;
        let mut state = self.lock_state();
        self.record_begin_value_locked(&mut state, value)?;
        Ok(self.snapshot_locked(&state))
    }

    pub(crate) fn record_end_value(&self, payload: &[u8]) -> Result<Value, String> {
        let value = serde_json::from_slice::<Value>(payload).map_err(|e| e.to_string())?;
        let mut state = self.lock_state();
        self.record_end_value_locked(&mut state, value)?;
        Ok(self.snapshot_locked(&state))
    }

    pub(crate) fn record_status_value(&self, payload: &[u8]) -> Result<Value, String> {
        let value = serde_json::from_slice::<Value>(payload).map_err(|e| e.to_string())?;
        let mut state = self.lock_state();
        self.record_status_value_locked(&mut state, value)?;
        Ok(self.snapshot_locked(&state))
    }

    pub(super) fn record_begin_value_locked(
        &self,
        state: &mut ProfilerState,
        value: Value,
    ) -> Result<(), String> {
        let wire: JobBeginWire =
            serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        let id = wire.id.unwrap_or_else(|| state.local_id());
        let category = sanitize_non_empty(wire.category.as_deref(), "custom_job");
        let budget = wire
            .budget_ms
            .unwrap_or_else(|| self.default_budget_for(&category));
        let name = wire.name.or(wire.label).unwrap_or_else(|| id.clone());

        let mut record = JobRecord {
            id: id.clone(),
            name,
            category,
            source: wire.source.unwrap_or_else(|| "event".to_owned()),
            lane: sanitize_non_empty(wire.lane.as_deref(), "unspecified"),
            priority: sanitize_non_empty(wire.priority.as_deref(), "unspecified"),
            dependency_group: sanitize_non_empty(wire.dependency_group.as_deref(), "unspecified"),
            frame_id: wire.frame_id,
            status: "running".to_owned(),
            detail: wire.detail.unwrap_or_default(),
            scheduled: true,
            blocked: false,
            polling: false,
            waited_on_gpu: false,
            stayed_async: false,
            exceeded_frame_budget: false,
            frame_budget_ms: wire.frame_budget_ms,
            gpu_wait_ms: wire.gpu_wait_ms,
            wait_reason: wire.wait_reason,
            async_mode: wire.async_mode,
            started_unix_ms: unix_ms(),
            ended_unix_ms: None,
            elapsed_ms: None,
            budget_ms: budget.max(0.001),
            load: None,
            progress: None,
            payload_bytes: wire.payload_bytes,
            output_bytes: None,
            error: None,
            metadata: wire.metadata.unwrap_or(value),
        };
        refresh_record_classification(&mut record);

        if state
            .active
            .insert(
                id.clone(),
                ActiveJob {
                    record,
                    started_at: Instant::now(),
                },
            )
            .is_some()
        {
            Self::push_diag_locked(
                &self.cfg,
                state,
                "warn",
                "job_restarted_without_end",
                format!("job '{id}' was started again before an end event"),
                Some(id),
            );
        }
        Ok(())
    }

    pub(super) fn record_end_value_locked(
        &self,
        state: &mut ProfilerState,
        value: Value,
    ) -> Result<(), String> {
        let wire: JobEndWire = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        let id = wire.id.clone();
        let Some(mut active) = state.active.remove(&id) else {
            if recently_completed_duplicate_terminal(state, &id) {
                return Ok(());
            }
            if let Some(record) =
                self.synthesize_plugin_load_terminal_without_begin(&id, &wire, value.clone())
            {
                self.complete_job_locked(state, record);
                return Ok(());
            }
            Self::push_diag_locked(
                &self.cfg,
                state,
                "warn",
                "job_end_without_begin",
                format!("job '{id}' ended without a matching begin event"),
                Some(id),
            );
            return Ok(());
        };

        let elapsed = active.started_at.elapsed();
        let elapsed_ms = duration_ms(elapsed);
        let status = wire.status.unwrap_or_else(|| {
            if wire.error.is_some() {
                "failed".to_owned()
            } else {
                "completed".to_owned()
            }
        });
        active.record.status = status;
        if let Some(detail) = wire.detail {
            active.record.detail = detail;
        }
        active.record.ended_unix_ms = Some(unix_ms());
        active.record.elapsed_ms = Some(elapsed_ms);
        active.record.load = Some(elapsed_ms / active.record.budget_ms.max(0.001));
        active.record.output_bytes = wire.output_bytes;
        active.record.error = wire.error;
        if let Some(extra) = wire.metadata {
            active.record.metadata = merge_metadata(active.record.metadata, extra);
        } else {
            active.record.metadata = merge_metadata(active.record.metadata, value);
        }
        if let Some(v) = wire.lane.as_deref() {
            active.record.lane = sanitize_non_empty(Some(v), active.record.lane.as_str());
        }
        if let Some(v) = wire.priority.as_deref() {
            active.record.priority = sanitize_non_empty(Some(v), active.record.priority.as_str());
        }
        if let Some(v) = wire.dependency_group.as_deref() {
            active.record.dependency_group =
                sanitize_non_empty(Some(v), active.record.dependency_group.as_str());
        }
        active.record.frame_id = wire.frame_id.or(active.record.frame_id);
        active.record.frame_budget_ms = wire.frame_budget_ms.or(active.record.frame_budget_ms);
        active.record.gpu_wait_ms = wire.gpu_wait_ms.or(active.record.gpu_wait_ms);
        active.record.wait_reason = wire
            .wait_reason
            .or_else(|| active.record.wait_reason.clone());
        active.record.async_mode = wire.async_mode.or_else(|| active.record.async_mode.clone());
        refresh_record_classification(&mut active.record);

        self.complete_job_locked(state, active.record);
        Ok(())
    }

    fn synthesize_plugin_load_terminal_without_begin(
        &self,
        id: &str,
        wire: &JobEndWire,
        value: Value,
    ) -> Option<JobRecord> {
        let metadata = wire.metadata.clone().unwrap_or_else(|| value.clone());
        let operation = metadata
            .get("operation")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if operation != "load_one" {
            return None;
        }
        let elapsed_ms = metadata
            .get("total_ms")
            .and_then(Value::as_f64)
            .or_else(|| metadata.get("elapsed_ms").and_then(Value::as_f64))?;
        let path = metadata.get("path").and_then(Value::as_str).unwrap_or(id);
        let budget_ms = self.default_budget_for("plugin_lifecycle");
        let now = unix_ms();
        let mut record = JobRecord {
            id: id.to_owned(),
            name: format!("plugin_load:{path}"),
            category: "plugin_lifecycle".to_owned(),
            source: "newengine-plugin-host".to_owned(),
            lane: "Plugin".to_owned(),
            priority: "Normal".to_owned(),
            dependency_group: "plugin-host".to_owned(),
            frame_id: wire.frame_id,
            status: wire
                .status
                .clone()
                .unwrap_or_else(|| "completed".to_owned()),
            detail: wire.detail.clone().unwrap_or_else(|| {
                "Plugin load terminal event captured after profiler route became available"
                    .to_owned()
            }),
            scheduled: true,
            blocked: false,
            polling: false,
            waited_on_gpu: false,
            stayed_async: false,
            exceeded_frame_budget: false,
            frame_budget_ms: wire.frame_budget_ms,
            gpu_wait_ms: wire.gpu_wait_ms,
            wait_reason: wire.wait_reason.clone(),
            async_mode: wire.async_mode.clone(),
            started_unix_ms: now.saturating_sub(elapsed_ms.round().max(0.0) as u128),
            ended_unix_ms: Some(now),
            elapsed_ms: Some(elapsed_ms),
            budget_ms,
            load: Some(elapsed_ms / budget_ms.max(0.001)),
            progress: None,
            payload_bytes: None,
            output_bytes: wire.output_bytes,
            error: wire.error.clone(),
            metadata,
        };
        refresh_record_classification(&mut record);
        Some(record)
    }

    pub(super) fn record_status_value_locked(
        &self,
        state: &mut ProfilerState,
        value: Value,
    ) -> Result<(), String> {
        let wire: JobStatusWire =
            serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        let phase = wire
            .phase
            .as_deref()
            .unwrap_or("running")
            .to_ascii_lowercase();
        let category = wire.kind.unwrap_or_else(|| "task_status".to_owned());
        let budget = wire
            .budget_ms
            .unwrap_or_else(|| self.default_budget_for("task_status"));
        let progress = match (wire.current, wire.total) {
            (Some(current), Some(total)) if total != 0 => {
                Some((current as f64 / total as f64).clamp(0.0, 1.0))
            }
            _ => None,
        };

        if matches!(phase.as_str(), "completed" | "failed" | "cancelled") {
            let end_payload = json!({
                "id": wire.id,
                "status": phase.clone(),
                "detail": wire.detail.unwrap_or_default(),
                "metadata": wire.metadata.unwrap_or(value),
                "lane": wire.lane,
                "priority": wire.priority,
                "dependency_group": wire.dependency_group,
                "frame_id": wire.frame_id,
                "frame_budget_ms": wire.frame_budget_ms,
                "gpu_wait_ms": wire.gpu_wait_ms,
                "wait_reason": wire.wait_reason,
                "async_mode": wire.async_mode,
            });
            self.record_end_value_locked(state, end_payload)?;
            return Ok(());
        }

        if let Some(active) = state.active.get_mut(&wire.id) {
            active.record.status = phase;
            if let Some(label) = wire.label {
                active.record.name = label;
            }
            if let Some(detail) = wire.detail {
                active.record.detail = detail;
            }
            active.record.progress = progress;
            active.record.budget_ms = budget.max(0.001);
            if let Some(v) = wire.lane.as_deref() {
                active.record.lane = sanitize_non_empty(Some(v), active.record.lane.as_str());
            }
            if let Some(v) = wire.priority.as_deref() {
                active.record.priority =
                    sanitize_non_empty(Some(v), active.record.priority.as_str());
            }
            if let Some(v) = wire.dependency_group.as_deref() {
                active.record.dependency_group =
                    sanitize_non_empty(Some(v), active.record.dependency_group.as_str());
            }
            active.record.frame_id = wire.frame_id.or(active.record.frame_id);
            active.record.frame_budget_ms = wire.frame_budget_ms.or(active.record.frame_budget_ms);
            active.record.gpu_wait_ms = wire.gpu_wait_ms.or(active.record.gpu_wait_ms);
            active.record.wait_reason = wire.wait_reason.or(active.record.wait_reason.clone());
            active.record.async_mode = wire.async_mode.or(active.record.async_mode.clone());
            if let Some(extra) = wire.metadata {
                active.record.metadata =
                    merge_metadata(std::mem::take(&mut active.record.metadata), extra);
            }
            refresh_record_classification(&mut active.record);
            return Ok(());
        }

        let begin = JobBeginWire {
            id: Some(wire.id),
            name: wire.label,
            label: None,
            category: Some(category),
            source: Some("task_status".to_owned()),
            detail: wire.detail,
            budget_ms: Some(budget),
            payload_bytes: None,
            metadata: wire.metadata.or(Some(value)),
            lane: wire.lane,
            priority: wire.priority,
            dependency_group: wire.dependency_group,
            frame_id: wire.frame_id,
            frame_budget_ms: wire.frame_budget_ms,
            gpu_wait_ms: wire.gpu_wait_ms,
            wait_reason: wire.wait_reason,
            async_mode: wire.async_mode,
        };
        let begin_value = serde_json::to_value(begin_to_json(begin)).map_err(|e| e.to_string())?;
        self.record_begin_value_locked(state, begin_value)?;
        Ok(())
    }

    pub(super) fn record_engine_task_event_locked(
        &self,
        state: &mut ProfilerState,
        value: Value,
    ) -> Result<(), String> {
        let id = value
            .get("task_id")
            .or_else(|| value.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| "engine task event has no task_id".to_owned())?;
        let phase = value
            .get("phase")
            .and_then(Value::as_str)
            .unwrap_or("Running")
            .to_ascii_lowercase();
        let category =
            sanitize_non_empty(value.get("category").and_then(Value::as_str), "engine_task");
        let source = sanitize_non_empty(
            value.get("source").and_then(Value::as_str),
            "engine.task.event",
        );
        let label = sanitize_non_empty(value.get("name").and_then(Value::as_str), id.as_str());
        let detail = value
            .get("detail")
            .or_else(|| value.get("status"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let progress = value.get("progress_01").and_then(Value::as_f64);
        let budget = value
            .get("budget_ms")
            .and_then(Value::as_f64)
            .unwrap_or_else(|| self.default_budget_for(category.as_str()))
            .max(0.001);

        if matches!(
            phase.as_str(),
            "completed" | "failed" | "cancelled" | "canceled"
        ) {
            let status = if phase == "cancelled" || phase == "canceled" {
                "cancelled"
            } else {
                phase.as_str()
            };
            let end_payload = json!({
                "id": id,
                "status": status,
                "detail": detail,
                "metadata": value,
            });
            self.record_end_value_locked(state, end_payload)?;
            return Ok(());
        }

        if let Some(active) = state.active.get_mut(&id) {
            active.record.status = phase;
            active.record.name = label;
            active.record.detail = detail;
            active.record.progress = progress;
            active.record.budget_ms = budget;
            active.record.metadata =
                merge_metadata(std::mem::take(&mut active.record.metadata), value);
            refresh_record_classification(&mut active.record);
            return Ok(());
        }

        let begin = json!({
            "id": id,
            "name": label,
            "category": category,
            "source": source,
            "detail": detail,
            "budget_ms": budget,
            "lane": value.get("lane").cloned(),
            "priority": value.get("priority").cloned(),
            "dependency_group": value.get("dependency_group").cloned(),
            "frame_id": value.get("frame_id").cloned(),
            "frame_budget_ms": value.get("frame_budget_ms").cloned(),
            "gpu_wait_ms": value.get("gpu_wait_ms").cloned(),
            "wait_reason": value.get("wait_reason").cloned().or_else(|| value.get("blocked_reason").cloned()),
            "async_mode": value.get("async_mode").cloned(),
            "metadata": value,
        });
        self.record_begin_value_locked(state, begin)
    }

    pub(super) fn record_custom_event_locked(
        &self,
        state: &mut ProfilerState,
        topic: &str,
        value: Value,
    ) {
        let id = value
            .get("id")
            .or_else(|| value.get("task_id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| state.local_id());
        let category = sanitize_non_empty(value.get("category").and_then(Value::as_str), "event");
        let source = sanitize_non_empty(value.get("source").and_then(Value::as_str), "event_bus");
        let name = sanitize_non_empty(
            value
                .get("name")
                .or_else(|| value.get("label"))
                .and_then(Value::as_str),
            topic,
        );
        let detail = value
            .get("detail")
            .or_else(|| value.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("event observed")
            .to_owned();
        let elapsed_ms = event_elapsed_ms(&value);
        let budget = value
            .get("budget_ms")
            .and_then(Value::as_f64)
            .unwrap_or_else(|| self.default_budget_for("custom_event"))
            .max(0.001);
        if is_high_frequency_zero_cost_event(&category, &source, &name, elapsed_ms) {
            return;
        }
        let load = elapsed_ms.map(|elapsed| elapsed / budget);
        let now = unix_ms();
        let started_unix_ms = elapsed_ms
            .map(|elapsed| now.saturating_sub(elapsed.round().max(0.0) as u128))
            .unwrap_or(now);

        let mut record = JobRecord {
            id,
            name,
            category,
            source,
            lane: sanitize_non_empty(value.get("lane").and_then(Value::as_str), "unspecified"),
            priority: sanitize_non_empty(
                value.get("priority").and_then(Value::as_str),
                "unspecified",
            ),
            dependency_group: sanitize_non_empty(
                value.get("dependency_group").and_then(Value::as_str),
                "unspecified",
            ),
            frame_id: value.get("frame_id").and_then(Value::as_u64),
            status: "completed".to_owned(),
            detail,
            scheduled: false,
            blocked: false,
            polling: false,
            waited_on_gpu: value
                .get("gpu_wait_ms")
                .and_then(Value::as_f64)
                .is_some_and(|wait| wait > 0.0),
            stayed_async: value
                .get("async_mode")
                .and_then(Value::as_str)
                .is_some_and(|mode| !mode.trim().is_empty() && mode != "sync"),
            exceeded_frame_budget: value
                .get("frame_budget_ms")
                .and_then(Value::as_f64)
                .zip(elapsed_ms)
                .is_some_and(|(budget, elapsed)| budget > 0.0 && elapsed > budget),
            frame_budget_ms: value.get("frame_budget_ms").and_then(Value::as_f64),
            gpu_wait_ms: value.get("gpu_wait_ms").and_then(Value::as_f64),
            wait_reason: value
                .get("wait_reason")
                .and_then(Value::as_str)
                .map(str::to_owned),
            async_mode: value
                .get("async_mode")
                .and_then(Value::as_str)
                .map(str::to_owned),
            started_unix_ms,
            ended_unix_ms: Some(now),
            elapsed_ms: Some(elapsed_ms.unwrap_or(0.0)),
            budget_ms: budget,
            load: Some(load.unwrap_or(0.0)),
            progress: None,
            payload_bytes: None,
            output_bytes: None,
            error: None,
            metadata: value,
        };
        trim_payload_preview(
            &mut record.metadata,
            self.cfg.diagnostics.max_payload_preview_bytes,
        );
        refresh_record_classification(&mut record);
        let breakdown_parts = self.build_breakdown_part_records(&record);
        self.complete_job_locked(state, record);
        for part in breakdown_parts {
            self.complete_job_locked(state, part);
        }
    }

    fn build_breakdown_part_records(&self, parent: &JobRecord) -> Vec<JobRecord> {
        let Some(breakdown) = parent
            .metadata
            .get("breakdown")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Vec::new();
        };

        let parts = parse_breakdown_parts(breakdown);
        let mut records = Vec::with_capacity(parts.len());
        for (idx, (part_name, elapsed_ms)) in parts.into_iter().enumerate() {
            let budget_ms = parent.budget_ms.max(0.001);
            let now = unix_ms();
            let id = format!("{}::part::{}", parent.id, idx + 1);
            let mut metadata = json!({
                "schema": "newengine.profiler.breakdown_part.v1",
                "parent_id": parent.id.clone(),
                "parent_name": parent.name.clone(),
                "part": part_name.clone(),
                "elapsed_ms": elapsed_ms,
                "source_event": parent.metadata.clone(),
            });
            trim_payload_preview(
                &mut metadata,
                self.cfg.diagnostics.max_payload_preview_bytes,
            );
            let mut part_record = JobRecord {
                id,
                name: format!("{}/{}", parent.name, part_name),
                category: format!("{}.breakdown", parent.category),
                source: parent.source.clone(),
                lane: parent.lane.clone(),
                priority: parent.priority.clone(),
                dependency_group: parent.dependency_group.clone(),
                frame_id: parent.frame_id,
                status: "completed".to_owned(),
                detail: format!("breakdown part from '{}'", parent.name),
                scheduled: parent.scheduled,
                blocked: parent.blocked,
                polling: parent.polling,
                waited_on_gpu: parent.waited_on_gpu,
                stayed_async: parent.stayed_async,
                exceeded_frame_budget: false,
                frame_budget_ms: parent.frame_budget_ms,
                gpu_wait_ms: None,
                wait_reason: parent.wait_reason.clone(),
                async_mode: parent.async_mode.clone(),
                started_unix_ms: now.saturating_sub(elapsed_ms.round().max(0.0) as u128),
                ended_unix_ms: Some(now),
                elapsed_ms: Some(elapsed_ms),
                budget_ms,
                load: Some(elapsed_ms / budget_ms),
                progress: None,
                payload_bytes: None,
                output_bytes: None,
                error: None,
                metadata,
            };
            refresh_record_classification(&mut part_record);
            records.push(part_record);
        }
        records
    }

    pub(super) fn complete_job_locked(&self, state: &mut ProfilerState, mut record: JobRecord) {
        refresh_record_classification(&mut record);
        trim_payload_preview(
            &mut record.metadata,
            self.cfg.diagnostics.max_payload_preview_bytes,
        );

        let elapsed = record.elapsed_ms.unwrap_or_default();
        if elapsed >= self.cfg.diagnostics.slow_job_warn_ms
            || record.load.unwrap_or_default() >= 1.0
        {
            Self::push_diag_locked(
                &self.cfg,
                state,
                "warn",
                "slow_or_over_budget_job",
                format!(
                    "job '{}' category='{}' elapsed_ms={:.3} budget_ms={:.3} load={:.2}",
                    record.name,
                    record.category,
                    elapsed,
                    record.budget_ms,
                    record.load.unwrap_or_default()
                ),
                Some(record.id.clone()),
            );
        }

        if record.status == "failed" {
            Self::push_diag_locked(
                &self.cfg,
                state,
                "error",
                "failed_job",
                format!(
                    "job '{}' failed: {}",
                    record.name,
                    record.error.as_deref().unwrap_or("<no error payload>")
                ),
                Some(record.id.clone()),
            );
        }

        state.completed.push_back(record);
        while state.completed.len() > self.cfg.diagnostics.max_recent_jobs {
            // Preserve rare stalls/failures when the bounded ring is flooded by
            // thousands of healthy simulation samples. Prefer evicting the oldest
            // ordinary record; only evict an outlier if the entire ring consists of
            // significant records. This keeps micro-freezes visible in the final report
            // without increasing the configured memory bound.
            if let Some(index) = state.completed.iter().position(|job| {
                !is_significant_completed_job(job, self.cfg.diagnostics.slow_job_warn_ms)
            }) {
                let _ = state.completed.remove(index);
            } else {
                state.completed.pop_front();
            }
        }
    }

    pub(super) fn default_budget_for(&self, category: &str) -> f64 {
        match category {
            "service_call" => self.cfg.budgets.service_call_ms,
            "plugin_lifecycle" => self.cfg.budgets.plugin_lifecycle_ms,
            "task_status" => self.cfg.budgets.task_status_ms,
            "profiler.report.flush" => self.cfg.scheduling.flush_job_budget_ms,
            _ => self.cfg.budgets.custom_job_ms,
        }
        .max(0.001)
    }
}

#[inline]
fn is_significant_completed_job(job: &JobRecord, slow_job_warn_ms: f64) -> bool {
    job.status == "failed"
        || job.elapsed_ms.unwrap_or_default() >= slow_job_warn_ms
        || job.load.unwrap_or_default() >= 1.0
        || job.exceeded_frame_budget
        || job.waited_on_gpu
        || job.gpu_wait_ms.unwrap_or_default() > 0.0
}
