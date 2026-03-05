use core::mem;
use core::ptr::NonNull;

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use libc::wchar_t;
use windows_sys::Win32::Foundation::FALSE;
use windows_sys::Win32::System::Memory::{FILE_MAP_ALL_ACCESS, MapViewOfFile, OpenFileMappingW};

macro_rules! wide {
    ($($b:literal),*) => {
        &[$($b as wchar_t,)* 0]
    }
}

const NAME: &'static [wchar_t] = wide!(b'M', b'u', b'm', b'b', b'l', b'e', b'L', b'i', b'n', b'k');

pub(super) fn copy(dest: &mut [wchar_t], src: &str) {
    let mut index = 0;

    for ch in src.encode_utf16().take(dest.len().saturating_sub(1)) {
        dest[index] = ch;
        index += 1;
    }

    dest[index] = 0;
}

pub(super) struct Map<T> {
    pub(super) ptr: NonNull<T>,
    _handle: OwnedHandle,
}

impl<T> Map<T> {
    pub(super) fn new() -> io::Result<Self> {
        unsafe {
            let handle = OpenFileMappingW(FILE_MAP_ALL_ACCESS, FALSE, NAME.as_ptr());

            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }

            let handle = OwnedHandle::from_raw_handle(handle);

            let ptr = MapViewOfFile(
                handle.as_raw_handle(),
                FILE_MAP_ALL_ACCESS,
                0,
                0,
                mem::size_of::<T>(),
            );

            if ptr.Value.is_null() {
                return Err(io::Error::last_os_error());
            }

            let ptr = ptr.Value.cast::<T>();

            if !ptr.is_aligned() {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Mapped pointer is not properly aligned",
                ));
            }

            Ok(Self {
                ptr: NonNull::new_unchecked(ptr),
                _handle: handle,
            })
        }
    }
}
