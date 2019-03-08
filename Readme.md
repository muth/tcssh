Transparent Cluster SSH

If you've used cssh, and the console was placed covering one of the xterms,
then you may want to try this, as it has a transparent console.

It's quicker than cssh, which had sleeps at various points,
and did DNS lookups serially before connecting to hosts.
tcssh skips the DNS lookups in almost all cases, but when asked
will perform the lookup in parallel.

1) Install rust https://www.rust-lang.org/
2) cargo build --release # prepare for 1.1G of intermediate files.
3) ./target/release/tcssh 127.0.0.1


Its main config directory is ~/.tcssh/ but if that does not exist
it will look for an existing ~/.clusterssh/ directory

Features which tcch shares with cssh

    $ cat ~/.tcssh/clusters
    foo host1.example.com host2.example.com
    bar host1.example.com host2.example.com
    baz other.example.com

    $ cat ~/.tcssh/tags
    host3.example.com bar baz

    $ tcssh foo # opens xterms to host[12].example.com
    $ tcssh bar # opens xterms to host[123].example.com
    $ tcssh baz # opens xterms to {other,host3}.example.com


tcssh also allows mosh to be used instead of ssh, if it called as such.

    ln -s tcssh tcmosh
    ./tcmosh

If the xterm started by tcmosh closes too quickly to read any error,
then you can debug it via 

    tcmosh --evaluate ::1

and that may tell you

    /usr/bin/mosh: Could not connect to ::1: Address family for hostname not supported

so you adjust and 

    tcmosh --evaluate 127.0.0.1

and that may tell you to change your environment variable for LANG to something supporting UTF-8


tcssh is as backwards compatible with cssh as possible

Where it is not compatible;

The "Add Host(s) or Cluster(s)" dialog uses only the clusters
defined in config files.  It does not spawn the command
configured as "external_cluster_command".  I'm leaving that bit commented out,
until I figure out how to swap signal handlers temporarily, or make the existing
one more robust.

No attempt is made to handle pasted text within the history window.

The tag resolution and DNS resolution logic differs from cssh, but only
if you have configured a tag which looks like an IPv6 address,
and you're connecting to a host with multiple A records.



If you get build errors.  Please note there is a .cargo/config file which is easy to overlook.
