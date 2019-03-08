// perl cssh used Tk, which had accelerators like "Alt-q" while gtk uses "<Alt>q"
// When we read legacy config files ~/.clusterssh/config then we may see "Alt-q"
// which we (make a best effort attempt to) translate here to their gtk equivalents.

use regex::Regex;

lazy_static! {
    static ref MODIFIER_DASH: Regex = Regex::new(r"^((?:(?:Alt|Control|Ctrl|Ctl|Shift|Button|Release)-)+)(.*)$")
        .expect("Regex error MODIFIER_DASH");

    static ref JUST_MODIFIER : Regex = Regex::new(r"^(Alt|Control|Ctrl|Ctl|Shift|Button)$") // todo, any of these work..
        .expect("Regex error JUST_MODIFIER");
}

pub fn translate_accel(tk: &str) -> Option<String> {
    if let Some(cap) = MODIFIER_DASH.captures(tk) {
        let n_caps = cap.len();
        if 2 <= n_caps && n_caps <= 3 {
            let mut out = String::with_capacity(tk.len() + n_caps);
            for modifier in cap[1].split('-') {
                if !modifier.is_empty() {
                    out += "<";
                    out += modifier;
                    out += ">";
                }
            }
            if n_caps == 3 {
                out += &cap[2];
            }
            return Some(out);
        }
    } else if let Some(cap) = JUST_MODIFIER.captures(tk) {
        if cap.len() == 2 {
            let len = cap[1].len();
            let mut out = String::with_capacity(len + 2);
            out += "<";
            out += &cap[1];
            out += ">";
            return Some(out);
        }
    }
    None
}

#[test]
fn test_tk2gtk_translate_accel() {
    let tests = [
        ("Alt-x", Some("<Alt>x".to_string())),
        (
            "Control-Shift-plus",
            Some("<Control><Shift>plus".to_string()),
        ),
        ("Alt", Some("<Alt>".to_string())),
        ("Alt-", Some("<Alt>".to_string())),
        ("x", None),
        ("Button-2", Some("<Button>2".to_string())), // TODO mouse mapping. Button-2, gtk doesn't work for me.
                                                     // Release also doesn't work for me.
    ];
    for (tk, gtk) in tests.iter() {
        let out = translate_accel(tk);
        assert_eq!(&out, gtk);
    }
}
