// This mod does 3 things
//
// 1)
// Clusters allow you to make short name aliases for one or more boxes.
// e.g.
//     $ cat ~/.tcssh/clusters
//     foo host1.example.com host2.example.com
//     bar host1.example.com host2.example.com
//     baz other.example.com
//
//     $ cat ~/.tcssh/tags
//     host3.example.com bar baz
//
//     $ tcssh foo # opens xterms to host[12].example.com
//     $ tcssh bar # opens xterms to host[123].example.com
//     $ tcssh baz # opens xterms to {other,host3}.example.com
//
// 2)
// This mod also allows expanding hosts to multiple IPs (--use-all-a-records)
// Which is only useful, if you know 'host' resolves to multiple IPs.
// If a host only resolves to one IP, then --use-all-a-records does nothing.
// e.g.
//     $ cat /etc/hosts
//     127.0.0.1 foo
//     127.0.0.2 foo
//     127.0.0.3 foo
//     $ tcssh foo                     # opens 1 xterm; to one random IP of the three
//     $ tcssh foo --use-all-a-records # opens 3 xterms; 127.0.0.1, 127.0.0.2, 127.0.0.3
//
// 3)
// This mod also handles filtering hosts via an external command.
// (Set via config file with key "external_cluster_command")
// Quoting the perl cssh documentation.
//     "The script must accept a list of tags to resolve and output a list of
//     hosts (space separated on a single line).  Any tags that cannot be
//     resolved should be returned unchanged.
//
//     A non-0 exit code will be counted as an error, a warning will be
//     printed and output ignored.  If the external command is given a -L option
//     it should output a list of tags (space separated on a single line)
//     it can resolve."
//

use regex::Regex;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use crate::config;
use crate::er::Result;
use crate::is_xfile::IsExecutableFile;
use crate::reader;
use crate::resolver;
use crate::wait_children;

lazy_static! {
    static ref USER_HOST: Regex = Regex::new(r"^(.*?)@(.*)$").expect("Regex error USER_HOST");
    static ref IPV4: Regex = Regex::new(r"^(\d{1,3}\.?){4}$").expect("Regex error IPV4");
    static ref IPV6: Regex =
        Regex::new(r"^([0-9a-f]{0,4}:){2,7}(:|[0-9a-f]{1,4})$").expect("Regex error IPV6");
}

type NeedDns = HashMap<String, Vec<Option<String>>>;

#[derive(Debug)]
pub struct Cluster {
    tags: HashMap<String, Vec<String>>,
}

impl Default for Cluster {
    fn default() -> Self {
        Cluster {
            tags: HashMap::new(),
        }
    }
}

impl Cluster {
    pub fn get_cluster_entries(&mut self, config: &mut config::Config) -> Result<()> {
        self.read_cluster_file(Path::new("/etc/clusters"))?;

        // Check for config_dir/clusters
        // where config_dir is either $HOME/.tcssh or $HOME/.clusterssh
        if let Some(ref mut cluster_file) = config.tcssh.get_config_dir() {
            cluster_file.push("clusters");
            if cluster_file.exists() {
                self.read_cluster_file(&cluster_file)?;
            }
        }
        for p in &config.misc.extra_cluster_file {
            self.read_cluster_file(&p)?;
        }
        Ok(())
    }

    pub fn get_tag_entries(&mut self, config: &mut config::Config) -> Result<()> {
        self.read_tag_file(Path::new("/etc/tags"))?;

        // Check for config_dir/tags
        // where config_dir is either $HOME/.tcssh or $HOME/.clusterssh
        if let Some(ref mut tag_file) = config.tcssh.get_config_dir() {
            tag_file.push("tags");
            if tag_file.exists() {
                self.read_tag_file(&tag_file)?;
            }
        }
        for p in &config.misc.extra_tag_file {
            self.read_tag_file(&p)?;
        }
        Ok(())
    }

    fn read_cluster_file(&mut self, filename: &Path) -> Result<()> {
        if filename.exists() {
            reader::read_file(filename, false, |key, value| {
                let tags: Vec<String> = value
                    .split_whitespace()
                    .map(std::string::ToString::to_string)
                    .collect();
                self.register_tag(key.to_string(), tags, false);
                // perl cssh Base.pm's load_file would handle repeated keys, in the config file,
                // by appending their values.
                // Since our file parsing reader.rs has no memory of previous keys/values,
                // it is up to this closure to append values via register_tag(..., false)
            })?;
        }
        Ok(())
    }

    fn read_tag_file(&mut self, filename: &Path) -> Result<()> {
        if filename.exists() {
            reader::read_file(filename, false, |key, value| {
                let tags: Vec<String> = value
                    .split_whitespace()
                    .map(std::string::ToString::to_string)
                    .collect();
                self.register_host(key.to_string(), tags);
            })?;
        }
        Ok(())
    }

    fn register_tag(&mut self, key: String, mut tags: Vec<String>, replace: bool) {
        match self.tags.entry(key) {
            Entry::Occupied(mut entry) => {
                let v = entry.get_mut();
                if replace {
                    v.clear();
                }
                v.append(&mut tags);
            }
            Entry::Vacant(entry) => {
                entry.insert(tags);
            }
        }
    }

    fn register_host(&mut self, host: String, tags: Vec<String>) {
        for tag in tags {
            match self.tags.entry(tag) {
                Entry::Occupied(mut entry) => {
                    let v = entry.get_mut(); // irksome asymmetry. here we have "let v" "v.push"
                    v.push(host.clone());
                    v.sort();
                }
                Entry::Vacant(entry) => {
                    let mut v = Vec::with_capacity(1); // while here we have "let mut v" "v.push"
                    v.push(host.clone()); // b/c this v is a mut Vec, while other v is a &mut Vec.
                    entry.insert(v);
                }
            }
        }
    }

    pub fn get_tag(&self, host: &str) -> Option<&Vec<String>> {
        self.tags.get(host)
    }

    pub fn list_tags(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .tags
            .keys()
            .map(std::string::ToString::to_string)
            .collect();
        v.sort();
        v
    }

    pub fn resolve_clusters(
        &mut self,
        hosts: &mut Vec<String>,
        use_all_a_records: bool,
    ) -> Result<Vec<String>> {
        // perl cssh appends to @servers while iterating over @servers.
        // In rust we cannot mutate a Vec if we're iterating over it.
        // So iterate over one Vec, while appending to another 'more_hosts'.
        let mut more_hosts = Vec::new();

        // perl cssh hits the network for DNS lookups serially,
        // we do it concurrently.  To do so, we altered the algorithm.
        // Specifically we loop first calling get_tag() for each host.
        // Once all tags are expanded, then (if requested), we resolve DNS,
        // after which we call register_tag(host,ips).
        //
        // Meanwhile perl cssh interleaves DNS resolution with tag expansion.
        // perl cssh calls get_tag(), if that fails, then (if requested) it'll
        // resolve DNS, and it stores that DNS resolution via register_tag(host,ips).
        // I think perl cssh did this so if a host is repeated in the CLI args,
        // then the previous DNS resolution is re-used (when the repeated host
        // is passed to get_tag()).
        //
        // But that interleaving of DNS resolution and IP lookup means
        // perl cssh and rust tcssh will yield different results for someone
        // who is;
        //   using --use-all-a-records
        //   and asked for a host which resolves multiple IPs,
        //   and at least one of those IPs is a tag in "~/.tcssh/cluster".
        // Perl cssh would expand that IP as a tag,
        // while rust tcssh would not expand that IP as a tag.
        // I feel this is a justifiable trade off.  We gain concurrent DNS
        // resolution, at the cost of people (mis)using IPs as tags.
        let mut need_dns = NeedDns::new();

        // In the most common case (use_all_a_records=false, and no tags),
        // the host strings are not cloned.  We pass a ref to filter(),
        // and _resolve_clusters() only allocates new strings if we're
        // doing tag expansion or DNS lookups.
        // So in the most common case 'out' stores the entries of 'hosts',
        // and nothing is added to 'more_hosts'.
        let mut out: Vec<String> = hosts
            .drain(..)
            .filter(|host| {
                self._resolve_clusters(host, use_all_a_records, &mut more_hosts, &mut need_dns)
            })
            .collect();

        // If filter found that 'host' is a tag and expands to 'foo', 'bar', ...
        // then 'more_hosts' now contains 'foo', 'bar', ...
        // and we go about calling _resolve_clusters on 'foo', 'bar', ...
        // expanding until nothing is left to expand.
        let mut sanity_check = 128;
        while !more_hosts.is_empty() {
            sanity_check -= 1;
            if sanity_check <= 0 {
                // This is not a limit on the number of hosts,
                // it is a limit on infinite tag expansion
                // e.g.
                //    $ cat ~/.tcssh/clusters
                //    foo bar
                //    bar foo
                //    $ tcssh foo
                eprintln!("excessive cluster resolution detected. Ending loop");
                break;
            }
            let mut tmp = more_hosts;
            more_hosts = Vec::new();

            for host in tmp.drain(..) {
                if self._resolve_clusters(&host, use_all_a_records, &mut more_hosts, &mut need_dns)
                {
                    out.push(host);
                }
            }
        }

        // Almost always need_dns is empty, and this is not run.
        // But if the user asked for --use-all-a-records
        // then we have some look ups to do.
        if !need_dns.is_empty() {
            let mut resolver = resolver::ResolverWrapper::new()?;

            let hosts = need_dns
                .keys()
                .map(std::string::ToString::to_string)
                .collect();
            let mut out2 = Vec::new();
            let mut register_tags2 = Vec::new();
            resolver.resolve(
                hosts, // get DNS for these hosts, and pass them to the closures below
                |host, ips| {
                    if handle_ip_resolution(&host, &ips, &mut out, &need_dns) {
                        // register_tag is only useful if someone
                        // uses the menu option "Add Host(s) or Cluster(s)"
                        // and they request a previously resolved tag.
                        self.register_tag(host, ips, true);
                    }
                },
                |host, _err_str| {
                    // error resolving host
                    // Maybe 'host' is an alias in ~/.cssh/config
                    // in which case, pass it through.
                    let ips = Vec::new();
                    if handle_ip_resolution(&host, &ips, &mut out2, &need_dns) {
                        register_tags2.push(host);
                    }
                },
            );
            // join results from both closures
            if !out2.is_empty() {
                out.append(&mut out2);
            }
            if !register_tags2.is_empty() {
                for host in register_tags2 {
                    self.register_tag(host, Vec::new(), true);
                }
            }
        }
        Ok(out)
    }

    fn _resolve_clusters(
        &self,
        host: &str,
        use_all_a_records: bool,
        more_hosts: &mut Vec<String>,
        need_dns: &mut NeedDns,
    ) -> bool {
        // extract (user,host) if host matches user_host aka ^.*@.*$
        let (user, host) = match USER_HOST.captures(host) {
            Some(cap) => {
                let user = match cap.get(1) {
                    Some(user) => {
                        let user = user.as_str();
                        if !user.is_empty() {
                            Some(user)
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                let host = match cap.get(2) {
                    // empty hosts are filtered much earlier by host.rs
                    // e.g. 'user@'  But if host.rs logic changes, then we should
                    // handle that case since our regex USER_HOST allows it.
                    Some(h) if h.as_str().is_empty() => return false,
                    Some(h) => h.as_str(),
                    _ => return false, // caller should not use host because it is junk
                };
                (user, host)
            }
            _ => (None, host),
        };

        let tags = self.get_tag(host);

        // perl cssh skipped DNS lookup if host looked like an IPV4.
        // we skip DNS lookup if it looks like IPv4 or IPV6.
        if use_all_a_records && tags.is_none() && !(IPV4.is_match(host) || IPV6.is_match(host)) {
            let user = user.map(std::string::ToString::to_string);
            match need_dns.entry(host.to_string()) {
                Entry::Occupied(mut entry) => {
                    entry.get_mut().push(user);
                }
                Entry::Vacant(entry) => {
                    let mut users = Vec::with_capacity(1);
                    users.push(user);
                    entry.insert(users);
                }
            }
            return false; // caller should not use host, because we've stuffed it into needs_dns
        }

        if let Some(tags) = tags {
            if !tags.is_empty() {
                for tag in tags {
                    // e.g.
                    //     $ cat ~/.tcssh/clusters
                    //     foo bar.com user1@baz.com
                    //
                    //     $ tcssh foo user2@foo
                    //
                    // The first CLI arg 'foo' gets us user=None, host=foo, and
                    // get_tag('foo') gives us tags 'bar.com', 'user1@baz.com'
                    // So more_hosts gets 'bar.com' and 'user1@baz.com' pushed.
                    //
                    // The second CLI arg 'user2@foo' is split so user=user2, host=foo, and
                    // get_tag('foo') gives us tags 'bar.com', 'user1@baz.com'
                    // So more_hosts gets user2@bar.com user2@baz.com pushed.
                    match user {
                        None => more_hosts.push(tag.clone()),
                        Some(user) => match USER_HOST.captures(&tag) {
                            Some(cap) => {
                                if let Some(host) = cap.get(2) {
                                    more_hosts.push(format!("{}@{}", user, host.as_str()));
                                } else {
                                    more_hosts.push(format!("{}@{}", user, tag));
                                }
                            }
                            None => {
                                more_hosts.push(format!("{}@{}", user, tag));
                            }
                        },
                    }
                }
                return false; // caller should not use host, because we've stuffed it into more_hosts
            }
        }
        true // caller should use host
    }
}

// Execute a command with hosts as args, read its output,
// and use those as the new set of hosts to use.
pub fn get_external_clusters(p: &Path, hosts: &[String]) -> Result<(Vec<String>)> {
    if !p.is_executable_file() {
        return Err("external cluster command is not executable".into());
    }
    // TODO.. Figure out how to swap signal handlers
    // so Menu "Add Host(s) or Cluster(s)" can call us.
    if wait_children::is_our_sig_handler_installed() {
        return Err("assertion failure. sig handler will interfere with spawned commands".into());
    }
    let mut command = Command::new(p);
    command.args(hosts);
    match command.output() {
        Ok(output) => {
            // treat no status as success, like perl cssh
            if let Some(status) = output.status.code() {
                if status != 0 {
                    // status is already >>8 from the raw value.
                    return Err(format!(
                        "External command failure.\nCommand: [{} {}]\nReturn Code: [{}]",
                        p.to_string_lossy(),
                        hosts.join(" "),
                        status
                    )
                    .into());
                }
            }
            match std::str::from_utf8(&output.stdout) {
                Ok(output) => Ok(output.trim_end().split(' ').map(String::from).collect()),
                Err(e) => Err(format!(
                    "ouput of external_cluster_command {} is not valid utf8. {}",
                    p.to_string_lossy(),
                    e
                )
                .into()),
            }
        }
        Err(e) => Err(e.into()),
    }
}

fn handle_ip_resolution(
    host: &str,
    ips: &[String],
    out: &mut Vec<String>,
    need_dns: &NeedDns,
) -> bool {
    match need_dns.get(host) {
        Some(users) => {
            // need_dns is a map of 'host' names to a list of users
            // e.g.  tcssh --use-all-a-records user1@foo user2@foo foo
            // then 'need_dns' for 'foo' contains [Some(user1), Some(user2), None]
            for user in users.iter() {
                if ips.len() <= 1 {
                    // if foo maps to one IP, then just use the host name
                    // (because --use-all-a-records only cares about multiple IPs)
                    //
                    // if foo maps to zero IPs (e.g. it is an alias within
                    // ~/.ssh/config) then we also just use the host name.
                    //
                    // So (continuing the example)
                    // e.g.  tcssh --use-all-a-records user1@foo user2@foo foo
                    // then 'out' becomes [ user1@foo, user2@foo, foo ];
                    match user {
                        None => out.push(host.to_string()),
                        Some(user) => out.push(format!("{}@{}", user, host)),
                    }
                } else {
                    for ip in ips.iter() {
                        // This time assume 'foo' resolves to 10.0.0.1 and 10.0.0.2
                        // e.g.  tcssh --use-all-a-records user1@foo user2@foo foo
                        // so 'out' becomes [
                        //     user1@10.0.0.1, user1@10.0.0.2,
                        //     user2@10.0.0.1, user2@10.0.0.2,
                        //           10.0.0.1,       10.0.0.2,
                        // ]
                        match user {
                            None => out.push(ip.to_string()),
                            Some(user) => out.push(format!("{}@{}", user, ip)),
                        }
                    }
                }
            }
            true
        }
        None => {
            eprintln!("Unrecognized host {}", host);
            // This shouldn't be possible.
            // We asked to resolve DNS for hosts ['foo']
            // and the call back is saying it has
            // resolved host='bar' to some IPs.
            false
        }
    }
}
