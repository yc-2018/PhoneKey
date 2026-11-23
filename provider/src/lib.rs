#![allow(non_snake_case, non_camel_case_types, dead_code)]

mod com;
mod credential;
mod helpers;
mod pipe;
mod provider;
mod win;

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicI32, Ordering};

use windows::core::{GUID, HRESULT, Interface, IUnknown};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, E_INVALIDARG, E_NOINTERFACE, S_FALSE, S_OK,
};
use windows::Win32::System::Com::IClassFactory;

use crate::provider::ClassFactory;

pub(crate) static LIVE_OBJECTS: AtomicI32 = AtomicI32::new(0);

pub const CLSID_UNLOCK_PROVIDER: GUID =
    GUID::from_u128(0xE0A8C5B2_9F3D_4E7A_B1C6_8D2F5A3E9B70);

#[no_mangle]
pub extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if ppv.is_null() {
        return E_INVALIDARG;
    }
    unsafe {
        *ppv = ptr::null_mut();
    }
    if rclsid.is_null() || riid.is_null() {
        return E_INVALIDARG;
    }
    if unsafe { *rclsid } != CLSID_UNLOCK_PROVIDER {
        return CLASS_E_CLASSNOTAVAILABLE;
    }

    let factory: IClassFactory = ClassFactory::new().into();
    let unknown: IUnknown = match factory.cast() {
        Ok(unknown) => unknown,
        Err(_) => return E_NOINTERFACE,
    };
    unsafe { unknown.query(riid, ppv) }
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if LIVE_OBJECTS.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}
