// Read config file (based on perl cssh).
//
// This means that we may parse stuff which perl cssh set,
// but if the members are not 'pub', then obviously they aren't used.
// TODO: rm non pub members, once all functionality is supported

use dirs;
use regex::Regex;
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use crate::er::Result;
use crate::host::STRICT_GEOMETRY;
use crate::is_xfile::IsExecutableFile;
use crate::reader;

lazy_static! {
    static ref TERM_SIZE: Regex = Regex::new(r"^(\d+)x(\d+)$").expect("Regex error TERM_SIZE");
    static ref SSH_CONFIG_META: Regex =
        Regex::new(r"[!*%?,]").expect("Regex error SSH_CONFIG_META");
}

#[derive(Debug, Default)]
pub struct Config {
    pub comms: Comms,
    pub dynamic: Dynamic,
    pub keymap: Keymap,
    pub macros: Macros,
    pub menu: Menu,
    pub misc: Misc,
    pub screen: Screen,
    pub terminal: Terminal,
    pub tcssh: Tcssh,
}

#[derive(Debug, Clone)]
pub enum CommsE {
    Console,
    Mosh,
    Rsh,
    Sftp,
    Ssh,
    Telnet,
    Invalid,
}

// Maybe turn this into an enum?
#[derive(Debug, Clone)]
pub struct Comms {
    pub comms: CommsE,
    pub command: Cow<'static, str>,
    console: Cow<'static, str>,
    console_args: Cow<'static, str>,
    mosh: Cow<'static, str>,
    mosh_args: Cow<'static, str>,
    rsh: Cow<'static, str>,
    rsh_args: Cow<'static, str>,
    telnet: Cow<'static, str>,
    telnet_args: Cow<'static, str>,
    ssh: Cow<'static, str>,
    pub ssh_args: Cow<'static, str>,
    sftp: Cow<'static, str>,
    sftp_args: Cow<'static, str>,
    //user: Cow<'static, str>,
}

impl Default for Comms {
    fn default() -> Self {
        Self {
            comms: CommsE::Invalid,
            command: Cow::Borrowed(""),
            console: Cow::Borrowed("console"),
            console_args: Cow::Borrowed(""),
            mosh: Cow::Borrowed("mosh"),
            mosh_args: Cow::Borrowed(""),
            rsh: Cow::Borrowed("rsh"),
            rsh_args: Cow::Borrowed(""),
            telnet: Cow::Borrowed("telnet"),
            telnet_args: Cow::Borrowed(""),
            ssh: Cow::Borrowed("ssh"),
            ssh_args: Cow::Borrowed(""),
            sftp: Cow::Borrowed("sftp"),
            sftp_args: Cow::Borrowed(""),
            //user: Cow::Borrowed(""),
        }
    }
}

// Every thing which was not in perl cssh's %default_config initialization
// but was added dynamically by bits of code everywhere.
#[derive(Debug, Clone, Default)]
pub struct Dynamic {
    pub username: Option<String>, // TODO, no setters!
    pub title: Option<String>,    // from arg0
}

#[derive(Debug)]
pub struct Terminal {
    pub allow_send_events: Cow<'static, str>,
    pub args: Option<String>,
    pub bg_style_dark: bool,
    pub colorize: bool,
    pub decoration_height: u32,
    pub decoration_width: u32,
    pub font: Cow<'static, str>,
    pub reserve_bottom: u32,
    pub reserve_left: u32,
    pub reserve_right: u32,
    pub reserve_top: u32,
    terminal_size: Cow<'static, str>,
    pub terminal_size_x: u32,
    pub terminal_size_y: u32,
    terminal_exists: Option<bool>,
    pub terminal_name: Cow<'static, str>, // perl cssh calls this config->{terminal}, everything else was terminal_*
    pub title_opt: Cow<'static, str>,
}

impl Default for Terminal {
    fn default() -> Self {
        Self {
            allow_send_events: Cow::Borrowed("-xrm '*.VT100.allowSendEvents:true'"),
            args: None,
            bg_style_dark: true,
            colorize: true,
            decoration_height: 10,
            decoration_width: 8,
            //font: Cow::Borrowed("9x15bold"),
            //font: Cow::Borrowed("8x16"),
            font: Cow::Borrowed("6x13"),
            reserve_bottom: 0,
            reserve_left: 5,
            reserve_right: 0,
            reserve_top: 5,
            terminal_size: Cow::Borrowed("80x24"),
            terminal_size_x: 80, // parsed from "80x24" above
            terminal_size_y: 24, // parsed from "80x24" above
            terminal_exists: None,
            terminal_name: Cow::Borrowed("xterm"),
            title_opt: Cow::Borrowed("-T"),
        }
    }
}

#[derive(Debug)]
pub struct Macros {
    pub enabled: bool, // perl cssh calls this config->{macros_enabled}, everything else was macro_*
    pub servername: Cow<'static, str>,
    pub hostname: Cow<'static, str>,
    pub username: Cow<'static, str>,
    pub newline: Cow<'static, str>,
    pub version: Cow<'static, str>,
    pub servername_re: Option<Regex>,
    pub hostname_re: Option<Regex>,
    pub username_re: Option<Regex>,
    pub newline_re: Option<Regex>,
    pub version_re: Option<Regex>,
    pub all_re: Option<Regex>,
}

impl Default for Macros {
    // Clippy warns about trivial re-exes r"%s" etc.
    // But these are user configurable, so may be arbitrarily complex.
    #[allow(clippy::trivial_regex)]
    fn default() -> Self {
        Self {
            enabled: true,
            servername: Cow::Borrowed("%s"),
            hostname: Cow::Borrowed("%h"),
            username: Cow::Borrowed("%u"),
            newline: Cow::Borrowed("%n"),
            version: Cow::Borrowed("%v"),
            servername_re: Some(Regex::new(r"%s").unwrap()),
            hostname_re: Some(Regex::new(r"%h").unwrap()),
            username_re: Some(Regex::new(r"%u").unwrap()),
            newline_re: Some(Regex::new(r"%n").unwrap()),
            version_re: Some(Regex::new(r"%v").unwrap()),
            all_re: Some(Regex::new(r"%[shunv]").unwrap()),
        }
    }
}

impl Macros {
    fn re_helper(&mut self, value: &str) -> Option<Regex> {
        self.all_re = None;
        if value.is_empty() {
            None
        } else if let Ok(re) = Regex::new(value) {
            Some(re)
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct Screen {
    pub reserve_top: u32,
    pub reserve_bottom: u32,
    pub reserve_left: u32,
    pub reserve_right: u32,
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            reserve_top: 0,
            reserve_bottom: 60,
            reserve_left: 0,
            reserve_right: 0,
        }
    }
}

#[derive(Debug)]
pub struct Keymap {
    pub use_hotkeys: bool,
    pub key_addhost: Cow<'static, str>,
    key_clientname: Cow<'static, str>,
    pub key_history: Cow<'static, str>,
    key_localname: Cow<'static, str>,
    key_macros_enable: Cow<'static, str>,
    pub key_paste: Cow<'static, str>,
    pub key_quit: Cow<'static, str>,
    pub key_raise_hosts: Cow<'static, str>,
    pub key_retile_hosts: Cow<'static, str>,
    //key_username: Cow<'static, str>, // unused
    //mouse_paste: Cow<'static, str>, // unused
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            use_hotkeys: true,
            key_addhost: Cow::Borrowed("<Control><Shift>plus"),
            key_clientname: Cow::Borrowed("<Alt>n"),
            key_history: Cow::Borrowed("<Alt>h"),
            key_localname: Cow::Borrowed("<Alt>l"),
            key_macros_enable: Cow::Borrowed("<Alt>p"),
            key_paste: Cow::Borrowed("<Control>v"),
            key_quit: Cow::Borrowed("<Alt>q"),
            key_raise_hosts: Cow::Borrowed("<Alt>i"),
            key_retile_hosts: Cow::Borrowed("<Alt>r"),
            //key_username: Cow::Borrowed("<Alt>u"),
            //mouse_paste: Cow::Borrowed("<Button>2"),
        }
    }
}

#[derive(Debug)]
pub struct Menu {
    pub max_addhost_menu_cluster_items: u8,
    //max_host_menu_items: u8, // unused
    //menu_host_autotearoff: u8, // unused
    //menu_send_autotearoff: u8, // unused
    //send_menu_xml_file: PathBuf, // unused
}

impl Default for Menu {
    fn default() -> Self {
        //let xml = PathBuf::from(env::var_os("HOME").unwrap_or_else(|| "".into()))
        //    .join("/.tcssh/send_menu");

        Self {
            max_addhost_menu_cluster_items: 6,
            //max_host_menu_items: 30,
            //menu_host_autotearoff: 0,
            //menu_send_autotearoff: 0,
            //send_menu_xml_file: xml,
        }
    }
}

#[derive(Debug)]
pub struct Misc {
    pub auto_close: Cow<'static, str>,
    pub auto_quit: bool,
    pub console_position: Option<String>,
    pub external_cluster_command: Option<PathBuf>,
    pub extra_cluster_file: Vec<PathBuf>,
    pub extra_tag_file: Vec<PathBuf>,
    pub history_height: u16,
    pub history_width: u16,
    pub port: Option<String>,
    pub show_history: bool,
    pub unique_servers: bool,
    pub unmap_on_redraw: bool,
    pub use_all_a_records: bool,
    //use_natural_sort: bool, // unused
    pub window_tiling: bool,
    pub window_tiling_right: bool,
}

impl Default for Misc {
    fn default() -> Self {
        Self {
            auto_close: Cow::Borrowed("5"),
            auto_quit: true,
            console_position: None,
            external_cluster_command: None,
            extra_cluster_file: Vec::new(),
            extra_tag_file: Vec::new(),
            history_height: 10,
            history_width: 40,
            port: None,
            show_history: false,
            unmap_on_redraw: false,
            unique_servers: false,
            use_all_a_records: false,
            //use_natural_sort: false,
            window_tiling: true,
            window_tiling_right: true,
        }
    }
}

#[derive(Debug)]
enum CheckedPathBuf {
    DoesNotExist,
    Exists(PathBuf),
}

#[derive(Debug)]
pub struct Tcssh {
    config_dir: Option<CheckedPathBuf>,
    pub opacity: f64,
    pub sleep: bool,
    pub transparent: bool,
}

impl Default for Tcssh {
    fn default() -> Self {
        Self {
            config_dir: None,
            opacity: 0.25f64,
            sleep: false,
            transparent: true,
        }
    }
}

impl Tcssh {
    pub fn set_opacity(&mut self, opacity: f64) {
        if opacity >= 1.0 {
            self.transparent = false;
            self.opacity = 1.0;
        } else {
            self.transparent = true;
            if opacity < 0.0 {
                self.opacity = 0.0;
            } else {
                self.opacity = opacity;
            }
        }
    }
}

impl Tcssh {
    pub fn get_config_dir(&mut self) -> Option<PathBuf> {
        // first time through?  Lets check the file system
        if self.config_dir.is_none() {
            if let Some(dir) = &mut dirs::home_dir() {
                dir.push(".tcssh");
                if dir.is_dir() {
                    self.config_dir = Some(CheckedPathBuf::Exists(dir.to_path_buf()));
                } else {
                    dir.pop();
                    dir.push(".clusterssh");
                    if dir.is_dir() {
                        self.config_dir = Some(CheckedPathBuf::Exists(dir.to_path_buf()));
                    } else {
                        self.config_dir = Some(CheckedPathBuf::DoesNotExist);
                    }
                }
            } else {
                self.config_dir = Some(CheckedPathBuf::DoesNotExist);
            }
        }
        match &self.config_dir {
            Some(checked_path_buf) => match checked_path_buf {
                CheckedPathBuf::DoesNotExist => None,
                CheckedPathBuf::Exists(path_buf) => Some(path_buf.clone()),
            },
            None => None,
        }
    }
    pub fn sleep(&self, ms: u64) {
        // perl cssh had sleeps all over the place,
        // with a usual comment "WMs are slow"
        // And since perl cssh has many years in the field,
        // with many bugs reported and fixed, I know there are systems
        // which rely on that timing..  but we'll turn off the
        // sleeps by default, and let them be enabled by config or --sleep
        thread::sleep(Duration::from_millis(ms));
    }
}

impl Config {
    pub fn setup(&mut self, arg0: &str) -> Result<()> {
        let arg0_fname = match Path::new(arg0).file_name() {
            Some(fname) => fname.to_string_lossy(), // loss is ok, invalid are mapped to ssh
            _ => Cow::Borrowed(""),
        };

        // This was more legible with strings.
        // e.g. "ccon" => "console" instead of CommsE:: noise :/
        let comms = match arg0_fname.as_ref() {
            // same mappings like perl cssh, plus t prefixes
            "cssh" | "clusterssh" | "tcssh" | "tclusterssh" => CommsE::Ssh,
            "cmosh" | "clustermosh" | "tcmosh" | "tclustermosh" => CommsE::Mosh,
            "ccon" | "cconsole" | "tccon" | "tconsole" => CommsE::Console,
            "ctel" | "ctelnet" | "tctel" | "tctelnet" => CommsE::Telnet,
            "crsh" | "tcrsh" => CommsE::Rsh,
            "csftp" | "tcsftp" => CommsE::Sftp,
            _ => CommsE::Ssh,
        };
        self.comms.comms = comms;
        self.dynamic.title = Some(arg0_fname.to_uppercase());

        check_terminal(self)?;

        Ok(())
    }

    pub fn get_script_args(&self) -> (&str, &str, &str, &str) {
        let (comms, comms_args) = match self.comms.comms {
            CommsE::Console => (&self.comms.console, &self.comms.console_args),
            CommsE::Mosh => (&self.comms.mosh, &self.comms.mosh_args),
            CommsE::Rsh => (&self.comms.rsh, &self.comms.rsh_args),
            CommsE::Sftp => (&self.comms.sftp, &self.comms.sftp_args),
            CommsE::Ssh => (&self.comms.ssh, &self.comms.ssh_args),
            CommsE::Telnet => (&self.comms.telnet, &self.comms.telnet_args),
            CommsE::Invalid => panic!("Config has no mapping for comms"),
        };
        (
            comms,
            comms_args,
            &self.comms.command,
            &self.misc.auto_close,
        )
    }
}

// try to find the path of 'xterm' (or whatever override we have in terminal_name)
fn check_terminal(config: &mut Config) -> Result<()> {
    // perl cssh called this* twice (*=Config.pm sub validate_args)
    // Once during initialization (via Config.pm sub new) and again after reading
    // a config file (sub load_configs) which may overwrite terminal_name
    //
    // We did the same early on, and cached the results in terminal_exists.
    // But now we only call this once.  So, it's kind of excessive to cache it.
    if config.terminal.terminal_exists.is_some() {
        return Ok(());
    }

    if config.terminal.terminal_name.is_empty() {
        return Err("missing terminal_name".into());
    }

    // to write back to 'terminal_name' we have to out live the borrow for 'binary'
    let mut new_term: Option<String> = None;
    {
        // absolute -> relative
        let binary = Path::new(config.terminal.terminal_name.as_ref());
        let relative = if binary.is_absolute() {
            if binary.is_executable_file() {
                config.terminal.terminal_exists = Some(true);
                return Ok(());
            } else if let Some(file_name) = binary.file_name() {
                Path::new(file_name)
            } else {
                return Err("Invalid terminal_name".into());
            }
        } else {
            binary
        };

        // search $ENV{PATH} This almost always contains 'xterm'
        if let Some(path) = env::var_os("PATH") {
            for mut p in env::split_paths(&path) {
                p.push(relative);
                if p.is_executable_file() {
                    if let Some(s) = p.to_str() {
                        new_term = Some(s.to_string());
                        break;
                    }
                    // else path isn't utf8, well.. tough, we need
                    // a string because we concatenate this file
                    // with other stuff when making the cmd line
                    // we send to execlp().
                }
            }
        }
        if new_term.is_none() {
            // no terminal_name in PATH, well perl cssh fell back
            // to searching through this list of dirs.
            let mut v: Vec<PathBuf> = vec![
                "/bin",
                "/sbin",
                "/usr/sbin",
                "/usr/bin",
                "/usr/local/bin",
                "/usr/local/sbin",
                "/opt/local/bin",
                "/opt/local/sbin",
            ]
            .into_iter()
            .map(PathBuf::from)
            .collect();

            // filter out PATH elements we've already tried
            if let Some(path) = env::var_os("PATH") {
                let mut seen = BTreeSet::new();
                for p in env::split_paths(&path) {
                    seen.insert(p);
                }
                v = v.into_iter().filter(|x| !seen.contains(x)).collect();
            }
            // Ok back to searching those directories.
            // This loop is common with a loop above. but it's short.
            //
            // Almost everyone has xterm is in PATH, so the code, as is,
            // is optimized for that case first.  Only if that's not true
            // do we bother with the extra alloctions for 'v' and 'seen'.
            //
            // Alternatively using a helper fn for the iteration isn't great,
            // because the loops iterate over different types, so they'd be
            // monomorphised back to seperate code.
            //
            // So repeat the loop and flag it with this comment,
            // so others don't think the DRY smell is thoughtless.
            for dir in v {
                let p = dir.join(relative);
                if p.is_executable_file() {
                    if let Some(s) = p.to_str() {
                        new_term = Some(s.to_string());
                        break;
                    }
                }
            }
        }
    } // end borrow of config.terminal.terminal_name
      // found something! Update config.
    if let Some(t) = new_term {
        config.terminal.terminal_exists = Some(true);
        config.terminal.terminal_name = Cow::from(t);
    } else {
        return Err("No valid terminal_name".into());
    }

    Ok(())
}

pub fn read_file(config: &mut Config, filename: &PathBuf) -> Result<()> {
    reader::read_file(filename, true, |key, value| {
        update_config(config, key, value);
    })?;

    Ok(())
}

fn update_config(config: &mut Config, key: &str, value: &str) {
    match key {
        "auto_close" => config.misc.auto_close = Cow::Owned(String::from(value)),

        // perl cssh defaults to "yes" and checked /yes/i
        // I don't like the allocation of to_ascii_lowercase(),
        // but the alternative of using a regex, leads to putting it within a lazy_static! macro,
        // which does not let us tell clippy to #allow(clippy::trivial_regex)]
        // using /ye[s]/ avoid the clippy warning, but seems kludgy.
        "auto_quit" => {
            config.misc.auto_quit =
                value.contains("yes") || value.to_ascii_lowercase().contains("yes")
        }

        // "command" => {} // command is not parsed from config, but it works on CLI. perl; 'cssh -a ls ::1'
        // "comms" => {}, // command, comms and title are not parsed from config.
        "console" => config.comms.console = Cow::Owned(String::from(value)),
        "console_args" => config.comms.console_args = Cow::Owned(String::from(value)),
        "console_position" => {
            if value.is_empty() {
                config.misc.console_position = None;
            } else if STRICT_GEOMETRY.is_match(value) {
                config.misc.console_position = Some(String::from(value));
            } else {
                eprintln!(
                    "Warn: Ignoring config value for console_position ({})",
                    value
                );
            }
        }
        // "debug" => {} // not read from config in tcssh, just CLI
        "external_cluster_command" => {
            config.misc.external_cluster_command = Some(PathBuf::from(value));
        }
        "extra_cluster_file" => {
            config.misc.extra_cluster_file = value.split(',').map(PathBuf::from).collect()
        }
        // perl cssh didn't have extra_tag_file in it's config.
        // it always relied on --tag-file argument
        //		"extra_tag_file" => config.misc.extra_tag_file = value.split(',').map(PathBuf::from).collect(),
        "history_height" => {
            if let Ok(value) = u16::from_str_radix(value, 10) {
                if value != 0 {
                    config.misc.history_height = value;
                }
            }
        }
        "history_width" => {
            if let Ok(value) = u16::from_str_radix(value, 10) {
                if value != 0 {
                    config.misc.history_width = value;
                }
            }
        }
        // Some of these keys aren't used yet.
        "key_addhost" => config.keymap.key_addhost = Cow::Owned(String::from(value)),
        "key_clientname" => config.keymap.key_clientname = Cow::Owned(String::from(value)),
        "key_history" => config.keymap.key_history = Cow::Owned(String::from(value)),
        "key_localname" => config.keymap.key_localname = Cow::Owned(String::from(value)),
        "key_macros_enable" => config.keymap.key_macros_enable = Cow::Owned(String::from(value)),
        "key_paste" => config.keymap.key_paste = Cow::Owned(String::from(value)),
        "key_quit" => config.keymap.key_quit = Cow::Owned(String::from(value)),
        "key_raise_hosts" => config.keymap.key_raise_hosts = Cow::Owned(String::from(value)), // perl cssh didn't read raise?
        "key_retilehosts" => config.keymap.key_retile_hosts = Cow::Owned(String::from(value)), // note _ missing in cfg
        //"key_username" => config.keymap.key_username = Cow::Owned(String::from(value)),

        //"lang" => {} // No L10N/I18N support
        "macro_hostname" => config.macros.hostname_re = config.macros.re_helper(value),
        "macro_newline" => config.macros.newline_re = config.macros.re_helper(value),
        "macro_servername" => config.macros.servername_re = config.macros.re_helper(value),
        "macro_username" => config.macros.username_re = config.macros.re_helper(value),
        "macro_version" => config.macros.version_re = config.macros.re_helper(value),

        // perl cssh defaulted to "yes" and checked eq 'yes'
        "macros_enabled" => config.macros.enabled = value == "yes",

        // None of these are used yet
        "max_addhost_menu_cluster_items" => {
            u8_parse(value, &mut config.menu.max_addhost_menu_cluster_items)
        }
        //"max_host_menu_items" => u8_parse(value, &mut config.menu.max_host_menu_items), // unused
        //"menu_host_autotearoff" => u8_parse(value, &mut config.menu.menu_host_autotearoff), // unused
        //"menu_send_autotearoff" => u8_parse(value, &mut config.menu.menu_send_autotearoff), // unused
        //"send_menu_xml_file" => config.menu.send_menu_xml_file = PathBuf::from(value), // unused
        //"mouse_paste" => config.keymap.mouse_paste = Cow::Owned(String::from(value)), // unused
        "opacity" => {
            if let Ok(value) = f64::from_str(value) {
                config.tcssh.set_opacity(value);
            }
        }

        "rsh" => config.comms.rsh = Cow::Owned(String::from(value)),
        "rsh_args" => config.comms.rsh_args = Cow::Owned(String::from(value)),

        "screen_reserve_bottom" => u32_parse(value, &mut config.screen.reserve_bottom),
        "screen_reserve_left" => u32_parse(value, &mut config.screen.reserve_left),
        "screen_reserve_right" => u32_parse(value, &mut config.screen.reserve_right),
        "screen_reserve_top" => u32_parse(value, &mut config.screen.reserve_top),

        // perl cssh defaulted to 0 and checked perl true.
        "show_history" => config.misc.show_history = perl_true(value),

        "sleep_enabled" => {
            config.tcssh.sleep =
                value.contains("yes") || value.to_ascii_lowercase().contains("yes");
        }

        "ssh" => config.comms.ssh = Cow::Owned(String::from(value)),
        "ssh_args" => config.comms.ssh_args = Cow::Owned(String::from(value)),
        "sftp" => config.comms.sftp = Cow::Owned(String::from(value)),
        "sftp_args" => config.comms.sftp_args = Cow::Owned(String::from(value)),
        "telnet" => config.comms.telnet = Cow::Owned(String::from(value)),
        "telnet_args" => config.comms.telnet_args = Cow::Owned(String::from(value)),

        //        "terminal" => {}
        "terminal_allow_send_events" => {
            config.terminal.allow_send_events = Cow::Owned(String::from(value))
        }
        "terminal_args" => {
            config.terminal.args = if value.is_empty() {
                None
            } else {
                Some(String::from(value))
            }
        }
        // perl cssh defaulted to 'dark' and checked eq 'dark'
        "terminal_bg_style" => config.terminal.bg_style_dark = "dark" == value,

        // perl cssh defaulted to 1 and checked perl true.
        "terminal_colorize" => config.terminal.colorize = perl_true(value),

        "terminal_decoration_height" => u32_parse(value, &mut config.terminal.decoration_height),
        "terminal_decoration_width" => u32_parse(value, &mut config.terminal.decoration_width),

        "terminal_font" => config.terminal.font = Cow::Owned(String::from(value)),

        "terminal_name" => {
            if !value.is_empty() {
                if config.terminal.terminal_exists.is_some()
                    && value != config.terminal.terminal_name
                {
                    // clear flag which caches the bool indicating if terminal_name
                    // is an executable file in PATH
                    config.terminal.terminal_exists = None
                }
                config.terminal.terminal_name = Cow::Owned(String::from(value));
            }
        }

        "terminal_reserve_bottom" => u32_parse(value, &mut config.terminal.reserve_bottom),
        "terminal_reserve_left" => u32_parse(value, &mut config.terminal.reserve_left),
        "terminal_reserve_right" => u32_parse(value, &mut config.terminal.reserve_right),
        "terminal_reserve_top" => u32_parse(value, &mut config.terminal.reserve_top),

        "terminal_size" => {
            if !value.is_empty() {
                if let Some(cap) = TERM_SIZE.captures(value) {
                    if let (Some(x), Some(y)) = (cap.get(1), cap.get(2)) {
                        if let Ok(x) = u32::from_str_radix(x.as_str(), 10) {
                            if let Ok(y) = u32::from_str_radix(y.as_str(), 10) {
                                if x != 0 && y != 0 {
                                    config.terminal.terminal_size_x = x;
                                    config.terminal.terminal_size_y = y;
                                    config.terminal.terminal_size = Cow::Owned(String::from(value));
                                }
                            }
                        }
                    }
                }
            }
        }

        "terminal_title_opt" => config.terminal.title_opt = Cow::Owned(String::from(value)),

        // "title" => {}, // command, comms and title are not parsed from config.

        // perl cssh defaulted to "no" checked /yes/i
        "unmap_on_redraw" => {
            config.misc.unmap_on_redraw =
                value.contains("yes") || value.to_ascii_lowercase().contains("yes")
        }

        // perl cssh defaulted to 0 checked perl true
        "use_all_a_records" => config.misc.use_all_a_records = perl_true(value),

        // perl cssh defaulted to "yes" and checked eq 'yes'
        "use_hotkeys" => config.keymap.use_hotkeys = value == "yes",

        //"user" => {} // perl skipped user in config, it only set it from getopt

        // perl cssh defaulted to "yes" and checked ne 'yes' and eq 'yes'
        // But getopt checked for perl_true, and defaulted to 0 so be mindful of arg parsing
        "window_tiling" => config.misc.window_tiling = value == "yes",

        // perl cssh defaulted to "right" and checked /right/i
        "window_tiling_direction" => {
            config.misc.window_tiling_right =
                value.contains("right") || value.to_ascii_lowercase().contains("right");
        }
        _ => {}
    }
}

struct OutConfig {
    buf: String,
}

impl OutConfig {
    // add yes no
    fn ayn(&mut self, key: &str, value: bool) {
        self.buf += key;
        if value {
            self.buf += "yes";
        } else {
            self.buf += "no";
        }
        self.buf += "\n";
    }
    // add 0 1
    fn a01(&mut self, key: &str, value: bool) {
        self.buf += key;
        if value {
            self.buf += "1";
        } else {
            self.buf += "0";
        }
        self.buf += "\n";
    }
}

trait Add<T> {
    fn add(&mut self, key: &str, value: T);
}

impl Add<&str> for OutConfig {
    fn add(&mut self, key: &str, value: &str) {
        self.buf += key;
        self.buf += value;
        self.buf += "\n";
    }
}

impl Add<&Cow<'static, str>> for OutConfig {
    fn add(&mut self, key: &str, value: &Cow<'static, str>) {
        self.buf += key;
        self.buf += value;
        self.buf += "\n";
    }
}

impl Add<&Option<String>> for OutConfig {
    fn add(&mut self, key: &str, value: &Option<String>) {
        self.buf += key;
        if let Some(value) = value {
            self.buf += value;
        }
        self.buf += "\n";
    }
}

impl Add<&Option<PathBuf>> for OutConfig {
    fn add(&mut self, key: &str, value: &Option<PathBuf>) {
        self.buf += key;
        if let Some(value) = value {
            self.buf += &value.to_string_lossy();
        }
        self.buf += "\n";
    }
}

pub fn dump_config(config: &Config) {
    let mut cfg = OutConfig {
        buf: String::with_capacity(2048),
    };

    cfg.add("auto_close=", &config.misc.auto_close);
    cfg.ayn("auto_quit=", config.misc.auto_quit);
    cfg.add("console=", &config.comms.console);
    cfg.add("console_args=", &config.comms.console_args);
    cfg.add("console_position=", &config.misc.console_position);
    cfg.add(
        "external_cluster_command=",
        &config.misc.external_cluster_command,
    );

    let tmp: Vec<String> = config
        .misc
        .extra_cluster_file
        .iter()
        .map(|x| x.to_string_lossy().into_owned())
        .collect();
    cfg.add("extra_cluster_file=", tmp.join(",").as_str());

    cfg.add(
        "history_height=",
        format!("{}", config.misc.history_height).as_str(),
    );
    cfg.add(
        "history_width=",
        format!("{}", config.misc.history_width).as_str(),
    );

    cfg.add("key_addhost=", &config.keymap.key_addhost);
    cfg.add("key_clientname=", &config.keymap.key_clientname);
    cfg.add("key_history=", &config.keymap.key_history);
    cfg.add("key_localname=", &config.keymap.key_localname);
    cfg.add("key_macros_enable=", &config.keymap.key_macros_enable);
    cfg.add("key_paste=", &config.keymap.key_paste);
    cfg.add("key_quit=", &config.keymap.key_quit);
    cfg.add("key_raise_hosts=", &config.keymap.key_raise_hosts);
    cfg.add("key_retilehosts=", &config.keymap.key_retile_hosts);

    cfg.add("macro_hostname=", &config.macros.hostname);
    cfg.add("macro_newline=", &config.macros.newline);
    cfg.add("macro_servername=", &config.macros.servername);
    cfg.add("macro_username=", &config.macros.username);
    cfg.add("macro_version=", &config.macros.version);

    cfg.ayn("macros_enabled=", config.macros.enabled);

    cfg.add(
        "max_addhost_menu_cluster_items=",
        format!("{}", config.menu.max_addhost_menu_cluster_items).as_str(),
    );

    cfg.add("opacity=", format!("{}", config.tcssh.opacity).as_str());

    cfg.add("rsh=", &config.comms.rsh);
    cfg.add("rsh_args=", &config.comms.rsh_args);

    cfg.add(
        "screen_reserve_bottom=",
        format!("{}", config.screen.reserve_bottom).as_str(),
    );
    cfg.add(
        "screen_reserve_left=",
        format!("{}", config.screen.reserve_left).as_str(),
    );
    cfg.add(
        "screen_reserve_right=",
        format!("{}", config.screen.reserve_right).as_str(),
    );
    cfg.add(
        "screen_reserve_top=",
        format!("{}", config.screen.reserve_top).as_str(),
    );

    cfg.add("sftp=", &config.comms.sftp);
    cfg.add("sftp_args=", &config.comms.sftp_args);

    cfg.a01("show_history=", config.misc.show_history);
    cfg.a01("sleep_enabled=", config.tcssh.sleep);

    cfg.add("ssh=", &config.comms.ssh);
    cfg.add("ssh_args=", &config.comms.ssh_args);
    cfg.add("telnet=", &config.comms.telnet);
    cfg.add("telnet_args=", &config.comms.telnet_args);

    cfg.add(
        "terminal_allow_send_events=",
        &config.terminal.allow_send_events,
    );
    cfg.add("terminal_args=", &config.terminal.args);

    let tmp = if config.terminal.bg_style_dark {
        "dark"
    } else {
        ""
    };
    cfg.add("terminal_bg_style=", tmp);

    cfg.a01("terminal_colorize=", config.terminal.colorize);
    cfg.add(
        "terminal_decoration_height=",
        format!("{}", config.terminal.decoration_height).as_str(),
    );
    cfg.add(
        "terminal_decoration_width=",
        format!("{}", config.terminal.decoration_width).as_str(),
    );

    cfg.add("terminal_font=", &config.terminal.font);
    cfg.add("terminal_name=", &config.terminal.terminal_name);

    cfg.add(
        "terminal_reserve_bottom=",
        format!("{}", config.terminal.reserve_bottom).as_str(),
    );
    cfg.add(
        "terminal_reserve_left=",
        format!("{}", config.terminal.reserve_left).as_str(),
    );
    cfg.add(
        "terminal_reserve_right=",
        format!("{}", config.terminal.reserve_right).as_str(),
    );
    cfg.add(
        "terminal_reserve_top=",
        format!("{}", config.terminal.reserve_top).as_str(),
    );
    cfg.add("terminal_size=", &config.terminal.terminal_size);
    cfg.add("terminal_title_opt=", &config.terminal.title_opt);
    cfg.ayn("unmap_on_redraw=", config.misc.unmap_on_redraw);
    cfg.a01("use_all_a_records=", config.misc.use_all_a_records);
    cfg.ayn("use_hotkeys=", config.keymap.use_hotkeys);
    cfg.ayn("window_tiling=", config.misc.window_tiling);

    let tmp = if config.misc.window_tiling_right {
        "right"
    } else {
        ""
    };
    cfg.add("window_tiling_direction=", tmp);

    print!("{}", cfg.buf);
}

fn u32_parse(value: &str, it: &mut u32) {
    if let Ok(value) = u32::from_str_radix(value, 10) {
        *it = value;
    }
}

fn u8_parse(value: &str, it: &mut u8) {
    if let Ok(value) = u8::from_str_radix(value, 10) {
        *it = value;
    }
}

fn perl_true(value: &str) -> bool {
    // perl false (in str context) is "" or "0"
    // perl true  (in str context) is "00", "0x0", " ", any other str
    !(value.is_empty() || value == "0")
}

pub fn parse_ssh_config_and_add_hosts(tags: &mut Vec<String>) {
    if let Some(dir) = &mut dirs::home_dir() {
        dir.push(".ssh");
        if dir.is_dir() {
            dir.push("config");
            let file = dir;
            if file.exists() {
                let mut stags = Vec::new();
                match reader::read_file(file, false, |key, value| match key {
                    "Host" => {
                        for value in value.split_whitespace() {
                            if !SSH_CONFIG_META.is_match(value) {
                                stags.push(value.to_string());
                            }
                        }
                    }
                    "HostName" => {
                        if !SSH_CONFIG_META.is_match(value) {
                            stags.push(value.to_string());
                        }
                    }
                    _ => {}
                }) {
                    Ok(_) => {
                        if !stags.is_empty() {
                            tags.append(&mut stags);
                            tags.sort_unstable();
                            tags.dedup();
                        }
                    }
                    Err(e) => {
                        eprintln!("Error parsing ~/.ssh/config {:?}", e);
                    }
                }
            }
        }
    }
}
