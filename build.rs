extern crate pkg_config;

use std::env;

fn main() {
    if cfg!(feature = "dox") {
        return;
    }

    let deps = [("x11", "1.4.99.1", "xlib")];

    for &(dep, version, feature) in deps.iter() {
        let var = format!("CARGO_FEATURE_{}", feature.to_uppercase().replace('-', "_"));
        if env::var_os(var).is_none() {
            continue;
        }
        pkg_config::Config::new()
            .atleast_version(version)
            .probe(dep)
            .unwrap();
    }
}
