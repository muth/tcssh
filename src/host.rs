// parse host strings, extracting username, hostname, port, geometry
// perl cssh accepted, but dropped geometry.  We do the same.

use regex::Regex;
use std::ops::Range;

lazy_static! {
    static ref HOST_IPV6: Regex = Regex::new(r"(?x)
		\A
		(?:(.*?)@)?                # username@ (optional)
		\[([\w:]*)\]               # [<sequence of chars>]
		(?::(\d+))?                # :port     (optional)
		(?:=(\d+\D\d+\D\d+\D\d+))? # =geometry (optional)
		\z
	").expect("Regex error HOST_IPV6");
    // The embeded =geometry within HOST_IPV6 & HOST_IPV4
    // is perl cssh's geometry regex, except we add
    // the missing last +
    //
    // The geometry regex disallows many valid x11 geometries
    // https://en.wikibooks.org/wiki/Guide_to_X11/Starting_Programs
    // Maybe we should allow
    // =(?:\d+)?(?:x\d+)?(?:[+-]\d+[+-]\d+)?
    // and ignore it if it's just =
    // FWIW: geometry is accepted but not used (both by perl cssh
    // and by rust tcssh).  Window placement is the domain of the
    // window managers.  All our placement settings are suggestions.
    // Trying to get all WMs to respect geometry placements is
    // a fools errand.

    static ref HOST_IPV4: Regex = Regex::new(r"(?x)
		\A 
		(?:(.*?)@)?               # username@ (optional)
		([\w\.-]*)                # hostname[.domain[.domain] | 123.123.123.123]
		(?::(\d+))?               # :port     (optional)
		(?:=(\d+\D\d+\D\d+\D\d+))? # =geometry (optional)
		\z
	").expect("Regex error HOST_IPV4");

    static ref USER:       Regex = Regex::new(r"\A(?:(.*?)@)").expect("Regex error USER");
    static ref SLASH_PORT: Regex = Regex::new(r"(?:/(\d+)$)" ).expect("Regex error SLASH_PORT");
    static ref COLON_PORT: Regex = Regex::new(r"(?::(\d+?))$").expect("Regex error COLON_PORT");
    static ref EQ_ANY_GEOMETRY: Regex = Regex::new(r"(?:=(.*?)$)" ).expect("Regex error EQ_ANY_GEOMETRY");
    pub
    static ref STRICT_GEOMETRY: Regex = Regex::new(r"^(?:\d+)?(?:x\d+)?(?:[+-]\d+[+-]\d+)?$" ).expect("Regex error STRICT_GEOMETRY");
}

// parse_str is the input, and everything else is a slice of parse_str.
#[derive(Debug, PartialEq)]
pub struct Host<'a> {
    pub parse_string: &'a str,
    pub username: Option<&'a str>,
    pub hostname: &'a str,
    pub port: Option<&'a str>,
    pub geometry: Option<&'a str>,
}

pub fn parse(host: &str) -> Option<Host<'_>> {
    // the parsing logic is right out of perl cssh.
    if let Some(cap) = HOST_IPV6.captures(host) {
        if let Some(c_hostname) = cap.get(2) {
            let s_hostname = c_hostname.as_str();
            if !s_hostname.is_empty() {
                return Some(Host {
                    parse_string: host,
                    username: cap.get(1).and_then(|m| Some(m.as_str())),
                    hostname: s_hostname,
                    port: cap.get(3).and_then(|m| Some(m.as_str())),
                    geometry: cap.get(4).and_then(|m| Some(m.as_str())),
                });
            }
        }
        return None;
    }

    if let Some(cap) = HOST_IPV4.captures(host) {
        if let Some(c_hostname) = cap.get(2) {
            let s_hostname = c_hostname.as_str();
            if !s_hostname.is_empty() {
                return Some(Host {
                    parse_string: host,
                    username: cap.get(1).and_then(|m| Some(m.as_str())),
                    hostname: s_hostname,
                    port: cap.get(3).and_then(|m| Some(m.as_str())),
                    geometry: cap.get(4).and_then(|m| Some(m.as_str())),
                });
            }
        }
        return None;
    }

    // Neither regex above worked.. so assume host[start..end] is a host name.
    // But adjust start..end via trying other regexes USER,EQ_ANY_GEOMETRY etc.
    let mut r_user: Option<Range<usize>> = None;
    let mut start: usize = 0;
    if let Some(cap) = USER.captures(host) {
        if let Some(m) = cap.get(1) {
            r_user = Some(Range {
                start: m.start(),
                end: m.end(),
            });
        }
        start = cap[0].len();
    }

    let mut r_geo: Option<Range<usize>> = None;
    let mut end: usize = host.len();
    if start >= end {
        return None; // unreachable, handled by HOST_IPV4
    }
    if let Some(cap) = EQ_ANY_GEOMETRY.captures(&host[start..]) {
        if let Some(m) = cap.get(1) {
            r_geo = Some(Range {
                start: start + m.start(),
                end: start + m.end(),
            });
        }
        end -= cap[0].len();
        if start >= end {
            return None; // unreachable, handled by HOST_IPV4
        }
    }

    let mut r_port: Option<Range<usize>> = None;
    if let Some(cap) = SLASH_PORT.captures(&host[start..end]) {
        if let Some(m) = cap.get(1) {
            r_port = Some(Range {
                start: start + m.start(),
                end: start + m.end(),
            });
        }
        end -= cap[0].len();
        if start >= end {
            return None; // reachable.
        }
    }

    let colon_count = host[start..end].chars().filter(|c| *c == ':').count();

    // if there are 7 colons assume its a full IPv6 address
    // if its 8 then assumed full IPv6 address with a port
    // also catch localhost address here
    if colon_count == 7 || colon_count == 8 || &host[start..end] == "::1" {
        if colon_count == 8 {
            if let Some(cap) = COLON_PORT.captures(&host[start..end]) {
                if let Some(m) = cap.get(1) {
                    r_port = Some(Range {
                        start: start + m.start(),
                        end: start + m.end(),
                    });
                }
                end -= cap[0].len();
            }
        }
    } else if colon_count > 1 && colon_count < 8 {
        // perl cssh would warn here..
        //		eprintln!("Ambiguous host string: "', $host_string, '"',   $/;
        //		eprintln!("Assuming you meant "[',    $host_string, ']"?', $/;
        // We'll silently let it through, after all I imagine abc::def is common enough.
    } else {
        return None;
    }
    if start >= end {
        return None; // unreachable.
    }

    Some(Host {
        parse_string: host,
        username: match r_user {
            Some(r) => Some(&host[r]),
            _ => None,
        },
        hostname: &host[start..end],
        port: match r_port {
            Some(r) => Some(&host[r]),
            _ => None,
        },
        geometry: match r_geo {
            Some(r) => Some(&host[r]),
            _ => None,
        },
    })
}

#[test]
fn test_parse() {
    {
        let hs = ["localhost", "127.0.0.1", "fe80::c3cf:9c90:59b5:3d0b", "::1"];

        for h in hs.iter() {
            let host = parse(h).expect(&format!("Expected to parse {}", h));
            assert_eq!(
                host,
                Host {
                    parse_string: h,
                    username: None,
                    hostname: h,
                    port: None,
                    geometry: None,
                }
            );
        }
    }

    {
        let h = "[fe80::c3cf:9c90:59b5:3d0b]";
        let host = parse(h).expect(&format!("Expected to parse {}", h));
        assert_eq!(
            host,
            Host {
                parse_string: h,
                username: None,
                hostname: "fe80::c3cf:9c90:59b5:3d0b",
                port: None,
                geometry: None,
            }
        );
    }

    {
        let h = "luser@[fe80::c3cf:9c90:59b5:3d0b]:1234=640x480+10+11";
        let host = parse(h).expect(&format!("Expected to parse {}", h));
        assert_eq!(
            host,
            Host {
                parse_string: h,
                username: Some("luser"),
                hostname: "fe80::c3cf:9c90:59b5:3d0b",
                port: Some("1234"),
                geometry: Some("640x480+10+11"),
            }
        );
    }

    {
        let h = "muser@123.234.12.34:4321=1024x768+20+21";
        let host = parse(h).expect(&format!("Expected to parse {}", h));
        assert_eq!(
            host,
            Host {
                parse_string: h,
                username: Some("muser"),
                hostname: "123.234.12.34",
                port: Some("4321"),
                geometry: Some("1024x768+20+21"),
            }
        );
    }

    {
        let h = "tuser@box-001.internal.xn--foo.computing:4321=320x240+34+45";
        let host = parse(h).expect(&format!("Expected to parse {}", h));
        assert_eq!(
            host,
            Host {
                parse_string: h,
                username: Some("tuser"),
                hostname: "box-001.internal.xn--foo.computing",
                port: Some("4321"),
                geometry: Some("320x240+34+45"),
            }
        );
    }

    {
        let h = "muser@abc::def/321=1920x1080+56+67";
        let host = parse(h).expect(&format!("Expected to parse {}", h));
        assert_eq!(
            host,
            Host {
                parse_string: h,
                username: Some("muser"),
                hostname: "abc::def",
                port: Some("321"),
                geometry: Some("1920x1080+56+67"),
            }
        );
    }

    {
        let h = "nuser@1:2:3:4:5:6:7:8:321=1920x1080+56+67";
        let host = parse(h).expect(&format!("Expected to parse {}", h));
        assert_eq!(
            host,
            Host {
                parse_string: h,
                username: Some("nuser"),
                hostname: "1:2:3:4:5:6:7:8",
                port: Some("321"),
                geometry: Some("1920x1080+56+67"),
            }
        );
    }

    {
        // specify conflicting ports in two different ways to check that if both port
        // parsing heuristics are hit, then the parsing doesn't go nuts.
        let h = "nuser@1:2:3:4:5:6:7:8:321/123=1920x1080+56+67";
        let host = parse(h).expect(&format!("Expected to parse {}", h));
        assert_eq!(
            host,
            Host {
                parse_string: h,
                username: Some("nuser"),
                hostname: "1:2:3:4:5:6:7:8",
                port: Some("321"),
                geometry: Some("1920x1080+56+67"),
            }
        );
    }

    {
        let no_hostname = [
            "",                            // handled by HOST_IPV4
            "muser@=1024x768+20+21",       // handled by HOST_IPV4
            "muser@:123=1024x768+20+22",   // handled by HOST_IPV4
            "muser@/123=1024x768+20+22",   // handled by start >= end in SLASH_PORT
            "muser@[]:123=1024x768+20+22", // handled by HOST_IPV6
        ];
        for h in no_hostname.iter() {
            let host = parse(h);
            assert_eq!(host, None);
        }
    }

    {
        // perl cssh passed along the unparsed host_str direct from its cmd line args
        // over into the cmd line constructed and passed to sh which launches the xterm
        // Since the regexes are permissive (e.g. .* in HOST_IPV4)
        // I can see users submitting ' or other nonsense.
        // And though I'm not clever enough to figure out why/how they'd get it to work,
        // Let them, as maybe someone has a usecase for this nonsense.  https://xkcd.com/1172/
        let trouble_but_let_user_doit = [
            "`foo ::`", // let sub shell determine hostname...?
            "::1'",     // single quote would cause trouble in our cmdline to xterm
            "::1\\",    // as would trailling \
        ];
        for h in trouble_but_let_user_doit.iter() {
            let host = parse(h).expect(&format!("Expected to parse {}", h));
            assert_eq!(
                host,
                Host {
                    parse_string: h,
                    username: None,
                    hostname: h,
                    port: None,
                    geometry: None,
                }
            );
        }
    }
}
