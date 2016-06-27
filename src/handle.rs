use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use registration::UserData;

/// This is an opaque handle to a registered socket and its user data. This handle is used for
/// re-registering or deleting a currently registered socket from a poller.
///
/// This handle is necessarily opaque as it contains a raw pointer to mutable data registered with
/// the kernel poller. This violates aliasing rules, but is necessary to allow arbitrary data types
/// to be re-registered with different event types, such as read -> read + write. Using it is safe,
/// due to the use of the atomic valid flag, because only one version will be boxed and owned at a
/// time logically.
///
pub struct Handle<T> {
    valid: Arc<AtomicBool>,
    user_data_ptr: *mut UserData<T>
}

impl<T> Handle<T> {
    pub fn new(valid: Arc<AtomicBool>, user_data_ptr: *mut UserData<T>) -> Handle<T> {
        Handle {
            valid: valid,
            user_data_ptr: user_data_ptr
        }
    }

    /// Turn a handle into its matching UserData<T> if it's still valid.
    ///
    /// If the Registration is still valid, meaning that it's registered with a poller, then
    /// invalidate the Registation and take ownership of the UserData (which contains the
    /// registration)
    pub fn into_user_data(self) -> Option<Box<UserData<T>>> {
        if self.invalidate() {
            // We own the registration, so reconstruct it.
            Some(unsafe { Box::from_raw(self.user_data_ptr) })
        } else {
            None
        }
    }

    /// Invalidate the registration.
    ///
    /// Returns true on success, or false if the registration is already invalid.
    fn invalidate(&self) -> bool {
        self.valid.compare_and_swap(true, false, Ordering::Relaxed)
    }
}
