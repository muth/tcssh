// When calling C libraries it's useful to specify a static null terminated
// slice of bytes (CStr), aka b"foo\0"
// But if we want to print them, or use them in rust, then we generally want
// them minus the last byte and get a str.
// That's what this is. "CStr and str".
//
// I find it more ergonomic to just write .as_str() on a CandStr (which cannot fail)
// instead of .to_str() on a CStr which we then have to unwrap().

use std::ffi::CStr;
use std::str;

pub struct CandStr {
    pub as_cstr: &'static CStr,
    pub as_str: &'static str,
}

impl CandStr {
    pub fn new(bytes: &'static [u8]) -> CandStr {
        let len = bytes.len();
        if len <= 1 {
            panic!(
                "Invalid static init {:?} expected longer length got {}",
                bytes, len
            );
        }
        let as_cstr = CStr::from_bytes_with_nul(bytes).unwrap();
        let as_str = str::from_utf8(&bytes[0..len - 1])
            .unwrap_or_else(|_| panic!("static var failed utf8 check {:?}", bytes));
        CandStr { as_cstr, as_str }
    }
}

#[test]
fn test_candstr() {
    let foo = CandStr::new(b"foo\0");

    assert_eq!(foo.as_cstr, CStr::from_bytes_with_nul(b"foo\0").unwrap());
    assert_eq!(foo.as_str, "foo");
}

#[test]
#[should_panic]
fn test_candstr_internal_nul() {
    CandStr::new(b"foo\0bar\0");
}

#[test]
#[should_panic]
fn test_candstr_empty() {
    CandStr::new(b"");
}

#[test]
#[should_panic]
fn test_candstr_no_null() {
    CandStr::new(b"foo");
}
