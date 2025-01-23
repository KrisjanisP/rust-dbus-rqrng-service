use crate::error::Error;
use libc;
use std::convert::TryFrom;
use std::mem::MaybeUninit;
use cfg_if::cfg_if;

/// Retrieves the last OS error.
fn last_os_error() -> Error {
    cfg_if! {
        if #[cfg(target_os = "linux")] {
            let errno: libc::c_int = unsafe { *libc::__errno_location() };
            match u32::try_from(errno) {
                Ok(code) if code != 0 => Error::OsError(code),
                _ => Error::ErrnoNotPositive,
            }
        } else {
            // For non-Linux systems, this function should not be called.
            Error::Unexpected
        }
    }
}

/// Fill a buffer by repeatedly invoking `sys_fill`.
///
/// The `sys_fill` function:
///   - should return -1 and set errno on failure
///   - should return the number of bytes written on success
fn sys_fill_exact(
    mut buf: &mut [MaybeUninit<u8>],
    sys_fill: impl Fn(&mut [MaybeUninit<u8>]) -> libc::ssize_t,
) -> Result<(), Error> {
    while !buf.is_empty() {
        let res = sys_fill(buf);
        match res {
            res if res > 0 => {
                let len = usize::try_from(res).map_err(|_| Error::Unexpected)?;
                buf = buf.get_mut(len..).ok_or(Error::Unexpected)?;
            }
            -1 => {
                let err = last_os_error();
                // Retry if the call was interrupted.
                if err != Error::OsError(libc::EINTR as u32) {
                    return Err(err);
                }
            }
            // Negative return codes not equal to -1 should be impossible.
            // EOF (ret = 0) should be impossible, as the data we are reading
            // should be an infinite stream of random bytes.
            _ => return Err(Error::Unexpected),
        }
    }
    Ok(())
}

/// Fills the buffer with random octets using the Linux `getrandom` syscall.
///
/// # Arguments
///
/// * `num_octets` - The number of random octets to generate.
///
/// # Returns
///
/// A `Result` containing the vector of random octets on success, or an `Error` on failure.
pub fn os_fill_rand_octets(num_octets: usize) -> Result<Vec<u8>, Error> {
    // Allocate a buffer with uninitialized memory
    let mut buffer: Vec<MaybeUninit<u8>> = Vec::with_capacity(num_octets);
    // It's safe to assume the capacity is set correctly
    unsafe { buffer.set_len(num_octets) }

    // Fill the buffer with random bytes
    sys_fill_exact(&mut buffer, |buffer| unsafe {
        libc::getrandom(
            buffer.as_mut_ptr() as *mut libc::c_void,
            buffer.len(),
            0, // Flags: 0 to use the default entropy pool
        )
    })?;

    // Convert to initialized bytes
    // Safety: We just filled the entire buffer with valid random bytes
    let initialized: Vec<u8> = unsafe {
        std::mem::transmute::<Vec<MaybeUninit<u8>>, Vec<u8>>(buffer)
    };
    Ok(initialized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fill_random_octets_success() {
        let num_octets = 16;
        let result = os_fill_rand_octets(num_octets);
        assert!(result.is_ok());
        let octets = result.unwrap();
        assert_eq!(octets.len(), num_octets);
    }

    #[test]
    fn test_fill_random_octets_zero() {
        let num_octets = 0;
        let result = os_fill_rand_octets(num_octets);
        assert!(result.is_ok());
        let octets = result.unwrap();
        assert_eq!(octets.len(), num_octets);
    }

    #[test]
    fn test_fill_random_octets_max() {
        let num_octets = 1024;
        let result = os_fill_rand_octets(num_octets);
        assert!(result.is_ok());
        let octets = result.unwrap();
        assert_eq!(octets.len(), num_octets);
    }
}
