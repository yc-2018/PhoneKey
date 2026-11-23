use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use windows::core::{implement, BOOL, Error, Interface, PCWSTR, PWSTR, Ref, Result};
use windows::Win32::Foundation::{
    E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY, E_UNEXPECTED, NTSTATUS,
};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::UI::Shell::{
    CPFIS_NONE, CPFS_DISPLAY_IN_BOTH, CPFS_DISPLAY_IN_SELECTED_TILE, CPFS_HIDDEN,
    CPGSR_NO_CREDENTIAL_NOT_FINISHED, CPGSR_RETURN_CREDENTIAL_FINISHED,
    CPSI_ERROR, CPSI_NONE, CPSI_SUCCESS, CPSI_WARNING, CPUS_LOGON, CPUS_UNLOCK_WORKSTATION,
    CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION, CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE,
    CREDENTIAL_PROVIDER_FIELD_STATE, CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE,
    CREDENTIAL_PROVIDER_STATUS_ICON, CREDENTIAL_PROVIDER_USAGE_SCENARIO,
    ICredentialProviderCredential, ICredentialProviderCredential_Impl,
    ICredentialProviderCredentialEvents,
};

use crate::com::{add_ref, release};
use crate::helpers::{
    alloc_wide, lookup_negotiate_package, pack_kerb, pcwstr_to_string, wipe_string,
};
use crate::pipe::{
    fetch_credentials, CredentialFetch, CREDENTIAL_TIMEOUT_MS, Shared,
};
use crate::win::{dbg, GetTickCount64};
use crate::LIVE_OBJECTS;

const AUTO_LOGON_GUARD_MS: u64 = 1500;

const FIELD_STATES: [CREDENTIAL_PROVIDER_FIELD_STATE; 5] = [
    CPFS_DISPLAY_IN_SELECTED_TILE,
    CPFS_DISPLAY_IN_BOTH,
    CPFS_HIDDEN,
    CPFS_HIDDEN,
    CPFS_DISPLAY_IN_SELECTED_TILE,
];

#[implement(ICredentialProviderCredential)]
pub struct Credential {
    shared: Arc<Shared>,
    events: AtomicUsize,
    advised_tick: AtomicU64,
    cpus: CREDENTIAL_PROVIDER_USAGE_SCENARIO,
    auth_package: Option<u32>,
}

impl Credential {
    pub fn new(shared: Arc<Shared>, cpus: CREDENTIAL_PROVIDER_USAGE_SCENARIO) -> Self {
        LIVE_OBJECTS.fetch_add(1, Ordering::SeqCst);
        let auth_package = match lookup_negotiate_package() {
            Ok(package) => Some(package),
            Err(error) => {
                dbg(&format!(
                    "PhoneKey CP: Negotiate lookup failed: {error}\n"
                ));
                None
            }
        };
        dbg(&format!(
            "CP: credential new cpus={} auth={}\n",
            cpus.0,
            auth_package.is_some()
        ));
        Self {
            shared,
            events: AtomicUsize::new(0),
            advised_tick: AtomicU64::new(0),
            cpus,
            auth_package,
        }
    }

    pub fn refresh_label(&self, iface: &ICredentialProviderCredential) {
        let raw = self.events.load(Ordering::Acquire);
        if raw == 0 {
            return;
        }
        let label = self.shared.label();
        let text: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let events = std::mem::ManuallyDrop::new(
                ICredentialProviderCredentialEvents::from_raw(raw as *mut c_void),
            );
            let _ = events.SetFieldString(iface, 1, PCWSTR(text.as_ptr()));
        }
    }
}

impl Drop for Credential {
    fn drop(&mut self) {
        let raw = self.events.swap(0, Ordering::AcqRel);
        if raw != 0 {
            unsafe {
                release(raw as *mut c_void);
            }
        }
        LIVE_OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
}

fn set_warning(text_ptr: *mut PWSTR, icon_ptr: *mut CREDENTIAL_PROVIDER_STATUS_ICON, text: &str) {
    if !text_ptr.is_null() {
        let label = alloc_wide(text);
        if !label.is_null() {
            unsafe {
                *text_ptr = label;
            }
        }
    }
    if !icon_ptr.is_null() {
        unsafe {
            *icon_ptr = CPSI_WARNING;
        }
    }
}

impl ICredentialProviderCredential_Impl for Credential_Impl {
    fn Advise(&self, pcpce: Ref<'_, ICredentialProviderCredentialEvents>) -> Result<()> {
        let raw = pcpce
            .as_ref()
            .map(|events| events.as_raw() as usize)
            .unwrap_or(0);
        let old = self.events.swap(raw, Ordering::AcqRel);
        if old != 0 {
            unsafe {
                release(old as *mut c_void);
            }
        }
        if raw != 0 {
            unsafe {
                add_ref(raw as *mut c_void);
            }
        }
        self.advised_tick
            .store(unsafe { GetTickCount64() }, Ordering::Release);
        Ok(())
    }

    fn UnAdvise(&self) -> Result<()> {
        let raw = self.events.swap(0, Ordering::AcqRel);
        if raw != 0 {
            unsafe {
                release(raw as *mut c_void);
            }
        }
        Ok(())
    }

    fn SetSelected(&self) -> Result<BOOL> {
        let connectable = matches!(self.cpus, CPUS_LOGON | CPUS_UNLOCK_WORKSTATION);
        let pending = self.shared.pending();
        let advised = self.advised_tick.load(Ordering::Acquire);
        let elapsed = if advised == 0 {
            0
        } else {
            unsafe { GetTickCount64() }.saturating_sub(advised)
        };
        let auto_logon = connectable && advised != 0 && elapsed >= AUTO_LOGON_GUARD_MS;
        dbg(&format!(
            "CP: SetSelected cpus={} pending={} elapsed={} auto={}\n",
            self.cpus.0, pending, elapsed, auto_logon
        ));
        Ok(BOOL(if auto_logon { 1 } else { 0 }))
    }

    fn SetDeselected(&self) -> Result<()> {
        Ok(())
    }

    fn GetFieldState(
        &self,
        dwfieldid: u32,
        pcpfs: *mut CREDENTIAL_PROVIDER_FIELD_STATE,
        pcpfis: *mut CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE,
    ) -> Result<()> {
        if dwfieldid >= FIELD_STATES.len() as u32 {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        if pcpfs.is_null() || pcpfis.is_null() {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        unsafe {
            *pcpfs = FIELD_STATES[dwfieldid as usize];
            *pcpfis = CPFIS_NONE;
        }
        Ok(())
    }

    fn GetStringValue(&self, dwfieldid: u32) -> Result<PWSTR> {
        let value = match dwfieldid {
            1 => self.shared.label(),
            2 => self.shared.username(),
            3 => String::new(),
            _ => return Err(Error::from_hresult(E_INVALIDARG)),
        };
        let text = alloc_wide(&value);
        if text.is_null() {
            Err(Error::from_hresult(E_OUTOFMEMORY))
        } else {
            Ok(text)
        }
    }

    fn GetBitmapValue(&self, dwfieldid: u32) -> Result<HBITMAP> {
        if dwfieldid != 0 {
            Err(Error::from_hresult(E_INVALIDARG))
        } else {
            Err(Error::from_hresult(E_NOTIMPL))
        }
    }

    fn GetCheckboxValue(
        &self,
        _dwfieldid: u32,
        _pbchecked: *mut BOOL,
        ppszlabel: *mut PWSTR,
    ) -> Result<()> {
        if !ppszlabel.is_null() {
            unsafe {
                *ppszlabel = PWSTR::null();
            }
        }
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn GetSubmitButtonValue(&self, dwfieldid: u32) -> Result<u32> {
        if dwfieldid == 4 {
            Ok(1)
        } else {
            Err(Error::from_hresult(E_INVALIDARG))
        }
    }

    fn GetComboBoxValueCount(
        &self,
        _dwfieldid: u32,
        pcitems: *mut u32,
        pdwselecteditem: *mut u32,
    ) -> Result<()> {
        if !pcitems.is_null() {
            unsafe {
                *pcitems = 0;
            }
        }
        if !pdwselecteditem.is_null() {
            unsafe {
                *pdwselecteditem = 0;
            }
        }
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn GetComboBoxValueAt(&self, _dwfieldid: u32, _dwitem: u32) -> Result<PWSTR> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn SetStringValue(&self, dwfieldid: u32, psz: &PCWSTR) -> Result<()> {
        let value = pcwstr_to_string(psz);
        match dwfieldid {
            2 => {
                self.shared.set_username(value);
                Ok(())
            }
            3 => {
                self.shared.set_password(value);
                Ok(())
            }
            _ => Err(Error::from_hresult(E_INVALIDARG)),
        }
    }

    fn SetCheckboxValue(&self, _dwfieldid: u32, _bchecked: BOOL) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn SetComboBoxSelectedValue(&self, _dwfieldid: u32, _dwselecteditem: u32) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn CommandLinkClicked(&self, _dwfieldid: u32) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn GetSerialization(
        &self,
        pcpgsr: *mut CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE,
        pcpcs: *mut CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION,
        ppszoptionalstatustext: *mut PWSTR,
        pcpsioptionalstatusicon: *mut CREDENTIAL_PROVIDER_STATUS_ICON,
    ) -> Result<()> {
        if pcpgsr.is_null() || pcpcs.is_null() {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        unsafe {
            *pcpgsr = CPGSR_NO_CREDENTIAL_NOT_FINISHED;
            *pcpcs = CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION::default();
            if !ppszoptionalstatustext.is_null() {
                *ppszoptionalstatustext = PWSTR::null();
            }
            if !pcpsioptionalstatusicon.is_null() {
                *pcpsioptionalstatusicon = CPSI_NONE;
            }
        }

        let Some(auth_package) = self.auth_package else {
            dbg("CP: GetSerialization without auth package\n");
            return Err(Error::from_hresult(E_UNEXPECTED));
        };
        dbg(&format!(
            "CP: GetSerialization cpus={} auth={}\n",
            self.cpus.0, auth_package
        ));

        if !matches!(self.cpus, CPUS_LOGON | CPUS_UNLOCK_WORKSTATION) {
            dbg("CP: GetSerialization unsupported scenario\n");
            set_warning(
                ppszoptionalstatustext,
                pcpsioptionalstatusicon,
                "手机解锁仅在锁屏界面可用",
            );
            return Ok(());
        }

        // The app only returns credentials after a fresh proximity check.
        let (domain, username, mut password) = match fetch_credentials(CREDENTIAL_TIMEOUT_MS) {
            CredentialFetch::Ok {
                domain,
                username,
                password,
            } => {
                dbg("CP: fetch OK\n");
                (domain, username, password)
            }
            CredentialFetch::Deny => {
                dbg("CP: fetch DENY\n");
                set_warning(
                    ppszoptionalstatustext,
                    pcpsioptionalstatusicon,
                    "暂时未检测到手机信号，请重试或使用 PIN",
                );
                return Ok(());
            }
            CredentialFetch::Off => {
                dbg("CP: fetch OFF\n");
                set_warning(
                    ppszoptionalstatustext,
                    pcpsioptionalstatusicon,
                    "手机解锁已关闭，请使用 PIN",
                );
                return Ok(());
            }
            CredentialFetch::NoCred => {
                dbg("CP: fetch NOCRED\n");
                set_warning(
                    ppszoptionalstatustext,
                    pcpsioptionalstatusicon,
                    "尚未保存 Windows 密码，请使用 PIN 或打开应用设置",
                );
                return Ok(());
            }
            CredentialFetch::Error => {
                dbg("CP: fetch ERROR\n");
                set_warning(
                    ppszoptionalstatustext,
                    pcpsioptionalstatusicon,
                    "手机解锁应用未响应，请使用 PIN 或稍后重试",
                );
                return Ok(());
            }
        };

        let domain = if domain.is_empty() {
            ".".to_string()
        } else {
            domain
        };

        match pack_kerb(&domain, &username, &password, self.cpus) {
            Ok((buffer, size)) => {
                wipe_string(&mut password);
                dbg(&format!("CP: serialization ready size={size}\n"));
                unsafe {
                    (*pcpcs).ulAuthenticationPackage = auth_package;
                    (*pcpcs).clsidCredentialProvider = crate::CLSID_UNLOCK_PROVIDER;
                    (*pcpcs).cbSerialization = size;
                    (*pcpcs).rgbSerialization = buffer;
                    *pcpgsr = CPGSR_RETURN_CREDENTIAL_FINISHED;
                }
                Ok(())
            }
            Err(error) => {
                wipe_string(&mut password);
                dbg(&format!("CP: pack failed {error}\n"));
                Err(error)
            }
        }
    }

    fn ReportResult(
        &self,
        ntsstatus: NTSTATUS,
        _ntssubstatus: NTSTATUS,
        ppszoptionalstatustext: *mut PWSTR,
        pcpsioptionalstatusicon: *mut CREDENTIAL_PROVIDER_STATUS_ICON,
    ) -> Result<()> {
        if !ppszoptionalstatustext.is_null() {
            unsafe {
                *ppszoptionalstatustext = PWSTR::null();
            }
        }
        if !pcpsioptionalstatusicon.is_null() {
            unsafe {
                *pcpsioptionalstatusicon = CPSI_NONE;
            }
        }

        if ntsstatus.0 == 0 {
            dbg("CP: ReportResult success\n");
            if !pcpsioptionalstatusicon.is_null() {
                unsafe {
                    *pcpsioptionalstatusicon = CPSI_SUCCESS;
                }
            }
        } else {
            let status = ntsstatus.0 as u32;
            dbg(&format!("CP: ReportResult status={status:08X}\n"));
            let message = match status {
                0xC000_006D => "Incorrect username or password.",
                0xC000_0072 => "Account is disabled.",
                0xC000_0234 => "Account is locked out.",
                0xC000_0193 => "Account has expired.",
                0xC000_0224 => "Password must be changed.",
                _ => "Logon failed.",
            };
            if !ppszoptionalstatustext.is_null() {
                let text = alloc_wide(message);
                if !text.is_null() {
                    unsafe {
                        *ppszoptionalstatustext = text;
                    }
                }
            }
            if !pcpsioptionalstatusicon.is_null() {
                unsafe {
                    *pcpsioptionalstatusicon = CPSI_ERROR;
                }
            }
        }

        self.shared.clear_pending_silent();
        Ok(())
    }
}
