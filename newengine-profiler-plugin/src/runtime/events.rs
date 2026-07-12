use super::*;

impl ProfilerRuntime {
    pub(crate) fn on_event(&self, topic: &str, payload: &[u8]) {
        if !self.cfg.enabled {
            return;
        }

        let mut state = self.lock_state();
        state.events_seen = state.events_seen.saturating_add(1);

        let parsed = match serde_json::from_slice::<Value>(payload) {
            Ok(v) => v,
            Err(e) => {
                state.malformed_events = state.malformed_events.saturating_add(1);
                Self::push_diag_locked(
                    &self.cfg,
                    &mut state,
                    "warn",
                    "malformed_event_json",
                    format!("event topic='{topic}' has invalid JSON: {e}"),
                    None,
                );
                return;
            }
        };

        let category = parsed
            .get("category")
            .and_then(Value::as_str)
            .unwrap_or_default();

        if !self.capture_topic(topic, category) {
            return;
        }

        if self.cfg.ignore_self && self.is_self_event(&parsed) {
            return;
        }

        let failure = match topic {
            TOPIC_ENGINE_TASK_EVENT => self
                .record_engine_task_event_locked(&mut state, parsed)
                .err()
                .map(|error| ("bad_engine_task_event", "bad engine task event", error)),
            TOPIC_ENGINE_TASK_ENVELOPE => self
                .record_engine_task_event_locked(
                    &mut state,
                    engine_job_envelope_to_profiler_event(parsed),
                )
                .err()
                .map(|error| ("bad_engine_job_event", "bad engine job event", error)),
            TOPIC_JOB_BEGIN => self
                .record_begin_value_locked(&mut state, parsed)
                .err()
                .map(|error| ("bad_job_begin_event", "bad job begin event", error)),
            TOPIC_JOB_END => self
                .record_end_value_locked(&mut state, parsed)
                .err()
                .map(|error| ("bad_job_end_event", "bad job end event", error)),
            TOPIC_JOB_STATUS => self
                .record_status_value_locked(&mut state, parsed)
                .err()
                .map(|error| ("bad_job_status_event", "bad job status event", error)),
            _ => {
                if self.cfg.capture.custom_events {
                    self.record_custom_event_locked(&mut state, topic, parsed);
                }
                None
            }
        };

        if let Some((code, context, error)) = failure {
            state.malformed_events = state.malformed_events.saturating_add(1);
            Self::push_diag_locked(
                &self.cfg,
                &mut state,
                "warn",
                code,
                format!("{context}: {error}"),
                None,
            );
        }
    }

    fn capture_topic(&self, topic: &str, category: &str) -> bool {
        if !self.cfg.enabled {
            return false;
        }
        if category == "service_call" && !self.cfg.capture.service_calls {
            return false;
        }
        if category == "plugin_lifecycle" && !self.cfg.capture.plugin_lifecycle {
            return false;
        }
        if topic == TOPIC_JOB_STATUS && !self.cfg.capture.task_status_events {
            return false;
        }
        true
    }

    fn is_self_event(&self, value: &Value) -> bool {
        value
            .get("plugin_id")
            .or_else(|| value.pointer("/metadata/plugin_id"))
            .or_else(|| value.get("service_id"))
            .or_else(|| value.pointer("/metadata/service_id"))
            .and_then(Value::as_str)
            .map(|id| {
                id == PROFILER_PLUGIN_ID
                    || id == PROFILER_SERVICE_ID
                    || id == ENGINE_PROFILER_GATEWAY_ID
            })
            .unwrap_or(false)
    }
}
