//! Thin registry helpers (HKCU/HKLM reads, string writes). Errors become `None`/`false`.

use windows::Win32::Foundation::*;
use windows::Win32::System::Registry::*;
use windows::core::PCWSTR;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(b: &[u8]) -> String {
    let u: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let n = u.iter().position(|c| *c == 0).unwrap_or(u.len());
    String::from_utf16_lossy(&u[..n])
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key was opened by us.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn open(root: HKEY, path: &str, access: REG_SAM_FLAGS) -> Option<Key> {
    let p = wide(path);
    let mut k = HKEY::default();
    // SAFETY: valid NUL-terminated path and out pointer.
    let r = unsafe { RegOpenKeyExW(root, PCWSTR(p.as_ptr()), None, access, &mut k) };
    (r == ERROR_SUCCESS).then_some(Key(k))
}

/// Raw value: (type, bytes).
pub fn read(root: HKEY, path: &str, name: &str) -> Option<(REG_VALUE_TYPE, Vec<u8>)> {
    let key = open(root, path, KEY_READ)?;
    let n = wide(name);
    let mut ty = REG_VALUE_TYPE::default();
    let mut len = 0u32;
    // SAFETY: size query with null data.
    let r = unsafe { RegQueryValueExW(key.0, PCWSTR(n.as_ptr()), None, Some(&mut ty), None, Some(&mut len)) };
    if r != ERROR_SUCCESS {
        return None;
    }
    let mut buf = vec![0u8; len as usize];
    // SAFETY: buf has `len` bytes.
    let r = unsafe {
        RegQueryValueExW(key.0, PCWSTR(n.as_ptr()), None, Some(&mut ty), Some(buf.as_mut_ptr()), Some(&mut len))
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    buf.truncate(len as usize);
    Some((ty, buf))
}

pub fn read_dword(root: HKEY, path: &str, name: &str) -> Option<u32> {
    let (ty, b) = read(root, path, name)?;
    (ty == REG_DWORD && b.len() >= 4).then(|| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

pub fn read_string(root: HKEY, path: &str, name: &str) -> Option<String> {
    let (ty, b) = read(root, path, name)?;
    (ty == REG_SZ || ty == REG_EXPAND_SZ).then(|| from_wide(&b))
}

fn create(root: HKEY, path: &str) -> Option<Key> {
    let p = wide(path);
    let mut k = HKEY::default();
    // SAFETY: valid NUL-terminated path and out pointer.
    let r = unsafe {
        RegCreateKeyExW(
            root,
            PCWSTR(p.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut k,
            None,
        )
    };
    (r == ERROR_SUCCESS).then_some(Key(k))
}

pub fn write_string(root: HKEY, path: &str, name: &str, value: &str) -> bool {
    let Some(key) = create(root, path) else { return false };
    let n = wide(name);
    let v = wide(value);
    let bytes: Vec<u8> = v.iter().flat_map(|c| c.to_le_bytes()).collect();
    // SAFETY: valid key and buffers.
    unsafe { RegSetValueExW(key.0, PCWSTR(n.as_ptr()), None, REG_SZ, Some(&bytes)) == ERROR_SUCCESS }
}

/// Deleting an absent value counts as success.
pub fn delete_value(root: HKEY, path: &str, name: &str) -> bool {
    let Some(key) = open(root, path, KEY_WRITE) else { return true };
    let n = wide(name);
    // SAFETY: valid key and name.
    let r = unsafe { RegDeleteValueW(key.0, PCWSTR(n.as_ptr())) };
    r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND
}

pub fn subkeys(root: HKEY, path: &str) -> Vec<String> {
    let Some(key) = open(root, path, KEY_READ) else { return Vec::new() };
    let mut out = Vec::new();
    for i in 0..4096u32 {
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        // SAFETY: buf and len describe a valid buffer.
        let r = unsafe {
            RegEnumKeyExW(key.0, i, Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None)
        };
        if r != ERROR_SUCCESS {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    out
}
