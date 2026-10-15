use crate::NativeError;
use libloading::Library;
use std::{
    ffi::{c_char, c_void, CStr},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
};

static CLIENT: Mutex<Option<Arc<Client>>> = Mutex::new(None);
static OWNER_PID: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" {
    fn fbfast_client_version(master: *mut c_void) -> u32;
    fn fbfast_client_root(master: *mut c_void, out: *mut c_char, size: usize);
    fn fbfast_client_signals(register_callback: *mut c_void) -> u32;
}

#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub path: PathBuf,
    pub version: u32,
    pub version_string: String,
    pub root: PathBuf,
}

pub struct Client {
    // Never unload a library while any of its interfaces may be alive.
    _library: Library,
    info: ClientInfo,
    pub(crate) master: crate::ffi::MasterFn,
    pub(crate) sqlstate: crate::ffi::SqlStateFn,
}

impl Client {
    pub(crate) fn check_process(&self) -> Result<(), NativeError> {
        if OWNER_PID.load(Ordering::SeqCst) != std::process::id() {
            return Err(NativeError::ForkSafety);
        }
        Ok(())
    }
    /// A single, canonical library is selected for the entire process.
    pub fn load(path: &Path) -> Result<Arc<Self>, NativeError> {
        let pid = std::process::id();
        let owner = OWNER_PID
            .compare_exchange(0, pid, Ordering::SeqCst, Ordering::SeqCst)
            .unwrap_or_else(|owner| owner);
        if owner != 0 && owner != pid {
            return Err(NativeError::ForkSafety);
        }
        let path = path.canonicalize().map_err(|e| NativeError::Library {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        let mut guard = CLIENT
            .lock()
            .map_err(|_| NativeError::InitializationPoisoned)?;
        if let Some(client) = guard.as_ref() {
            if client.info.path != path {
                return Err(NativeError::ClientConflict {
                    loaded: client.info.path.display().to_string(),
                    requested: path.display().to_string(),
                });
            }
            return Ok(Arc::clone(client));
        }
        let client = Arc::new(Self::open(path)?);
        *guard = Some(Arc::clone(&client));
        Ok(client)
    }

    fn open(path: PathBuf) -> Result<Self, NativeError> {
        let error = |e: libloading::Error| NativeError::Library {
            path: path.display().to_string(),
            reason: e.to_string(),
        };
        // SAFETY: explicit canonical path to an SDK-compatible client. The Library
        // is kept for process lifetime by CLIENT; no symbol can outlive it.
        let library = unsafe { Library::new(&path) }.map_err(error)?;
        // SAFETY: stable Firebird C ABI; documented version string fits 256 bytes.
        let version_string = unsafe {
            let get_version = library
                .get::<unsafe extern "C" fn(*mut c_char)>(b"isc_get_client_version\0")
                .map_err(error)?;
            let mut buffer = [0 as c_char; 256];
            get_version(buffer.as_mut_ptr());
            CStr::from_ptr(buffer.as_ptr())
                .to_string_lossy()
                .into_owned()
        };
        if !supported_version(&version_string) {
            return Err(NativeError::UnsupportedVersion(version_string));
        }
        // SAFETY: Firebird's published entry point has this exact signature.
        let get_master = unsafe {
            library.get::<unsafe extern "C" fn() -> *mut c_void>(b"fb_get_master_interface\0")
        }
        .map_err(error)?;
        // SAFETY: symbol is held by library. The returned interface is borrowed.
        let master = unsafe { get_master() };
        let master_fn = *get_master;
        // SAFETY: published stable C ABI, held alive by the Library owner.
        let sqlstate =
            *unsafe { library.get::<crate::ffi::SqlStateFn>(b"fb_sqlstate\0") }.map_err(error)?;
        if master.is_null() {
            return Err(NativeError::Library {
                path: path.display().to_string(),
                reason: "fb_get_master_interface returned null".into(),
            });
        }
        // SAFETY: non-null official master; output buffer is writable for its size.
        let (version, root) = unsafe {
            let version = fbfast_client_version(master);
            let mut root = [0 as c_char; 4096];
            fbfast_client_root(master, root.as_mut_ptr(), root.len());
            (
                version,
                CStr::from_ptr(root.as_ptr()).to_string_lossy().into_owned(),
            )
        };
        if version != 5 * 256 {
            return Err(NativeError::UnsupportedVersion(version_string));
        }
        // SAFETY: SDK-typed C++ helper receives the official function address.
        // Registration is process-wide and serialized by CLIENT, before attach.
        // The callback has static code/data lifetime and never enters Python.
        let signals = unsafe {
            let register = library
                .get::<*mut c_void>(b"fb_shutdown_callback\0")
                .map_err(error)?;
            fbfast_client_signals(*register)
        };
        if signals != 0 {
            return Err(NativeError::Library {
                path: path.display().to_string(),
                reason: format!("could not preserve host signal handling (stage {signals})"),
            });
        }
        Ok(Self {
            _library: library,
            master: master_fn,
            sqlstate,
            info: ClientInfo {
                path,
                version,
                version_string,
                root: root.into(),
            },
        })
    }

    pub fn info(&self) -> &ClientInfo {
        &self.info
    }
}

fn supported_version(banner: &str) -> bool {
    // C API retains legacy V6.3.<patch> prefix; actual major/minor follows Firebird.
    let Some((build, product)) = banner.split_once(" Firebird ") else {
        return false;
    };
    let patch = build.split('.').nth(2).and_then(|v| v.parse::<u32>().ok());
    product == "5.0" && patch.is_some_and(|patch| patch >= 3)
}

#[cfg(test)]
mod tests {
    use super::supported_version;

    #[test]
    fn validates_real_and_legacy_banners() {
        assert!(supported_version("UA-V6.3.3.1683 Firebird 5.0"));
        assert!(supported_version("LI-V5.0.4.1700 Firebird 5.0"));
        assert!(!supported_version("LI-V5.0.2.1600 Firebird 5.0"));
        assert!(!supported_version("LI-V6.0.3.1000 Firebird 6.0"));
        assert!(!supported_version("corrupt"));
    }
}
