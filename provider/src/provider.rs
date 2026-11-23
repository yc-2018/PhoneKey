use std::ffi::c_void;
use std::mem;
use std::ptr;
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once};

use windows::core::{implement, AsImpl, BOOL, Error, GUID, Interface, Ref, Result, IUnknown};
use windows::Win32::Foundation::{
    CLASS_E_NOAGGREGATION, E_INVALIDARG, E_NOINTERFACE, E_OUTOFMEMORY,
};
use windows::Win32::System::Com::{CoTaskMemAlloc, IClassFactory, IClassFactory_Impl};
use windows::Win32::UI::Shell::{
    CPFT_EDIT_TEXT, CPFT_LARGE_TEXT, CPFT_PASSWORD_TEXT, CPFT_SUBMIT_BUTTON,
    CPFT_TILE_IMAGE, CPUS_CREDUI, CPUS_LOGON, CPUS_UNLOCK_WORKSTATION,
    CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION, CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR,
    CREDENTIAL_PROVIDER_FIELD_TYPE, CREDENTIAL_PROVIDER_NO_DEFAULT,
    CREDENTIAL_PROVIDER_USAGE_SCENARIO, ICredentialProvider, ICredentialProvider_Impl,
    ICredentialProviderCredential, ICredentialProviderEvents,
};

use crate::com::{add_ref, call_credentials_changed, release};
use crate::credential::Credential;
use crate::helpers::{alloc_wide, free_alloc};
use crate::pipe::{Shared, WM_UNLOCK_CREDENTIAL};
use crate::win::*;
use crate::LIVE_OBJECTS;

const WND_CLASS: &str = "PhoneKeyUnlockProviderMsgWnd";
const FIELD_COUNT: u32 = 5;
const FIELD_LABELS: [&str; 5] = [
    "Image",
    "手机感应解锁",
    "Username",
    "Password",
    "Submit",
];
const FIELD_TYPES: [CREDENTIAL_PROVIDER_FIELD_TYPE; 5] = [
    CPFT_TILE_IMAGE,
    CPFT_LARGE_TEXT,
    CPFT_EDIT_TEXT,
    CPFT_PASSWORD_TEXT,
    CPFT_SUBMIT_BUTTON,
];

static REGISTER_CLASS: Once = Once::new();

#[implement(ICredentialProvider)]
pub struct Provider {
    shared: Arc<Shared>,
    events: Mutex<Option<usize>>,
    advise_context: AtomicUsize,
    hwnd: AtomicIsize,
    credential: Mutex<Option<usize>>,
}

impl Provider {
    pub fn new() -> Self {
        LIVE_OBJECTS.fetch_add(1, Ordering::SeqCst);
        dbg("CP: provider new\n");
        Self {
            shared: Shared::new(),
            events: Mutex::new(None),
            advise_context: AtomicUsize::new(0),
            hwnd: AtomicIsize::new(0),
            credential: Mutex::new(None),
        }
    }

    fn create_credential(&self, cpus: CREDENTIAL_PROVIDER_USAGE_SCENARIO) {
        let credential: ICredentialProviderCredential =
            Credential::new(self.shared.clone(), cpus).into();
        let raw = credential.into_raw() as usize;
        let mut guard = self
            .credential
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(old) = guard.take() {
            unsafe {
                release(old as *mut c_void);
            }
        }
        *guard = Some(raw);
    }

    fn create_window(self: &Self) {
        REGISTER_CLASS.call_once(|| unsafe {
            let class_name = wide(WND_CLASS);
            let class = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: GetModuleHandleW(ptr::null()),
                lpszClassName: class_name.as_ptr(),
                ..Default::default()
            };
            RegisterClassW(&class);
        });

        if self.hwnd.load(Ordering::Acquire) != 0 {
            return;
        }

        let class_name = wide(WND_CLASS);
        let title = wide("");
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                ptr::null_mut(),
                instance,
                self as *const Provider as *mut c_void,
            )
        };
        if hwnd == 0 {
            dbg("PhoneKey CP: message window creation failed\n");
            return;
        }
        self.hwnd.store(hwnd, Ordering::Release);
        self.shared.set_hwnd(hwnd);
    }

    fn fire_change(&self) {
        let raw = *self
            .events
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let context = self.advise_context.load(Ordering::Acquire);
        if let Some(raw) = raw {
            unsafe {
                add_ref(raw as *mut c_void);
                let _ = call_credentials_changed(raw as *mut c_void, context);
                release(raw as *mut c_void);
            }
        } else {
            self.shared.mark_change_pending();
        }
        self.refresh_label();
    }

    fn refresh_label(&self) {
        let raw = *self
            .credential
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let Some(raw) = raw else {
            return;
        };
        unsafe {
            let iface = &*(raw as *const ICredentialProviderCredential);
            let credential: &Credential = iface.as_impl();
            credential.refresh_label(iface);
        }
    }

    fn release_events(&self) {
        let raw = self
            .events
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
        if let Some(raw) = raw {
            unsafe {
                release(raw as *mut c_void);
            }
        }
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        dbg("CP: provider drop\n");
        self.shared.set_hwnd(0);
        self.shared.stop_listener();
        let hwnd = self.hwnd.swap(0, Ordering::AcqRel);
        if hwnd != 0 {
            unsafe {
                DestroyWindow(hwnd);
            }
        }
        let credential = self
            .credential
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
        if let Some(raw) = credential {
            unsafe {
                release(raw as *mut c_void);
            }
        }
        self.release_events();
        LIVE_OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    if msg == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        return 1;
    }

    if msg == WM_UNLOCK_CREDENTIAL {
        let provider = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Provider;
        if !provider.is_null() {
            (*provider).fire_change();
        }
        return 0;
    }

    DefWindowProcW(hwnd, msg, wparam, lparam)
}

impl ICredentialProvider_Impl for Provider_Impl {
    fn SetUsageScenario(
        &self,
        cpus: CREDENTIAL_PROVIDER_USAGE_SCENARIO,
        _dwflags: u32,
    ) -> Result<()> {
        if !matches!(cpus, CPUS_LOGON | CPUS_UNLOCK_WORKSTATION | CPUS_CREDUI) {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        dbg(&format!("CP: SetUsageScenario {}\n", cpus.0));
        self.create_credential(cpus);
        self.create_window();
        self.shared.start_listener();
        Ok(())
    }

    fn SetSerialization(
        &self,
        _pcpcs: *const CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION,
    ) -> Result<()> {
        Err(Error::from_hresult(windows::Win32::Foundation::E_NOTIMPL))
    }

    fn Advise(
        &self,
        pcpe: Ref<'_, ICredentialProviderEvents>,
        upadvisecontext: usize,
    ) -> Result<()> {
        let raw = pcpe
            .as_ref()
            .map(|events| events.as_raw() as usize)
            .unwrap_or(0);

        {
            let mut guard = self
                .events
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if let Some(old) = guard.take() {
                unsafe {
                    release(old as *mut c_void);
                }
            }
            if raw != 0 {
                unsafe {
                    add_ref(raw as *mut c_void);
                }
                *guard = Some(raw);
            }
        }
        self.advise_context.store(upadvisecontext, Ordering::Release);
        dbg("CP: Advise\n");

        if self.shared.take_pending_change() {
            self.fire_change();
        }
        Ok(())
    }

    fn UnAdvise(&self) -> Result<()> {
        dbg("CP: UnAdvise\n");
        self.shared.stop_listener();
        self.release_events();
        Ok(())
    }

    fn GetFieldDescriptorCount(&self) -> Result<u32> {
        Ok(FIELD_COUNT)
    }

    fn GetFieldDescriptorAt(
        &self,
        dwindex: u32,
    ) -> Result<*mut CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR> {
        if dwindex >= FIELD_COUNT {
            return Err(Error::from_hresult(E_INVALIDARG));
        }

        let descriptor =
            unsafe { CoTaskMemAlloc(mem::size_of::<CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR>()) }
                as *mut CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR;
        if descriptor.is_null() {
            return Err(Error::from_hresult(E_OUTOFMEMORY));
        }

        let label = alloc_wide(FIELD_LABELS[dwindex as usize]);
        if label.is_null() {
            free_alloc(descriptor as *mut u8);
            return Err(Error::from_hresult(E_OUTOFMEMORY));
        }

        unsafe {
            ptr::write(
                descriptor,
                CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR {
                    dwFieldID: dwindex,
                    cpft: FIELD_TYPES[dwindex as usize],
                    pszLabel: label,
                    guidFieldType: GUID::zeroed(),
                },
            );
        }
        Ok(descriptor)
    }

    fn GetCredentialCount(
        &self,
        pdwcount: *mut u32,
        pdwdefault: *mut u32,
        pbautologonwithdefault: *mut BOOL,
    ) -> Result<()> {
        if pdwcount.is_null() || pdwdefault.is_null() || pbautologonwithdefault.is_null() {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        unsafe {
            *pdwcount = 1;
            *pdwdefault = CREDENTIAL_PROVIDER_NO_DEFAULT;
            *pbautologonwithdefault = BOOL(0);
        }
        Ok(())
    }

    fn GetCredentialAt(&self, dwindex: u32) -> Result<ICredentialProviderCredential> {
        if dwindex != 0 {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let raw = *self
            .credential
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let Some(raw) = raw else {
            return Err(Error::from_hresult(E_INVALIDARG));
        };
        unsafe {
            add_ref(raw as *mut c_void);
            Ok(ICredentialProviderCredential::from_raw(raw as *mut c_void))
        }
    }
}

#[implement(IClassFactory)]
pub struct ClassFactory;

impl ClassFactory {
    pub fn new() -> Self {
        LIVE_OBJECTS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for ClassFactory {
    fn drop(&mut self) {
        LIVE_OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
}

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Ref<'_, IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut c_void,
    ) -> Result<()> {
        if ppvobject.is_null() {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        unsafe {
            *ppvobject = ptr::null_mut();
        }
        if !punkouter.is_null() {
            return Err(Error::from_hresult(CLASS_E_NOAGGREGATION));
        }
        if riid.is_null() {
            return Err(Error::from_hresult(E_INVALIDARG));
        }

        let provider: ICredentialProvider = Provider::new().into();
        let unknown: IUnknown = match provider.cast() {
            Ok(unknown) => unknown,
            Err(_) => return Err(Error::from_hresult(E_NOINTERFACE)),
        };
        let hr = unsafe { unknown.query(riid, ppvobject) };
        hr.ok()
    }

    fn LockServer(&self, flock: BOOL) -> Result<()> {
        if flock.0 != 0 {
            LIVE_OBJECTS.fetch_add(1, Ordering::SeqCst);
        } else {
            LIVE_OBJECTS.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}
