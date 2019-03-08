// After a fork we call handle_fork() which sets up the command to exec().
//
// This used to be one function within app.rs, but
// clippy suggested that an fn with 11 args was too may,
// so I pulled it into it's own struct and file,
// but struct refs need lifetimes, so it's noisy.
//
// I came across this issue..
// https://github.com/nix-rust/nix/issues/586
// We aren't using any threads in the parent process. :)
// But if you ever find the need to create threads,
// then be warned, allocating mem after fork() (as we do
// here) in a multi-treaded app (so not this app) may
// lead to deadlock.

use libc;
use std::ffi::CStr;
use std::ffi::CString;
use std::io;
use std::path::Path;
use std::ptr;

use crate::config;
use crate::host::Host;
use crate::macros;

// One shared lifetime... seems like all these annotations
// should be automatic, possibly required because Host<'a>
// or maybe it is automatic, but I haven't checked recently
#[derive(Debug)]
pub struct Child<'a> {
    pub config: &'a config::Config,
    pub comms: &'a str,
    pub comms_args: &'a str,
    pub command: &'a str,
    pub auto_close: &'a str,
    pub host_str: &'a str,
    pub host: &'a Host<'a>,
    pub given_server_name: &'a str,
    pub pipenm: &'a Path,
    pub server_key: &'a str,
    pub me: &'a str,
}

impl<'a> Child<'a> {
    // divergent function. It does not return
    pub fn handle_fork(&self) -> ! {
        let mut cmd = String::with_capacity(1024);

        cmd += self.config.terminal.terminal_name.as_ref();
        cmd += " ";

        if self.config.terminal.colorize {
            if self.config.terminal.bg_style_dark {
                cmd += "-bg \\#000000 -fg ";
            } else {
                cmd += "-fg \\#000000 -bg ";
            }
            pick_color(&mut cmd, &self.host.hostname);
            cmd += " ";
        }

        if let Some(args) = self.config.terminal.args.as_ref() {
            cmd += &args;
            cmd += " ";
        }
        cmd += &self.config.terminal.allow_send_events;
        cmd += " ";
        cmd += &self.config.terminal.title_opt;
        cmd += " '";
        if let Some(ref title) = self.config.dynamic.title {
            cmd += title;
        }
        cmd += ": ";
        cmd += &self.host_str; // host_str is untouched from cmd line mimic-ing perl cssh.
                               // This allows the user to inject ' or ` etc into our cmd, which is odd.
                               // but trust the user to not shoot themselves in the foot.

        cmd += "' -font ";
        cmd += &self.config.terminal.font;
        cmd += " -e ";
        cmd += self.me;
        cmd += " --helper ";
        cmd += " ";
        cmd += self.comms;
        cmd += " '";
        cmd += self.comms_args;
        cmd += "' '";

        if !self.command.is_empty() {
            // When run with --action (or -a, or a config value of command set) then perl cssh would
            // "Run the command in each session, e.g. C<-a 'grep foo /etc/bar'> to drop straight into a vi session."
            // This is passed as command line arguments twice,
            // once here (forked process to helper.rs process)
            // and once more (helper process to spawned shell).

            match macros::substitute(
                self.command,
                &self.config.macros,
                self.server_key,
                self.given_server_name,
                &self.host.username.and_then(|u| Some(String::from(u))),
            ) {
                macros::Subst::None => cmd += self.command,
                macros::Subst::Same { text } => cmd += &text,
                macros::Subst::Diff { text } => cmd += &text,
            }
        }

        cmd += "' '";
        cmd += self.auto_close;
        cmd += "' ";
        cmd += &self.pipenm.to_string_lossy();
        cmd += " ";
        cmd += self.given_server_name;
        cmd += " '";
        if let Some(u) = self.host.username {
            cmd += u;
        } else if let Some(u) = &self.config.dynamic.username {
            cmd += u;
        }
        cmd += "' '";
        if let Some(p) = self.host.port {
            cmd += p;
        } else if let Some(p) = &self.config.misc.port {
            cmd += p;
        }
        cmd += "'";

        exec(&cmd);
    }
}

// pick a color for xterm text.
// We want repeatable colors for hosts upon subsequent runs,
// and we want xterms with the the same hosts to get the same colors,
// so use a hash of the hostname as the random seed.
// For this requirement, libc is so much easier to use than
// the rand crate, and is closer to the algoritm perl cssh used
fn pick_color(cmd: &mut String, hostname: &str) {
    let sum: libc::c_uint = hostname.bytes().map(u32::from).sum();
    unsafe {
        libc::srand(sum);
    };
    *cmd += "\\#";
    // pick a random number in range 0..63, then grab 2 bits at a time.
    let rand = unsafe { libc::rand() };
    let mut bits = rand / ((libc::RAND_MAX / 64) + 1);
    for _ in 0..3 {
        *cmd += match bits & 3 {
            0 => "AA",
            1 => "BB",
            2 => "CC",
            _ => "EE",
        };
        bits >>= 2;
    }
}

// perl's exec($foo) calls 'sh -c' implicitly, if it sees that $foo contains a shell meta character
// So we call "sh -c" explicitly.
pub fn exec(command: &str) -> ! {
    let sh = CStr::from_bytes_with_nul(b"sh\0").unwrap();
    let _c = CStr::from_bytes_with_nul(b"-c\0").unwrap();

    let cmd = CString::new(command.as_bytes()).unwrap();

    unsafe {
        libc::execlp(
            sh.as_ptr(),
            sh.as_ptr(),
            _c.as_ptr(),
            cmd.as_ptr(),
            ptr::null() as *const char,
        )
    };

    panic!(format!("execlp failed {}", io::Error::last_os_error()));
}
