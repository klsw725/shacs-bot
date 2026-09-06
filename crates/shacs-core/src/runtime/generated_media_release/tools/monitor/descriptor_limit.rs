use super::Spec034ReleaseArtifactError;

const DESCRIPTORS_PER_PATH: usize = 2;
const DESCRIPTOR_HEADROOM: usize = 256;

pub(super) fn ensure(path_count: usize) -> Result<(), Spec034ReleaseArtifactError> {
    let required = path_count
        .checked_mul(DESCRIPTORS_PER_PATH)
        .and_then(|count| count.checked_add(DESCRIPTOR_HEADROOM))
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let required = libc::rlim_t::try_from(required)
        .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    let mut limits = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    // SAFETY: [Category 4 - uninitialized memory] `getrlimit` initializes the complete output
    // on success, and the value is only assumed initialized after a zero return status.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limits.as_mut_ptr()) } != 0 {
        return Err(Spec034ReleaseArtifactError::Io(std::io::Error::last_os_error()));
    }
    // SAFETY: [Category 4 - uninitialized memory] the successful call above initialized it.
    let mut limits = unsafe { limits.assume_init() };
    if limits.rlim_cur >= required {
        return Ok(());
    }
    if limits.rlim_max < required {
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
    }
    limits.rlim_cur = required;
    // SAFETY: [Category 8 - FFI boundary] `limits` is fully initialized and preserves the hard
    // limit returned by the kernel; only the current process soft limit is raised.
    if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limits) } != 0 {
        return Err(Spec034ReleaseArtifactError::Io(std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_limit_covers_two_handles_per_path_and_headroom() {
        assert_eq!(
            10usize
                .checked_mul(DESCRIPTORS_PER_PATH)
                .and_then(|count| count.checked_add(DESCRIPTOR_HEADROOM)),
            Some(276)
        );
    }
}
