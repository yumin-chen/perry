//! Container module for Perry
//!
//! Provides OCI container management with platform-adaptive backend selection.

pub mod backend;
pub mod capability;
pub mod compose;
pub mod types;
pub mod verification;

mod mod_private {
    use super::get_global_backend;
    use crate::container::backend::ContainerBackend;
    use std::sync::Arc;

    pub async fn get_global_backend_instance() -> Result<Arc<dyn ContainerBackend>, String> {
        get_global_backend()
            .await
            .map(|b| Arc::clone(b))
            .map_err(|e| e.to_string())
    }
}

// Re-export commonly used types
pub use types::{
    ComposeHandle, ComposeSpec, ContainerError, ContainerHandle, ContainerInfo, ContainerLogs,
    ContainerSpec, ImageInfo, ListOrDict,
};

use perry_runtime::{js_promise_new, Promise, StringHeader};
pub use backend::{detect_backend, ContainerBackend};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;

// Global backend instance - initialized once at first use
static BACKEND: OnceLock<Arc<dyn ContainerBackend>> = OnceLock::new();
static BACKEND_INIT_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Get or initialize the global backend instance.
///
/// Per SPEC §5.1 step 4: on `detect_backend()` failure, if stderr is an
/// interactive TTY *and* `PERRY_NO_INSTALL_PROMPT` is unset, hand off to
/// `BackendInstaller` so the user can pick + install a runtime. Both gates
/// must hold; otherwise the original `NoBackendFound` error propagates.
async fn get_global_backend() -> Result<&'static Arc<dyn ContainerBackend>, ContainerError> {
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }

    let _guard = BACKEND_INIT_MUTEX.lock().await;

    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }

    let b = match detect_backend().await {
        Ok(backend) => Arc::from(backend) as Arc<dyn ContainerBackend>,
        Err(e) => {
            use std::io::IsTerminal;
            let interactive = std::io::stderr().is_terminal();
            let prompt_disabled = std::env::var("PERRY_NO_INSTALL_PROMPT").is_ok();
            if interactive && !prompt_disabled {
                let installer = perry_container_compose::BackendInstaller::new();
                match installer.run().await {
                    Ok(backend) => Arc::from(backend) as Arc<dyn ContainerBackend>,
                    Err(_) => return Err(ContainerError::from(e)),
                }
            } else {
                return Err(ContainerError::from(e));
            }
        }
    };

    let _ = BACKEND.set(b);
    Ok(BACKEND.get().unwrap())
}

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// Helper to create a JS string from a Rust string
unsafe fn string_to_js(s: &str) -> *const StringHeader {
    let bytes = s.as_bytes();
    perry_runtime::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Optionally verify a container image's signature before pulling/running.
///
/// Gated on `PERRY_CONTAINER_VERIFY_IMAGES=1` so the default path stays
/// cosign-free for development + CI parity. When the env var is set, the
/// image is run through `verification::verify_image()` (cosign keyless
/// verification against Chainguard identity) and a failure short-circuits
/// the FFI call with a `verification failed` error string.
///
/// SPEC §11.2 calls this out as "present but not yet enforced in HEAD"; this
/// helper is the integration point. Per-call guard rather than a global
/// `up()`-only one so users can pin individual `run`/`create`/`pullImage`
/// invocations to verified images while leaving compose stacks unchecked.
async fn maybe_verify_image(image: &str) -> Result<(), String> {
    if std::env::var("PERRY_CONTAINER_VERIFY_IMAGES")
        .ok()
        .as_deref()
        != Some("1")
    {
        return Ok(());
    }
    crate::container::verification::verify_image(image)
        .await
        .map(|_digest| ())
}

// ============ Container Lifecycle ============

/// Run a container from the given spec
/// FFI: js_container_run(spec_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_run(spec_ptr: *const StringHeader) -> *mut Promise {
    let promise = js_promise_new();

    let spec = match types::parse_container_spec(spec_ptr) {
        Ok(s) => s,
        Err(e) => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>(e)
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        if let Err(e) = maybe_verify_image(&spec.image).await {
            return Err::<u64, String>(e);
        }
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.run(&spec).await {
            Ok(handle) => {
                let handle_id = types::register_container_handle(handle);
                Ok(handle_id as u64)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Start compose services
/// FFI: js_container_compose_start(handle_id: i64, services_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_start(
    handle_id: i64,
    services_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let handle = match types::get_compose_handle(handle_id as u64) {
        Some(h) => h.clone(),
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    let services_json = unsafe { string_from_header(services_json_ptr) };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let services: Vec<String> = services_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        handle.start(&services).await.map(|_| 0u64).map_err(|e| e.to_string())
    });

    promise
}

/// Stop compose services
/// FFI: js_container_compose_stop(handle_id: i64, services_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_stop(
    handle_id: i64,
    services_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let handle = match types::get_compose_handle(handle_id as u64) {
        Some(h) => h.clone(),
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    let services_json = unsafe { string_from_header(services_json_ptr) };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let services: Vec<String> = services_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        handle.stop(&services).await.map(|_| 0u64).map_err(|e| e.to_string())
    });

    promise
}

/// Restart compose services
/// FFI: js_container_compose_restart(handle_id: i64, services_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_restart(
    handle_id: i64,
    services_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let handle = match types::get_compose_handle(handle_id as u64) {
        Some(h) => h.clone(),
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    let services_json = unsafe { string_from_header(services_json_ptr) };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let services: Vec<String> = services_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        handle.restart(&services).await.map(|_| 0u64).map_err(|e| e.to_string())
    });

    promise
}

/// Get compose configuration
/// FFI: js_container_compose_config(handle_id: i64) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_config(handle_id: i64) -> *mut Promise {
    let promise = js_promise_new();

    let handle = match types::get_compose_handle(handle_id as u64) {
        Some(h) => h.clone(),
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move { handle.config().map_err(|e| e.to_string()) },
        |yaml| {
            let str_ptr = perry_runtime::js_string_from_bytes(yaml.as_ptr(), yaml.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}

/// Create a container from the given spec without starting it
/// FFI: js_container_create(spec_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_create(spec_ptr: *const StringHeader) -> *mut Promise {
    let promise = js_promise_new();

    let spec = match types::parse_container_spec(spec_ptr) {
        Ok(s) => s,
        Err(e) => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>(e)
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        if let Err(e) = maybe_verify_image(&spec.image).await {
            return Err::<u64, String>(e);
        }
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.create(&spec).await {
            Ok(handle) => {
                let handle_id = types::register_container_handle(handle);
                Ok(handle_id as u64)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Start a previously created container
/// FFI: js_container_start(id: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_start(id_ptr: *const StringHeader) -> *mut Promise {
    let promise = js_promise_new();

    let id = match string_from_header(id_ptr) {
        Some(s) => s,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid container ID".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.start(&id).await {
            Ok(()) => Ok(0u64),
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Stop a running container
/// FFI: js_container_stop(id: *const StringHeader, timeout: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_stop(
    id_ptr: *const StringHeader,
    timeout: i32,
) -> *mut Promise {
    let promise = js_promise_new();

    let id = match string_from_header(id_ptr) {
        Some(s) => s,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid container ID".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let timeout_opt = if timeout < 0 { None } else { Some(timeout as u32) };
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.stop(&id, timeout_opt).await {
            Ok(()) => Ok(0u64),
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Remove a container
/// FFI: js_container_remove(id: *const StringHeader, force: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_remove(
    id_ptr: *const StringHeader,
    force: i32,
) -> *mut Promise {
    let promise = js_promise_new();

    let id = match string_from_header(id_ptr) {
        Some(s) => s,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid container ID".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.remove(&id, force != 0).await {
            Ok(()) => Ok(0u64),
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// List containers
/// FFI: js_container_list(all: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_list(all: i32) -> *mut Promise {
    let promise = js_promise_new();

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.list(all != 0).await {
            Ok(containers) => {
                let handle_id = types::register_container_info_list(containers);
                Ok(handle_id as u64)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Inspect a container
/// FFI: js_container_inspect(id: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_inspect(id_ptr: *const StringHeader) -> *mut Promise {
    let promise = js_promise_new();

    let id = match string_from_header(id_ptr) {
        Some(s) => s,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid container ID".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.inspect(&id).await {
            Ok(info) => {
                let handle_id = types::register_container_info(info);
                Ok(handle_id as u64)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Get the current backend name
/// FFI: js_container_getBackend() -> *const StringHeader
#[no_mangle]
pub unsafe extern "C" fn js_container_getBackend() -> *const StringHeader {
    // Note: this is synchronous and might return "unknown" if not initialized
    if let Some(b) = BACKEND.get() {
        return string_to_js(b.backend_name());
    }
    string_to_js("unknown")
}

/// Detect backend and return probed info
/// FFI: js_container_detectBackend() -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_detectBackend() -> *mut Promise {
    let promise = js_promise_new();
    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            match detect_backend().await {
                Ok(b) => {
                    let name = b.backend_name().to_string();
                    let json = serde_json::json!([{
                        "name": name,
                        "available": true,
                        "reason": ""
                    }])
                    .to_string();
                    Ok(json)
                }
                Err(e) => {
                    use perry_container_compose::error::ComposeError;
                    let json = match e {
                        ComposeError::NoBackendFound { probed } => {
                            serde_json::to_string(&probed).unwrap_or_else(|_| "[]".to_string())
                        }
                        _ => serde_json::json!([{
                            "name": "unknown",
                            "available": false,
                            "reason": e.to_string()
                        }])
                        .to_string(),
                    };
                    Ok(json)
                }
            }
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );
    promise
}

// ============ Container Logs and Exec ============

/// Get logs from a container
/// FFI: js_container_logs(id: *const StringHeader, tail: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_logs(id_ptr: *const StringHeader, tail: i32) -> *mut Promise {
    let promise = js_promise_new();

    let id = match string_from_header(id_ptr) {
        Some(s) => s,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid container ID".to_string())
            });
            return promise;
        }
    };

    let tail_opt = if tail >= 0 { Some(tail as u32) } else { None };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.logs(&id, tail_opt).await {
            Ok(logs) => {
                let handle_id = types::register_container_logs(logs);
                Ok(handle_id as u64)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Execute a command in a container
/// FFI: js_container_exec(id: *const StringHeader, cmd_json: *const StringHeader, env_json: *const StringHeader, workdir: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_exec(
    id_ptr: *const StringHeader,
    cmd_json_ptr: *const StringHeader,
    env_json_ptr: *const StringHeader,
    workdir_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let id = match string_from_header(id_ptr) {
        Some(s) => s,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid container ID".to_string())
            });
            return promise;
        }
    };

    let cmd_json = string_from_header(cmd_json_ptr);
    let env_json = string_from_header(env_json_ptr);
    let workdir = string_from_header(workdir_ptr);

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let cmd: Vec<String> = cmd_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let env: Option<HashMap<String, String>> =
            env_json.and_then(|s| serde_json::from_str(&s).ok());

        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend
            .exec(&id, &cmd, env.as_ref(), workdir.as_deref())
            .await
        {
            Ok(logs) => {
                let handle_id = types::register_container_logs(logs);
                Ok(handle_id as u64)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

// ============ Image Management ============

/// Pull a container image
/// FFI: js_container_pullImage(reference: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_pullImage(reference_ptr: *const StringHeader) -> *mut Promise {
    let promise = js_promise_new();

    let reference = match string_from_header(reference_ptr) {
        Some(s) => s,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid image reference".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        if let Err(e) = maybe_verify_image(&reference).await {
            return Err::<u64, String>(e);
        }
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.pull_image(&reference).await {
            Ok(()) => Ok(0u64),
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// List images
/// FFI: js_container_listImages() -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_listImages() -> *mut Promise {
    let promise = js_promise_new();

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.list_images().await {
            Ok(images) => {
                let handle_id = types::register_image_info_list(images);
                Ok(handle_id as u64)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Build a container image
/// FFI: js_container_build(spec_json: *const StringHeader, image_name: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_build(
    spec_ptr: *const StringHeader,
    image_name_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let spec_json = string_from_header(spec_ptr).unwrap_or_else(|| "{}".to_string());
    let image_name = string_from_header(image_name_ptr).unwrap_or_default();

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let spec: perry_container_compose::types::ComposeServiceBuild =
            serde_json::from_str(&spec_json).map_err(|e| format!("Invalid build spec: {}", e))?;

        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };

        match backend.build(&spec, &image_name).await {
            Ok(()) => Ok(0u64),
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Remove an image
/// FFI: js_container_removeImage(reference: *const StringHeader, force: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_removeImage(
    reference_ptr: *const StringHeader,
    force: i32,
) -> *mut Promise {
    let promise = js_promise_new();

    let reference = match string_from_header(reference_ptr) {
        Some(s) => s,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid image reference".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        match backend.remove_image(&reference, force != 0).await {
            Ok(()) => Ok(0u64),
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

// ============ Compose Functions ============

/// Bring up a Compose stack
/// FFI: js_container_composeUp(spec_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_composeUp(
    spec_ptr: *const perry_runtime::StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let spec = match types::parse_compose_spec(spec_ptr) {
        Ok(s) => s,
        Err(e) => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>(e)
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        let wrapper = compose::ComposeWrapper::new(spec, backend);
        match wrapper.up().await {
        Ok(_handle) => {
            let handle_id = types::register_compose_handle(wrapper.engine().clone());
            Ok(handle_id)
        }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Alias for js_container_composeUp
#[no_mangle]
pub unsafe extern "C" fn js_compose_up(spec_ptr: *const StringHeader) -> *mut Promise {
    js_container_composeUp(spec_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_down(handle_id: i64, volumes: i32) -> *mut Promise {
    js_container_compose_down(handle_id, volumes)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_ps(handle_id: i64) -> *mut Promise {
    js_container_compose_ps(handle_id)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_logs(
    handle_id: i64,
    service_ptr: *const StringHeader,
    tail: i32,
) -> *mut Promise {
    js_container_compose_logs(handle_id, service_ptr, tail)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_exec(
    handle_id: i64,
    service_ptr: *const StringHeader,
    cmd_json_ptr: *const StringHeader,
) -> *mut Promise {
    js_container_compose_exec(handle_id, service_ptr, cmd_json_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_config(handle_id: i64) -> *mut Promise {
    js_container_compose_config(handle_id)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_start(
    handle_id: i64,
    services_json_ptr: *const StringHeader,
) -> *mut Promise {
    js_container_compose_start(handle_id, services_json_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_stop(
    handle_id: i64,
    services_json_ptr: *const StringHeader,
) -> *mut Promise {
    js_container_compose_stop(handle_id, services_json_ptr)
}

#[no_mangle]
pub unsafe extern "C" fn js_compose_restart(
    handle_id: i64,
    services_json_ptr: *const StringHeader,
) -> *mut Promise {
    js_container_compose_restart(handle_id, services_json_ptr)
}

/// Stop and remove compose stack.
/// FFI: js_container_compose_down(handle_id: i64, volumes: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_down(handle_id: i64, volumes: i32) -> *mut Promise {
    let promise = js_promise_new();

    let handle = match types::take_compose_handle(handle_id as u64) {
        Some(h) => h,
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let _backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        let wrapper = compose::ComposeWrapper::new_from_engine(handle);
        match wrapper.down(volumes != 0).await {
            Ok(()) => Ok(0u64),
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Get container info for compose stack
/// FFI: js_container_compose_ps(handle_id: i64) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_ps(handle_id: i64) -> *mut Promise {
    let promise = js_promise_new();

    let handle = match types::get_compose_handle(handle_id as u64) {
        Some(h) => h.clone(),
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let _backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        let wrapper = compose::ComposeWrapper::new_from_engine(handle);
        match wrapper.ps().await {
            Ok(containers) => {
                let h = types::register_container_info_list(containers);
                Ok(h as u64)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Get logs from compose stack
/// FFI: js_container_compose_logs(handle_id: i64, service: *const StringHeader, tail: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_logs(
    handle_id: i64,
    service_ptr: *const StringHeader,
    tail: i32,
) -> *mut Promise {
    let promise = js_promise_new();

    let handle = match types::get_compose_handle(handle_id as u64) {
        Some(h) => h.clone(),
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    let service = unsafe { string_from_header(service_ptr) };
    let tail_opt = if tail >= 0 { Some(tail as u32) } else { None };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let _backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        let wrapper = compose::ComposeWrapper::new_from_engine(handle);
        match wrapper.logs(service.as_deref(), tail_opt).await {
            Ok(logs) => {
                let h = types::register_container_logs(logs);
                Ok(h as u64)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Execute command in compose service
/// FFI: js_container_compose_exec(handle_id: i64, service: *const StringHeader, cmd_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_container_compose_exec(
    handle_id: i64,
    service_ptr: *const StringHeader,
    cmd_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let handle = match types::get_compose_handle(handle_id as u64) {
        Some(h) => h.clone(),
        None => {
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, String>("Invalid compose handle".to_string())
            });
            return promise;
        }
    };

    let service_opt = unsafe { string_from_header(service_ptr) };
    let cmd_json = unsafe { string_from_header(cmd_json_ptr) };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let service = match service_opt {
            Some(s) => s,
            None => return Err::<u64, String>("Invalid service name".to_string()),
        };

        let cmd: Vec<String> = cmd_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let _backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };
        let wrapper = compose::ComposeWrapper::new_from_engine(handle);
        match wrapper.exec(&service, &cmd).await {
            Ok(logs) => {
                let h = types::register_container_logs(logs);
                Ok(h as u64)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

// ============ Workload Functions ============

/// Create a workload graph
/// FFI: js_workload_graph(name: *const StringHeader, nodes_json: *const StringHeader) -> *const StringHeader
#[no_mangle]
pub unsafe extern "C" fn js_workload_graph(
    name_ptr: *const StringHeader,
    nodes_json_ptr: *const StringHeader,
) -> *const StringHeader {
    let name = string_from_header(name_ptr).unwrap_or_default();
    let nodes_json = string_from_header(nodes_json_ptr).unwrap_or_else(|| "{}".to_string());

    let graph = perry_container_compose::WorkloadGraph {
        name,
        nodes: serde_json::from_str(&nodes_json).unwrap_or_default(),
        edges: vec![], // Edges inferred from depends_on in nodes
    };

    let json = serde_json::to_string(&graph).unwrap_or_default();
    string_to_js(&json)
}

/// Create a workload node
/// FFI: js_workload_node(name: *const StringHeader, spec_json: *const StringHeader) -> *const StringHeader
#[no_mangle]
pub unsafe extern "C" fn js_workload_node(
    name_ptr: *const StringHeader,
    spec_json_ptr: *const StringHeader,
) -> *const StringHeader {
    let name = string_from_header(name_ptr).unwrap_or_default();
    let spec_json = string_from_header(spec_json_ptr).unwrap_or_else(|| "{}".to_string());

    let mut node: perry_container_compose::WorkloadNode =
        serde_json::from_str(&spec_json).unwrap_or_else(|_| perry_container_compose::WorkloadNode {
            id: name.clone(),
            name: name.clone(),
            image: None,
            resources: None,
            ports: vec![],
            env: HashMap::new(),
            depends_on: vec![],
            runtime: perry_container_compose::RuntimeSpec::Auto,
            policy: perry_container_compose::PolicySpec::default(),
        });
    node.id = name.clone();
    node.name = name;

    let json = serde_json::to_string(&node).unwrap_or_default();
    string_to_js(&json)
}

/// Run a workload graph
/// FFI: js_workload_runGraph(graph_json: *const StringHeader, opts_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_runGraph(
    graph_json_ptr: *const StringHeader,
    opts_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let graph_json = string_from_header(graph_json_ptr).unwrap_or_else(|| "{}".to_string());
    let opts_json = string_from_header(opts_json_ptr).unwrap_or_else(|| "{}".to_string());

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let graph: perry_container_compose::WorkloadGraph =
            serde_json::from_str(&graph_json).map_err(|e| format!("Failed to parse graph: {}", e))?;
        let opts: perry_container_compose::RunGraphOptions =
            serde_json::from_str(&opts_json).map_err(|e| format!("Failed to parse options: {}", e))?;

        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };

        let engine = Arc::new(perry_container_compose::WorkloadGraphEngine::new(
            graph, backend,
        ));
        match engine.run(opts).await {
            Ok(_) => {
                let handle_id = types::register_workload_handle(engine);
                Ok(handle_id)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Inspect a workload graph
/// FFI: js_workload_inspectGraph(handle_id: i64) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_inspectGraph(handle_id: i64) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
                Some(e) => e.clone(),
                None => return Err("Invalid workload handle".to_string()),
            };

            match engine.status().await {
                Ok(status) => {
                    let json = serde_json::to_string(&status).unwrap_or_default();
                    Ok(json)
                }
                Err(e) => Err(e.to_string()),
            }
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}

/// Stop and remove a workload graph
/// FFI: js_workload_handle_down(handle_id: i64, force: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_down(handle_id: i64, force: i32) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
            Some(e) => e.clone(),
            None => return Err("Invalid workload handle".to_string()),
        };

        match engine.down(force != 0).await {
            Ok(_) => {
                if let Some(handles) = types::WORKLOAD_HANDLES.get() {
                    handles.remove(&id);
                }
                Ok(0u64)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Get status of a workload graph
/// FFI: js_workload_handle_status(handle_id: i64) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_status(handle_id: i64) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
                Some(e) => e.clone(),
                None => return Err("Invalid workload handle".to_string()),
            };

            match engine.status().await {
                Ok(status) => {
                    let json = serde_json::to_string(&status).unwrap_or_default();
                    Ok(json)
                }
                Err(e) => Err(e.to_string()),
            }
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}

/// Get logs from a workload node
/// FFI: js_workload_handle_logs(handle_id: i64, node_id: *const StringHeader, tail: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_logs(
    handle_id: i64,
    node_id_ptr: *const StringHeader,
    tail: i32,
) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;
    let node_id = string_from_header(node_id_ptr).unwrap_or_default();
    let tail_opt = if tail >= 0 { Some(tail as u32) } else { None };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
            Some(e) => e.clone(),
            None => return Err("Invalid workload handle".to_string()),
        };

        match engine.logs(&node_id, tail_opt).await {
            Ok(logs) => {
                let handle_id = types::register_container_logs(logs);
                Ok(handle_id)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Execute command in a workload node
/// FFI: js_workload_handle_exec(handle_id: i64, node_id: *const StringHeader, cmd_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_exec(
    handle_id: i64,
    node_id_ptr: *const StringHeader,
    cmd_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;
    let node_id = string_from_header(node_id_ptr).unwrap_or_default();
    let cmd_json = string_from_header(cmd_json_ptr).unwrap_or_else(|| "[]".to_string());

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let cmd: Vec<String> = serde_json::from_str(&cmd_json).unwrap_or_default();
        let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
            Some(e) => e.clone(),
            None => return Err("Invalid workload handle".to_string()),
        };

        match engine.exec(&node_id, &cmd).await {
            Ok(logs) => {
                let handle_id = types::register_container_logs(logs);
                Ok(handle_id)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Get process status of a workload graph
/// FFI: js_workload_handle_ps(handle_id: i64) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_ps(handle_id: i64) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
            Some(e) => e.clone(),
            None => return Err("Invalid workload handle".to_string()),
        };

        match engine.ps().await {
            Ok(infos) => {
                // Register NodeInfo list as a container info list (compatible for now)
                // Actually we should probably have a register_node_info_list
                let handle_id = types::register_container_info_list(
                    infos
                        .into_iter()
                        .map(|i| ContainerInfo {
                            id: i.container_id.unwrap_or_default(),
                            name: i.name,
                            image: i.image.unwrap_or_default(),
                            status: format!("{:?}", i.state),
                            ports: vec![],
                            labels: HashMap::new(),
                            created: "".to_string(),
                            ip_address: i.ip_address.unwrap_or_default(),
                        })
                        .collect(),
                );
                Ok(handle_id)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Get graph JSON from workload handle
/// FFI: js_workload_handle_graph(handle_id: i64) -> *const StringHeader
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_graph(handle_id: i64) -> *const StringHeader {
    let id = handle_id as u64;
    let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
        Some(e) => e.clone(),
        None => return std::ptr::null(),
    };

    let json = serde_json::to_string(&engine.graph).unwrap_or_default();
    string_to_js(&json)
}

// ============ Module Initialization ============

/// Initialize the container module (called during runtime startup).
///
/// Per SPEC §11.6 / Task 18.1, this is a one-shot link-time anchor that:
/// 1. Forces `libperry_stdlib`'s container symbols to be retained (any
///    user code calling `js_container_module_init()` will pull in the
///    transitively-referenced FFI symbols and prevent dead-strip).
/// 2. Pre-warms the backend singleton when called from a tokio context —
///    avoids paying the probe latency on the first user `run()` call.
///
/// Backend probing is async + may invoke the interactive `BackendInstaller`,
/// so we must not block here. Instead we spawn the probe as a detached
/// tokio task; if a tokio runtime isn't yet running (called from `main`
/// before any async setup), the task simply doesn't run and the first
/// real FFI call will trigger probe-on-demand the same way it always has.
#[no_mangle]
pub extern "C" fn js_container_module_init() {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async {
            let _ = get_global_backend().await;
        });
    }
}

#[cfg(test)]
mod smoke_tests {
    use super::*;

    /// Task 27.1: `js_container_module_init` must be callable without panic
    /// outside an active tokio runtime. The link-anchor purpose mustn't
    /// depend on async setup.
    #[test]
    fn module_init_is_safe_to_call_outside_tokio() {
        js_container_module_init();
    }

    /// Task 27.1: when called inside a tokio runtime, module_init schedules
    /// the backend probe without blocking the caller. The detached probe
    /// task may fail (no backend installed in CI); we only assert the call
    /// itself returns synchronously without panic and that the runtime is
    /// still alive afterwards.
    #[test]
    fn module_init_inside_tokio_runtime_does_not_block() {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            js_container_module_init();
            // If we reach here without hanging, the call returned
            // synchronously — invariant proved.
        });
    }

    /// Task 27.1: the canonical FFI symbols listed in SPEC §9.1 must all be
    /// addressable from this crate (link-time check). Unresolved symbols
    /// would fail to build, so this test merely takes the address of each
    /// to force the rustc usage check.
    #[test]
    fn ffi_symbols_resolve() {
        let _ = js_container_run as unsafe extern "C" fn(_) -> _;
        let _ = js_container_create as unsafe extern "C" fn(_) -> _;
        let _ = js_container_start as unsafe extern "C" fn(_) -> _;
        let _ = js_container_stop as unsafe extern "C" fn(_, _) -> _;
        let _ = js_container_remove as unsafe extern "C" fn(_, _) -> _;
        let _ = js_container_list as unsafe extern "C" fn(_) -> _;
        let _ = js_container_inspect as unsafe extern "C" fn(_) -> _;
        let _ = js_container_logs as unsafe extern "C" fn(_, _) -> _;
        let _ = js_container_pullImage as unsafe extern "C" fn(_) -> _;
        let _ = js_container_listImages as unsafe extern "C" fn() -> _;
        let _ = js_container_getBackend as unsafe extern "C" fn() -> _;
        let _ = js_container_module_init as extern "C" fn();
    }
}
