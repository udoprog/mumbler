use core::ffi::CStr;
use core::ffi::c_int;
use core::mem;
use core::ptr;
use core::ptr::NonNull;

use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use libc::{self, mode_t, wchar_t};

pub(super) fn copy(dest: &mut [wchar_t], src: &str) {
    let mut index = 0;

    for ch in src.chars().take(dest.len().saturating_sub(1)) {
        dest[index] = ch as wchar_t;
        index += 1;
    }

    dest[index] = 0;
}

#[cfg(target_os = "linux")]
unsafe fn shm_open(path: &CStr, flag: c_int, mode: mode_t) -> i32 {
    unsafe { libc::shm_open(path.as_ptr(), flag, mode) }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn shm_open(path: &CStr, flag: c_int, mode: mode_t) -> i32 {
    unsafe { libc::shm_open(path.as_ptr(), flag, mode as c_int) }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "ios")))]
compile_error!("Unsupported platform");

pub(super) struct Map<T> {
    pub(super) ptr: NonNull<T>,
    _fd: OwnedFd,
}

impl<T> Map<T> {
    pub(super) fn new() -> io::Result<Map<T>> {
        unsafe {
            let path = CString::new(format!("/MumbleLink.{}", libc::getuid())).unwrap();

            let fd = shm_open(&path, libc::O_RDWR, libc::S_IRUSR | libc::S_IWUSR);

            if fd < 0 {
                return Err(io::Error::last_os_error());
            }

            let fd = OwnedFd::from_raw_fd(fd);

            let ptr = libc::mmap(
                ptr::null_mut(),
                mem::size_of::<T>(),
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            );

            if ptr as isize == -1 {
                return Err(io::Error::last_os_error());
            }

            let ptr = ptr.cast::<T>();

            if !ptr.is_aligned() {
                return Err(io::Error::other("Mapped pointer is not properly aligned"));
            }

            Ok(Map {
                ptr: NonNull::new_unchecked(ptr),
                _fd: fd,
            })
        }
    }
}

impl<T> Drop for Map<T> {
    fn drop(&mut self) {
        unsafe {
            _ = libc::munmap(self.ptr.as_ptr().cast(), mem::size_of::<T>());
        }
    }
}
