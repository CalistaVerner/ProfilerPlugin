use serde_json::{json, Value};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use crate::config::ProfilerConfig;
use crate::constants::{
    ENGINE_PROFILER_GATEWAY_ID, ENGINE_THREADING_GATEWAY_ID, METHOD_FLUSH_REPORT_SYNC_V1,
    PROFILER_PLUGIN_ID, PROFILER_SERVICE_ID, TASK_INVOKE_SERVICE_V1, TOPIC_ENGINE_TASK_ENVELOPE,
    TOPIC_ENGINE_TASK_EVENT, TOPIC_JOB_BEGIN, TOPIC_JOB_END, TOPIC_JOB_STATUS,
};
use crate::records::{
    ActiveJob, FlushRequestRecord, JobBeginWire, JobEndWire, JobRecord, JobStatusWire,
    ProfilerDiagnostic, ProfilerState,
};
use crate::scheduler::HostJobScheduler;
use crate::util::{
    begin_to_json, duration_ms, merge_metadata, sanitize_non_empty, trim_payload_preview, unix_ms,
};

pub(crate) static RUNTIME: OnceLock<Arc<ProfilerRuntime>> = OnceLock::new();

mod classification;
mod events;
mod flush;
mod jobs;
mod parsing;
mod snapshot;
#[cfg(test)]
mod tests;

use classification::*;
use parsing::*;

pub(crate) struct ProfilerRuntime {
    pub(crate) cfg: ProfilerConfig,
    scheduler: Option<HostJobScheduler>,
    state: Mutex<ProfilerState>,
}

impl ProfilerRuntime {
    pub(crate) fn new(cfg: ProfilerConfig, scheduler: Option<HostJobScheduler>) -> Self {
        Self {
            cfg,
            scheduler,
            state: Mutex::new(ProfilerState::new()),
        }
    }
    pub(crate) fn lock_state(&self) -> std::sync::MutexGuard<'_, ProfilerState> {
        match self.state.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        }
    }
}
