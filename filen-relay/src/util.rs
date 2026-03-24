use std::{ops::Deref, sync::OnceLock};

/// A wrapper around OnceLock that panics if accessed before initialization.
/// This is useful for when you know the value will be initialized and want to avoid
/// explicitly calling unwrap() everywhere.
pub struct UnwrapOnceLock<T>(OnceLock<T>);

impl<T> UnwrapOnceLock<T> {
    pub const fn new() -> Self {
        UnwrapOnceLock(OnceLock::new())
    }
}

impl<T> UnwrapOnceLock<T> {
    pub fn init(&self, val: T) {
        let _ = self.0.set(val);
    }
}

impl<T> Deref for UnwrapOnceLock<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.get().expect("OnceLock not initialized")
    }
}
