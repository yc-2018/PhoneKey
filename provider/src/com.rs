use std::ffi::c_void;

use windows::core::HRESULT;

pub unsafe fn add_ref(raw: *mut c_void) -> u32 {
    let vtable = *(raw as *mut *const usize);
    let method: extern "system" fn(*mut c_void) -> u32 =
        std::mem::transmute(*vtable.add(1));
    method(raw)
}

pub unsafe fn release(raw: *mut c_void) -> u32 {
    let vtable = *(raw as *mut *const usize);
    let method: extern "system" fn(*mut c_void) -> u32 =
        std::mem::transmute(*vtable.add(2));
    method(raw)
}

pub unsafe fn call_credentials_changed(raw: *mut c_void, context: usize) -> HRESULT {
    let vtable = *(raw as *mut *const usize);
    let method: extern "system" fn(*mut c_void, usize) -> HRESULT =
        std::mem::transmute(*vtable.add(3));
    method(raw, context)
}
