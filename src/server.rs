// A Server struct holds info about about every xterm spawned.
// App also holds a BTreeMap of Servers, and this file has a few routines
// for handling that BTreeMap too.

use gtk::{
    CheckMenuItem,
    CheckMenuItemExt, // for set_active()
    ContainerExt,     // for menu.remove()
    Menu,
};
use nix::sys::signal;
use nix::unistd::{fork, ForkResult, Pid};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::app::Wid;
use crate::child;
use crate::config;
use crate::er::Result;
use crate::host;
use crate::tmpnam;

pub type BumpType = u8;

#[derive(Debug, Default)]
pub struct Server {
    pub wid: Wid,
    pub pid: Option<Pid>,
    pub active: bool,
    pub bump_num: BumpType,
    pub connect_string: String,
    pub givenname: String,
    pub username: Option<String>,
    pub pipenm: Option<PathBuf>,
    pub menu_item: Option<CheckMenuItem>,
}

impl Server {
    pub fn terminate_host(&self) {
        if let Some(pid) = self.pid {
            // aka kill(pid,0) aka check pid exists
            if signal::kill(pid, None).is_ok() {
                // now that we know pid exists, send an actual kill
                // I don't know why perl cssh did this two phase kill.
                // but it has many years of use, in various environments
                // so I assume there's some good reason.
                signal::kill(pid, signal::Signal::SIGKILL).ok(); // ignore error
            }
        }
    }
    pub fn remove_menu_item(&self, hosts_menu: &Menu) {
        if let Some(ref menu_item) = self.menu_item {
            hosts_menu.remove(menu_item);
        }
    }
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
        if let Some(ref m) = self.menu_item {
            m.set_active(active);
        }
    }
}

fn get_server_key(servers: &mut BTreeMap<String, Server>, hostname: &str) -> Option<String> {
    // This bump_num stuff deserves a bit of explanation.
    // If you invoke tcssh with repeated host names
    // e.g. "tcssh ::1 ::1 ::1" then the first '::1' becomes associated
    // with server_key '::1' within the BTreeMap known as app.servers.
    //
    // The second '::1' finds that '::1' is already present in app.servers
    // so we increment the bump_num within the entry for '::1' and
    // set the second server_key to '::1 1'
    //
    // The third server_key is '::1 2' with the bump_num for '::1' set to 2.
    //
    // This also checks for overflow, and stops if we hit 256 (BumpType::max_value()).
    // The above is if you're starting with a clean slate.
    //
    // But if a user has closed some terminals, then uses the menu option to
    // "Re-add closed _session(s)" then app.servers has random elements missing.
    // So before calling this, the caller should call clear_bump_nums() so
    // we can re-use some numbers, (and we have to check for collisions).
    let mut server_key = hostname.to_string();
    let max_value = BumpType::max_value();
    loop {
        if let Some(v) = servers.get_mut(&server_key) {
            if v.bump_num == max_value {
                eprintln!("Reached limit of {} connections to same host...", max_value);
                return None;
            }
            v.bump_num += 1;
            server_key = format!("{} {}", hostname, v.bump_num);
        // Most common case we have one allocation for the name,
        // and re-use it as the server_key in the BTreeMap
        // But the worst case.. that's one allocation per loop :(
        } else {
            return Some(server_key);
        }
    }
}

pub fn clear_bump_nums(servers: &mut BTreeMap<String, Server>) {
    for server in servers.values_mut() {
        server.bump_num = 0;
    }
}

pub fn open_client_windows(
    host_strs: &[String],
    servers: &mut BTreeMap<String, Server>,
    config: &config::Config,
    internal_activate_autoquit: &mut bool,
    me: &str,
) -> Result<()> {
    let (comms, comms_args, command, auto_close) = config.get_script_args();

    for host_str in host_strs {
        if host_str.is_empty() {
            continue;
        }

        let host = match host::parse(&host_str) {
            Some(host) => host,
            None => {
                eprintln!("Could not parse host_str {}", host_str);
                // perl cssh would die if any host failed to parse.
                //  cssh 127.0.0.1 user@ # terminates with 'hostname is undefined'
                // tcssh 127.0.0.1 user@ # prints above & opens an xterm to 127.0.0.1
                continue;
            }
        };

        let pipenm = tmpnam::tmpnam_and_mkfifo()?;

        let given_server_name = host.hostname;

        let server_key = match get_server_key(servers, given_server_name) {
            Some(server_key) => server_key,
            None => continue,
        };

        match fork() {
            Ok(ForkResult::Child) => {
                let child = child::Child {
                    config: &config,
                    comms,
                    comms_args,
                    command,
                    auto_close,
                    host_str: &host_str,
                    host: &host,
                    given_server_name,
                    pipenm: &pipenm,
                    server_key: &server_key,
                    me,
                };
                child.handle_fork();
            }
            Ok(ForkResult::Parent { child }) => {
                let server = Server {
                    wid: 0,
                    pid: Some(child),
                    active: false,
                    bump_num: 0,
                    connect_string: host_str.to_owned(),
                    givenname: given_server_name.to_owned(),
                    username: host.username.and_then(|u| Some(String::from(u))),
                    pipenm: Some(pipenm),
                    menu_item: None,
                };

                servers.insert(server_key, server);
            }
            Err(e) => {
                println!("fork() error {:?}", e);
                fs::remove_file(&pipenm).ok();
            }
        }
    }

    let mut err_servers = Vec::new();
    for (ref server_key, ref mut server) in servers.iter_mut() {
        if let Some(ref mut pipenm) = server.pipenm {
            // perl slept here 0.1s for each server, with the comment
            // "sleep for a moment to give system time to come up"
            // But the parent creates the pipe, so the parent can read
            // and block waiting for input.
            // So avoid sleep by default, but if configured, then doit.
            config.tcssh.sleep(100);

            // TODO add a timeout to read_pipe, else children who die before
            // writing to pipe cause us to block forever.
            // But wait for futures to stabalize.. because the complexity
            // of the current code feels just about right.. (minus the
            // read timeout for it to be rock solid).  And again perl cssh
            // did not have a timeout, and after many years deployed in the
            // field.. never needed one.
            if let Err(e) = read_pipe(&pipenm, &mut server.pid, &mut server.wid) {
                eprintln!("Error reading pipe {} {}", pipenm.to_string_lossy(), e);
                // perl just printed to stderr, then marked as active (no pid, no wid).
                // which seems odd, so lets remove this server since we don't know it's pid or wid.
                err_servers.push(server_key.to_string());
            } else {
                server.active = true;
                *internal_activate_autoquit = true;
            }
            fs::remove_file(&pipenm).ok(); // ignore error
        }
        server.pipenm = None;
    }
    // if we couldn't read the pipe, no pid, no wid, then remove them.
    if !err_servers.is_empty() {
        for server_key in err_servers.iter() {
            servers.remove(server_key);
        }
    }

    Ok(())
}

// Parent makes a pipe/mkfifo per child,
// and passes the pipe's name to each child.
// The child writes back PID:WINDOWID
// this is the parent's routine to read PID:WINDOWID from the pipe
//
// This is not part of the impl block because the caller already has
// an immutable reference to self.config, and a mutable reference to self.servers,
// so it cannot create another reference (of any kind) to self.
fn read_pipe(pipenm: &Path, pid_out: &mut Option<Pid>, wid_out: &mut Wid) -> Result<()> {
    let file = fs::OpenOptions::new()
        .read(true)
        .create_new(false)
        .open(pipenm)?;

    let mut buf = String::with_capacity(46); // pid:windowid+4 ~ len(2^64)*2+5
                                             // 4 is just padding. 5 includes the :
    let mut reader = BufReader::new(file);
    reader.read_line(&mut buf)?;
    let mut i = buf.trim_end().split(':');

    if let Some(pid_str) = i.next() {
        if let Ok(pid) = u64::from_str(pid_str) {
            if let Some(wid_str) = i.next() {
                if let Ok(wid) = u64::from_str(wid_str) {
                    *wid_out = wid as Wid;
                    *pid_out = Some(Pid::from_raw(pid as i32));
                    return Ok(());
                }
            }
        }
    }
    Err("Expected PID:WINDOWID".into())
}
