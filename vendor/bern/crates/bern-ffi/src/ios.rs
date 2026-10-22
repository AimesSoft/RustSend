//! iOS host services. The UIKit host installs a process-lifetime callback
//! before starting the engine; returned strings use libc's allocator.

use bern_command::platform::{self, Services};
use bern_protocol::CommandError;
use std::ffi::{CStr, CString, c_char, c_void};

type Request = extern "C" fn(*const c_char, *const c_char, *const c_char) -> *mut c_char;

struct UIKitServices(Request);

unsafe extern "C" {
    fn free(pointer: *mut c_void);
}

impl Services for UIKitServices {
    fn request(&self, operation: &str, first: &str, second: &str) -> Result<String, CommandError> {
        let text = |value: &str| {
            CString::new(value)
                .map_err(|_| CommandError::new("invalid_argument", "text contains a null byte"))
        };
        let (operation, first, second) = (text(operation)?, text(first)?, text(second)?);
        let pointer = (self.0)(operation.as_ptr(), first.as_ptr(), second.as_ptr());
        if pointer.is_null() {
            return Err(CommandError::new(
                "platform_failed",
                "iOS returned no result",
            ));
        }
        // SAFETY: the installed UIKit callback returns a NUL-terminated,
        // malloc-owned response; the Rust side reads and frees it once.
        let response = unsafe { CStr::from_ptr(pointer).to_string_lossy().into_owned() };
        unsafe { free(pointer.cast()) };
        match response.strip_prefix('+') {
            Some(value) => Ok(value.to_owned()),
            None => Err(CommandError::new(
                "platform_failed",
                response.strip_prefix('-').unwrap_or(&response),
            )),
        }
    }
}

/// Install the UIKit services before the first engine is created.
#[unsafe(no_mangle)]
pub extern "C" fn bern_platform_install(request: Request) {
    platform::install(UIKitServices(request));
}
