use gtk;
use libc;
use nix::sys::signal;
use nix::sys::wait;
use nix::unistd::Pid;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::app;
use crate::er::Result;

// perl cssh installs this handler for SIGCHLD
//    $SIG{CHLD} = sub {
//        my $kid;
//        do {
//            $kid = waitpid( -1, WNOHANG );
//        } until ( $kid == -1 || $kid == 0 );
//    };
//
// perl cssh tells Tk::MainWindow to poll every 500ms,
//   and check if each child is alive (via kill 0 PID).
//   It updates the UI's menu check boxes (if it notices a change),
//   and stops the parent process if all children are dead.
//
// Polling sounds inefficient, but signal handlers cannot do much.
//   They cannot use a mutex (since we could get a signal while the
//   main thread has the mutex locked, so we'd deadlock).
//   Without a mutex, a signal handler cannot adjust app.servers.
//
// Furthermore, even if we could get around the locking, we cannot
//   update the UI's menu check boxes.  Calls to gtk may only be made
//   via the main thread (else gtk panics).
//
// So we're going to be polling anyways, so may as well tend to the
//   children at that time.

extern "C" fn handle_sigchld(_: i32) {
    for _ in 0..1000 {
        if let Ok(s) = wait::waitpid(
            Some(Pid::from_raw(-1 as libc::pid_t)),
            Some(wait::WaitPidFlag::WNOHANG),
        ) {
            match s {
                wait::WaitStatus::Stopped(_, _) => break,
                wait::WaitStatus::Exited(_, _) => break,
                _ => continue,
            }
        } else {
            break;
            // perl looped regardless of errors returned.
            // But I once got "ECHILD (No child processes)" and the
            // CPU was stuck spining at 100%.
            // So, now we have this 'break' and switched the
            // unbounded 'loop' to a bounded 'for'.
            //
            // FWIW: The ECHILD is when this signal handler is
            // installed and we spawn via std::process::Command.
            // So we no longer do that, and have a check to prevent it.
        }
    }
}

pub fn setup_sig_chld_handler() -> Result<()> {
    let flags = signal::SaFlags::empty();
    let mask = signal::SigSet::empty();

    INSTALLED.store(true, Ordering::Relaxed);
    // Arguably we should alter INSTALLED within the match below,
    // but the unsafe block is already large enough.
    // So this is sufficiently accurate.
    // Especially since this is just a guard against code rot.

    let sig_action =
        signal::SigAction::new(signal::SigHandler::Handler(handle_sigchld), flags, mask);
    unsafe {
        match signal::sigaction(signal::SIGCHLD, &sig_action) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Error setting up SIGCHLD handler {}", e.description()).into()),
        }
    }
}

pub fn setup_poll_children(rapp: &app::Rapp) {
    let rapp = rapp.clone();
    gtk::timeout_add(500, move || poll_children_once(&rapp));
}

fn poll_children_once(rapp: &app::Rapp) -> gtk::Continue {
    let mut n_servers = 0;
    let mut app = rapp.borrow_mut();
    app.handle_events(rapp);

    // Ok back to the main purpose of this poll.
    // Check if the children are alive/dead and update the UI.
    // (FWIW Vec::new() does not allocate anthing until push())
    let mut dead_keys = Vec::new();
    // iterate once, gather dead_keys so we can prune them.
    for (server_key, ref server) in app.servers.iter() {
        if let Some(pid) = server.pid {
            if signal::kill(pid, None).is_err() {
                // aka kill(pid,0) aka check pid exists
                dead_keys.push(server_key.to_owned());
            } else {
                n_servers += 1; // It's alive.
            }
        } else {
            dead_keys.push(server_key.to_owned());
        }
    }

    if !dead_keys.is_empty() {
        for server_key in dead_keys.iter() {
            if let Some(server) = app.servers.remove(server_key) {
                server.terminate_host();
                if let Some(ref g) = app.gtkstuff {
                    server.remove_menu_item(&g.hosts_menu);
                }
                app.dead_servers.push(server.connect_string);
                println!("{} session closed", server_key);
            }
        }
        dead_keys.clear();
        n_servers = app.servers.len();
        if let Some(ref g) = app.gtkstuff {
            g.change_main_window_title(&app);
        }
    }

    // if no servers are left, maybe we quit
    if n_servers == 0 && app.config.misc.auto_quit && app.internal_activate_autoquit {
        gtk::main_quit();
        return gtk::Continue(false);
    }

    // perl cssh cleared the text_entry upon every idle loop, and Tk kept it clear
    // But gtk we momentarily see the keystrokes in the text_entry field.
    // TODO figure out how to not have the keys displayed at all, not even momentarily.
    //g.text_entry.get_buffer().set_text("");
    gtk::Continue(true)
}

static INSTALLED: AtomicBool = AtomicBool::new(false);

// Our signal handler seems to interfere with std::process::Command
// So check if we've installed it, before any command.
pub fn is_our_sig_handler_installed() -> bool {
    INSTALLED.load(Ordering::Relaxed)
}
