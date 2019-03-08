// Figures out size and location for xterms

use std::collections::BTreeMap;

use crate::app::Wid;
use crate::config;
use crate::er::Result;
use crate::server;

// Traits for mocking.
pub trait RetileXDisplay {
    fn get_wh(&self) -> (u32, u32);
    fn flush(&self);
    fn map_window(&self, wid: Wid);
    fn raise_window(&self, wid: Wid);
    fn unmap_window(&self, wid: Wid);
}

pub trait RetileApp<X: RetileXDisplay> {
    fn get_config(&self) -> &config::Config;
    fn get_servers(&self) -> &BTreeMap<String, server::Server>;
    fn get_font_wh(&self) -> (u32, u32);

    fn show_console(&mut self) -> Result<()>;
    fn send_resizemove(&self, wid: Wid, x: u32, y: u32, w: u32, h: u32) -> Result<()>;
    fn sleep(&self, ms: u64);
    fn get_xdisplay(&self) -> &X;
}

pub fn retile_hosts<X: RetileXDisplay, T: RetileApp<X>>(
    app: &mut T,
    raise: bool,
) -> Result<(bool)> {
    // rust release mode will panic on overflow.
    // Even though that's unlikely, we really should check, instead of crash.
    // So you'll see a short comment showing the math desired,
    // followed by all the checks.
    // FWIW: Code is auto-formatted via 'rust fmt'.

    let n_servers = app.get_servers().len() as u32;
    if n_servers == 0 {
        app.show_console()?;
        return Ok(true);
    }

    let c = app.get_config();
    let (font_w, font_h) = app.get_font_wh();

    // work out terminal pixel size from terminal size & font size
    // does not include any title bars or scroll bars - purely text area

    //let w = (c.terminal.terminal_size_x * app.font_w) + c.terminal.decoration_width;
    let w = match c
        .terminal
        .terminal_size_x
        .checked_mul(font_w)
        .and_then(|tmp| tmp.checked_add(c.terminal.decoration_width))
    {
        Some(tmp) if tmp > 0 => tmp,
        _ => return Err("retile overflow".into()),
    };

    //let h = (c.terminal.terminal_size_y * app.font_h) + c.terminal.decoration_height;
    let h = match c
        .terminal
        .terminal_size_y
        .checked_mul(font_h)
        .and_then(|tmp| tmp.checked_add(c.terminal.decoration_height))
    {
        Some(tmp) if tmp > 0 => tmp,
        _ => return Err("retile overflow".into()),
    };

    let xdisplay = app.get_xdisplay();
    let (screen_w, screen_h) = xdisplay.get_wh();

    // Now, work out how many columns of terminals we can fit on screen
    //let columns = (screen_w - c.screen.reserve_left - c.screen.reserve_right)
    //    / (w + c.terminal.reserve_left + c.terminal.reserve_right);
    // First compute denominator (it's re-used later).
    // let w_reserve = w + c.terminal.reserve_left + c.terminal.reserve_right;
    let w_reserve = match w
        .checked_add(c.terminal.reserve_left)
        .and_then(|tmp| tmp.checked_add(c.terminal.reserve_right))
    {
        Some(tmp) if tmp > 0 => tmp,
        _ => return Err("retile overflow".into()),
    };
    let columns = match screen_w
        .checked_sub(c.screen.reserve_left)
        .and_then(|tmp| tmp.checked_sub(c.screen.reserve_right))
        .and_then(|tmp| tmp.checked_div(w_reserve))
    {
        Some(tmp) if tmp > 0 => tmp,
        Some(tmp) if tmp == 0 => 1, // terminal is wider than screen.
        _ => return Err("retile overflow".into()),
    };

    // Work out the number of rows we need to use to fit everything on screen
    let rows = (n_servers / columns)
		// round up
		+ if (n_servers % columns) > 0 { 1 } else { 0 };
    if rows == 0 {
        // unreachable
        return Err("retile overflow".into());
    }

    // Now adjust the height of the terminal to either the max given,
    // or to get everything on screen
    //let h = {
    //    let height = ((screen_h - c.screen.reserve_top - c.screen.reserve_bottom)
    //        - (rows * (c.terminal.reserve_top + c.terminal.reserve_bottom)))
    //        / rows;
    //
    //    if height > h {
    //        h
    //    } else {
    //        height
    //    }
    //};
    let h = {
        let height = {
            screen_h
                .checked_sub(c.screen.reserve_top)
                .and_then(|tmp| tmp.checked_sub(c.screen.reserve_bottom))
                .and_then(|a| {
                    c.terminal
                        .reserve_top
                        .checked_add(c.terminal.reserve_bottom)
                        .and_then(|tmp| rows.checked_mul(tmp))
                        .and_then(|tmp| a.checked_sub(tmp))
                        .and_then(|tmp| tmp.checked_div(rows))
                })
        };
        match height {
            Some(height) if height > h => h,
            Some(height) => height,
            None => h,
        }
    };

    // now we have the info, plot window positions
    if c.misc.window_tiling_right {
        tile_right(app, w, h, columns, w_reserve)?;
    } else {
        tile_left(app, w, h, screen_w, screen_h)?;
    }

    // Now remap in right order to get overlaps correct
    for (_, ref mut server) in app.get_servers().iter().rev() {
        xdisplay.map_window(server.wid);
        if raise {
            xdisplay.raise_window(server.wid);
        }
        xdisplay.flush();
        app.sleep(100); // sleep for a moment for the WM (if --sleep)
    }

    Ok(false)
}

fn tile_right<X: RetileXDisplay, T: RetileApp<X>>(
    app: &T,
    width: u32,
    height: u32,
    columns: u32,
    w_reserve: u32,
) -> Result<()> {
    let c = &app.get_config();

    //let default_x = c.screen.reserve_left + c.terminal.reserve_left;
    let default_x = c
        .screen
        .reserve_left
        .checked_add(c.terminal.reserve_left)
        .unwrap_or(c.screen.reserve_left);
    let mut x = default_x;
    //let mut y = c.screen.reserve_top + c.terminal.reserve_top;
    let mut y = c
        .screen
        .reserve_top
        .checked_add(c.terminal.reserve_top)
        .unwrap_or(c.screen.reserve_top);
    let mut column = 0;
    //let h_reserve = c.terminal.reserve_top + c.terminal.reserve_bottom + height;
    let h_reserve = c
        .terminal
        .reserve_top
        .checked_add(c.terminal.reserve_bottom)
        .and_then(|tmp| tmp.checked_add(height))
        .unwrap_or(height);

    // Unmap windows (hide them)
    // Move windows to new locatation
    // Remap all windows in correct order
    let xdisplay = app.get_xdisplay();
    for (_, ref server) in app.get_servers().iter() {
        if c.misc.unmap_on_redraw {
            xdisplay.unmap_window(server.wid);
        }
        app.send_resizemove(server.wid, x, y, width, height)?;
        xdisplay.flush();
        app.sleep(100); // sleep for a moment for the WM (if --sleep)

        // starting top left, and move right and down
        column += 1;

        if column < columns {
            // x += c.terminal.reserve_left + c.terminal.reserve_right + width;
            //  aka
            // x += w_reserve;
            x = x.checked_add(w_reserve).unwrap_or(default_x);
        } else {
            // x = c.screen.reserve_left + c.terminal.reserve_left;
            x = default_x;
            // y += c.terminal.reserve_top + c.terminal.reserve_bottom + height;
            //  aka
            // y += h_reserve;
            y = y.checked_add(h_reserve).unwrap_or(y);
            column = 0;
        }
    }
    Ok(())
}

fn tile_left<X: RetileXDisplay, T: RetileApp<X>>(
    app: &T,
    width: u32,
    height: u32,
    screen_w: u32,
    screen_h: u32,
) -> Result<()> {
    let c = &app.get_config();
    // perl cssh left tiling seems buggy.
    // 1) All windows are moved to the same x, y.
    // 2) Windows are given negative coordinates, (offscreen).
    // Try it out. Edit ~/.clusterssh/config
    // and set "window_tiling_direction=left"
    // then "cssh ::1 ::1 ::1 127.0.0.1"
    //
    // If someone can explain what left tiling is supposed to do
    // then I'll allow negative placement, but for now I clamp
    // the negative values to 0, so we produce different results
    // than perl cssh for left tiling.

    //let x = c.screen.reserve_right - screen_w - c.terminal.reserve_right - width;
    let x = c
        .screen
        .reserve_right
        .checked_sub(screen_w)
        .and_then(|tmp| tmp.checked_sub(c.terminal.reserve_right))
        .and_then(|tmp| tmp.checked_sub(width))
        .unwrap_or(0);

    //let y = c.screen.reserve_bottom - screen_h - c.terminal.reserve_bottom - height;
    let y = c
        .screen
        .reserve_bottom
        .checked_sub(screen_h)
        .and_then(|tmp| tmp.checked_sub(c.terminal.reserve_bottom))
        .and_then(|tmp| tmp.checked_sub(height))
        .unwrap_or(0);

    let xdisplay = app.get_xdisplay();
    for (_, ref server) in app.get_servers().iter().rev() {
        if c.misc.unmap_on_redraw {
            xdisplay.unmap_window(server.wid);
        }
        app.send_resizemove(server.wid, x, y, width, height)?;
        xdisplay.flush();
        app.sleep(100); // sleep for a moment for the WM (if --sleep)
    }
    Ok(())
}

#[cfg(test)]
mod retile_tests {
    use super::*; // so we can access non pub stuff in the retile mod.

    // So both mocks can share a mutable test_event logger.
    use std::cell::RefCell;
    use std::rc::Rc;

    // mocks add test_events while tests check for test_events.
    #[derive(Debug, PartialEq)]
    enum TestEvent {
        // XDisplay test_events
        Flush {},
        Map {
            wid: Wid,
        },
        Raise {
            wid: Wid,
        },
        Unmap {
            wid: Wid,
        },

        // App test_events
        ShowConsole {},
        Move {
            wid: Wid,
            x: u32,
            y: u32,
            w: u32,
            h: u32,
        },
        Sleep {
            ms: u64,
        },
    }

    type TestEvents = Rc<RefCell<Vec<TestEvent>>>;

    struct TestXDisplay {
        width_in_pixels: u32,
        height_in_pixels: u32,
        test_events: TestEvents,
    }

    impl RetileXDisplay for TestXDisplay {
        fn get_wh(&self) -> (u32, u32) {
            (self.width_in_pixels, self.height_in_pixels)
        }
        fn flush(&self) {
            self.test_events.borrow_mut().push(TestEvent::Flush {});
        }
        fn map_window(&self, wid: Wid) {
            self.test_events.borrow_mut().push(TestEvent::Map { wid });
        }
        fn raise_window(&self, wid: Wid) {
            self.test_events.borrow_mut().push(TestEvent::Raise { wid });
        }
        fn unmap_window(&self, wid: Wid) {
            self.test_events.borrow_mut().push(TestEvent::Unmap { wid });
        }
    }

    struct TestApp {
        config: config::Config,
        servers: BTreeMap<String, server::Server>,
        xdisplay: TestXDisplay,
        font_w: u32,
        font_h: u32,
        test_events: TestEvents,
    }

    impl RetileApp<TestXDisplay> for TestApp {
        fn get_config(&self) -> &config::Config {
            &self.config
        }
        fn get_servers(&self) -> &BTreeMap<String, server::Server> {
            &self.servers
        }
        fn get_font_wh(&self) -> (u32, u32) {
            (self.font_w, self.font_h)
        }
        fn get_xdisplay(&self) -> &TestXDisplay {
            &self.xdisplay
        }

        fn show_console(&mut self) -> Result<()> {
            self.test_events
                .borrow_mut()
                .push(TestEvent::ShowConsole {});
            Ok(())
        }
        fn send_resizemove(&self, wid: Wid, x: u32, y: u32, w: u32, h: u32) -> Result<()> {
            self.test_events
                .borrow_mut()
                .push(TestEvent::Move { wid, x, y, w, h });
            Ok(())
        }
        fn sleep(&self, ms: u64) {
            self.test_events.borrow_mut().push(TestEvent::Sleep { ms });
        }
    }

    fn make_test_server(wid: Wid) -> server::Server {
        server::Server {
            wid,
            pid: None,
            active: true,
            bump_num: 0,
            connect_string: "".into(),
            givenname: "".into(),
            username: None,
            pipenm: None,
            menu_item: None,
        }
    }

    struct Scenario {
        app: TestApp,
    }

    fn new_scenario() -> Scenario {
        let test_events = Rc::new(RefCell::new(Vec::new()));

        let mut xdisplay = TestXDisplay {
            width_in_pixels: 1024,
            height_in_pixels: 968,
            test_events: test_events.clone(),
        };
        xdisplay.width_in_pixels = 1024;
        xdisplay.height_in_pixels = 968;

        let mut servers = BTreeMap::new();
        servers.insert("10".into(), make_test_server(1));
        servers.insert("20".into(), make_test_server(2));
        servers.insert("30".into(), make_test_server(3));
        let mut app = TestApp {
            config: Default::default(),
            servers,
            xdisplay,
            font_w: 8,
            font_h: 16,
            test_events: test_events.clone(),
        };

        // all measurements are pixels except the columns and rows.
        app.font_w = 8;
        app.font_h = 16;
        app.config.terminal.decoration_height = 10;
        app.config.terminal.decoration_width = 8;
        app.config.terminal.reserve_bottom = 1;
        app.config.terminal.reserve_left = 5;
        app.config.terminal.reserve_right = 2;
        app.config.terminal.reserve_top = 3;
        app.config.terminal.terminal_size_x = 80; // columns
        app.config.terminal.terminal_size_y = 24; // rows

        app.config.screen.reserve_top = 1;
        app.config.screen.reserve_bottom = 60;
        app.config.screen.reserve_left = 2;
        app.config.screen.reserve_right = 3;

        app.config.misc.window_tiling_right = true;
        app.config.misc.unmap_on_redraw = false;

        Scenario { app }
    }

    fn filter_test_events(scenario: &Scenario) -> Vec<TestEvent> {
        // all our tests only check for a few test_events, so select only those
        let mut test_events = scenario.app.test_events.borrow_mut();
        let got = test_events
            .drain(..)
            .filter(|e| match e {
                TestEvent::Map { wid: _ } => true,
                TestEvent::ShowConsole {} => true,
                TestEvent::Move {
                    wid: _,
                    x: _,
                    y: _,
                    w: _,
                    h: _,
                } => true,
                _ => false,
            })
            .collect();
        got
    }

    #[test]
    fn test_retile_3_vertical() {
        // default scenario has three terminals stacked vertically
        let mut scenario = new_scenario();
        let result = retile_hosts(&mut scenario.app, false);
        assert_eq!(result, Ok(false));

        let got = filter_test_events(&scenario);

        let mut expected = Vec::new();
        // I'd like these all on one line
        // but rustfmt::skip on blocks is iffy
        //        #[rustfmt::skip]
        {
            expected.push(TestEvent::Move {
                wid: 1,
                x: 7,
                y: 4,
                w: 648,
                h: 298,
            });
            expected.push(TestEvent::Move {
                wid: 2,
                x: 7,
                y: 306,
                w: 648,
                h: 298,
            });
            expected.push(TestEvent::Move {
                wid: 3,
                x: 7,
                y: 608,
                w: 648,
                h: 298,
            });
        }
        expected.push(TestEvent::Map { wid: 3 });
        expected.push(TestEvent::Map { wid: 2 });
        expected.push(TestEvent::Map { wid: 1 });

        assert_eq!(got, expected);
    }

    #[test]
    fn test_retile_3_horizontal() {
        // make terminals so narrow that they all stack horizontally
        let mut scenario = new_scenario();
        scenario.app.config.terminal.terminal_size_x = 8; // columns

        let result = retile_hosts(&mut scenario.app, false);
        assert_eq!(result, Ok(false));

        let got = filter_test_events(&scenario);

        let mut expected = Vec::new();
        //        #[rustfmt::skip]
        {
            expected.push(TestEvent::Move {
                wid: 1,
                x: 7,
                y: 4,
                w: 72,
                h: 394,
            });
            expected.push(TestEvent::Move {
                wid: 2,
                x: 86,
                y: 4,
                w: 72,
                h: 394,
            });
            expected.push(TestEvent::Move {
                wid: 3,
                x: 165,
                y: 4,
                w: 72,
                h: 394,
            });
        }
        expected.push(TestEvent::Map { wid: 3 });
        expected.push(TestEvent::Map { wid: 2 });
        expected.push(TestEvent::Map { wid: 1 });

        assert_eq!(got, expected);
    }

    #[test]
    fn test_retile_2x2() {
        // adjust terminal width so they form an "r"
        //   1 2
        //   3
        let mut scenario = new_scenario();
        scenario.app.config.terminal.terminal_size_x = 60; // columns

        let result = retile_hosts(&mut scenario.app, false);
        assert_eq!(result, Ok(false));

        let got = filter_test_events(&scenario);

        let mut expected = Vec::new();
        //        #[rustfmt::skip]
        {
            expected.push(TestEvent::Move {
                wid: 1,
                x: 7,
                y: 4,
                w: 488,
                h: 394,
            });
            expected.push(TestEvent::Move {
                wid: 2,
                x: 502,
                y: 4,
                w: 488,
                h: 394,
            });
            expected.push(TestEvent::Move {
                wid: 3,
                x: 7,
                y: 402,
                w: 488,
                h: 394,
            });
        }
        expected.push(TestEvent::Map { wid: 3 });
        expected.push(TestEvent::Map { wid: 2 });
        expected.push(TestEvent::Map { wid: 1 });

        assert_eq!(got, expected);
    }

    #[test]
    fn test_terminals_larger_than_screen() {
        // terminals are so tall/wide that they up short and stacked vertically
        let mut scenario = new_scenario();
        scenario.app.config.terminal.terminal_size_x = 140; // columns. width maintained
        scenario.app.config.terminal.terminal_size_y = 70; // rows. height shrunk to fit

        let result = retile_hosts(&mut scenario.app, false);
        assert_eq!(result, Ok(false));

        let got = filter_test_events(&scenario);

        let mut expected = Vec::new();
        //        #[rustfmt::skip]
        {
            expected.push(TestEvent::Move {
                wid: 1,
                x: 7,
                y: 4,
                w: 1128,
                h: 298,
            });
            expected.push(TestEvent::Move {
                wid: 2,
                x: 7,
                y: 306,
                w: 1128,
                h: 298,
            });
            expected.push(TestEvent::Move {
                wid: 3,
                x: 7,
                y: 608,
                w: 1128,
                h: 298,
            });
        }
        expected.push(TestEvent::Map { wid: 3 });
        expected.push(TestEvent::Map { wid: 2 });
        expected.push(TestEvent::Map { wid: 1 });

        assert_eq!(got, expected);
    }

    #[test]
    fn test_zero_padding_2x2() {
        // set all config padding to zero
        // and screen just 2x width and 2x height of terminals.
        let mut scenario = new_scenario();
        {
            let app = &mut scenario.app;
            app.font_w = 10;
            app.font_h = 20;

            app.config.terminal.decoration_height = 0;
            app.config.terminal.decoration_width = 0;
            app.config.terminal.reserve_bottom = 0;
            app.config.terminal.reserve_left = 0;
            app.config.terminal.reserve_right = 0;
            app.config.terminal.reserve_top = 0;
            app.config.terminal.terminal_size_x = 10; // columns
            app.config.terminal.terminal_size_y = 5; // rows

            app.xdisplay.width_in_pixels = 200;
            app.xdisplay.height_in_pixels = 200;
            app.config.screen.reserve_top = 0;
            app.config.screen.reserve_bottom = 0;
            app.config.screen.reserve_left = 0;
            app.config.screen.reserve_right = 0;
        }

        let result = retile_hosts(&mut scenario.app, false);
        assert_eq!(result, Ok(false));

        let got = filter_test_events(&scenario);

        let mut expected = Vec::new();
        //        #[rustfmt::skip]
        {
            expected.push(TestEvent::Move {
                wid: 1,
                x: 0,
                y: 0,
                w: 100,
                h: 100,
            });
            expected.push(TestEvent::Move {
                wid: 2,
                x: 100,
                y: 0,
                w: 100,
                h: 100,
            });
            expected.push(TestEvent::Move {
                wid: 3,
                x: 0,
                y: 100,
                w: 100,
                h: 100,
            });
        }
        expected.push(TestEvent::Map { wid: 3 });
        expected.push(TestEvent::Map { wid: 2 });
        expected.push(TestEvent::Map { wid: 1 });

        assert_eq!(got, expected);
    }

    #[test]
    fn test_overflow() {
        // Trigger the last overflow (subtracting right screen padding)
        let mut scenario = new_scenario();
        scenario.app.config.screen.reserve_right = u32::max_value();
        let result = retile_hosts(&mut scenario.app, false);
        assert!(result.is_err());

        // Trigger muliplication overflow. font width * columns == 2^32
        scenario = new_scenario();
        let x = u32::from(u16::max_value()) + 1;
        scenario.app.font_w = x;
        scenario.app.config.terminal.terminal_size_x = x;
        let result = retile_hosts(&mut scenario.app, false);
        assert!(result.is_err());
    }
}
