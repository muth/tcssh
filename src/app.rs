use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::process;
use std::rc::Rc;
use structopt::StructOpt;

use crate::cluster;
use crate::config;
use crate::er::Result;
use crate::evaluate;
use crate::g::GtkStuff;
use crate::getopt;
use crate::retile;
use crate::send_text;
use crate::server;
use crate::text2x11;
use crate::wait_children;
use crate::x;

#[derive(Debug)]
pub struct App {
    pub cluster: cluster::Cluster,
    pub config: config::Config,
    getopt: getopt::Getopt,
    pub servers: BTreeMap<String, server::Server>,
    pub dead_servers: Vec<String>,
    pub xdisplay: x::XDisplay,
    pub gtkstuff: Option<GtkStuff>,
    pub text2x11: Option<text2x11::Text2X11>,

    pub internal_activate_autoquit: bool,
    font_w: u32,
    font_h: u32,
    me: String,

    pub events: VecDeque<Event>,
}

// gtk callbacks need static lifetime so wrap App in Rc<RefCell<>>
// and gtk is forced single threaded, so no need for Arc or other.
pub type Rapp = Rc<RefCell<App>>;

pub type Wid = u64; // window id

// UI Events are injected when required and dequeued in our idle loop.
#[derive(Debug)]
pub enum Event {
    ShowConsole(u8),       // count down of times idle has called us before we run.
    AddHosts(Vec<String>), // hosts|tags to open
}

impl App {
    pub fn new_ref(arg0: &str, me: &str) -> Result<Rapp> {
        let mut app = App {
            cluster: Default::default(),
            config: Default::default(),
            getopt: getopt::Getopt::from_args(), // parses CLI --args
            servers: BTreeMap::new(),
            dead_servers: Vec::new(),
            xdisplay: Default::default(),
            gtkstuff: Default::default(),
            text2x11: Default::default(),
            internal_activate_autoquit: false,
            font_w: 0,
            font_h: 0,
            me: me.into(),
            events: VecDeque::with_capacity(4),
        };

        // Populate app.config by reading config file which is
        // either specified on CLI --config_file=foo
        // or default ~/.tcssh/config or even ~/.clusterssh/config
        app.getopt.setup(&mut app.config)?;

        // setup some config values (based on how we were invoked, arg0)
        // Also check that 'xterm' is installed and executable
        app.config.setup(&arg0)?;

        // If there was an --arg it should override config file value.
        app.getopt.override_config_with_args(&mut app.config)?;

        if app.getopt.dump_config {
            config::dump_config(&app.config);
            app.exit_prog();
        }

        Ok(Rc::new(RefCell::new(app)))
    }

    pub fn run(&mut self, rself: &Rapp) -> Result<()> {
        self.xdisplay = x::XDisplay::new()?;

        // NLL version is nicer
        //if let Some(ref mut evaluate) = self.getopt.evaluate { // NLL -mut
        //    evaluate::evaluate_commands(evaluate, &self.config);
        //    self.exit_prog();
        //}
        // But without NLL we need another level
        if self.getopt.evaluate.is_some() {
            if let Some(ref evaluate) = self.getopt.evaluate {
                // NLL -mut
                evaluate::evaluate_commands(evaluate, &self.config);
            }
            self.exit_prog();
        }

        // I'd like to write the next 3 lines as one
        // (self.font_w, self.font_h) = self.get_font_size()?;
        // but the above yields E0070 "left-hand ... not valid"
        // so assign to temp, then unpack.
        let (w, h) = self.get_font_size()?;
        self.font_w = w;
        self.font_h = h;

        let keymap = text2x11::Text2X11::new(&mut self.xdisplay)?;
        self.text2x11 = Some(keymap);

        self.cluster.get_cluster_entries(&mut self.config)?;
        self.cluster.get_tag_entries(&mut self.config)?;

        if self.getopt.list.is_some() {
            self.handle_list();
            self.exit_prog();
        }

        if self.getopt.hosts.is_empty() {
            if let Some(hosts) = self.cluster.get_tag("default") {
                self.getopt.hosts.extend_from_slice(hosts);
            }
        }

        self.resolve_names(true)?;

        let g = GtkStuff::create_windows(&self.config, rself)?;

        g.create_menubar(self, rself);
        g.change_main_window_title(self);
        g.capture_map_events();

        // Set our signal handler, but only after resolve_names(),
        // because it seems to interfere with std::process::Command
        wait_children::setup_sig_chld_handler()?;

        server::open_client_windows(
            &self.getopt.hosts,
            &mut self.servers,
            &self.config,
            &mut self.internal_activate_autoquit,
            &self.me,
        )?;

        g.build_hosts_menu(self, rself);

        self.gtkstuff = Some(g);

        if self.config.misc.window_tiling {
            self.retile_hosts(false, false)?;
        } else {
            self.show_console()?;
        }

        Ok(())
    }

    pub fn show_console(&mut self) -> Result<()> {
        self.xdisplay.flush();

        self.sleep(200);
        // NLL edtion 2018 makes this clearer
        //if let Some(ref mut gtkstuff) = self.gtkstuff {
        // and counter done in line.
        // But for without that, we need this extra Option<u8> bit
        if self.gtkstuff.is_some() {
            let mut counter = None;
            if let Some(ref mut gtkstuff) = self.gtkstuff {
                if self.servers.is_empty() {
                    // There are no servers/xterms, then go ahead and show it right away.
                    gtkstuff.show_main_window();
                } else {
                    // This seems odd, we're show_console(), but doing the opposite.
                    // TODO write comment explaining this, or remove it.
                    gtkstuff.hide_main_window();

                    // perl cssh slept "for a moment to give WM time to bring console back"
                    //self.sleep(500);
                    // We avoid explicit sleep, but do so indirectly via stuffing an event
                    // into our event queue, and wait for our idle callback to handle it later.

                    counter = gtkstuff.get_main_window_request_delay();
                }
            }
            if let Some(counter) = counter {
                // NLL would allow this to be used where it's set
                self.add_event_show_console(counter);
            }
        }
        Ok(())
    }

    fn add_event_show_console(&mut self, counter: u8) {
        self.events.push_back(Event::ShowConsole(counter));
    }

    pub fn handle_event_show_console(&mut self, counter: u8) {
        // Telling window managers where to place windows is just a suggestion.
        // If the console is in the way, WMs can ignore our specified geomeotry
        // and place an xterm so it doesn't interfere with the console.
        // So we hide our console before retiling, and unhide it after.
        if counter == 0 {
            if let Some(ref mut gtkstuff) = self.gtkstuff {
                gtkstuff.show_main_window();
            }
        } else {
            self.add_event_show_console(counter - 1);
        }
    }

    pub fn handle_events(&mut self, rapp: &Rapp) {
        // First figure out an upper limit so can put hard bounds
        // on the number of times we loop.
        let mut limit = self.events.len();
        while limit > 0 {
            limit -= 1;
            match self.events.pop_front() {
                Some(Event::ShowConsole(counter)) => {
                    self.handle_event_show_console(counter);
                }
                Some(Event::AddHosts(to_open)) => {
                    self.getopt.hosts = to_open;
                    if let Err(e) = self.resolve_names(false) {
                        eprintln!("Failed top resolve_names {:?}", e);
                    } else if let Err(e) = server::open_client_windows(
                        // TODO add hide_console, before open
                        &self.getopt.hosts,
                        &mut self.servers,
                        &self.config,
                        &mut self.internal_activate_autoquit,
                        &self.me,
                    ) {
                        eprintln!("Failed top open windows {:?}", e);
                    } else if self.gtkstuff.is_some() {
                        // NLL lets us shorten this.. but for now..
                        if let Some(ref g) = self.gtkstuff {
                            // reproduce g.build_hosts_menu() here due to borrowing.
                            for (ref server_key, ref mut server) in self.servers.iter_mut() {
                                g.build_host_menu(server_key, server, rapp);
                            }
                            g.change_main_window_title(self);
                        }
                        let _ = self.retile_hosts(false, false);
                    }
                }
                None => return,
            }
        }
    }

    // handle CLI arg --list
    fn handle_list(&mut self) {
        // More NLL code which gets cleaner with NLL and edition 2018, instead a flag
        let mut flag = false;
        let (tab, eol) = if self.getopt.quiet {
            ("", ' ')
        } else {
            ("\t", '\n')
        };
        if let Some(list) = &self.getopt.list {
            if list.is_empty() {
                if !self.getopt.quiet {
                    println!("Available cluster tags:");
                }
                for tag in self.cluster.list_tags() {
                    print!("{}{}{}", tab, tag, eol);
                }
                // perl cssh didn't print \n if quiet, (and no external clusters) so neither do we.

                if let Some(cmd) = &self.config.misc.external_cluster_command {
                    if let Ok(mut clusters) =
                        cluster::get_external_clusters(cmd, &["-L".to_string()])
                    {
                        if !clusters.is_empty() {
                            clusters.sort();
                            if !self.getopt.quiet {
                                println!("Available external command tags:");
                            }
                            for tag in clusters {
                                print!("{}{}{}", tab, tag, eol);
                            }
                            println!();
                        }
                    }
                }
            } else {
                if !self.getopt.quiet {
                    println!("Tag resolved to hosts: ");
                }
                self.getopt.hosts.clear();
                self.getopt.hosts.push(list.to_string());
                flag = true;
            }
        }
        if flag {
            match self.resolve_names(true) {
                Ok(()) => {
                    for host in &self.getopt.hosts {
                        print!("{}{}{}", tab, host, eol);
                    }
                    println!();
                }
                Err(e) => {
                    println!("Error resolve_names(): {:?}", e);
                }
            }
        }
    }

    pub fn resolve_names(&mut self, run_external: bool) -> Result<()> {
        // There are a few places which call this, so it seems
        // a bit messy to have the non-main callers stuff their
        // data into self.getopt.hosts, but it just makes borrowing easier).
        self.getopt.hosts = self
            .cluster
            .resolve_clusters(&mut self.getopt.hosts, self.config.misc.use_all_a_records)?;

        if run_external {
            if let Some(cmd) = &self.config.misc.external_cluster_command {
                match cluster::get_external_clusters(cmd, &self.getopt.hosts) {
                    Ok(new_hosts) => self.getopt.hosts = new_hosts,
                    Err(e) => eprintln!("Error running external_cluster command: {:?}", e), // no change to self.getopt.hosts
                }
            }
        }
        let hosts = &mut self.getopt.hosts;

        hosts.retain(|host| !host.is_empty()); // in place, preservers order

        if self.config.misc.unique_servers {
            hosts.sort_unstable();
            hosts.dedup();
        }
        Ok(())
    }

    pub fn get_n_servers(&self) -> usize {
        self.servers.len()
    }

    fn get_font_size(&mut self) -> Result<(u32, u32)> {
        let x = self.xdisplay.get_font_size(&self.config.terminal.font)?;
        Ok((x.0, x.1))
    }

    pub fn retile_hosts(&mut self, force: bool, raise: bool) -> Result<()> {
        let console_shown = if !self.config.misc.window_tiling && !force {
            for (_, ref mut server) in self.servers.iter().rev() {
                self.xdisplay.map_window(server.wid);
            }
            self.xdisplay.flush();
            false
        } else {
            retile::retile_hosts(self, raise)?
        };
        if !console_shown {
            // console maintains its own state so we don't really need
            // to have a console_shown flag.  But if we know we just called
            // it, we can skip a bit of work.
            self.show_console()
        } else {
            Ok(())
        }
    }

    pub fn send_resizemove(&self, wid: Wid, x: u32, y: u32, w: u32, h: u32) -> Result<()> {
        self.xdisplay.change_property(wid, x, y, w, h)?;
        self.xdisplay.configure_window(wid, x, y, w, h)?;
        if self.getopt.debug {
            println!(
                "at {}:{} send_resizemove x={:4} y={:4} w={:4} h={:4} wid={}",
                file!(),
                line!(),
                x,
                y,
                w,
                h,
                wid
            );
        }
        Ok(())
    }

    // handle paste events, send text to all active servers.
    pub fn send_text(&mut self, text: &str) {
        send_text::send_text(self, text);
    }

    pub fn send_variable_text(&mut self) {
        send_text::send_variable_text(self);
    }

    pub fn send_event(&self, wid: Wid, state: u32, keycode: u32) {
        if self.xdisplay.send_event(wid, state, keycode).is_err() {
            eprintln!("Error sending event to {}", wid);
        }
    }

    pub fn toggle_active_state(&mut self) {
        for (_, ref mut server) in self.servers.iter_mut() {
            // server.set_active( ! server.active ); // Borrow checker rejects this.. sigh.
            let tmp = !server.active;
            server.set_active(tmp);
        }
    }

    pub fn set_all_active(&mut self) {
        for (_, ref mut server) in self.servers.iter_mut() {
            server.set_active(true);
        }
    }

    pub fn set_half_inactive(&mut self) {
        let mut half: usize = (self.servers.len() + 1) / 2;
        for (_, ref mut server) in self.servers.iter_mut() {
            server.set_active(false);
            half -= 1;
            if half == 0 {
                break;
            }
        }
    }

    pub fn close_inactive_sessions(&self) {
        for value in self.servers.values() {
            if !value.active {
                value.terminate_host();
            }
        }
    }

    pub fn re_add_closed_sessions(&mut self, rapp: &Rapp) {
        if self.dead_servers.is_empty() {
            return;
        }
        server::clear_bump_nums(&mut self.servers);
        let dead_servers: Vec<String> = self.dead_servers.drain(..).collect();
        // I tried hiding the console here, but that's async.
        if let Err(e) = server::open_client_windows(
            &dead_servers,
            &mut self.servers,
            &self.config,
            &mut self.internal_activate_autoquit,
            &self.me,
        ) {
            eprintln!("Failed top open windows {:?}", e);
            // Show
        }
        // more non NLL flag nonsense
        let mut flag = false;
        if let Some(ref g) = self.gtkstuff {
            // reproduce g.build_hosts_menu() here due to borrowing.
            for (ref server_key, ref mut server) in self.servers.iter_mut() {
                g.build_host_menu(server_key, server, rapp);
            }
            g.change_main_window_title(self);
            flag = true;
        }
        if flag {
            let _ = self.retile_hosts(false, false);
        }
    }

    pub fn sleep(&self, ms: u64) {
        self.config.tcssh.sleep(ms);
    }

    pub fn exit_prog(&mut self) -> ! {
        for value in self.servers.values() {
            value.terminate_host();
        }
        self.xdisplay.close_display();
        process::exit(0);
    }
}

impl retile::RetileApp<x::XDisplay> for App {
    // accessors
    fn get_config(&self) -> &config::Config {
        &self.config
    }
    fn get_servers(&self) -> &BTreeMap<String, server::Server> {
        &self.servers
    }
    fn get_font_wh(&self) -> (u32, u32) {
        (self.font_w, self.font_h)
    }
    fn get_xdisplay(&self) -> &x::XDisplay {
        &self.xdisplay
    }

    // delegators. It seems silly to have to writes these.
    // It also seems odd to read.. they look recursive.
    fn show_console(&mut self) -> Result<()> {
        self.show_console()
    }

    fn send_resizemove(&self, wid: Wid, x: u32, y: u32, w: u32, h: u32) -> Result<()> {
        self.send_resizemove(wid, x, y, w, h)
    }
    fn sleep(&self, ms: u64) {
        self.sleep(ms);
    }
}
