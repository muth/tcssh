// perl cssh's macros are invoked when pasting text
// and when spawning child xterms.
//
// They are just regex replacements of (configurable) strings.
// e.g. %s gets replaced with a the cooked servername for that xterm.

use libc::getpwuid;
use regex::Regex;
use std::borrow::Cow;
use std::ffi::CStr;

use crate::config::Macros;
use crate::is_xfile;

pub static VERSION_JUST_NUMBER: &'static str = "0.2.0";
static VERSION_LONG: &'static str = "Transparent Cluster SSH 0.2.0";

lazy_static! {
    static ref USERNAME: String = unsafe {
        let passwd = getpwuid( *is_xfile::EUID );
        // man getpwuid says returned value is pointer to static mem, so no free(),
        // and may be overwritten by subsequent calls, so we copy it.
        if (! passwd.is_null()) && (! (*passwd).pw_name.is_null()) {
            CStr::from_ptr((*passwd).pw_name).to_string_lossy().into_owned()
        } else {
            String::from("")
        }
    };

    static ref STRIP_WS: Regex = Regex::new(r"\s+").unwrap();
}

pub enum Subst {
    None,                  // No substitition
    Same { text: String }, // subst will be the same for all xterms
    Diff { text: String }, // subst may differ for different xterms
}

// This could be simpler if we didn't care about Diff/Same and always returned .to_owned()
pub fn substitute<'a>(
    text: &'a str,
    macros: &Macros,
    servername: &str,
    hostname: &str,
    username: &Option<String>,
) -> Subst {
    // Most likely the text being pasted does not contain any macros.
    // so check for it in one shot and return.
    if let Some(ref re) = macros.all_re {
        // In the default config we know one RegEx which covers all preset macros.
        // But if the user has supplied their own RegEx for any of the macros then
        // we have to clear 'all_re' and look up individual ones.
        if !re.is_match(text) {
            return Subst::None;
        }
    }

    let mut flag_same = false;
    let mut flag_diff = false;

    let text = if let Some(ref re) = macros.servername_re {
        // stripping WS is what perl cssh does so we do it too.
        let stripped: &str = &(STRIP_WS.replace(servername, ""));
        let cow = re.replace_all(&text, stripped);
        if let Cow::Owned(_) = cow {
            flag_diff = true;
        }
        cow
    } else {
        Cow::from(text) // borrows original text
    };

    let text = if let Some(ref re) = macros.hostname_re {
        let cow = re.replace_all(&text, hostname);
        if !flag_diff {
            if let Cow::Owned(_) = cow {
                flag_diff = true;
            }
        }
        cow
    } else {
        text
    };

    let text = if let Some(ref re) = macros.username_re {
        let mut set_same = false;
        let username: &str = match username {
            Some(username) => &username,
            None => {
                set_same = true;
                &*USERNAME
            }
        };
        let cow = re.replace_all(&text, username);
        if let Cow::Owned(_) = cow {
            if (!flag_diff) && set_same {
                flag_same = true;
            } else {
                flag_diff = true;
            }
        }
        cow
    } else {
        text
    };

    let text = if let Some(ref re) = macros.newline_re {
        let cow = re.replace_all(&text, "\n");
        if !flag_diff {
            if let Cow::Owned(_) = cow {
                flag_same = true;
            }
        }
        cow
    } else {
        text
    };

    let text = if let Some(ref re) = macros.version_re {
        let cow = re.replace_all(&text, VERSION_LONG);
        if !flag_diff {
            if let Cow::Owned(_) = cow {
                flag_same = true;
            }
        }
        cow
    } else {
        text
    };

    if flag_diff {
        Subst::Diff {
            text: text.into_owned(),
        } // into_owned should be nop
    } else if flag_same {
        Subst::Same {
            text: text.into_owned(),
        }
    } else {
        Subst::None
    }
}

#[cfg(test)]
mod macros_tests {
    use super::*; // so we can access non pub stuff in the retile mod.

    #[test]
    fn test_macros() {
        let macros: Macros = Default::default();

        // check simple text, no substitutions.
        match substitute("foo", &macros, &"", &"", &None) {
            Subst::None => {
                assert!(true);
            }
            _ => assert!(false),
        }

        // check all patterns get substituted
        {
            let the_username: &Option<String> = &Some(String::from("the_username"));
            match substitute(
                "foo %s bar %h baz %u bip %v bop",
                &macros,
                &"the_servername",
                &"the_hostname",
                the_username,
            ) {
                Subst::Diff { text: got } => {
                    let expected = format!(
                        "foo the_servername bar the_hostname baz the_username bip {} bop",
                        VERSION_LONG
                    );
                    assert_eq!(got, expected);
                }
                _ => assert!(false),
            }
        }

        // check macros are expaned in order, and first white space is stripped from servername
        {
            match substitute("x %s y", &macros, &"% h\t", &"%u", &None) {
                Subst::Diff { text: got } => {
                    // "x %s y" subst %s with "% h\t" with its first white space stripped
                    // "x %h\t y" subst %h with "%u"
                    // "x %u\t y" subst %u with *USERNAME
                    let expected = format!("x {}\t y", *USERNAME);
                    assert_eq!(got, expected);
                }
                _ => assert!(false),
            }
        }

        // check that we get ::Same on non-volatile substs
        {
            match substitute(
                "foo %u bar %n baz %v bip",
                &macros,
                &"the_servername",
                &"the_hostname",
                &None,
            ) {
                Subst::Same { text: got } => {
                    let expected = format!("foo {} bar \n baz {} bip", *USERNAME, VERSION_LONG);
                    assert_eq!(got, expected);
                }
                Subst::Diff { text: got } => {
                    // We should get a Subst::Same instead of Subst::Diff,
                    // so force a mismatch, and show the contents.
                    let expected = format!(
                        "foo {} bar \n baz {} bip BUT should be Subst::Same",
                        *USERNAME, VERSION_LONG
                    );
                    assert_eq!(got, expected);
                }
                _ => assert!(false),
            }
        }
    }
}
