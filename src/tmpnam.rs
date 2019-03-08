use libc;
use nix::sys::stat;
use nix::unistd::mkfifo;
use std::error::Error;
use std::ffi::CStr;
use std::path::PathBuf;

use crate::er::Result;

pub fn tmpnam() -> Result<PathBuf> {
    // man tmpnam -> """Note: Avoid use of tmpnam();
    //	...
    //	Although tmpnam() generates names that are difficult to guess,
    //	it is nevertheless possible that between the time that tmpnam()
    //	returns a pathname, and the time that the program opens it, another
    //	program might create that pathname using open(2), or create it as
    //	a symbolic link.  This can lead to security holes.
    //  ...
    //  returns a pointer to a unique temporary filename,
    //  or NULL if a unique name cannot be generated.
    //	"""
    //
    // For perl cssh, the result of tmpnam() is fed into mkfifo() and the children
    // write their PID:WINDOWID to the parent.
    // The parent uses WINDOWID to send key events (like passwords being typed) to children.
    // So the data passed in the fifo seems important, but I'm too unfamiliar with X11 to
    // know if that's a problem. This spec page
    // https://www.x.org/releases/X11R7.6/doc/libX11/specs/libX11/libX11.html#display_functions
    // says "X does not provide any protection on a per-window basis.
    // If you find out the resource ID of a resource, you can manipulate it."
    //
    // So does that mean if someone were to MITM the fifo, and injected their own WINDOWID,
    // could they intercept key events to get the password?
    //
    // Well MITM has a few hurdles.
    // If an attacker guesses tmpnam() and pre-creates a fifo,
    // then perl cssh dies, "File exists" (see test_mkfifo_exists)
    //
    // We create the fifo in mode 600, so an attacker cannot read/write to it. (see test_mkfifo_mode)
    //
    // So that means an attacker would have to rm the fifo and replace it with their own,
    // but on proper systems /tmp likely has the sticky bit set,
    // so only owner/root/dir_owner can rm/mv/rm the fifo. (see test_mkfifo_stickybit)
    //
    // So, it is concerning, but seems secure enough. (Especially since this is most often
    // used from a single user desktop, (Doh, that was true, but now ssh "jump boxes" have
    // became a popular way for sec-ops to lock down and record access)).
    //
    // FWIW: You may think an anonymous pipe() or shared memory region would be better,
    // but we don't talk directly with the child which we fork, the child exec()s a 'sh'
    // which exec()s an 'xterm' which calls our helper script (which calls ssh).
    // The helper script is what opens and writes to the fifo.
    // The the helper script wouldn't have access to the parent's anonymous pipe.
    // e.g.  tcssh -> sh -> xterm -> helper script which writes to the tmpnam fifo
    //
    // Since there are so many processes between us and the eventual writers to
    // the fifo, it makes secure communication difficult.
    //
    // Adding encryption does not solve this problem.
    // Any encryption would have to pass a key to the helper script.
    // An attacker could learn the key, rm the fifo, encrypt its own windowid and pid.
    // And then tcssh would just send along key events (password) to the attacker.
    //
    // So encrypting doesn't really matter, we're back to an attack on the
    // fifo (if an attacker guesses the name, rm-s, creates their own,
    // they can capture keys, and get the password).
    //
    // TODO: Worth moving from KISS FIFO to rube-goldberg-esque secure commmunication?
    // TODO: change the test_mkfifo_stickybit() to a run time check?
    let mut out = PathBuf::new();
    let mut buf: Vec<i8> = Vec::with_capacity(libc::L_tmpnam as usize);
    let ptmp = unsafe { libc::tmpnam(buf.as_mut_ptr()) };
    if ptmp.is_null() {
        return Err("tmpnam failed".into());
    }
    match unsafe { CStr::from_ptr(ptmp) }.to_str() {
        Ok(s) => {
            out.push(s);
            Ok(out)
        }
        Err(_) => Err("tmpnam returned non utf8".into()),
    }
}

// separate fn just so it's callable in a test
fn _mkfifo(pipenm: &PathBuf) -> Result<()> {
    match mkfifo(pipenm, stat::Mode::S_IRUSR | stat::Mode::S_IWUSR) {
        Err(e) => Err(format!("mkfifo failed {} {}", pipenm.display(), e.description()).into()),
        Ok(()) => Ok(()),
    }
}

pub fn tmpnam_and_mkfifo() -> Result<PathBuf> {
    let pipenm = tmpnam()?;
    _mkfifo(&pipenm)?;
    Ok(pipenm)
}

#[test]
fn test_mkfifo_exists() {
    let pipenm = tmpnam().unwrap();
    assert_eq!(_mkfifo(&pipenm).is_ok(), true);
    let e = _mkfifo(&pipenm);
    use std::fs::remove_file;
    remove_file(&pipenm).unwrap();
    assert_eq!(e.is_err(), true);
}

#[test]
fn test_mkfifo_mode() {
    let pipenm = tmpnam().unwrap();
    assert_eq!(_mkfifo(&pipenm).is_ok(), true);

    let x: Result<()> = match pipenm.metadata() {
        Ok(v) => {
            use std::os::linux::fs::MetadataExt;
            let mode = v.st_mode();
            if 0o600 == (0o777 & mode) {
                // security check
                if stat::SFlag::S_IFIFO.bits() == (stat::SFlag::S_IFIFO.bits() & mode) {
                    // over testing
                    Ok(())
                } else {
                    Err(format!(
                        "mkfifo mode {:o} indicates it is not a fifo, for pipenm {:?}",
                        mode, pipenm
                    )
                    .into())
                }
            } else {
                Err(format!(
                    "mkfifo mode expected 600 got {:o} for pipenm {:?}",
                    mode & 0o777,
                    pipenm
                )
                .into())
            }
        }
        Err(e) => Err(format!(
            "Failed to get metadata for pipenm {:?} err={}",
            pipenm,
            e.description()
        )
        .into()),
    };
    use std::fs::remove_file;
    remove_file(&pipenm).unwrap();
    assert_eq!(x, Ok(()));
}

#[test]
fn test_mkfifo_stickybit() {
    let pipenm = tmpnam().unwrap();
    let dir = pipenm.parent().unwrap();

    let x: Result<()> = match dir.metadata() {
        Ok(v) => {
            use std::os::linux::fs::MetadataExt;
            let mode = v.st_mode();
            if stat::Mode::S_ISVTX.bits() == (stat::Mode::S_ISVTX.bits() & mode) {
                Ok(())
            } else {
                Err(format!(
                    "tmpnam() directory {:?} does not have the sticky bit set mode={:o}",
                    dir, mode
                )
                .into())
            }
        }
        Err(e) => Err(format!(
            "Failed to get metadata for dir {:?} err={}",
            dir,
            e.description()
        )
        .into()),
    };
    assert_eq!(x, Ok(()));
}
