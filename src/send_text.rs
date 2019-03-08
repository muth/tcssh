// This sends text to all servers which are flagged as active.
use libc;

use crate::app;
use crate::app::Wid;
use crate::macros;

enum SendTo {
    All {},
    One { wid: Wid },
}

pub fn send_variable_text(app: &mut app::App) {
    // We do not want random.  We want repeatable.  So use libc's simple random routines.
    // e.g. if you run this via
    //    cssh ::1 ::1
    // and then via the "Send" menu, choose "Random Number" you'll get say 123 and 456
    // then exit both xterms, and close cssh
    // and repeat the above with a new cssh process.
    // and you'll get the same numbers again 123 456.
    // So since cssh did it.. tcshh will do it too.
    for (_, ref server) in app.servers.iter() {
        if !server.active {
            continue;
        }
        let rand = unsafe { libc::rand() };
        let rand_1024 = rand / ((libc::RAND_MAX / 1024) + 1);
        let text = format!("{}", rand_1024);
        translate_and_send(&text, app, SendTo::One { wid: server.wid });
    }
    app.xdisplay.flush();
}

pub fn send_text(app: &mut app::App, text: &str) {
    let macros_enabled = app.config.macros.enabled;

    for (ref server_key, ref server) in app.servers.iter() {
        if !server.active {
            continue;
        }
        if !macros_enabled {
            translate_and_send(&text, app, SendTo::All {});
            break;
        }

        match macros::substitute(
            text,
            &app.config.macros,
            server_key,
            &server.givenname,
            &server.username,
        ) {
            macros::Subst::None => {
                translate_and_send(text, app, SendTo::All {});
                break;
            }
            macros::Subst::Same { text } => {
                translate_and_send(&text, app, SendTo::All {});
                break;
            }
            macros::Subst::Diff { text } => {
                translate_and_send(&text, app, SendTo::One { wid: server.wid });
            }
        }
    }
    app.xdisplay.flush();
}

fn translate_and_send(text: &str, app: &app::App, to: SendTo) {
    if let Some(ref text2x11) = app.text2x11 {
        for c in text.chars() {
            match text2x11.translate(c as u32) {
                None => {
                    eprintln!(
                        "Unknown character in xmodmap keytable: {:x} {}",
                        u32::from(c),
                        c
                    );
                }
                Some(sc) => match to {
                    SendTo::One { wid } => {
                        app.send_event(wid, sc.state as u32, sc.code);
                    }
                    SendTo::All {} => {
                        for (_, ref server) in app.servers.iter() {
                            if !server.active {
                                continue;
                            }
                            app.send_event(server.wid, sc.state as u32, sc.code);
                        }
                    }
                },
            }
        }
    }
}
