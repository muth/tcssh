// read config file, cluster file, or tag file.
// All follow the same pattern,
//     ignore # comments
//     ignore blank lines
//     either entire file has lines like "key=value" or is "key value"
//     and if a line ends with \ then continue on the next line

use regex::Regex;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::er::Result;

lazy_static! {
    static ref SPLIT_FIRST_WHITESPACE: Regex =
        Regex::new(r"^\s*(\S+)\s+(.*)$").expect("Regex error SPLIT_FIRST_WHITESPACE");
}

pub fn read_file<F>(p: &Path, is_key_eq_value: bool, f: F) -> Result<()>
where
    F: FnMut(&str, &str),
{
    let file = OpenOptions::new().read(true).create_new(false).open(p)?;

    let mut reader = BufReader::new(file);

    read_buf(&mut reader, is_key_eq_value, f)
}

fn read_buf<R, F>(mut buf_reader: R, is_key_eq_value: bool, mut f: F) -> Result<()>
where
    R: BufRead,
    F: FnMut(&str, &str),
{
    let mut line_string = String::with_capacity(256); // read_line grows line as needed
    let mut continuation = true; // line (kind of) ended with \
    let mut continuation_start = 0;
    let mut nll_flag = None; // flag which isn't needed if we had NLL edition 2018
    let mut nll_string = None; // flag which isn't needed if we had NLL edition 2018
    loop {
        if !continuation {
            line_string.clear();
        } else if let Some(n) = nll_flag {
            line_string.truncate(n);
            nll_flag = None;
        } else if let Some(s) = nll_string {
            line_string = s;
            nll_string = None;
        }
        // EOF
        if buf_reader.read_line(&mut line_string)? == 0 && !continuation {
            break;
        }
        let mut line = line_string.as_str();

        if continuation {
            continuation = false;
            if continuation_start > 0 {
                line = &line_string[continuation_start..];
            }
        }
        line = line.trim_start();

        // skip empty lines and lines which start with comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // remove any trailing # comments
        let mut s = line.splitn(2, '#');
        let line = match s.next() {
            None => line, // Shouldn't be possible
            Some(line) => line,
        };
        // trim trailing white space
        let mut line = line.trim_end();

        if line.ends_with('\\') {
            line = line.trim_end_matches('\\');
            continuation_start = 0;
            continuation = true;
            // line is what we're interested in.
            // line is a str slice of the String line.
            match line_string.find(line) {
                Some(i) => {
                    continuation_start = i;
                    let n = i + line.len();
                    if n < line_string.len() {
                        nll_flag = Some(n);
                    }
                }
                // NLL code
                //None => line_string = line.to_string(), // impossible, but fall back to another heap allocation
                // non-NLL code
                None => {
                    nll_string = Some(line.to_string()); // impossible, but fall back to another heap allocation
                }
            }
            continue;
        }

        if is_key_eq_value {
            // key=value
            // \s*(\S+)\s*=\s*(.*)
            let mut s = line.rsplitn(2, '=');
            let value = s.next();
            let key = s.next();
            if key.is_some() && value.is_some() {
                let key = key.unwrap().trim_end();
                let value = value.unwrap().trim_start();
                if !key.is_empty() {
                    f(key, value);
                }
            }
        } else {
            //key value
            // \s*(\S+)\s+(.*)
            if let Some(cap) = SPLIT_FIRST_WHITESPACE.captures(line) {
                if cap.len() == 3 {
                    f(&cap[1], &cap[2]);
                }
            }
        }
    }
    Ok(())
}

#[test]
fn test_reader_key_eq_value() {
    let data = r"foo=bar
		#ignore=comment
		has = spaces # ...
		no=spaces# ...
		empty1= # ...
		empty2=
		empty3=\

		continuation1=fragile\
 depends \ # on code indentation
 on code indentation
		continuation2=but \ # but this time
no leading space this time \ # lorem
more data # ipsum
		ignored because no equals sign
		mulitple=equals=hmmm
		foo=bar2
		=missing key
		final=done
	"
    .as_bytes();

    let expected = [
        ["foo", "bar"],
        ["has", "spaces"],
        ["no", "spaces"],
        ["empty1", ""],
        ["empty2", ""],
        ["empty3", ""],
        ["continuation1", "fragile depends  on code indentation"],
        ["continuation2", "but no leading space this time more data"],
        ["mulitple=equals", "hmmm"],
        ["foo", "bar2"],
        ["final", "done"],
    ];
    let mut i = 0;

    let br = BufReader::new(data);
    let ret = read_buf(br, true, |x, y| {
        println!("{}=>{}", x, y);
        assert_eq!(x, expected[i][0]);
        assert_eq!(y, expected[i][1]);
        i += 1;
    });
    assert_eq!(ret, Ok(()));
}

#[test]
fn test_reader_key_sp_value() {
    let data = r"foo bar
		#ignore comment
		has   spaces # ...
		no_spaces# ...
		empty1 # ...
		empty2
		empty3\

		continuation1 fragile\
 depends \ # on code indentation
 on code indentation
		continuation2 but \ # but this time
no leading space this time \ # lorem
more data # ipsum
		not ignored because no equals sign
		foo bar2
		final done
	"
    .as_bytes();

    let expected = [
        ["foo", "bar"],
        ["has", "spaces"],
        ["continuation1", "fragile depends  on code indentation"],
        ["continuation2", "but no leading space this time more data"],
        ["not", "ignored because no equals sign"],
        ["foo", "bar2"],
        ["final", "done"],
    ];
    let mut i = 0;

    let br = BufReader::new(data);
    let ret = read_buf(br, false, |x, y| {
        println!("{}={}", x, y);
        assert_eq!(x, expected[i][0]);
        assert_eq!(y, expected[i][1]);
        i += 1;
    });
    assert_eq!(ret, Ok(()));
}
