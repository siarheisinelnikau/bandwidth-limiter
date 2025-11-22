//! Reject policy/installation-directory ACLs writable or owned by ordinary accounts.
use std::{
    path::Path,
    ptr::{addr_of, null_mut},
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{Authorization::*, *},
};

unsafe fn trusted(sid: PSID) -> bool {
    // SAFETY: callers pass a SID returned by Windows security APIs or a validated ACE.
    unsafe {
        !sid.is_null()
            && IsValidSid(sid) != 0
            && (IsWellKnownSid(sid, WinLocalSystemSid) != 0
                || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0)
    }
}
pub fn require_admin_owned(path: &Path) -> Result<(), String> {
    use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
    if std::fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .file_attributes()
        & 0x400
        != 0
    {
        return Err("Policy and installation directory must not be reparse points".into());
    }
    let name: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut owner = null_mut();
    let mut acl = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: Windows allocates descriptor and points owner/ACL into that allocation; freed below.
    let code = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut acl,
            null_mut(),
            &mut descriptor,
        )
    };
    if code != 0 {
        return Err(format!("Cannot inspect policy ACL: {code}"));
    }
    let result = (|| {
        // SAFETY: ACL and owner came from GetNamedSecurityInfoW and are used while descriptor is live.
        unsafe {
            if !trusted(owner) || acl.is_null() || IsValidAcl(acl) == 0 {
                return Err("Policy owner/DACL must be SYSTEM or Administrators with a valid restrictive ACL".into());
            }
            for index in 0..u32::from((*acl).AceCount) {
                let mut ace = null_mut();
                if GetAce(acl, index, &mut ace) == 0 {
                    return Err("Cannot inspect policy ACE".into());
                }
                let header = &*ace.cast::<ACE_HEADER>();
                if header.AceType == 1 {
                    continue;
                } // deny entries cannot grant write access
                if header.AceType != 0 {
                    return Err(
                        "Unsupported policy ACE type; use the documented explicit ACL".into(),
                    );
                }
                let allowed = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                let sid = addr_of!(allowed.SidStart).cast_mut().cast();
                const MUTATION: u32 = 0x500d0156; // GENERIC_ALL/WRITE, DELETE, WRITE_DAC/OWNER, data/EA/attributes/delete-child
                if allowed.Mask & MUTATION != 0 && !trusted(sid) {
                    return Err("Policy or installation directory grants write/delete/ownership rights to a non-administrator".into());
                }
            }
        }
        Ok(())
    })();
    // SAFETY: descriptor was allocated by the Windows security API with LocalAlloc semantics.
    unsafe {
        LocalFree(descriptor);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_sids_do_not_authorize_builtin_users() {
        for (kind, expected) in [
            (WinLocalSystemSid, true),
            (WinBuiltinAdministratorsSid, true),
            (WinBuiltinUsersSid, false),
        ] {
            let mut buffer = [0u64; 16];
            let mut size = size_of_val(&buffer) as u32;
            // SAFETY: synthetic SID construction only, no account lookup, token or system mutation.
            unsafe {
                assert_ne!(
                    CreateWellKnownSid(kind, null_mut(), buffer.as_mut_ptr().cast(), &mut size),
                    0
                );
                assert_eq!(trusted(buffer.as_mut_ptr().cast()), expected);
            }
        }
    }
}
