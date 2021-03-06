Transparent Cluster SSH

If you haven't used `cssh` before, it's a way of starting multiple xterms
each ssh-ed into some host, and every keypress you make (while the main
console has focus) gets sent to each xterm.

If you have used `cssh`, and the console was placed covering one of the xterms,
then you may want to try this, as it has a transparent console.

It's quicker than `cssh`, which had sleeps at various points,
and did DNS lookups serially before connecting to hosts.
`tcssh` skips the DNS lookups in almost all cases, but when asked
will perform the lookup in parallel.

1) `apt-get install rustc` # or if that doesn't work then install rust by https://www.rust-lang.org/ 
2) `apt-get install libatk1.0-dev libcairo2-dev libgdk-pixbuf2.0-dev libglib2.0-dev libgtk-3-dev libpango1.0-dev libx11-dev`
3) `rustc --version` # if your version is < 1.33 then either upgrade (hint: `rustup`) or `git checkout pre-2018`
4) `cargo build --release` # prepare for 1.1G of intermediate files. (see Build Errors)
5) `./target/release/tcssh --opacity 1 127.0.0.1` # 1 = opaque, 0.5 = semi-transparent, 0 = transparent.

# Common Build Errors

1) If you get an error about "edition 2018", then

    git checkout pre-2018

And build again.

2) Re: "error: linking with `cc` failed: exit code: 1", then

...

    /usr/bin/ld: cannot find -latk-1.0
    /usr/bin/ld: cannot find -lcairo
    /usr/bin/ld: cannot find -lcairo-gobject
    /usr/bin/ld: cannot find -lgdk-3
    /usr/bin/ld: cannot find -lgdk_pixbuf-2.0
    /usr/bin/ld: cannot find -lgio-2.0
    /usr/bin/ld: cannot find -lglib-2.0
    /usr/bin/ld: cannot find -lgobject-2.0
    /usr/bin/ld: cannot find -lgtk-3
    /usr/bin/ld: cannot find -lpango-1.0

then

    apt-get install libatk1.0-dev libcairo2-dev libgdk-pixbuf2.0-dev libglib2.0-dev libgtk-3-dev libpango1.0-dev libx11-dev


# Compatibility

`tcssh` is as backwards compatible with `cssh` as possible

The config directory is `~/.tcssh/` but if that does not exist
it will look for an existing `~/.clusterssh/` directory.

    ./tcssh --dump-config # To see what can be configured
    ./tcssh --help # for all options

Features which `tcssh` shares with `cssh`

    $ cat ~/.tcssh/clusters
    foo host1.example.com host2.example.com
    bar host1.example.com host2.example.com
    baz other.example.com

    $ cat ~/.tcssh/tags
    host3.example.com bar baz

    $ tcssh foo # opens xterms to host[12].example.com
    $ tcssh bar # opens xterms to host[123].example.com
    $ tcssh baz # opens xterms to {other,host3}.example.com


`tcssh` also allows `mosh` to be used instead of `ssh`, if it called as such.

    ln -s tcssh tcmosh
    ./tcmosh

`tcssh` parses `~/.ssh/config` for HostName and Host aliases and presents those
as options within the "Add Host(s) or Cluster(s)" dialog.

Where it is not compatible;

The "Add Host(s) or Cluster(s)" dialog uses only the clusters
defined in config files.  It does not spawn the command
configured as "external_cluster_command".  I'm leaving that bit commented out,
until I figure out how to swap signal handlers temporarily, or make the existing
one more robust.

No attempt is made to handle pasted text within the history window.

The tag resolution and DNS resolution logic differs from `cssh`, but only
if you're using `--use-all-a-records`,
and asked for a host which resolves multiple IPs,
and at least one of those IPs is a tag in `~/.tcssh/cluster` or `/etc/clusters`.
Perl `cssh` would expand that IP as a tag, while rust `tcssh` would not expand that IP as a tag.

Hot keys aren't enabled yet for macros (servername,hostname,username).


# Common tcmosh errors

If the `xterm` started by `tcmosh` closes too quickly to read any error,
then you can debug it via 

    tcmosh --evaluate ::1

and that may tell you

    /usr/bin/mosh: Could not connect to ::1: Address family for hostname not supported

so you adjust and 

    tcmosh --evaluate 127.0.0.1

and that may tell you to change your environment variable for `LANG` to something supporting UTF-8


