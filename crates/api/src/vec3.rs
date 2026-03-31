use core::fmt;
use core::ops::{Add, AddAssign, Div, Mul, Sub};

use musli_core::{Decode, Encode};

#[derive(Clone, Copy, Default, PartialEq, Encode, Decode)]
#[musli(crate = musli_core)]
#[repr(C)]
pub struct Vec3 {
    /// The x coordinate in meters from the origin (left / right).
    pub x: f32,
    /// The y coordinate in meters from the origin (up / down).
    pub y: f32,
    /// The z coordinate in meters from the origin (forward / backward).
    pub z: f32,
}

impl Vec3 {
    /// A unit vector pointing up in the world positive y direction.
    pub const Y: Self = Self::new(0.0, 1.0, 0.0);

    /// Calculate the cross product of `self` and `other`.
    pub fn cross(&self, other: &Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Calculate the dot product of `self` and `other`.
    pub fn dot(&self, other: &Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Normalize the vector to a unit vector.
    pub fn normalize(&self) -> Self {
        let len = (self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        Self::new(self.x / len, self.y / len, self.z / len)
    }

    /// Coerce into an array of floats.
    #[inline]
    pub fn as_array(&self) -> &[f32; 3] {
        // SAFETY: This struct is repr(C), which guarantees the layout.
        unsafe { &*(self as *const Self as *const [f32; 3]) }
    }

    /// Get the length of the vector.
    ///
    /// # Examples
    ///
    /// ```
    /// use api::Vec3;
    ///
    /// let a = Vec3::new(1.0, 2.0, 3.0);
    /// assert!((a.len() - 3.7416573867739413).abs() < 1e-6);
    /// ```
    #[inline]
    pub fn len(&self) -> f32 {
        (self.x.powi(2) + self.y.powi(2) + self.z.powi(2)).sqrt()
    }

    /// Calculate the distance from `self` to `other`.
    ///
    /// # Examples
    ///
    /// ```
    /// use api::Vec3;
    ///
    /// let a = Vec3::new(1.0, 2.0, 3.0);
    /// let b = Vec3::new(4.0, 6.0, 8.0);
    ///
    /// assert!((a.dist(b) - 7.0710678118654755).abs() < 1e-6);
    /// ```
    #[inline]
    pub fn dist(&self, other: Self) -> f32 {
        (*self - other).len()
    }

    /// Calculate the direction from `self` to `other` as a unit vector.
    #[inline]
    pub fn direction_to(&self, other: Self) -> Self {
        let d = other - *self;
        d / d.len()
    }

    /// Calculate the angle at which the XZ vector is facing in the xz plane
    /// where 0 degrees means facing in the positive x direction.
    #[inline]
    pub fn angle_xz(&self) -> f32 {
        (-self.z).atan2(self.x)
    }
}

impl Sub for Vec3 {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

impl Div<f32> for Vec3 {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        Self::new(self.x / rhs, self.y / rhs, self.z / rhs)
    }
}

impl Add for Vec3 {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl AddAssign for Vec3 {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}

impl fmt::Debug for Vec3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Vec3")
            .field(&self.x)
            .field(&self.y)
            .field(&self.z)
            .finish()
    }
}

impl Vec3 {
    /// The zero vector.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    /// A unit vector pointing forward in the world (negative z direction).
    pub const FORWARD: Self = Self::new(0.0, 0.0, -1.0);

    /// Constructs a new position with the given coordinates.
    #[inline]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Constuct a zero vector.
    #[inline]
    pub const fn zero() -> Self {
        Self::ZERO
    }
}
