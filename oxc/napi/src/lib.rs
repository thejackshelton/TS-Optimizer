#![deny(clippy::all)]
#![deny(clippy::perf)]
#![deny(clippy::nursery)]

use napi::Result;
use napi_derive::napi;
use qwik_optimizer::js_lib_interface;

#[cfg(windows)]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[napi]
pub async fn transform_modules(opts: serde_json::Value) -> Result<serde_json::Value> {
    let config: js_lib_interface::TransformModulesOptions =
        serde_json::from_value(opts).map_err(|e| napi::Error::from_reason(e.to_string()))?;

    let result = tokio::task::spawn_blocking(move || js_lib_interface::transform_modules(config))
        .await
        .unwrap()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;

    serde_json::to_value(&result).map_err(|e| napi::Error::from_reason(e.to_string()))
}
