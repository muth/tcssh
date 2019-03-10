// Contains the majority of the interaction with gtk via gtk::*
// but there is some leakage to other modules.
use gdk::{
    ModifierType,
    Screen,
    ScreenExt, // for get_rgba_visual()
    SELECTION_CLIPBOARD,
};
use gtk;
use gtk::prelude::*;
use gtk::{
    Box,
    CheckMenuItemExt, // for set_active()
    Entry,
    Menu,
    MenuBar,
    MenuItem,
    MenuShellExt, // for menu.append()
    PolicyType,
    TextView,
    WidgetExt, // for show_all()
    Window,
};
use std::os::raw::c_uint;

use crate::app;
use crate::config;
use crate::er::Result;
use crate::host::STRICT_GEOMETRY;
use crate::macros::VERSION_JUST_NUMBER;
use crate::server;
use crate::tk2gtk;

#[derive(Debug)]
enum Console {
    HiddenBeforeFirstDraw(Option<String>), // initial geometry from ~/.tcssh/config console_position=+123+123
    Hidden(i32, i32),                      // (x,y) of console before we hide it
    Shown,
}

#[derive(Debug)]
pub struct GtkStuff {
    main_window: Window,
    console: Console,
    menu_bar: MenuBar,
    pub hosts_menu: Menu,
    send_menu: Menu,
    main_box: Box,
    text_entry_in_use: bool, // are we showing text_entry or history_window
    text_entry: Entry,
    history_window: gtk::ScrolledWindow,
}

impl Console {
    fn show(&mut self, main_box: &Box, main_window: &Window, text_entry: &Entry) {
        match self {
            Console::HiddenBeforeFirstDraw(geometry) => {
                main_box.show_all();
                if let Some(ref geometry) = geometry {
                    if STRICT_GEOMETRY.is_match(geometry) {
                        main_window.parse_geometry(geometry);
                    }
                }
                main_window.map(); // clippy crash, patched
                main_window.show_all();
            }
            Console::Hidden(x, y) => {
                main_window.deiconify();
                main_window.present_with_time(0);
                main_window.map(); // clippy crash, patched
                main_window.show_all();
                main_window.move_(*x, *y);
                // unmap/map often makes WM move the console
                // so we explicitly move it back to where it was before.
            }
            Console::Shown => {
                return; // nop
            }
        };
        main_window.grab_focus();
        text_entry.grab_focus();
        *self = Console::Shown;
    }
    fn hide(&mut self, main_window: &Window) {
        if let Console::Shown = self {
            let (x, y) = main_window.get_position();
            *self = Console::Hidden(x, y);
            main_window.unmap();
        }
    }
}

impl GtkStuff {
    pub fn create_windows(config: &config::Config, rapp: &app::Rapp) -> Result<GtkStuff> {
        if gtk::init().is_err() {
            return Err("gtk::init failed".into()); // init returns BoolErr so no additional info
        }

        let main_window = Window::new(gtk::WindowType::Toplevel);
        main_window.hide();
        main_window.set_title("tcssh");

        if config.tcssh.transparent {
            set_visual(&main_window, &None);
            main_window.connect_screen_changed(set_visual);
            main_window.set_app_paintable(true); // crucial for transparency
            main_window.set_opacity(config.tcssh.opacity);
        }

        let main_box = Box::new(gtk::Orientation::Vertical, 10);
        main_window.add(&main_box);

        let menu_bar = MenuBar::new();
        main_box.pack_start(&menu_bar, false, false, 0);

        let hosts_menu = Menu::new();
        let send_menu = Menu::new();

        let text_entry = Entry::new();
        text_entry.set_width_chars(25);
        text_entry.set_visibility(false); // So we don't see text in the entry box. (intended for password entry)

        let history_window = gtk::ScrolledWindow::new(None, None);
        {
            history_window.set_policy(PolicyType::Automatic, PolicyType::Automatic);

            let height = i32::from(config.misc.history_height) * 16;
            let width = i32::from(config.misc.history_width) * 9;
            // *16 because we may be reading perl config file
            // and Tk seemed to use font as metric, instead of pixels.
            history_window.set_min_content_height(height);
            history_window.set_min_content_width(width);

            let text_view = TextView::new();
            history_window.add(&text_view);
        }

        let text_entry_in_use = !config.misc.show_history;
        if text_entry_in_use {
            main_box.add(&text_entry);
        } else {
            main_box.add(&history_window);
        }

        main_window.connect_delete_event(|_, _| {
            gtk::main_quit();
            Inhibit(false)
        });

        // yes.. compare to "null".. because it's a string from a config file
        // and we may be reading perl cssh's config file,
        // and "null" is what perl cssh checked for.
        if config.keymap.key_paste != "null" {
            let rapp_clone = rapp.clone();
            let clipboard = gtk::Clipboard::get(&SELECTION_CLIPBOARD);
            text_entry.connect_paste_clipboard(move |_| {
                if let Some(str) = clipboard.wait_for_text() {
                    rapp_clone.borrow_mut().send_text(&str);
                }
            });
        }

        let console_position = match config.misc.console_position {
            Some(ref s) => Some(s.clone()),
            None => None,
        };

        Ok(GtkStuff {
            main_window,
            console: Console::HiddenBeforeFirstDraw(console_position),
            menu_bar,
            main_box,
            text_entry_in_use,
            text_entry,
            history_window,
            hosts_menu,
            send_menu,
        })
    }

    pub fn create_menubar(&self, app: &app::App, rapp: &app::Rapp) {
        let file = MenuItem::new_with_label("File");
        let hosts = MenuItem::new_with_label("Hosts");
        let send = MenuItem::new_with_label("Send");
        let help = MenuItem::new_with_label("Help");

        let file_menu = Menu::new();
        let file_history = MenuItem::new_with_mnemonic("Show _History");
        let file_quit = MenuItem::new_with_mnemonic("_Quit");

        let rapp_clone = rapp.clone();
        file_history.connect_activate(move |_| {
            if let Some(ref mut gtkstuff) = rapp_clone.borrow_mut().gtkstuff {
                gtkstuff.toggle_history();
            }
        });
        self.bind_accelerator(&app.config.keymap.key_history, &file_history);

        file_quit.connect_activate(|_| {
            gtk::main_quit();
        });
        self.bind_accelerator(&app.config.keymap.key_quit, &file_quit);

        file_menu.append(&file_history);
        file_menu.append(&file_quit);

        file.set_submenu(Some(&file_menu));

        let hosts_retile = MenuItem::new_with_mnemonic("_Retile Windows");
        let hosts_raise = MenuItem::new_with_mnemonic("Ra_ise and Retile Windows");
        let hosts_active = MenuItem::new_with_mnemonic("Set _all active");
        let hosts_inactive = MenuItem::new_with_mnemonic("Set _half inactive");
        let hosts_toggle = MenuItem::new_with_mnemonic("_Toggle active state");
        let hosts_close = MenuItem::new_with_mnemonic("_Close inactive sessions");
        let hosts_add = MenuItem::new_with_mnemonic("Add _Host(s) or Cluster(s)");
        let hosts_re_add = MenuItem::new_with_mnemonic("Re-add closed _session(s)");

        self.hosts_menu.append(&hosts_retile);
        self.hosts_menu.append(&hosts_raise);
        self.hosts_menu.append(&hosts_active);
        self.hosts_menu.append(&hosts_inactive);
        self.hosts_menu.append(&hosts_toggle);
        self.hosts_menu.append(&hosts_close);
        self.hosts_menu.append(&hosts_add);
        self.hosts_menu.append(&hosts_re_add);

        hosts.set_submenu(Some(&self.hosts_menu));

        let rapp_clone = rapp.clone();
        hosts_retile.connect_activate(move |_| {
            rapp_clone.borrow_mut().retile_hosts(false, false).ok();
        });
        self.bind_accelerator(&app.config.keymap.key_retile_hosts, &hosts_retile);

        let rapp_clone = rapp.clone();
        hosts_raise.connect_activate(move |_| {
            rapp_clone.borrow_mut().retile_hosts(false, true).ok();
        });
        self.bind_accelerator(&app.config.keymap.key_raise_hosts, &hosts_raise);

        let rapp_clone = rapp.clone();
        hosts_active.connect_activate(move |_| {
            rapp_clone.borrow_mut().set_all_active();
        });

        let rapp_clone = rapp.clone();
        hosts_inactive.connect_activate(move |_| {
            rapp_clone.borrow_mut().set_half_inactive();
        });

        let rapp_clone = rapp.clone();
        hosts_toggle.connect_activate(move |_| {
            rapp_clone.borrow_mut().toggle_active_state();
        });

        let rapp_clone = rapp.clone();
        hosts_close.connect_activate(move |_| {
            rapp_clone.borrow_mut().close_inactive_sessions();
        });

        self.populate_add_hosts_or_clusters_menu(&hosts_add, app, rapp);

        let rapp_clone = rapp.clone();
        hosts_re_add.connect_activate(move |_| {
            rapp_clone.borrow_mut().re_add_closed_sessions(&rapp_clone);
        });

        self.populate_send_menu(&send, app, rapp);

        let help_menu = Menu::new();
        let help_about = MenuItem::new_with_label("About");
        //let help_docs = MenuItem::new_with_label("Documentation");

        help_menu.append(&help_about);
        //help_menu.append(&help_docs);

        let main_window_clone = self.main_window.clone();
        help_about.connect_activate(move |_| {
            let p = gtk::AboutDialog::new();
            p.set_authors(&["Mark Nieweglowski"]);
            p.set_comments(Some("Transparent Cluster SSH"));
            p.set_copyright(Some("2019 Mark Nieweglowski"));
            p.set_license_type(gtk::License::Gpl30);
            p.set_program_name(&"tcssh");
            p.set_title("About Transparent Cluster SSH");
            p.set_transient_for(Some(&main_window_clone));
            p.set_version(Some(VERSION_JUST_NUMBER));
            p.run();
            p.destroy();
        });

        help.set_submenu(Some(&help_menu));

        self.menu_bar.append(&file);
        self.menu_bar.append(&hosts);
        self.menu_bar.append(&send);
        self.menu_bar.append(&help);

        self.main_window.show_all();

        let text_entry = self.text_entry.clone();
        let use_hotkeys = app.config.keymap.use_hotkeys;

        let rapp_clone = rapp.clone();
        self.main_window.connect_key_press_event(move |_, event| {
            text_entry.get_buffer().set_text("");

            let keyval = event.get_keyval();
            let keycode = event.get_hardware_keycode();
            let state = event.get_state();

            if use_hotkeys {
                // TODO
                // stuff.  like Alt? == hostname/username/quit
            }

            // ctrl-d with zero servers == exit program
            if ModifierType::CONTROL_MASK == state
                && 'd' as u32 == keyval
                && rapp_clone.borrow().servers.is_empty()
            {
                gtk::main_quit();
                // after gtk's main loop app calls its exit_prog()
                // which terminates children, closes display, ends process.
                return Inhibit(false);
            }

            // TODO if we're showing history. keypresses need to
            // be translated and sent to history window
            //    $self->update_display_text( $keycodetosym{$keysymdec} )
            //        if ( $event eq "KeyPress" && $keycodetosym{$keysymdec} );

            let app = rapp_clone.borrow();
            let mut flush = false;
            for (ref server_key, ref server) in app.servers.iter() {
                if !server.active {
                    continue;
                }
                flush = true;
                if app
                    .xdisplay
                    .send_event(server.wid, state.bits() as c_uint, keycode.into())
                    .is_err()
                {
                    println!("Error sending event to {}", server_key);
                }
            }
            if flush {
                app.xdisplay.flush();
            }
            text_entry.get_buffer().set_text("");
            Inhibit(false)
        });
    }

    fn populate_add_hosts_or_clusters_menu(
        &self,
        hosts_add: &MenuItem,
        app: &app::App,
        rapp: &app::Rapp,
    ) {
        let flags = gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT;
        let dialog = gtk::Dialog::new_with_buttons(
            Some(&"Add Host(s) or Cluster(s)"),
            Some(&self.main_window),
            flags,
            &[
                (&"_Add", gtk::ResponseType::Accept.into()),
                (&"_Cancel", gtk::ResponseType::Reject.into()),
            ],
        );
        let mut tags = app.cluster.list_tags();
        // perl cssh runs its external cluster command each time the dialog pops up,
        // and perl cssh removes its sig handler for the duration of the external
        // call (via local $SIG{CHLD} = undef;)
        // I'm not going to mess with our sig handler.
        // So this is a behavior change from perl cssh.
        // if let Some(cmd) = &app.config.misc.external_cluster_command {
        //     if let Ok(mut clusters) = cluster::get_external_clusters(cmd, &["-L".to_string()]) {
        //        if !clusters.is_empty() {
        //            tags.append(&mut clusters);
        //        }
        //     }
        // }
        config::parse_ssh_config_and_add_hosts(&mut tags);

        let list_box = gtk::ListBox::new();
        list_box.set_selection_mode(gtk::SelectionMode::Multiple);
        list_box.set_activate_on_single_click(true);
        let mut max_len = 20;
        for tag in &tags {
            let len = tag.len();
            if len > max_len {
                max_len = len;
            }
            let label = gtk::Label::new(Some(tag.as_str()));
            label.set_justify(gtk::Justification::Left);
            label.set_halign(gtk::Align::Start);
            let list_box_row = gtk::ListBoxRow::new();
            list_box_row.add(&label);
            list_box.add(&list_box_row);
        }
        let max_len: i32 = if max_len < (i32::max_value() as usize) {
            max_len as i32
        } else {
            i32::max_value()
        };
        let text_entry = Entry::new();
        text_entry.set_width_chars(max_len);
        text_entry.set_visibility(true);

        let dialog_box = Box::new(gtk::Orientation::Vertical, 10);
        let n = tags.len();
        if n > app.config.menu.max_addhost_menu_cluster_items as usize {
            let scroll = gtk::ScrolledWindow::new(None, None);
            scroll.set_policy(PolicyType::Automatic, PolicyType::Automatic);
            let height = i32::from(app.config.menu.max_addhost_menu_cluster_items) * 16;
            scroll.set_min_content_height(height);
            // perl cssh used Tk, and used height of max_addhost_menu_cluster_items
            // but gtk seems to use pixels intead of items, so *16,
            // Who knows what font gtk is using in this dialog, 16 for height seems
            // as good a guess as any.
            scroll.add(&list_box);
            dialog_box.pack_start(&scroll, true, true, 0);
        } else {
            dialog_box.pack_start(&list_box, true, true, 0);
        }
        dialog_box.pack_end(&text_entry, false, false, 0);

        let content_area = dialog.get_content_area();
        content_area.pack_start(&dialog_box, true, true, 0);
        content_area.show_all();

        let rapp_clone = rapp.clone();
        hosts_add.connect_activate(move |_| {
            text_entry.set_text("");
            list_box.unselect_all();
            text_entry.grab_focus();
            let button_pressed = dialog.run();
            dialog.hide(); // .hide() is async. cannot create/tile via an event.
            if button_pressed == gtk::ResponseType::Accept.into() {
                let mut to_open = Vec::new();
                for row in list_box.get_selected_rows() {
                    let i = row.get_index();
                    if i >= 0 {
                        let i = i as usize;
                        if i < n {
                            if let Some(tag) = tags.get(i) {
                                if !tag.is_empty() {
                                    to_open.push(tag.clone());
                                }
                            }
                        }
                    }
                }
                if let Some(gstring) = text_entry.get_text() {
                    let gstring = gstring.as_str();
                    if !gstring.is_empty() {
                        let gstring = gstring.trim();
                        if !gstring.is_empty() {
                            to_open.push(gstring.to_string());
                        }
                    }
                }
                match rapp_clone.try_borrow_mut() {
                    Ok(ref mut app) => {
                        app.events.push_back(app::Event::AddHosts(to_open));
                    }
                    Err(e) => {
                        // should be impossible since this gtk app is single threaded.
                        // and we're called from dialog, nothing else should have app borrowed.
                        eprintln!("failed to rapp.borrow_mut() in add_hosts {:?}", e);
                    }
                }
            }
        });
        self.bind_accelerator(&app.config.keymap.key_addhost, &hosts_add);
    }

    fn bind_accelerator(&self, accel: &str, menu_item: &MenuItem) {
        if accel.is_empty() {
            return;
        }
        let (mut key, mut modifier) = gtk::accelerator_parse(accel);

        if key == 0 {
            // parse failures return 0, 0.
            if let Some(accel) = tk2gtk::translate_accel(accel) {
                let (k, m) = gtk::accelerator_parse(&accel);
                key = k;
                modifier = m;
            }
            if key == 0 {
                eprintln!("Ignoring accelerator {} because it is not recognized by gtk::accelerator_parse()", accel);
                return;
            }
        }
        let group = gtk::AccelGroup::new();
        self.main_window.add_accel_group(&group);
        menu_item.add_accelerator("activate", &group, key, modifier, gtk::AccelFlags::VISIBLE);
    }

    fn populate_send_menu(&self, send: &MenuItem, app: &app::App, rapp: &app::Rapp) {
        send.set_submenu(Some(&self.send_menu));

        let send_macros = gtk::CheckMenuItem::new_with_label("Use Macros");
        send_macros.set_active(app.config.macros.enabled);
        let rapp_clone = rapp.clone();
        send_macros.connect_toggled(move |c| {
            let mut app = rapp_clone.borrow_mut();
            app.config.macros.enabled = c.get_active();
        });

        let send_servername = MenuItem::new_with_mnemonic("Remote Hostname");
        let send_hostname = MenuItem::new_with_mnemonic("Local Hostname");
        let send_username = MenuItem::new_with_mnemonic("Username");
        let send_test = MenuItem::new_with_mnemonic("Test Text");
        let send_random = MenuItem::new_with_mnemonic("Random Number");

        self.send_menu.append(&send_macros);
        self.send_menu.append(&send_servername);
        self.send_menu.append(&send_hostname);
        self.send_menu.append(&send_username);
        self.send_menu.append(&send_test);
        self.send_menu.append(&send_random);

        let rapp_clone = rapp.clone();
        let text = app.config.macros.servername.clone();
        send_servername.connect_activate(move |_| {
            rapp_clone.borrow_mut().send_text(&text);
        });

        let rapp_clone = rapp.clone();
        let text = app.config.macros.hostname.clone();
        send_hostname.connect_activate(move |_| {
            rapp_clone.borrow_mut().send_text(&text);
        });

        let rapp_clone = rapp.clone();
        let text = app.config.macros.username.clone();
        send_username.connect_activate(move |_| {
            rapp_clone.borrow_mut().send_text(&text);
        });

        let rapp_clone = rapp.clone();
        send_test.connect_activate(move |_| {
            rapp_clone.borrow_mut().send_text(&"Lorem Ipsum");
        });

        let rapp_clone = rapp.clone();
        send_random.connect_activate(move |_| {
            rapp_clone.borrow_mut().send_variable_text();
        });
    }

    pub fn change_main_window_title(&self, app: &app::App) {
        self.main_window.set_title(&format!(
            "{} [{}]",
            match app.config.dynamic.title {
                Some(ref title) => title,
                None => "",
            },
            app.get_n_servers()
        ));
    }

    pub fn hide_main_window(&mut self) {
        self.console.hide(&self.main_window)
    }

    pub fn show_main_window(&mut self) {
        self.console
            .show(&self.main_box, &self.main_window, &self.text_entry);
    }

    pub fn get_main_window_request_delay(&mut self) -> Option<u8> {
        match self.console {
            Console::HiddenBeforeFirstDraw(_) => Some(0),
            Console::Hidden(_, _) => Some(2),
            Console::Shown => None,
        }
    }

    pub fn capture_map_events(&self) {
        // perl cssh would call retile_hosts() if the console's state moved from
        // 'iconic' to anything else, during a map event (aka show console).
        // I cannot trigger that behavior in perl cssh.
        // I can minimize/maximize but I cannot trigger an 'iconic' state.
        // So I'm not going to try to reproduce it.
        // retile_host() is always available via hotkey, or hosts menu.
    }

    pub fn build_hosts_menu(&self, app: &mut app::App, rapp: &app::Rapp) {
        for (ref server_key, ref mut server) in app.servers.iter_mut() {
            self.build_host_menu(server_key, server, rapp);
        }
        self.change_main_window_title(app);
    }

    pub fn build_host_menu(&self, server_key: &str, server: &mut server::Server, rapp: &app::Rapp) {
        if server.menu_item.is_none() {
            let menu_item = gtk::CheckMenuItem::new_with_label(server_key);
            menu_item.set_active(true);
            let server_key = server_key.to_string(); // copy string so closure can own it.
            let rapp = rapp.clone();
            menu_item.connect_toggled(move |c| {
                // If this host is clicked in the hosts_menu,
                // then we can borrow rapp (because caller is gtk directly to us).
                // But, if menu options like "set all active" are clicked,
                // then that handler borrows rapp (so it can iterating over servers)
                // and when that iteration updates the UI, this gets triggered,
                // synchronously, where we try to borrow rapp again.  Bang BorrowMutError
                // So.. try_borrow_mut()
                // This sounds like a crude assumption, but KISS, gtk is single threaded,
                // and I don't want to wrap the bool in a Rc<RefCell<>> so it can be
                // referenced in this static callback.
                if let Ok(ref mut app) = rapp.try_borrow_mut() {
                    if let Some(ref mut server) = app.servers.get_mut(&server_key) {
                        server.active = c.get_active();
                    }
                }
            });
            self.hosts_menu.append(&menu_item);
            menu_item.show_all();
            server.menu_item = Some(menu_item);
        }
    }

    fn toggle_history(&mut self) {
        if self.text_entry_in_use {
            self.text_entry_in_use = false;
            self.main_box.add(&self.history_window);
            self.main_box.remove(&self.text_entry);
        } else {
            self.text_entry_in_use = true;
            self.main_box.add(&self.text_entry);
            self.main_box.remove(&self.history_window);
        }
        self.main_box.show_all();
    }
}

fn set_visual(window: &Window, _screen: &Option<Screen>) {
    // stolen from gtk-rs examples
    if let Some(screen) = window.get_screen() {
        if let Some(visual) = screen.get_rgba_visual() {
            window.set_visual(&visual); // crucial for transparency
        }
    }
}
