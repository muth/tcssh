// If the user hits a key on the keyboard we get a gtk event
// which is a 1:1 translation to an x11 event which we send
// to all our child xterms.
//
// But if the user pastes text into the common window,
// then we do not have a keyboard event, we have utf8 text.
// So this module takes pasted text and generates x11 events for
// every keypress required to get generate the pasted text.

use gdk;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::os::raw::{c_int, c_uchar, c_void};
use x11::xlib::{Mod5Mask, NoSymbol, ShiftMask, XDisplayKeycodes, XFree, XGetKeyboardMapping};

use crate::er::Result;
use crate::x;

// A Keysym represents an abstract concept like 'A' or 'EuroSymbol'
type Keysym = u32;

// keycode is the value we send to x11 indicating a keypress (and may have a modifier)
// so a KeySym for 'A' may expand to a keycode 'a' with a 'ShiftMask' modifier.
type Keycode = u32;

static MODIFIER_TO_STATE: [u32; 4] = [
    0,         // Normal, no modifier
    ShiftMask, // == 1  shift
    Mod5Mask,  // == 128 alt
    ShiftMask | Mod5Mask, // == 129 shift+alt
               // Test case ensures relative ordering is preserved.
               // search relative-ordering in this file to find where it's used.
];

// Hold mappings of text 'A' to keyboard StateCode pairs { state=ShiftMask, code=the_key_code_for('a') }
#[derive(Copy, Clone, Debug)]
pub struct StateCode {
    pub state: u32,
    pub code: Keycode,
}

// When we query X11 for the keyboard mappings,
// it sends back an array of keysym to key code
// but not indexed from zero, it's indexed from min (up to max) keysym.
// This is a representation of that array.
#[derive(Debug)]
pub struct Text2X11 {
    min_keycode: u32,
    max_keycode: u32,
    keysym2code: HashMap<Keysym, StateCode>,
}

impl Text2X11 {
    pub fn new(xdisplay: &mut x::XDisplay) -> Result<Self> {
        let display = match xdisplay.display {
            None => return Err("No display".into()),
            Some(display) => display,
        };
        let mut min_keycode: c_int = 0;
        let mut max_keycode: c_int = 0;
        unsafe {
            XDisplayKeycodes(display, &mut min_keycode, &mut max_keycode);
        }
        // "man XDisplayKeycodes" is silent on return code for above.
        // so just do a sanity check, instead of checking return code.
        if min_keycode < 0
            || max_keycode < 1
            || min_keycode >= max_keycode
            || max_keycode == c_int::max_value()
        {
            // The above is sufficient to rule out a bunch of checks later on,
            // but perform those checks regardless,
            // because logic errors can creep into code over time..
            return Err("out of range min/max keycodes".into());
        }
        let n_keys: i32 = match (max_keycode - min_keycode).checked_add(1) {
            Some(tmp) if tmp > 0 && tmp <= 255 => tmp,
            _ => return Err("Overflow in keycode range".into()),
        };
        let min_keycode: u32 = min_keycode as u32; // cast to size is safe, we checked <0 <1 above
        let max_keycode = max_keycode as u32;

        if min_keycode > u32::from(c_uchar::max_value()) {
            // clippy genuinely caught this error.. :)
            // for a while I had        min_keycode > c_uchar::max_value   as u32
            // instead of the correct   min_keycode > c_uchar::max_value() as u32
            // casting a function pointer v.s. casting it's returned value
            return Err("min_key_code returned by XDisplayKeycodes() exceeds input size for XGetKeyboardMapping()".into());
        }
        let checked_min_key_code = min_keycode as c_uchar;

        let mut keysyms_per_keycode: c_int = 0;

        // "man XDisplayKeycodes" indicates the map_raw array (below) is small-ish
        // (max 255 entries * keysyms_per_keycode_return which itself is 8 or fewer)
        // and the documentation indicates that map_raw should be freed by XFree()
        // The odd thing is map_raw is an array of key symbols and is an array of c_ulong aka >u64<
        // But other functions which return KeySymbols return >u32< gdk::unicode_to_keyval, also NoSymbol is >u32<
        let map_raw = unsafe {
            XGetKeyboardMapping(
                display,
                checked_min_key_code,
                n_keys,
                &mut keysyms_per_keycode,
            )
        };
        if map_raw.is_null() {
            return Err("XGetKeyboardMapping failed".into());
        } else if keysyms_per_keycode < 1 || keysyms_per_keycode > 8 {
            return Err("keysyms_per_keycode out of range".into());
        }
        // This cannot overflow..
        // (keysyms_per_keycode <= 8 and n_keys <= 255)
        // but code rots over time so maybe checks above will disappear.
        // If we don't explicity do the overflow check, then it'll be
        // done implicitly, and panic instead returning an error.
        let index_max = match n_keys
            .checked_mul(keysyms_per_keycode)
            .and_then(|tmp| tmp.checked_add(4)) // 4 is the number of modifiers.
        {
            Some(tmp) => tmp as usize,
            _ => return Err("n_keys and keysyms_per_keycode out of range".into()),
        };
        let hash_size = match n_keys.checked_mul(4) {
            // 4 is the number of modifiers
            Some(tmp) => tmp as usize,
            _ => return Err("n_keys out of range".into()),
        };

        let keysyms_per_keycode = keysyms_per_keycode as usize;
        let mut keysym2code: HashMap<Keysym, StateCode> = HashMap::with_capacity(hash_size);

        for i in 0..n_keys {
            for (modifier, ref_new_state) in MODIFIER_TO_STATE.iter().enumerate().take(3) {
                let i = i as usize;
                let checked_index = match i
                    .checked_mul(keysyms_per_keycode)
                    .and_then(|tmp| tmp.checked_add(modifier))
                {
                    Some(tmp) if tmp < index_max => tmp,
                    _ => return Err("Impossible overflow attempted on map_raw deref".into()),
                };
                // unsafe { *(map_raw.add(i * keysyms_per_keycode + modifier)) } as Keysym;
                let symbol = unsafe { *(map_raw.add(checked_index)) } as Keysym;
                if symbol == NoSymbol as Keysym || symbol == 0 {
                    continue;
                }
                let new_state = *ref_new_state;
                let keycode = (i + min_keycode as usize) as Keycode;
                match keysym2code.entry(symbol) {
                    Entry::Occupied(mut entry) => {
                        let sc = entry.get_mut();
                        if new_state < sc.state {
                            // relative-ordering used here so that we prefer
                            // a mapping of no modifer over shift modifier,
                            // over alt modifier, over shift-alt modifier.
                            sc.state = new_state;
                            sc.code = keycode;
                        }
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(StateCode {
                            state: new_state,
                            code: keycode,
                        });
                    }
                }
            }
        }
        // There are returns before this XFree, so we could leak.
        // But each of those returns will end the program, so moot.
        unsafe { XFree(map_raw as *mut c_void) };

        Ok(Self {
            min_keycode,
            max_keycode,
            keysym2code,
        })
    }

    pub fn translate(&self, wc: u32) -> Option<StateCode> {
        if wc < self.min_keycode || wc > self.max_keycode {
            return None;
        }
        // convert 'Return' 10 to sym 65293 (just like perl cssh)
        // for the rest, let gdk figure out the mapping.
        let sym = if wc != 10 {
            gdk::unicode_to_keyval(wc) as Keysym
        } else {
            '\u{FF0D}' as Keysym
        };
        if sym == NoSymbol as Keysym {
            return None;
        }
        match self.keysym2code.get(&sym) {
            Some(sc) => Some(*sc),
            _ => None,
        }
    }
}

#[test]
fn test_constant_order() {
    assert!(0 < ShiftMask);
    assert!(ShiftMask < Mod5Mask);
    assert!(Mod5Mask < Mod5Mask | ShiftMask);
    // check relative-ordering is what we expect.
}
