//! [<img alt="github" src="https://img.shields.io/badge/github-udoprog/mumbler-8da0cb?style=for-the-badge&logo=github" height="20">](https://github.com/udoprog/mumbler)
//! [<img alt="crates.io" src="https://img.shields.io/crates/v/mumbler.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/mumbler)
//! [<img alt="docs.rs" src="https://img.shields.io/badge/docs.rs-mumbler-66c2a5?style=for-the-badge&logoColor=white&logo=data:image/svg+xml;base64,PHN2ZyByb2xlPSJpbWciIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyIgdmlld0JveD0iMCAwIDUxMiA1MTIiPjxwYXRoIGZpbGw9IiNmNWY1ZjUiIGQ9Ik00ODguNiAyNTAuMkwzOTIgMjE0VjEwNS41YzAtMTUtOS4zLTI4LjQtMjMuNC0zMy43bC0xMDAtMzcuNWMtOC4xLTMuMS0xNy4xLTMuMS0yNS4zIDBsLTEwMCAzNy41Yy0xNC4xIDUuMy0yMy40IDE4LjctMjMuNCAzMy43VjIxNGwtOTYuNiAzNi4yQzkuMyAyNTUuNSAwIDI2OC45IDAgMjgzLjlWMzk0YzAgMTMuNiA3LjcgMjYuMSAxOS45IDMyLjJsMTAwIDUwYzEwLjEgNS4xIDIyLjEgNS4xIDMyLjIgMGwxMDMuOS01MiAxMDMuOSA1MmMxMC4xIDUuMSAyMi4xIDUuMSAzMi4yIDBsMTAwLTUwYzEyLjItNi4xIDE5LjktMTguNiAxOS45LTMyLjJWMjgzLjljMC0xNS05LjMtMjguNC0yMy40LTMzLjd6TTM1OCAyMTQuOGwtODUgMzEuOXYtNjguMmw4NS0zN3Y3My4zek0xNTQgMTA0LjFsMTAyLTM4LjIgMTAyIDM4LjJ2LjZsLTEwMiA0MS40LTEwMi00MS40di0uNnptODQgMjkxLjFsLTg1IDQyLjV2LTc5LjFsODUtMzguOHY3NS40em0wLTExMmwtMTAyIDQxLjQtMTAyLTQxLjR2LS42bDEwMi0zOC4yIDEwMiAzOC4ydi42em0yNDAgMTEybC04NSA0Mi41di03OS4xbDg1LTM4Ljh2NzUuNHptMC0xMTJsLTEwMiA0MS40LTEwMi00MS40di0uNmwxMDItMzguMiAxMDIgMzguMnYuNnoiPjwvcGF0aD48L3N2Zz4K" height="20">](https://docs.rs/mumbler)
//!
//! Client side Mumble Link implementation.
//!
//! See the [`mumbler`] example for usage.
//!
//! [`mumbler`]: https://github.com/udoprog/mumbler/blob/main/examples/mumbler.rs

// This is a rewrite of mumble-link, which was copied under the MIT license.
//
// See: https://github.com/SpaceManiac/mumble-link-rs

use core::mem;
use core::ptr;
use core::sync::atomic::AtomicU32;

use core::sync::atomic::Ordering;
use std::io;

use libc::wchar_t;

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(not(windows), path = "unix.rs")]
mod imp;

const VERSION_FLAG: u8 = 0b0000_0001;
const NAME_FLAG: u8 = 0b0000_0010;
const DESCRIPTION_FLAG: u8 = 0b0000_0100;
const CONTEXT_FLAG: u8 = 0b0000_1000;
const IDENTITY_FLAG: u8 = 0b0001_0000;
const AVATAR_FLAG: u8 = 0b0010_0000;
const CAMERA_FLAG: u8 = 0b0100_0000;
const ALL_FLAGS: u8 = 0b0111_1111;

/// A position in three-dimensional space.
///
/// The vectors are in a left-handed coordinate system: X positive towards
/// "right", Y positive towards "up", and Z positive towards "front". One unit
/// is treated as one meter by the sound engine.
///
/// `front` and `top` should be unit vectors and perpendicular to each other.
#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(C)]
pub struct Position {
    /// The character's position in space.
    pub position: [f32; 3],
    /// A unit vector pointing out of the character's eyes.
    pub front: [f32; 3],
    /// A unit vector pointing out of the top of the character's head.
    pub top: [f32; 3],
}

impl Position {
    /// An empty position, with all vectors set the zero position rotated forward.
    pub const FORWARD: Self = Position::new([0., 0., 0.]);

    /// An empty position, with all vectors set to zero.
    pub const ZERO: Self = Position {
        position: [f32::from_bits(0); 3],
        front: [f32::from_bits(0); 3],
        top: [f32::from_bits(0); 3],
    };

    /// Construct a position at the given position with default rotation.
    pub const fn new(position: [f32; 3]) -> Self {
        Self {
            position,
            front: [0., 0., 1.],
            top: [0., 1., 0.],
        }
    }
}

#[repr(C)]
struct Header {
    ui_version: AtomicU32,
    ui_tick: AtomicU32,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Body {
    avatar: Position,
    name: [wchar_t; 256],
    camera: Position,
    identity: [wchar_t; 256],
    context_len: u32,
    context: [u8; 256],
    description: [wchar_t; 2048],
}

const _: () = const {
    // Assert that Body has no padding between fields or at the end.
    assert!(
        mem::offset_of!(Body, avatar) == 0,
        "Body must not be padded"
    );
    assert!(
        mem::offset_of!(Body, name) == mem::offset_of!(Body, avatar) + mem::size_of::<Position>(),
        "Body must not be padded"
    );
    assert!(
        mem::offset_of!(Body, camera)
            == mem::offset_of!(Body, name) + mem::size_of::<[wchar_t; 256]>(),
        "Body must not be padded"
    );
    assert!(
        mem::offset_of!(Body, identity)
            == mem::offset_of!(Body, camera) + mem::size_of::<Position>(),
        "Body must not be padded"
    );
    assert!(
        mem::offset_of!(Body, context_len)
            == mem::offset_of!(Body, identity) + mem::size_of::<[wchar_t; 256]>(),
        "Body must not be padded"
    );
    assert!(
        mem::offset_of!(Body, context)
            == mem::offset_of!(Body, context_len) + mem::size_of::<u32>(),
        "Body must not be padded"
    );
    assert!(
        mem::offset_of!(Body, description)
            == mem::offset_of!(Body, context) + mem::size_of::<[u8; 256]>(),
        "Body must not be padded"
    );
    assert!(
        mem::size_of::<Body>()
            == mem::offset_of!(Body, description) + mem::size_of::<[wchar_t; 2048]>(),
        "Body must not be padded"
    );
};

impl Body {
    #[inline]
    const fn zero() -> Self {
        Self {
            avatar: Position::ZERO,
            name: [0; 256],
            camera: Position::ZERO,
            identity: [0; 256],
            context_len: 0,
            context: [0; 256],
            description: [0; 2048],
        }
    }

    /// Zero out all the non-atomic fields.
    ///
    /// We want to avoid incidentally sending a signal that the struct has been
    /// updated on platforms which support it despite the writes not being
    /// synchronized, so we avoid the fields which are used to signal an update.
    #[inline]
    unsafe fn zero_non_atomic(this: *mut Self) {
        unsafe {
            ptr::addr_of_mut!((*this).avatar).write_volatile(Position::ZERO);
            ptr::addr_of_mut!((*this).name).write_volatile([0; 256]);
            ptr::addr_of_mut!((*this).camera).write_volatile(Position::ZERO);
            ptr::addr_of_mut!((*this).identity).write_volatile([0; 256]);
            ptr::addr_of_mut!((*this).context_len).write_volatile(0);
            ptr::addr_of_mut!((*this).context).write_volatile([0; 256]);
            ptr::addr_of_mut!((*this).description).write_volatile([0; 2048]);
        }
    }

    #[inline]
    fn context(&self) -> &[u8] {
        &self.context[..self.context_len as usize]
    }

    #[inline]
    fn set_context(&mut self, context: &[u8]) {
        let len = context.len().min(self.context.len());
        self.context[..len].copy_from_slice(&context[..len]);
        self.context[len..].fill(0);
        self.context_len = len as u32;
    }

    #[inline]
    fn set_name(&mut self, name: &str) {
        imp::copy(&mut self.name, name);
    }

    #[inline]
    fn set_identity(&mut self, identity: &str) {
        imp::copy(&mut self.identity, identity);
    }

    #[inline]
    fn set_description(&mut self, description: &str) {
        imp::copy(&mut self.description, description);
    }
}

#[repr(C)]
struct Memory {
    header: Header,
    body: Body,
}

const _: () = const {
    // Assert that Memory has no padding between fields or at the end.
    assert!(
        mem::offset_of!(Memory, header) == 0,
        "Shared memory must not be padded"
    );
    assert!(
        mem::offset_of!(Memory, body) == mem::offset_of!(Memory, header) + mem::size_of::<Header>(),
        "Shared memory must not be padded"
    );
};

/// A mumble link connection.
pub struct Link {
    map: Option<imp::Map<Memory>>,
    local: Body,
    changes: u8,
}

impl Link {
    /// Open the Mumble link, providing the specified application name and
    /// description.
    pub fn new() -> io::Result<Self> {
        let map = imp::Map::<Memory>::new()?;

        let map = unsafe {
            Body::zero_non_atomic(ptr::addr_of_mut!((*map.ptr.as_ptr()).body));
            (*ptr::addr_of!((*map.ptr.as_ptr()).header.ui_version)).store(0, Ordering::SeqCst);
            (*ptr::addr_of!((*map.ptr.as_ptr()).header.ui_tick)).store(0, Ordering::SeqCst);
            map
        };

        Ok(Self {
            map: Some(map),
            local: Body::zero(),
            changes: VERSION_FLAG,
        })
    }

    /// Reconnect the link, reopening the shared memory if it was lost.
    pub fn reconnect(&mut self) -> io::Result<()> {
        self.disable();
        self.map = Some(imp::Map::<Memory>::new()?);
        Ok(())
    }

    /// Update the avatar position.
    pub fn set_avatar(&mut self, avatar: Position) {
        if self.local.avatar != avatar {
            self.local.avatar = avatar;
            self.changes |= AVATAR_FLAG;
        }
    }

    /// Update the camera position.
    pub fn set_camera(&mut self, camera: Position) {
        if self.local.camera != camera {
            self.local.camera = camera;
            self.changes |= CAMERA_FLAG;
        }
    }

    /// Set the name of the link.
    ///
    /// This doesn't affect positional data, but will be made visible in mumble
    /// when the link client connects.
    #[inline]
    pub fn set_name(&mut self, identity: &str) {
        self.local.set_name(identity);
        self.changes |= NAME_FLAG;
    }

    /// Update the identity, uniquely identifying the player in the given
    /// context. This is usually the in-game name or ID.
    ///
    /// The identity may also contain any additional information about the
    /// player which might be useful for the Mumble server, for example to move
    /// teammates to the same channel or give squad leaders additional powers.
    /// It is recommended that a parseable format like JSON or CSV is used for
    /// this.
    ///
    /// The identity should be changed infrequently, at most a few times per
    /// second.
    ///
    /// The identity has a maximum length of 255 UTF-16 code units.
    #[inline]
    pub fn set_identity(&mut self, identity: &str) {
        self.local.set_identity(identity);
        self.changes |= IDENTITY_FLAG;
    }

    /// Update the context string, used to determine which users on a Mumble
    /// server should hear each other positionally.
    ///
    /// If context between two Mumble users does not match, the positional audio
    /// data is stripped server-side and voice will be received as
    /// non-positional. Accordingly, the context should only match for players
    /// on the same game, server, and map, depending on the game itself. When in
    /// doubt, err on the side of including less; this allows for more
    /// flexibility in the future.
    ///
    /// The context should be changed infrequently, at most a few times per
    /// second.
    ///
    /// The context has a maximum length of 256 bytes, anything longer than that
    /// will be truncated.
    #[inline]
    pub fn set_context(&mut self, context: &[u8]) {
        if self.local.context() != context {
            self.local.set_context(context);
            self.changes |= CONTEXT_FLAG;
        }
    }

    /// Set the description of the link.
    #[inline]
    pub fn set_description(&mut self, description: &str) {
        self.local.set_description(description);
        self.changes |= DESCRIPTION_FLAG;
    }

    /// Update the link with the latest position information.
    ///
    /// This must be called fairly periodically to keep the link alive, even if
    /// there are no updates. A sleep of 100ms is likely sufficient.
    pub fn update(&mut self) {
        let Some(map) = &mut self.map else {
            return;
        };

        let changes = mem::take(&mut self.changes);

        if changes != 0 {
            unsafe {
                let body = ptr::addr_of_mut!((*map.ptr.as_ptr()).body);

                if changes & AVATAR_FLAG != 0 {
                    ptr::addr_of_mut!((*body).avatar).write_volatile(self.local.avatar);
                }

                if changes & NAME_FLAG != 0 {
                    ptr::addr_of_mut!((*body).name).write_volatile(self.local.name);
                }

                if changes & CAMERA_FLAG != 0 {
                    ptr::addr_of_mut!((*body).camera).write_volatile(self.local.camera);
                }

                if changes & IDENTITY_FLAG != 0 {
                    ptr::addr_of_mut!((*body).identity).write_volatile(self.local.identity);
                }

                if changes & CONTEXT_FLAG != 0 {
                    ptr::addr_of_mut!((*body).context).write_volatile(self.local.context);
                    ptr::addr_of_mut!((*body).context_len).write_volatile(self.local.context_len);
                }

                if changes & DESCRIPTION_FLAG != 0 {
                    ptr::addr_of_mut!((*body).description).write_volatile(self.local.description);
                }
            }
        }

        unsafe {
            if changes & VERSION_FLAG != 0 {
                (*ptr::addr_of!((*map.ptr.as_ptr()).header.ui_version)).store(2, Ordering::SeqCst);
            }

            // Atomically increment tick.
            let ui_tick = ptr::addr_of_mut!((*map.ptr.as_ptr()).header.ui_tick);
            (*ui_tick).fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Get the status of the shared link. See `Status` for details.
    pub fn is_active(&self) -> bool {
        self.map.is_some()
    }

    /// Deactivate the shared link effectively disabling it.
    ///
    /// Should be called when `update()` will not be called again for a while,
    /// such as if the player is no longer in-game.
    pub fn disable(&mut self) {
        if let Some(map) = self.map.take() {
            unsafe {
                // Zero out non-atomic fields of shared memory.
                Body::zero_non_atomic(ptr::addr_of_mut!((*map.ptr.as_ptr()).body));

                // Atomically update ui_version to try and ensure that updates
                // to those fields are not missed by other link clients.
                //
                // Preferably these should be using a seqlock.
                (*ptr::addr_of!((*map.ptr.as_ptr()).header.ui_version)).store(0, Ordering::SeqCst);
                (*ptr::addr_of!((*map.ptr.as_ptr()).header.ui_tick)).store(0, Ordering::SeqCst);
            }

            // A deactivation causes the remote context to be fully out of sync.
            self.changes = ALL_FLAGS;
        }
    }
}

unsafe impl Send for Link {}

impl Drop for Link {
    fn drop(&mut self) {
        self.disable();
    }
}
