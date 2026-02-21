use std::sync::atomic::AtomicU32;
///! COM Class Factory implementation
use windows::{core::*, Win32::Foundation::*, Win32::System::Com::*};

use super::CBXShell;

/// ClassFactory for creating CBXShell instances
#[implement(IClassFactory)]
pub struct ClassFactory {
    #[allow(dead_code)] // Used by COM infrastructure through #[implement] macro
    ref_count: AtomicU32,
}

impl ClassFactory {
    /// Create a new class factory
    pub fn new() -> Result<IClassFactory> {
        tracing::debug!("Creating ClassFactory");

        let factory = ClassFactory {
            ref_count: AtomicU32::new(1),
        };

        crate::add_dll_ref();
        Ok(factory.into())
    }
}

impl IClassFactory_Impl for ClassFactory {
    /// Create an instance of CBXShell
    fn CreateInstance(
        &self,
        punkouter: Option<&IUnknown>,
        riid: *const GUID,
        ppv: *mut *mut std::ffi::c_void,
    ) -> Result<()> {
        tracing::debug!("ClassFactory::CreateInstance called");
        crate::utils::debug_log::debug_log("===== ClassFactory::CreateInstance CALLED =====");
        crate::utils::debug_log::debug_log(&format!("IID requested: {:?}", unsafe { *riid }));

        // Aggregation not supported
        if punkouter.is_some() {
            tracing::warn!("Aggregation not supported");
            crate::utils::debug_log::debug_log("ERROR: Aggregation not supported");
            return Err(CLASS_E_NOAGGREGATION.into());
        }

        unsafe {
            // Create CBXShell instance
            crate::utils::debug_log::debug_log("Creating CBXShell instance...");
            let cbxshell = CBXShell::new()?;
            crate::utils::debug_log::debug_log("CBXShell instance created");

            // Cast to IUnknown and query for requested interface
            match cbxshell.cast::<IUnknown>() {
                Ok(iunknown) => {
                    crate::utils::debug_log::debug_log("CBXShell cast to IUnknown succeeded");

                    match iunknown.query(riid, ppv as *mut _) {
                        S_OK => {
                            tracing::debug!("CBXShell instance created successfully");
                            crate::utils::debug_log::debug_log(
                                "SUCCESS: QueryInterface succeeded - CBXShell instance returned",
                            );
                            Ok(())
                        }
                        hr => {
                            crate::utils::debug_log::debug_log(&format!(
                                "ERROR: QueryInterface failed with HRESULT: {:?}",
                                hr
                            ));
                            Err(Error::from(hr))
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to cast CBXShell to IUnknown: {:?}", e);
                    crate::utils::debug_log::debug_log(&format!(
                        "ERROR: Cast to IUnknown failed: {:?}",
                        e
                    ));
                    Err(Error::from(E_NOINTERFACE))
                }
            }
        }
    }

    /// Lock/unlock server in memory
    fn LockServer(&self, flock: BOOL) -> Result<()> {
        if flock.as_bool() {
            crate::add_dll_ref();
            tracing::debug!("Server locked");
        } else {
            crate::release_dll_ref();
            tracing::debug!("Server unlocked");
        }
        Ok(())
    }
}

impl Drop for ClassFactory {
    fn drop(&mut self) {
        crate::release_dll_ref();
        tracing::debug!("ClassFactory dropped");
    }
}
