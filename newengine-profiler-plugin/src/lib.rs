#![allow(non_snake_case)]
#![forbid(unsafe_op_in_unsafe_fn)]
#![allow(non_local_definitions)]

mod archive;
mod budget_tree;
mod config;
mod constants;
mod plugin;
mod population_pressure;
mod records;
mod report;
mod runtime;
mod scheduler;
mod service;
mod streaming_pressure;
mod util;

use abi_stable::sabi_trait::TD_Opaque;
use abi_stable::std_types::RString;
use newengine_plugin_api::{
    export_plugin_root, PluginBootstrapPhase, PluginKind, PluginModuleDyn, PluginModule_TO,
    PluginSignatureV1, PluginUiAssetsV1,
};

use crate::constants::{PROFILER_PLUGIN_ID, PROFILER_PLUGIN_NAME};
use crate::plugin::ProfilerPlugin;

newengine_plugin_api::export_newengine_plugin_descriptor_v2!(crate::plugin::descriptor_v2);
export_plugin_root!(create_module, ui_assets_v1);

extern "C" fn create_module() -> PluginModuleDyn<'static> {
    PluginModule_TO::from_value(ProfilerPlugin::default(), TD_Opaque)
}

extern "C" fn ui_assets_v1() -> PluginUiAssetsV1 {
    PluginUiAssetsV1::empty()
}

#[no_mangle]
pub extern "C" fn newengine_plugin_signature_v1() -> PluginSignatureV1 {
    PluginSignatureV1 {
        id: RString::from(PROFILER_PLUGIN_ID),
        name: RString::from(PROFILER_PLUGIN_NAME),
        version: RString::from(env!("CARGO_PKG_VERSION")),
        kind: PluginKind::Runtime,
        bootstrap_phase: PluginBootstrapPhase::Bootstrap,
    }
}
