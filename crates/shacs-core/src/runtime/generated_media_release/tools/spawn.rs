#[cfg(not(test))]
use super::ExecutionLedger;
use super::Spec034ReleaseArtifactError;
#[cfg(not(test))]
use std::fs::File;
#[cfg(not(test))]
use std::process::Command;

mod child;
pub(crate) use child::ExecutionChild;
mod identity;
#[cfg(not(test))]
pub(crate) use identity::capture_observed_process_identity;
pub(crate) use identity::{
    capture_process_identity, capture_static_identity, static_identity_matches, ProcessIdentity,
};

#[cfg(target_vendor = "apple")]
pub(super) fn static_cdhash(
    path: &std::path::Path,
) -> Result<Vec<u8>, Spec034ReleaseArtifactError> {
    darwin::static_cdhash(path)
}

#[cfg(not(target_vendor = "apple"))]
pub(super) fn static_cdhash(
    _path: &std::path::Path,
) -> Result<Vec<u8>, Spec034ReleaseArtifactError> {
    Err(Spec034ReleaseArtifactError::InvalidConfig)
}

#[cfg(target_vendor = "apple")]
pub(super) fn live_cdhash(pid: libc::pid_t) -> Result<Vec<u8>, Spec034ReleaseArtifactError> {
    darwin::live_cdhash(pid)
}

#[cfg(target_vendor = "apple")]
pub(super) fn process_executable(
    pid: libc::pid_t,
) -> Result<std::path::PathBuf, Spec034ReleaseArtifactError> {
    use std::os::unix::ffi::OsStringExt;

    let mut bytes = vec![0_u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let capacity = u32::try_from(bytes.len())
        .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    // SAFETY: [Category 8 - FFI boundary] `bytes` is writable for `capacity` bytes and
    // `proc_pidpath` only writes within the caller-supplied buffer.
    let count = unsafe { libc::proc_pidpath(pid, bytes.as_mut_ptr().cast(), capacity) };
    if count <= 0 {
        return Err(Spec034ReleaseArtifactError::Io(
            std::io::Error::last_os_error(),
        ));
    }
    bytes.truncate(
        usize::try_from(count).map_err(|_| Spec034ReleaseArtifactError::InvalidEvidence)?,
    );
    Ok(std::path::PathBuf::from(std::ffi::OsString::from_vec(
        bytes,
    )))
}

#[cfg(all(not(test), target_vendor = "apple"))]
pub(super) fn spawn_verified(
    command: &Command,
    stdout: &File,
    stderr: &File,
    ledger: &ExecutionLedger,
) -> Result<ExecutionChild, Spec034ReleaseArtifactError> {
    darwin::spawn_verified(command, stdout, stderr, ledger)
}

#[cfg(all(not(test), not(target_vendor = "apple")))]
pub(super) fn spawn_verified(
    _command: &Command,
    _stdout: &File,
    _stderr: &File,
    _ledger: &ExecutionLedger,
) -> Result<ExecutionChild, Spec034ReleaseArtifactError> {
    Err(Spec034ReleaseArtifactError::InvalidConfig)
}

#[cfg(target_vendor = "apple")]
mod darwin {
    use super::*;
    use core_foundation::base::TCFType;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::url::CFURL;
    use core_foundation_sys::base::{CFRelease, CFTypeRef, OSStatus};
    use core_foundation_sys::data::{CFDataGetBytePtr, CFDataGetLength, CFDataRef};
    use core_foundation_sys::dictionary::{CFDictionaryGetValue, CFDictionaryRef};
    use security_framework_sys::code_signing::*;
    #[cfg(not(test))]
    use std::ffi::{CString, OsStr};
    #[cfg(not(test))]
    use std::os::fd::AsRawFd;
    #[cfg(not(test))]
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    unsafe extern "C" {
        static kSecCodeInfoUnique: core_foundation_sys::string::CFStringRef;
        fn SecCodeCopySigningInformation(
            code: SecStaticCodeRef,
            flags: SecCSFlags,
            information: *mut CFDictionaryRef,
        ) -> OSStatus;
        #[cfg(not(test))]
        fn posix_spawn_file_actions_addchdir_np(
            actions: *mut libc::posix_spawn_file_actions_t,
            path: *const libc::c_char,
        ) -> libc::c_int;
    }

    #[cfg(not(test))]
    pub(super) fn spawn_verified(
        command: &Command,
        stdout: &File,
        stderr: &File,
        ledger: &ExecutionLedger,
    ) -> Result<ExecutionChild, Spec034ReleaseArtifactError> {
        let executable = Path::new(command.get_program());
        let expected = static_cdhash(executable)?;
        let program = c_string(command.get_program())?;
        let arguments = std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(c_string)
            .collect::<Result<Vec<_>, _>>()?;
        let environment = command
            .get_envs()
            .filter_map(|(key, value)| value.map(|value| (key, value)))
            .map(|(key, value)| {
                let mut bytes = key.as_bytes().to_vec();
                bytes.push(b'=');
                bytes.extend(value.as_bytes());
                CString::new(bytes).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let argv = pointer_array(&arguments);
        let envp = pointer_array(&environment);
        let mut actions = std::mem::MaybeUninit::<libc::posix_spawn_file_actions_t>::uninit();
        let mut attributes = std::mem::MaybeUninit::<libc::posix_spawnattr_t>::uninit();
        // SAFETY: [Category 4 - uninitialized memory] `actions` points to writable storage
        // sized and aligned for one posix_spawn_file_actions_t value.
        spawn_call(unsafe { libc::posix_spawn_file_actions_init(actions.as_mut_ptr()) })?;
        // SAFETY: [Category 4 - uninitialized memory] the successful initializer above wrote
        // the complete file-actions object.
        let mut actions = unsafe { actions.assume_init() };
        spawn_call(unsafe { libc::posix_spawn_file_actions_adddup2(&mut actions, stdout.as_raw_fd(), 1) })?;
        spawn_call(unsafe { libc::posix_spawn_file_actions_adddup2(&mut actions, stderr.as_raw_fd(), 2) })?;
        let cwd = c_string(
            command
                .get_current_dir()
                .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
                .as_os_str(),
        )?;
        // SAFETY: [Category 8 - FFI boundary] `actions` is initialized and `cwd` is a live,
        // NUL-terminated absolute path for the duration of the call.
        spawn_call(unsafe { posix_spawn_file_actions_addchdir_np(&mut actions, cwd.as_ptr()) })?;
        spawn_call(unsafe { libc::posix_spawnattr_init(attributes.as_mut_ptr()) })?;
        // SAFETY: [Category 4 - uninitialized memory] the successful initializer above wrote
        // the complete spawn-attribute object.
        let mut attributes = unsafe { attributes.assume_init() };
        let flags = libc::POSIX_SPAWN_START_SUSPENDED
            | libc::POSIX_SPAWN_SETPGROUP
            | libc::POSIX_SPAWN_CLOEXEC_DEFAULT;
        spawn_call(unsafe { libc::posix_spawnattr_setflags(&mut attributes, flags as i16) })?;
        spawn_call(unsafe { libc::posix_spawnattr_setpgroup(&mut attributes, 0) })?;
        ledger.verify()?;
        let mut pid = 0;
        // SAFETY: [Category 8 - FFI boundary] all C strings and pointer arrays remain alive for
        // the call, arrays are NULL-terminated, and initialized spawn structures are supplied.
        let status = unsafe {
            libc::posix_spawn(
                &mut pid,
                program.as_ptr(),
                &actions,
                &attributes,
                argv.as_ptr(),
                envp.as_ptr(),
            )
        };
        // SAFETY: [Category 13 - library contract] both destroy calls receive objects that were
        // successfully initialized and are no longer used afterward.
        unsafe {
            libc::posix_spawn_file_actions_destroy(&mut actions);
            libc::posix_spawnattr_destroy(&mut attributes);
        }
        spawn_call(status)?;
        let identity = capture_process_identity(pid)?;
        let child = ExecutionChild { pid, identity, status: None, cleaned: false };
        if live_cdhash(pid)? != expected || ledger.verify().is_err() {
            // SAFETY: [Category 8 - FFI boundary] `pid` is a positive child PID returned by
            // posix_spawn and SIGKILL is a valid signal number.
            unsafe { libc::kill(pid, libc::SIGKILL) };
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        // SAFETY: [Category 8 - FFI boundary] the child is intentionally suspended and the
        // validated positive PID may be resumed with SIGCONT.
        if unsafe { libc::kill(pid, libc::SIGCONT) } != 0 {
            return Err(Spec034ReleaseArtifactError::Io(std::io::Error::last_os_error()));
        }
        Ok(child)
    }

    pub(super) fn static_cdhash(path: &Path) -> Result<Vec<u8>, Spec034ReleaseArtifactError> {
        let url = CFURL::from_path(path, false).ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
        let mut code = std::ptr::null_mut();
        // SAFETY: [Category 8 - FFI boundary] URL is a live CF object and `code` is writable;
        // success guarantees a retained non-null SecStaticCodeRef.
        status(unsafe { SecStaticCodeCreateWithPath(url.as_concrete_TypeRef(), 0, &mut code) })?;
        // SAFETY: [Category 8 - FFI boundary] successful creation returned a live static-code
        // object and the null requirement requests intrinsic signature validation.
        status(unsafe {
            SecStaticCodeCheckValidity(
                code,
                kSecCSStrictValidate | kSecCSNoNetworkAccess,
                std::ptr::null_mut(),
            )
        })?;
        let result = cdhash(code);
        // SAFETY: [Category 12 - invalid free] Security returned one retained CF object on the
        // successful create call and it is released exactly once here.
        unsafe { CFRelease(code.cast()) };
        result
    }

    pub(super) fn live_cdhash(pid: libc::pid_t) -> Result<Vec<u8>, Spec034ReleaseArtifactError> {
        let number = CFNumber::from(pid);
        let key = unsafe { core_foundation::string::CFString::wrap_under_get_rule(kSecGuestAttributePid) };
        let attrs = CFDictionary::from_CFType_pairs(&[(key, number)]);
        let mut code = std::ptr::null_mut();
        // SAFETY: [Category 8 - FFI boundary] the dictionary contains the required numeric PID
        // attribute and `code` is writable; success returns one retained SecCodeRef.
        status(unsafe { SecCodeCopyGuestWithAttributes(std::ptr::null_mut(), attrs.as_concrete_TypeRef(), 0, &mut code) })?;
        // SAFETY: [Category 8 - FFI boundary] the guest lookup returned a live code object and
        // the null requirement requests intrinsic live-code validation.
        status(unsafe {
            SecCodeCheckValidity(
                code,
                kSecCSStrictValidate | kSecCSNoNetworkAccess,
                std::ptr::null_mut(),
            )
        })?;
        let result = cdhash(code.cast());
        // SAFETY: [Category 12 - invalid free] the retained guest object is released once.
        unsafe { CFRelease(code.cast()) };
        result
    }

    fn cdhash(code: SecStaticCodeRef) -> Result<Vec<u8>, Spec034ReleaseArtifactError> {
        let mut information = std::ptr::null();
        status(unsafe { SecCodeCopySigningInformation(code, 1 << 1, &mut information) })?;
        let value = unsafe { CFDictionaryGetValue(information, kSecCodeInfoUnique.cast()) } as CFDataRef;
        if value.is_null() {
            unsafe { CFRelease(information.cast()) };
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        let length = usize::try_from(unsafe { CFDataGetLength(value) })
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
        let bytes = unsafe { std::slice::from_raw_parts(CFDataGetBytePtr(value), length) }.to_vec();
        unsafe { CFRelease(information as CFTypeRef) };
        Ok(bytes)
    }

    #[cfg(not(test))]
    fn c_string(value: &OsStr) -> Result<CString, Spec034ReleaseArtifactError> {
        CString::new(value.as_bytes()).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)
    }

    #[cfg(not(test))]
    fn pointer_array(values: &[CString]) -> Vec<*mut libc::c_char> {
        values
            .iter()
            .map(|value| value.as_ptr().cast_mut())
            .chain(std::iter::once(std::ptr::null_mut()))
            .collect()
    }

    #[cfg(not(test))]
    fn spawn_call(status: libc::c_int) -> Result<(), Spec034ReleaseArtifactError> {
        (status == 0).then_some(()).ok_or_else(|| {
            Spec034ReleaseArtifactError::Io(std::io::Error::from_raw_os_error(status))
        })
    }

    fn status(value: OSStatus) -> Result<(), Spec034ReleaseArtifactError> {
        (value == 0).then_some(()).ok_or(Spec034ReleaseArtifactError::InvalidConfig)
    }
}
