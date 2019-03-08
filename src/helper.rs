// The perl version of cssh fork()s for each terminal then calls
// exec("xterm -e perl -e 'in-place-script-with-values-from-$config' $pipe $svr $usr $port $mstr");
// So the parent passes $config values through to xterm and to perl via a quoted 'in-place-script...'
//
// This executable, is the same as the 'in-place-script...', it
// 1) opens the pipe and writes the PID and WINDOWID back to the parent,
// 2) shells out to run the real command (most often ssh),
// 3) after ssh exits, it echos and sleeps (or reads) so user gets feedback after ssh exits.
//
// This is called by child.rs which calls (executable compiled from) main.rs with --helper.
//
// TODO: there are plenty of .expect() calls within this.
// So we can fail before opening and writing anything to the pipe.
// The parent though.. currently blocks waiting for input.
// TODO parent should timeout if no input available for $some-TBD-config-seconds
// But it's not vital, since there was no such timeout in perl cssh.

use std::env;
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::io::Write;
use std::thread;
use std::time::Duration;

use crate::child;

pub fn run(args: &mut env::Args) {
    let (pipe, command) = parse_args(args);

    write_to_pipe(pipe, get_pid_and_windowid());

    // perl cssh has a warn before exec, mimic it.
    eprintln!("Running: {}", &command);

    child::exec(&command);
}

fn parse_args(args: &mut env::Args) -> (String, String) {
    let comms = args
        .next()
        .expect("Expected first argument to be ssh, console, rsh, sftp, or telnet");
    let comms_args = args
        .next()
        .expect("Expected second argument to be comm args");
    let config_command = args
        .next()
        .expect("Expected third argument to be config_command");
    let auto_close = args
        .next()
        .expect("Expected fourth argument to be auto_close");
    let pipe = args
        .next()
        .expect("Expected fifth argument to be the path to a named pipe");

    let mut command = String::with_capacity(256);
    command += &comms;
    command += " ";
    command += &comms_args;
    command += " ";

    let svr_str: String;
    let svr = if let Some(svr) = args.next() {
        svr_str = svr;
        if svr_str.ends_with("==") {
            let trimmed = svr_str.trim_end_matches("==");
            eprintln!("\nWARNING: failed to resolve IP address for {}.\n", trimmed);
            let five_seconds = Duration::new(5, 0);
            thread::sleep(five_seconds);
            trimmed
        } else {
            &svr_str
        }
    } else {
        ""
    };

    if let Some(user) = args.next() {
        if (!user.is_empty()) && comms != "telnet" {
            command += "-l ";
            command += &user;
            command += " ";
        }
    }

    let port_str: String;
    let port = if let Some(port) = args.next() {
        port_str = port;
        &port_str
    } else {
        ""
    };

    if comms == "telnet" {
        command += svr;
        command += " ";
        command += port;
    } else if !port.is_empty() {
        command += "-p ";
        command += port;
        command += " ";
        command += svr;
    } else {
        command += svr;
    }

    if !config_command.is_empty() {
        command += " \"";
        command += &config_command;
        command += "\"";
    }

    command += " ; ";
    if auto_close.is_empty() || auto_close == "0" {
        command += "echo Press RETURN to continue; read IGNORE";
    } else {
        // perl didn't quote the echo params.. so do the same.
        command += "echo Sleeping for ";
        command += &auto_close;
        command += " seconds; sleep ";
        command += &auto_close;
    };

    (pipe, command)
}

fn get_pid_and_windowid() -> String {
    let pid = std::process::id(); //let pid: i32 = unsafe { libc::getpid() };  // before rust 1.27

    let windowid = std::env::var("WINDOWID").expect("No WINDOWID env var");

    format!("{}:{}\n", pid, windowid)
}

fn write_to_pipe(fname: String, s: String) {
    let f = OpenOptions::new()
        .write(true)
        .create_new(false)
        .append(true)
        .open(&fname);

    match f {
        Ok(file) => {
            let mut writer = BufWriter::new(file);
            let buf = s.as_bytes();
            let wrote = writer.write(buf).unwrap();
            if wrote != buf.len() {
                panic!(
                    "Failed to write everything to pipe {}. Wrote only {} bytes of {}",
                    fname, wrote, s
                );
            }
            // The rust book says writer would flush() before drop/close.
            writer
                .flush()
                .unwrap_or_else(|_| panic!("Failed to flush pipe {}", fname));
        }
        Err(e) => {
            panic!("Could not open {} {:?}", fname, e);
        }
    };
}
