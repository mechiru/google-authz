use std::fmt;

/// RefGuard wraps a `Send` type to make it `Sync`, by ensuring that it is only
/// ever accessed through a &mut pointer.
pub(crate) struct RefGuard<T> {
    value: T,
}

impl<T: Send> RefGuard<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T: fmt::Debug> fmt::Debug for RefGuard<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RefGuard").field(&self.value).finish()
    }
}

unsafe impl<T: Send> Sync for RefGuard<T> {}
