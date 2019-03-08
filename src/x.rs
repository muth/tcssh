// Contains the interaction with X11 via x11::xlib
use std::env;
use std::ffi::CString;
use std::os::raw::{c_int, c_uint, c_ulong};
use x11::xlib;

use crate::app::Wid;
use crate::candstr::CandStr;
use crate::er::Result;
use crate::retile;

#[derive(Debug, Default)]
pub struct XDisplay {
    pub display: Option<*mut xlib::Display>,
    root: Wid,
    pub width_in_pixels: u32,
    pub height_in_pixels: u32,
    wm_normal_hints: xlib::Atom,
    wm_size_hints: xlib::Atom,
}

impl XDisplay {
    pub fn new() -> Result<XDisplay> {
        let display_c = match env::var("DISPLAY") {
            Ok(e) => {
                match CString::new(e) {
                    Ok(display_c) => display_c,
                    Err(_) => {
                        // Internal null byte?
                        eprintln!("Error decoding env var DISPLAY, falling back to 'unix:0'");
                        CString::new("unix:0").unwrap()
                    }
                }
            }
            Err(_) => {
                // perl cssh injects unix:0 if DISPLAY env isn't set
                eprintln!("Can't find DISPLAY -- guessing 'unix:0'");
                CString::new("unix:0").unwrap()
            }
        };

        let display_cptr = display_c.as_ptr();
        let display_p = unsafe { xlib::XOpenDisplay(display_cptr) };
        if display_p.is_null() {
            return Err("Failed to get X connection".into());
        }

        let screen = unsafe { xlib::XDefaultScreenOfDisplay(display_p) };
        if screen.is_null() {
            return Err("Failed to get screen".into());
        }
        let r = unsafe { (*screen).root };
        let w: i32 = unsafe { (*screen).width };
        let h: i32 = unsafe { (*screen).height };
        if w <= 0 || h <= 0 {
            return Err("Screen bounds out of range".into());
        }
        Ok(XDisplay {
            display: Some(display_p),
            root: r as Wid,
            width_in_pixels: w as u32,
            height_in_pixels: h as u32,
            wm_normal_hints: get_atom(display_p, &CandStr::new(b"WM_NORMAL_HINTS\0"), false)?,
            wm_size_hints: get_atom(display_p, &CandStr::new(b"WM_SIZE_HINTS\0"), false)?,
        })
    }

    pub fn send_event(&self, wid: Wid, state: c_uint, keycode: c_uint) -> Result<()> {
        if let Some(display) = self.display {
            let wid = wid as c_ulong;
            let propogate = 0 as c_int;
            // send key press, then key release events.
            for (event_type, event_mask) in [
                (xlib::KeyPress, xlib::KeyPressMask),
                (xlib::KeyRelease, xlib::KeyReleaseMask),
            ]
            .iter()
            {
                // XEvent is a union, so it's initialized by setting exactly one member.
                let mut event = xlib::XEvent {
                    key: xlib::XKeyEvent {
                        type_: *event_type,
                        serial: 0,
                        send_event: 0 as xlib::Bool,
                        display,
                        window: wid,
                        root: self.root,
                        subwindow: wid,
                        time: 0,
                        x: 0,
                        y: 0,
                        x_root: 0,
                        y_root: 0,
                        state,
                        keycode,
                        same_screen: 1 as xlib::Bool,
                    },
                };
                if 0 == unsafe {
                    xlib::XSendEvent(display, wid, propogate, *event_mask, &mut event)
                } {
                    return Err("XSendEvent failed. Possible data conversion error".into());
                }
            }
        }
        Ok(())
    }

    pub fn flush(&self) {
        if let Some(display) = self.display {
            let _ = unsafe { xlib::XFlush(display) };
            // man XFlush is silent on return values.
            // I think errors are async, so ignore the values returned from
            // XFlush/XUnmapWindow/XMapWindow/XChangeProperty/XConfigureWindow
            // just like perl's X11Protocol.pm which doesn't parse the responses
        }
    }

    pub fn unmap_window(&self, wid: Wid) {
        if let Some(display) = self.display {
            let _ = unsafe { xlib::XUnmapWindow(display, wid) };
        }
    }

    pub fn map_window(&self, wid: Wid) {
        if let Some(display) = self.display {
            let _ = unsafe { xlib::XMapWindow(display, wid) };
        }
    }

    pub fn raise_window(&self, wid: Wid) {
        if let Some(display) = self.display {
            let _ = unsafe { xlib::XRaiseWindow(display, wid) };
        }
    }

    pub fn close_display(&mut self) {
        if let Some(ptr) = self.display {
            unsafe {
                xlib::XCloseDisplay(ptr);
            }
            self.display = None;
        }
    }

    pub fn change_property(&self, wid: Wid, x: u32, y: u32, w: u32, h: u32) -> Result<()> {
        if let Some(display) = self.display {
            // A helper struct containing the size info we send to X11.
            // Re: perl cssh packs with 'L' so native endian-ness.
            #[derive(Debug, Clone, Copy)]
            #[repr(C)]
            struct MessageStruct {
                three: u32,
                xywh: [u32; 4],
                zeros: [u32; 12],
            };

            // a union so we can cast the above struct into *const u8
            #[repr(C)]
            union MessageUnion {
                as_char: [u8; 68],
                as_struct: MessageStruct,
            };

            let message_union = MessageUnion {
                as_struct: MessageStruct {
                    three: 3, // perl cssh has a '1 | 2' hard coded. No idea what it stands for.
                    xywh: [x, y, w, h],
                    zeros: [0; 12],
                },
            };

            let _ = unsafe {
                xlib::XChangeProperty(
                    display,
                    wid,
                    self.wm_normal_hints,
                    self.wm_size_hints,
                    32,
                    xlib::PropModeReplace,
                    &message_union.as_char as *const u8,
                    17, // 17 == sizeof(MessageUnion) / 4
                )
            };
        }
        Ok(())
    }

    pub fn configure_window(&self, wid: Wid, x: u32, y: u32, w: u32, h: u32) -> Result<()> {
        if let Some(display) = self.display {
            let mut win_changes = xlib::XWindowChanges {
                x: x as c_int,
                y: y as c_int,
                width: w as c_int,
                height: h as c_int,
                border_width: 0,
                sibling: 0,
                stack_mode: 0,
            };
            let mask = xlib::CWX | xlib::CWY | xlib::CWWidth | xlib::CWHeight;
            let _ = unsafe { xlib::XConfigureWindow(display, wid, mask.into(), &mut win_changes) };
        }
        Ok(())
    }

    pub fn get_font_size(&self, terminal_font: &str) -> Result<(u32, u32)> {
        match self.display {
            None => Err("No XDisplay".into()),
            Some(display_p) => {
                let quad_width = get_atom(display_p, &CandStr::new(b"QUAD_WIDTH\0"), false)?;
                let pixel_size = get_atom(display_p, &CandStr::new(b"PIXEL_SIZE\0"), false)?;

                let cfont = match CString::new(terminal_font) {
                    Ok(cfont) => cfont,
                    Err(_) => return Err("terminal_font contains an interior null byte".into()),
                };

                let font = unsafe { xlib::XLoadQueryFont(display_p, cfont.as_ptr()) };
                if font.is_null() {
                    return Err(format!(
                        "Fatal: Unrecognised font used ({}).\n\
                         Please amend $HOME/.tcssh/config with a valid font.\n\
                         XLoadQueryFont returned null",
                        terminal_font
                    )
                    .into());
                }
                let result = unsafe {
                    // each *font and *p deref is unsafe.. so one block.
                    match (*font).n_properties {
                        n if n <= 0 => Err("XLoadQueryFont returned no properties".into()),
                        n => {
                            let n: isize = n as isize;
                            let prop = (*font).properties;
                            if prop.is_null() {
                                Err("XLoadQueryFont returned null properties".into())
                            } else {
                                let mut width = 0u32;
                                let mut height = 0u32;
                                for i in 0..n {
                                    let p = prop.offset(i);
                                    let name = (*p).name;
                                    if name == quad_width {
                                        // The member is called card32, and the X11 wire
                                        // protocol seems to be 32 bits (at least that's
                                        // what perl unpacks the binary as)
                                        // but for some odd reason the rust interface has
                                        // a c_ulong which is 64 bits.
                                        width = (*p).card32 as u32;
                                    } else if name == pixel_size {
                                        height = (*p).card32 as u32;
                                    } else {
                                        continue;
                                    }
                                    if width > 0 && height > 0 {
                                        break;
                                    }
                                }
                                if width > 0 && height > 0 {
                                    Ok((width, height))
                                } else {
                                    Err(format!("Fatal: Unrecognised font used ({}).\n\
										Please amend $HOME/.tcssh/config with a valid font (see man page).\n\
										XLoadQueryFont did not return font width/height", terminal_font)
                                    .into())
                                }
                            }
                        }
                    }
                };
                // perl cssh didn't unload the font.. but since we don't need it (just sub process xterms)
                let _ = unsafe { xlib::XFreeFont(display_p, font) }; // ignore Free's result
                result
            }
        }
    }
}

impl Drop for XDisplay {
    fn drop(&mut self) {
        self.close_display();
    }
}

fn get_atom(
    display_p: *mut xlib::Display,
    name: &CandStr,
    only_if_exists: bool,
) -> Result<xlib::Atom> {
    let flag: c_int = if only_if_exists { 1 } else { 0 };
    match unsafe { xlib::XInternAtom(display_p, name.as_cstr.as_ptr(), flag) } {
        atom if atom == u64::from(xlib::BadAlloc) => {
            Err(format!("XInternAtom returned BadAlloc for {}", name.as_str).into())
        }
        atom if atom == u64::from(xlib::BadAtom) => {
            Err(format!("XInternAtom returned BadAtom  for {}", name.as_str).into())
        }
        atom if atom == u64::from(xlib::BadValue) => {
            Err(format!("XInternAtom returned BadValue for {}", name.as_str).into())
        }
        atom => Ok(atom),
    }
}

// This interface exists so retile.rs can talk to and mock our XDisplay
impl retile::RetileXDisplay for XDisplay {
    fn get_wh(&self) -> (u32, u32) {
        (self.width_in_pixels, self.height_in_pixels)
    }
    fn flush(&self) {
        self.flush();
    }
    fn map_window(&self, wid: Wid) {
        self.map_window(wid);
    }
    fn raise_window(&self, wid: Wid) {
        self.raise_window(wid);
    }
    fn unmap_window(&self, wid: Wid) {
        self.unmap_window(wid);
    }
}
