//#![feature(stmt_expr_attributes)] // so retile.rs can use #[rustfmt::skip]

extern crate dirs;
extern crate futures;
extern crate gdk;
extern crate gdk_sys;
extern crate gtk;
#[macro_use]
extern crate lazy_static;
extern crate libc;
extern crate nix;
extern crate regex;
extern crate structopt;
extern crate tokio;
extern crate trust_dns_resolver;
extern crate x11;

mod app;
mod candstr;
mod child;
mod cluster;
mod config;
mod er;
mod evaluate;
mod g;
mod getopt;
mod helper;
mod host;
mod is_xfile;
mod macros;
mod reader;
mod resolver;
mod retile;
mod send_text;
mod server;
mod text2x11;
mod tk2gtk;
mod tmpnam;
mod wait_children;
mod x;

fn main() {
    // arg0 determines what we run in each xterm.
    // if it's tcssh then each xterm runs ssh.
    let mut args = std::env::args();
    let arg0 = args.next().unwrap_or_else(|| "tcssh".to_string());

    // When called with --helper, we're the glue between xterm and ssh
    if let Some(arg1) = args.next() {
        if arg1 == "--helper" {
            helper::run(&mut args);
            std::process::exit(1);
        }
    }

    // To later call ourselves with --helper, we need to know our full name
    let me = match std::env::current_exe() {
        Ok(me) => me,
        Err(_) => {
            println!("Error: could not determine own file name");
            return;
        }
    };
    // Later we concatenate 'me' with other strings,
    // and pass that string to sh/xterm.
    // So 'me' needs to be representable as a String.
    let me = match me.to_str() {
        Some(me) => me,
        None => {
            println!("Error: own file name is not utf8");
            return;
        }
    };

    // Ok start up the app
    let rapp = match app::App::new_ref(&arg0, me) {
        Ok(rapp) => rapp,
        Err(e) => {
            println!("Error setting up app {:?}", e);
            return;
        }
    };
    {
        let mut app = rapp.borrow_mut();
        if let Err(e) = app.run(&rapp) {
            println!("Error: {}", e);
            return;
        };
    }
    wait_children::setup_poll_children(&rapp);

    // pass control to gtk, from here on in we only handle callbacks from the UI,
    // or the idle func we registered, or our signal handler.
    gtk::main();

    rapp.borrow_mut().exit_prog();
}
