//! CBXShell - Windows Shell Extension for Comic Book Archives
//!
//! Provides thumbnail preview and tooltips for ZIP, RAR, and 7z image archives
//! including CBZ, CBR, CB7 formats with WebP and AVIF image support.
//!
//! This is a pure Rust rewrite of the original C++ ATL/WTL implementation,
//! providing memory safety and modern archive format support.

#![allow(non_snake_case)]

use std::sync::atomic::{AtomicU32, Ordering};
use windows::{core::*, Win32::Foundation::*};

mod archive;
pub mod com;
mod image_processor;
pub mod registry;
mod utils;

pub use com::CBXShell;
pub use image_processor::thumbnail::create_thumbnail_with_size;
pub use utils::error::CbxError;

/// Global reference count for COM objects
/// Used to determine when DLL can be safely unloaded
static DLL_REF_COUNT: AtomicU32 = AtomicU32::new(0);

/// DLL module handle
/// Stored during DllMain to use for GetModuleFileNameW
static DLL_MODULE: std::sync::OnceLock<HINSTANCE> = std::sync::OnceLock::new();

/// Increment DLL reference count
pub fn add_dll_ref() {
    DLL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
}

/// Decrement DLL reference count
pub fn release_dll_ref() {
    DLL_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
}

/// Get current DLL reference count
pub fn get_dll_ref_count() -> u32 {
    DLL_REF_COUNT.load(Ordering::SeqCst)
}

/// Get the DLL module handle
pub fn get_dll_module() -> Option<HINSTANCE> {
    DLL_MODULE.get().copied()
}

/// DllMain entry point
///
/// Required by Windows when DLL is loaded/unloaded
#[no_mangle]
pub extern "system" fn DllMain(
    hinst_dll: HINSTANCE,
    fdw_reason: u32,
    _lpv_reserved: *mut std::ffi::c_void,
) -> BOOL {
    const DLL_PROCESS_ATTACH: u32 = 1;
    const DLL_PROCESS_DETACH: u32 = 0;

    match fdw_reason {
        DLL_PROCESS_ATTACH => {
            // Store the DLL module handle for use in GetModuleFileNameW
            let _ = DLL_MODULE.set(hinst_dll);

            // Initialize tracing for debugging
            #[cfg(debug_assertions)]
            {
                use tracing_subscriber::{fmt, EnvFilter};
                let _ = fmt()
                    .with_env_filter(EnvFilter::from_default_env())
                    .try_init();
            }
            tracing::info!("CBXShell DLL loaded");

            // CRITICAL: File-based debug logging to diagnose Explorer integration
            utils::debug_log::debug_log(
                "===== DLL_PROCESS_ATTACH - CBXShell DLL loaded by Explorer =====",
            );
            utils::debug_log::debug_log(&format!("DLL HINSTANCE: {:?}", hinst_dll));

            TRUE
        }
        DLL_PROCESS_DETACH => {
            tracing::info!("CBXShell DLL unloaded");
            utils::debug_log::debug_log("===== DLL_PROCESS_DETACH - CBXShell DLL unloaded =====");
            TRUE
        }
        _ => TRUE,
    }
}

/// DllCanUnloadNow
///
/// Determines whether the DLL can be unloaded from memory
/// Returns S_OK if no objects are in use, S_FALSE otherwise
#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    let ref_count = get_dll_ref_count();
    if ref_count == 0 {
        tracing::debug!("DllCanUnloadNow: S_OK (ref count = 0)");
        utils::debug_log::debug_log("DllCanUnloadNow: S_OK (ref count = 0)");
        S_OK
    } else {
        tracing::debug!("DllCanUnloadNow: S_FALSE (ref count = {})", ref_count);
        utils::debug_log::debug_log(&format!(
            "DllCanUnloadNow: S_FALSE (ref count = {})",
            ref_count
        ));
        S_FALSE
    }
}

/// DllGetClassObject
///
/// Returns a class factory for the requested CLSID
#[no_mangle]
pub extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut std::ffi::c_void,
) -> HRESULT {
    utils::debug_log::debug_log("===== DllGetClassObject CALLED =====");

    // UNAVOIDABLE UNSAFE: Dereferencing COM raw pointers for logging
    // Why unsafe is required:
    // 1. COM ABI: rclsid and riid are raw pointers from COM caller
    // 2. Standard COM interface: DllGetClassObject is COM specification
    // Safety guarantees:
    // - COM runtime ensures pointers are valid
    // - Null check performed below (line 127)
    // - Only dereferencing for read (no mutation)
    utils::debug_log::debug_log(&format!("CLSID requested: {:?}", unsafe { *rclsid }));
    utils::debug_log::debug_log(&format!("IID requested: {:?}", unsafe { *riid }));

    if ppv.is_null() {
        utils::debug_log::debug_log("ERROR: ppv is null pointer");
        return E_POINTER;
    }

    // UNAVOIDABLE UNSAFE: COM interface implementation
    // Why unsafe is required:
    // 1. COM ABI: All COM interfaces use raw pointers (C++ compatible)
    // 2. Output parameters: ppv is an "out" parameter (double pointer)
    // 3. GUID comparison: rclsid must be dereferenced to compare CLSIDs
    // 4. QueryInterface: COM method requires raw pointer manipulation
    //
    // Safety guarantees:
    // - ppv validated as non-null above
    // - Initialized to null_mut() before use
    // - rclsid validated by comparison
    // - Error propagation via HRESULT
    // - windows-rs handles reference counting
    unsafe {
        *ppv = std::ptr::null_mut();

        if *rclsid != com::CLSID_CBXSHELL {
            tracing::warn!("DllGetClassObject: CLASS_E_CLASSNOTAVAILABLE");
            utils::debug_log::debug_log("ERROR: CLSID does not match CLSID_CBXSHELL");
            utils::debug_log::debug_log(&format!("Expected: {:?}", com::CLSID_CBXSHELL));
            return CLASS_E_CLASSNOTAVAILABLE;
        }

        utils::debug_log::debug_log("CLSID matches - creating ClassFactory");

        // Create and return class factory
        match com::ClassFactory::new() {
            Ok(factory) => {
                utils::debug_log::debug_log("ClassFactory created successfully");

                // Cast to IUnknown and query for the requested interface
                match factory.cast::<IUnknown>() {
                    Ok(iunknown) => {
                        utils::debug_log::debug_log("ClassFactory cast to IUnknown succeeded");

                        match iunknown.query(riid, ppv as *mut _) {
                            S_OK => {
                                tracing::debug!("DllGetClassObject: S_OK");
                                utils::debug_log::debug_log(
                                    "DllGetClassObject: SUCCESS - Returning class factory",
                                );
                                S_OK
                            }
                            hr => {
                                tracing::error!(
                                    "DllGetClassObject QueryInterface failed: {:?}",
                                    hr
                                );
                                utils::debug_log::debug_log(&format!(
                                    "ERROR: QueryInterface failed with HRESULT: {:?}",
                                    hr
                                ));
                                hr
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("DllGetClassObject cast to IUnknown failed: {:?}", e);
                        utils::debug_log::debug_log(&format!(
                            "ERROR: Cast to IUnknown failed: {:?}",
                            e
                        ));
                        E_NOINTERFACE
                    }
                }
            }
            Err(e) => {
                tracing::error!("DllGetClassObject factory creation failed: {}", e);
                utils::debug_log::debug_log(&format!("ERROR: ClassFactory creation failed: {}", e));
                e.code()
            }
        }
    }
}

/// DllRegisterServer
///
/// Registers the COM server and shell extension handlers
#[no_mangle]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    const SELFREG_E_CLASS: HRESULT = HRESULT(0x80040201u32 as i32);

    // Pass None to use the DLL module handle (set in DllMain)
    match registry::register_server(None) {
        Ok(()) => {
            tracing::info!("DllRegisterServer: S_OK");
            S_OK
        }
        Err(e) => {
            tracing::error!("DllRegisterServer failed: {}", e);
            SELFREG_E_CLASS
        }
    }
}

/// DllUnregisterServer
///
/// Unregisters the COM server and shell extension handlers
#[no_mangle]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    const SELFREG_E_CLASS: HRESULT = HRESULT(0x80040201u32 as i32);

    match registry::unregister_server() {
        Ok(()) => {
            tracing::info!("DllUnregisterServer: S_OK");
            S_OK
        }
        Err(e) => {
            tracing::error!("DllUnregisterServer failed: {}", e);
            SELFREG_E_CLASS
        }
    }
}
