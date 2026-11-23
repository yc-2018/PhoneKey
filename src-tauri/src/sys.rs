use windows::core::{Error, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use windows::Win32::Security::{
    LogonUserW, LOGON32_LOGON_NETWORK, LOGON32_PROVIDER_DEFAULT,
};
use windows::Win32::System::RemoteDesktop::{
    WTSFreeMemory, WTSQuerySessionInformationW, WTS_CURRENT_SESSION, WTSSessionInfoEx, WTSINFOEXW,
};
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, GetUserObjectInformationW, OpenInputDesktop, DESKTOP_ACCESS_FLAGS,
    DESKTOP_READOBJECTS, DESKTOP_SWITCHDESKTOP, UOI_NAME,
};
use windows::Win32::System::WindowsProgramming::GetUserNameW;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_REMOTESESSION};

pub fn win32_code(error: &Error) -> u32 {
    let code = error.code().0 as u32;
    if code & 0xFFFF_0000 == 0x8007_0000 {
        code & 0xFFFF
    } else {
        code
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, Error> {
    let mut input = CRYPT_INTEGER_BLOB {
        cbData: plain.len() as u32,
        pbData: plain.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let description = wide("PhoneKey");
    unsafe {
        CryptProtectData(
            &mut input,
            PCWSTR(description.as_ptr()),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )?;
    }
    let data = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut _)));
    }
    Ok(data)
}

pub fn dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>, Error> {
    let mut input = CRYPT_INTEGER_BLOB {
        cbData: blob.len() as u32,
        pbData: blob.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &mut input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )?;
    }
    let data = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut _)));
    }
    Ok(data)
}

fn session_lock_flag() -> Option<i32> {
    let mut buffer = PWSTR::null();
    let mut size = 0u32;
    unsafe {
        WTSQuerySessionInformationW(
            None,
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut size,
        )
        .ok()?;
    }
    let flags = unsafe {
        let info = buffer.0 as *const WTSINFOEXW;
        let value = if info.is_null() || (*info).Level != 1 {
            None
        } else {
            Some((*info).Data.WTSInfoExLevel1.SessionFlags)
        };
        WTSFreeMemory(buffer.0 as *mut _);
        value
    };
    flags
}

fn desktop_locked() -> bool {
    let desktop = match unsafe {
        OpenInputDesktop(
            Default::default(),
            false,
            DESKTOP_ACCESS_FLAGS(DESKTOP_READOBJECTS.0 | DESKTOP_SWITCHDESKTOP.0),
        )
    } {
        Ok(desktop) => desktop,
        Err(_) => return true,
    };
    let mut name = [0u16; 256];
    let mut needed = 0u32;
    let result = unsafe {
        GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr() as *mut _),
            std::mem::size_of_val(&name) as u32,
            Some(&mut needed),
        )
    };
    let locked = match result {
        Ok(()) => {
            let len = name.iter().position(|value| *value == 0).unwrap_or(name.len());
            String::from_utf16_lossy(&name[..len]).to_ascii_lowercase() != "default"
        }
        Err(_) => false,
    };
    unsafe {
        let _ = CloseDesktop(desktop);
    }
    locked
}

pub fn is_locked() -> bool {
    match session_lock_flag() {
        Some(0) => true,
        Some(1) => false,
        _ => desktop_locked(),
    }
}

pub fn is_remote_session() -> bool {
    unsafe { GetSystemMetrics(SM_REMOTESESSION) != 0 }
}

pub fn lock_workstation() -> bool {
    unsafe { LockWorkStation().is_ok() }
}

pub fn account_name() -> String {
    let mut buffer = vec![0u16; 256];
    let mut size = buffer.len() as u32;
    let result = unsafe { GetUserNameW(Some(PWSTR(buffer.as_mut_ptr())), &mut size) };
    if result.is_err() {
        return std::env::var("USERNAME").unwrap_or_default();
    }
    let len = buffer.iter().position(|value| *value == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
}

pub fn account_domain() -> String {
    std::env::var("USERDOMAIN").unwrap_or_else(|_| ".".to_string())
}

pub fn verify_credentials(username: &str, domain: &str, password: &str) -> Result<(), String> {
    let user = wide(username);
    let domain = wide(domain);
    let password = wide(password);
    let mut token = HANDLE::default();
    let result = unsafe {
        LogonUserW(
            PCWSTR(user.as_ptr()),
            PCWSTR(domain.as_ptr()),
            PCWSTR(password.as_ptr()),
            LOGON32_LOGON_NETWORK,
            LOGON32_PROVIDER_DEFAULT,
            &mut token,
        )
    };
    match result {
        Ok(()) => {
            if !token.is_invalid() {
                unsafe {
                    let _ = CloseHandle(token);
                }
            }
            Ok(())
        }
        Err(error) => {
            let code = win32_code(&error);
            let message = match code {
                1326 => "用户名或密码不正确",
                1327 => "该账户不允许使用空密码",
                1330 => "密码已过期",
                1331 => "账户已被禁用",
                1385 => "该账户没有远程登录权限",
                1907 => "首次登录前必须先修改密码",
                _ => return Err(format!("Windows 错误 {code}")),
            };
            Err(message.to_string())
        }
    }
}
