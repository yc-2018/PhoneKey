use std::mem;
use std::ptr;

use windows::core::{Error, HRESULT, PSTR, Result, PWSTR};
use windows::Win32::Foundation::{E_INVALIDARG, E_OUTOFMEMORY, HANDLE, LUID};
use windows::Win32::Security::Authentication::Identity::{
    KerbInteractiveLogon, KerbWorkstationUnlockLogon, KERB_INTERACTIVE_UNLOCK_LOGON,
    LSA_STRING, LSA_UNICODE_STRING, LsaConnectUntrusted, LsaDeregisterLogonProcess,
    LsaLookupAuthenticationPackage,
};
use windows::Win32::System::Com::{CoTaskMemAlloc, CoTaskMemFree};
use windows::Win32::UI::Shell::{CPUS_CREDUI, CPUS_LOGON, CPUS_UNLOCK_WORKSTATION, CREDENTIAL_PROVIDER_USAGE_SCENARIO};

pub fn lookup_negotiate_package() -> Result<u32> {
    let mut handle = HANDLE::default();
    let status = unsafe { LsaConnectUntrusted(&mut handle) };
    if status.0 < 0 {
        return Err(Error::from_hresult(HRESULT::from_nt(status.0)));
    }

    let package_name = b"Negotiate\0";
    let name = LSA_STRING {
        Length: 9,
        MaximumLength: 10,
        Buffer: PSTR(package_name.as_ptr() as *mut u8),
    };
    let mut package = 0u32;
    let status = unsafe { LsaLookupAuthenticationPackage(handle, &name, &mut package) };
    unsafe {
        let _ = LsaDeregisterLogonProcess(handle);
    }
    if status.0 < 0 {
        return Err(Error::from_hresult(HRESULT::from_nt(status.0)));
    }
    Ok(package)
}

pub fn pack_kerb(
    domain: &str,
    username: &str,
    password: &str,
    cpus: CREDENTIAL_PROVIDER_USAGE_SCENARIO,
) -> Result<(*mut u8, u32)> {
    let message_type = if cpus == CPUS_UNLOCK_WORKSTATION {
        KerbWorkstationUnlockLogon
    } else if cpus == CPUS_LOGON || cpus == CPUS_CREDUI {
        KerbInteractiveLogon
    } else {
        return Err(Error::from_hresult(E_INVALIDARG));
    };

    let mut domain_w = wide(domain);
    let mut username_w = wide(username);
    let mut password_w = wide(password);

    let to_lsa = |value: &mut Vec<u16>| LSA_UNICODE_STRING {
        Length: ((value.len() - 1) * 2) as u16,
        MaximumLength: ((value.len() - 1) * 2) as u16,
        Buffer: PWSTR(value.as_mut_ptr()),
    };

    let mut kiul: KERB_INTERACTIVE_UNLOCK_LOGON = unsafe { mem::zeroed() };
    kiul.Logon.MessageType = message_type;
    kiul.Logon.LogonDomainName = to_lsa(&mut domain_w);
    kiul.Logon.UserName = to_lsa(&mut username_w);
    kiul.Logon.Password = to_lsa(&mut password_w);
    kiul.LogonId = LUID::default();

    let struct_size = mem::size_of::<KERB_INTERACTIVE_UNLOCK_LOGON>();
    let strings_size = (domain_w.len() - 1 + username_w.len() - 1 + password_w.len() - 1) * 2;
    let total = struct_size + strings_size;

    let buffer = unsafe { CoTaskMemAlloc(total) } as *mut u8;
    if buffer.is_null() {
        wipe_utf16(&mut password_w);
        return Err(Error::from_hresult(E_OUTOFMEMORY));
    }

    unsafe {
        ptr::copy_nonoverlapping(
            &kiul as *const _ as *const u8,
            buffer,
            struct_size,
        );
        let packed = &mut *(buffer as *mut KERB_INTERACTIVE_UNLOCK_LOGON);
        let mut offset = struct_size;

        let domain_bytes = (domain_w.len() - 1) * 2;
        ptr::copy_nonoverlapping(domain_w.as_ptr(), buffer.add(offset) as *mut u16, domain_w.len() - 1);
        packed.Logon.LogonDomainName = LSA_UNICODE_STRING {
            Length: domain_bytes as u16,
            MaximumLength: domain_bytes as u16,
            Buffer: PWSTR(offset as *mut u16),
        };
        offset += domain_bytes;

        let username_bytes = (username_w.len() - 1) * 2;
        ptr::copy_nonoverlapping(username_w.as_ptr(), buffer.add(offset) as *mut u16, username_w.len() - 1);
        packed.Logon.UserName = LSA_UNICODE_STRING {
            Length: username_bytes as u16,
            MaximumLength: username_bytes as u16,
            Buffer: PWSTR(offset as *mut u16),
        };
        offset += username_bytes;

        let password_bytes = (password_w.len() - 1) * 2;
        ptr::copy_nonoverlapping(password_w.as_ptr(), buffer.add(offset) as *mut u16, password_w.len() - 1);
        packed.Logon.Password = LSA_UNICODE_STRING {
            Length: password_bytes as u16,
            MaximumLength: password_bytes as u16,
            Buffer: PWSTR(offset as *mut u16),
        };
    }

    wipe_utf16(&mut password_w);
    Ok((buffer, total as u32))
}

pub fn alloc_wide(value: &str) -> PWSTR {
    let text: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = text.len() * 2;
    let buffer = unsafe { CoTaskMemAlloc(bytes) } as *mut u16;
    if buffer.is_null() {
        return PWSTR::null();
    }
    unsafe {
        ptr::copy_nonoverlapping(text.as_ptr(), buffer, text.len());
    }
    PWSTR(buffer)
}

pub fn free_alloc(ptr: *mut u8) {
    if !ptr.is_null() {
        unsafe {
            CoTaskMemFree(Some(ptr as *const _));
        }
    }
}

pub fn pcwstr_to_string(value: &windows::core::PCWSTR) -> String {
    if value.0.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    unsafe {
        while *value.0.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(value.0, len);
        String::from_utf16_lossy(slice)
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn wipe_utf16(value: &mut Vec<u16>) {
    if !value.is_empty() {
        unsafe {
            ptr::write_bytes(value.as_mut_ptr(), 0, value.len());
        }
        value.clear();
    }
}

pub fn wipe_string(value: &mut String) {
    if !value.is_empty() {
        unsafe {
            ptr::write_bytes(value.as_mut_ptr(), 0, value.len());
        }
        value.clear();
    }
}
