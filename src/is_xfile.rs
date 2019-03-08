// Is this an executable file?
// Convenience to check both at once, so only has one call to metadata()

use libc::{getegid, geteuid};
use std::os::linux::fs::MetadataExt;
use std::path::Path;

lazy_static! {
    pub // pub EUID so macros.rs can ask OS for username of euid
    // man geteuid() "These functions are always successful.".. so unsafe?
    static ref EUID: u32 = unsafe { geteuid() };
    static ref EGID: u32 = unsafe { getegid() };
}

pub fn is_executable_file<P: AsRef<Path>>(p: P) -> bool {
    match p.as_ref().metadata() {
        Ok(md) => {
            if !md.is_file() {
                // directories which are 755 don't interest us, we're looking
                // for executable files.  sym links are dereferenced by is_file()
                // until we either find a file or a dir.
                false
            } else {
                let mode = md.st_mode();
                ((mode & 1) == 1)
                    || ((0o100 & mode) == 0o100 && md.st_uid() == *EUID)
                    || ((0o010 & mode) == 0o010 && md.st_gid() == *EGID)
            }
        }
        Err(_) => false,
    }
}

pub trait IsExecutableFile {
    fn is_executable_file(&self) -> bool;
}

// allow p.is_executable() for Path p, but user has to 'use IsExecutableFile'
impl IsExecutableFile for Path {
    fn is_executable_file(&self) -> bool {
        is_executable_file(self)
    }
}
