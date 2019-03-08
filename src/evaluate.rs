// handle the --evaluate and --list CLI args

use std::ffi::OsStr;
use std::process::Command;

use crate::config;
use crate::host;
use crate::wait_children;

pub fn evaluate_commands(evaluate: &str, config: &config::Config) {
    if wait_children::is_our_sig_handler_installed() {
        println!("assertion failure. sig handler will interfere with spawned commands");
        return;
    }

    match host::parse(evaluate) {
        None => return,
        Some(host) => {
            let user_life;
            let user = match host.username {
                None => "",
                Some(user) => {
                    user_life = format!("-l {}", user);
                    &user_life
                }
            };

            let port_life;
            let port = match host.port {
                None => "",
                Some(port) => match config.comms.comms {
                    config::CommsE::Telnet => port,
                    _ => {
                        port_life = format!("-p {}", port);
                        &port_life
                    }
                },
            };

            // 1) Testing terminal
            eprintln!("Testing terminal - running command:");
            eprintln!(
                "{} {} -e sh -c 'echo \"Base terminal test\"; sleep 2'",
                config.terminal.terminal_name, config.terminal.allow_send_events,
            );

            let terminal_name = OsStr::new(&config.terminal.terminal_name as &str);

            let mut command = Command::new(&terminal_name);
            for i in config.terminal.allow_send_events.split_whitespace() {
                command.arg(i);
            }
            command
                .arg("-e")
                .arg("sh")
                .arg("-c")
                .arg("echo \"Base terminal test\"; sleep 2");

            if let Err(e) = command.status() {
                println!("Failed to run terminal {:?} {:?}", e, command);
                return;
            }

            // 2) Testing comms
            let (comms, comms_args, _, _) = config.get_script_args();
            let mut c = String::with_capacity(256);
            c += comms;
            c += " ";
            c += comms_args;
            c += " ";
            match config.comms.comms {
                config::CommsE::Telnet => {
                    c += host.hostname;
                    c += " ";
                    c += port;
                }
                _ => {
                    c += user;
                    c += " ";
                    c += port;
                    c += " ";
                    c += host.hostname;
                    c += " hostname ; echo Got hostname via ssh; sleep 2";
                }
            };

            eprintln!("\nTesting comms - running command:\nsh -c '{}'", c);

            let mut command = Command::new("sh");
            command.arg("-c").arg(&c);

            if let Err(e) = command.status() {
                println!("Failed to run comms {:?} {:?}", e, command);
                return;
            }

            // 3) Testing terminal calling comms
            let mut command = Command::new(&terminal_name);
            for i in config.terminal.allow_send_events.split_whitespace() {
                command.arg(i);
            }
            command.arg("-e").arg("sh").arg("-c").arg(c);

            if let Err(e) = command.status() {
                println!("Failed to run terminal comms {:?} {:?}", e, command);
                return;
            }
        }
    }
}
