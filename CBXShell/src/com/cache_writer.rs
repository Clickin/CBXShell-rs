//! Bounded, best-effort cache publication without retaining archive streams.

use std::sync::atomic::{AtomicUsize, Ordering};

use cbxshell_thumbnail_cache::key::{CacheVariant, SourceIdentity};
use cbxshell_thumbnail_cache::store::ThumbnailCache;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{
    FreeLibraryAndExitThread, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
};
use windows::Win32::System::Threading::{CreateThread, THREAD_CREATION_FLAGS};

use super::l2_cache;
use crate::image_processor::decoder::preview::DecodedPreview;

// Avoid an unbounded queue of full-size proxies during a large folder refresh.
const MAX_WRITERS: usize = 2;
static ACTIVE_WRITERS: AtomicUsize = AtomicUsize::new(0);

struct WriteJob {
    cache: ThumbnailCache,
    identity: SourceIdentity,
    variant: CacheVariant,
    proxy: DecodedPreview,
    module: HMODULE,
}

fn write_cache(job: &WriteJob) {
    match l2_cache::encode_proxy(job.proxy.width, job.proxy.height, &job.proxy.rgba) {
        Ok(encoded) => match job.cache.store(&job.identity, &job.variant, &encoded) {
            Ok(()) => crate::utils::debug_log::debug_log(&format!(
                "L2 CACHE STORE (source={}, proxy={}x{}, bytes={})",
                l2_cache::source_id_tag(&job.identity),
                job.proxy.width,
                job.proxy.height,
                encoded.len()
            )),
            Err(e) => crate::utils::debug_log::debug_log(&format!("L2 CACHE STORE FAILED: {}", e)),
        },
        Err(e) => crate::utils::debug_log::debug_log(&format!("L2 CACHE STORE FAILED: {}", e)),
    }
}

unsafe extern "system" fn writer_entry(context: *mut std::ffi::c_void) -> u32 {
    let job = unsafe { Box::from_raw(context.cast::<WriteJob>()) };
    let module = job.module;
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| write_cache(&job)));
    drop(job);
    ACTIVE_WRITERS.fetch_sub(1, Ordering::Release);
    // The OS unloads the DLL and exits the thread atomically; returning from
    // DLL code after FreeLibrary would race COM's DLL unload.
    if !module.is_invalid() {
        unsafe { FreeLibraryAndExitThread(module, 0) }
    }
    0
}

pub(super) fn queue(
    cache: ThumbnailCache,
    identity: SourceIdentity,
    variant: CacheVariant,
    proxy: DecodedPreview,
) {
    if ACTIVE_WRITERS
        .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |n| {
            (n < MAX_WRITERS).then_some(n + 1)
        })
        .is_err()
    {
        return;
    }

    // Integration tests link this crate into the test executable. In the DLL,
    // acquire an OS loader reference in addition to COM's object references.
    let mut module = HMODULE::default();
    if crate::get_dll_module().is_some()
        && unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                PCWSTR(writer_entry as *const () as *const u16),
                &mut module,
            )
        }
        .is_err()
    {
        ACTIVE_WRITERS.fetch_sub(1, Ordering::Release);
        return;
    }

    let context = Box::into_raw(Box::new(WriteJob {
        cache,
        identity,
        variant,
        proxy,
        module,
    }));
    match unsafe {
        CreateThread(
            None,
            0,
            Some(writer_entry),
            Some(context.cast()),
            THREAD_CREATION_FLAGS(0),
            None,
        )
    } {
        Ok(handle) => {
            let _ = unsafe { CloseHandle(handle) };
        }
        Err(_) => {
            drop(unsafe { Box::from_raw(context) });
            ACTIVE_WRITERS.fetch_sub(1, Ordering::Release);
            if !module.is_invalid() {
                let _ = unsafe { FreeLibrary(module) };
            }
        }
    }
}
