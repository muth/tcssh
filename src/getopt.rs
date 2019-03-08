use regex::Regex;
use std::borrow::Cow;
use std::path::PathBuf;
use structopt::StructOpt;

use crate::config;
use crate::er::Result;

#[derive(Debug, StructOpt)]
#[structopt(name = "Getopt", rename_all = "kebab-case")]
pub struct Getopt {
    /// Number of seconds to wait before closing finished terminal windows.
    #[structopt(short = "K", long = "autoclose")]
    auto_close: Option<String>, // "man sleep" accepts floats and optional suffix s m h d

    /// Use supplied file as additional cluster file.
    ///
    /// Accepts csv of files "--cluster-file file1,file2,file3"
    /// defaults /etc/clusters and $CONFIG_DIR/clusters
    /// where $CONFIG_DIR is either ~/.tcssh or ~/.clusterssh
    /// format is "tag host1 host2 ..."
    /// See src/cluster.rs for full example
    #[structopt(short = "c", long = "cluster-file")]
    cluster_file: Option<String>,

    // perl changed the CLI args available based upon $0 aka argv[0] (aka how executable is invoked)
    // that's a bit too dynamic for us.  So allow all and add validation to prevent nonsense.
    // available for ssh rsh, but not telnet or console
    /// Run the command in each session, e.g. "-a 'ping 1.1.1.1'" to run ping in each xterm
    #[structopt(short = "a", long = "action")]
    command: Option<String>,

    /// Use supplied file the configuration file.
    /// Defaults is $CONFIG_DIR/config
    /// where $CONFIG_DIR is either ~/.tcssh or ~/.clusterssh
    #[structopt(short = "C", long = "config-file")]
    config_file: Option<PathBuf>,

    // perl cssh allowed '--debug level' and multiple --debug options without args.
    // We cannot mimic that, and only have one level of debug.. so make it bool
    /// Debug
    #[structopt(long = "debug")]
    pub debug: bool,

    /// Dump the default configuration in the format used by ~/.tcssh/config
    #[structopt(short = "d", long = "dump-config")]
    pub dump_config: bool,

    /// Display and evaluate the terminal and connection arguments to display any potential errors.
    /// The <hostname> is required to aid the evaluation. [user@]<host>[:port]
    ///
    /// e.g. You try "tcmosh" and the xterm closes right away, too quickly to read the error
    /// so you want to debug it, so you may try "tcmosh --evaluate ::1" and that may tell you
    ///   "/usr/bin/mosh: Could not connect to ::1: Address family for hostname not supported"
    /// so you adjust and "tcmosh --evaluate 127.0.0.1" and that may tell you to change your LANG
    /// to something supporting utf8
    #[structopt(short = "e", long = "evaluate")]
    pub evaluate: Option<String>,

    /// Specify the font to use in the terminal windows. Use standard X font notation such as "5x8".
    #[structopt(short = "f", long = "font")]
    font: Option<String>,

    pub hosts: Vec<String>,

    // perl's GetOpt allows optional arguments.
    // so    'cssh --list'     lists available tags.
    // while 'cssh --list foo' lists the expansion of the tag 'foo'
    // I haven't found a way to make StructOpt to allow the above.
    // StructOpt uses clap, and clap::Arg has a fn takes_value(bool), not
    // a fn takes_value(some_enum_allowing_yes_no_or_optional)
    //
    // So we either take an arg or not, there is no way to have an optional argument
    // So   'tcssh --list=' or 'tcssh --list ""' for equivalent of 'cssh --list'
    /// If empty (-L '') then this lists available cluster tags, else the hosts for that tag are listed.  NOTE: format of output changes when using "--quiet" or "-Q" option.
    #[structopt(short = "L", long = "list")]
    pub list: Option<String>,

    /// Specify an alternate port for connections.
    #[structopt(short = "p", long = "port")]
    port: Option<u16>,

    /// Do not output extra text when using some options
    #[structopt(short = "Q", long = "quiet")]
    pub quiet: bool,

    /// Show history within console window.
    #[structopt(short = "s", long = "show-history")]
    show_history: bool,

    /// Sleep to allow window manager to catch up.  Default is false.  If true then tcssh sleeps like cssh.
    #[structopt(short = "S", long = "sleep")]
    sleep: bool,

    /// Specify arguments to be passed to ssh when making the connection.
    ///
    /// NOTE: options for ssh should normally be put into the ssh configuration file;
    /// see ssh_config and $HOME/.ssh/config for more details.'
    ///
    #[structopt(short = "o", long = "options")] // default "-x -o ConnectTimeout=10"
    ssh_args: Option<String>,

    /// Use supplied file as additional tag file.
    ///
    /// Accepts csv of files --tag-file file1,file2,file3
    /// defaults /etc/tags and $CONFIG_DIR/tags
    /// where $CONFIG_DIR is either ~/.tcssh or ~/.clusterssh
    /// format is "host tag1 tag2 ..."
    #[structopt(short = "r", long = "tag-file")]
    tag_file: Option<String>,

    /// Specify arguments to be passed to terminals being used.
    #[structopt(short = "t", long = "term-args")]
    term_args: Option<String>,

    /// Toggle window tiling (overriding the config file).
    #[structopt(short = "g", long = "tile")]
    tile: bool,

    /// Specify the initial part of the title used in the console and client windows.
    #[structopt(short = "T", long = "title")]
    title: Option<String>,

    /// Opacity. 1 = opaque, 0.5 = semi-transparent, 0 = transparent.
    #[structopt(short = "O", long = "opacity")]
    opacity: Option<f64>,

    /// Toggle connecting to each host only once when a hostname has been specified multiple times.
    #[structopt(short = "u", long = "unique-servers")]
    unique_servers: bool,

    /// If a hostname resolves to multiple IPs, then toggle connecting to all of them.
    #[structopt(short = "A", long = "use-all-a-records")]
    use_all_a_records: bool,
}

impl Getopt {
    pub fn setup(&self, config: &mut config::Config) -> Result<()> {
        // handle --config_file=foo, error out if foo does not exist
        if let Some(config_file) = &self.config_file {
            config::read_file(config, config_file)?;
        } else {
            // which config_dir are we using? $HOME/.tcssh or $HOME/.clusterssh
            // if config_dir/config exists, then try reading it.
            if let Some(ref mut config_file) = config.tcssh.get_config_dir() {
                config_file.push("config");
                if config_file.exists() {
                    config::read_file(config, config_file)?;
                }
            }
        }
        Ok(())
    }

    pub fn override_config_with_args(&self, config: &mut config::Config) -> Result<()> {
        // Now override config with getopt --args

        if let Some(auto_close) = &self.auto_close {
            // clone because Config.auto_close is Cow<'static> but Getopt is not 'static.
            config.misc.auto_close = Cow::Owned(auto_close.clone());
        }
        if let Some(cluster_file) = &self.cluster_file {
            let mut v = cluster_file.split(',').map(PathBuf::from).collect();
            config.misc.extra_cluster_file.append(&mut v);
        }
        if let Some(command) = &self.command {
            config.comms.command = Cow::Owned(command.clone());
        }
        if let Some(font) = &self.font {
            config.terminal.font = Cow::Owned(font.clone());
        }
        if self.show_history {
            config.misc.show_history = true;
        }
        if let Some(port) = self.port {
            config.misc.port = Some(format!("{}", port));
        }
        if self.sleep {
            config.tcssh.sleep = true;
        }
        if config.comms.ssh_args.is_empty() && self.ssh_args.is_none() {
            // inject default, (if nothing in config file and no --arg)
            config.comms.ssh_args = Cow::Borrowed("-x -o ConnectTimeout=10");
        } else if let Some(ssh_args) = &self.ssh_args {
            // else use --arg if it exists
            config.comms.ssh_args = Cow::Owned(ssh_args.clone());
        }
        if let Some(tag_file) = &self.tag_file {
            let mut v = tag_file.split(',').map(PathBuf::from).collect();
            config.misc.extra_tag_file.append(&mut v);
        }
        if let Some(term_args) = &self.term_args {
            config.terminal.args = Some(term_args.clone());

            // if term args matched /-class (\w+)/ then adjust config.allow_send_events.
            match Regex::new(r"-class (\w+)") {
                Err(e) => return Err(e.into()),
                Ok(re) => {
                    if let Some(cap) = re.captures(term_args) {
                        if let Some(item) = cap.get(1) {
                            config.terminal.allow_send_events = Cow::Owned(format!(
                                "-xrm '{}.VT100.allowSendEvents:true'",
                                item.as_str()
                            ));
                        }
                    }
                }
            }
        }
        if self.tile {
            config.misc.window_tiling = !config.misc.window_tiling;
        }
        if let Some(title) = &self.title {
            config.dynamic.title = Some(title.clone());
        }
        if let Some(opacity) = self.opacity {
            config.tcssh.set_opacity(opacity);
        }
        if self.unique_servers {
            config.misc.unique_servers = !config.misc.unique_servers;
        }
        if self.use_all_a_records {
            config.misc.use_all_a_records = !config.misc.use_all_a_records;
        }
        Ok(())
    }
}
