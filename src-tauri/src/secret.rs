use std::fs;

use log::error;

use crate::paths::secret_path;
use crate::sys::{dpapi_protect, dpapi_unprotect};

const SECRET_MAGIC: &[u8; 4] = b"MPK1";
pub const PASSWORD_MAX: usize = 200;

pub fn save_secret(password: &str) -> Result<(), String> {
    if password.is_empty() {
        return Err("密码不能为空".to_string());
    }
    if password.chars().count() > PASSWORD_MAX {
        return Err("密码过长".to_string());
    }
    let encrypted =
        dpapi_protect(password.as_bytes()).map_err(|err| format!("加密密码失败：{err}"))?;
    let mut payload = Vec::with_capacity(SECRET_MAGIC.len() + encrypted.len());
    payload.extend_from_slice(SECRET_MAGIC);
    payload.extend_from_slice(&encrypted);

    let path = secret_path();
    let temporary = path.with_extension("bin.tmp");
    fs::write(&temporary, &payload).map_err(|err| format!("写入密码失败：{err}"))?;
    fs::rename(&temporary, &path).map_err(|err| format!("替换密码文件失败：{err}"))?;
    Ok(())
}

pub fn load_secret() -> Option<String> {
    let payload = fs::read(secret_path()).ok()?;
    if !payload.starts_with(SECRET_MAGIC) {
        return None;
    }
    let plain = match dpapi_unprotect(&payload[SECRET_MAGIC.len()..]) {
        Ok(plain) => plain,
        Err(err) => {
            error!("读取保存的密码失败: {err}");
            return None;
        }
    };
    String::from_utf8(plain).ok()
}

pub fn has_secret() -> bool {
    let Ok(mut file) = fs::File::open(secret_path()) else {
        return false;
    };
    let mut magic = [0u8; 4];
    use std::io::Read;
    file.read_exact(&mut magic).is_ok() && &magic == SECRET_MAGIC
}

pub fn clear_secret() -> Result<(), String> {
    match fs::remove_file(secret_path()) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => {
            error!("删除保存的密码失败: {err}");
            Err(format!("删除保存的密码失败：{err}"))
        }
    }
}
