// Concurrent DNS lookups (only used when --use-all-a-records)
//
// Tested via a CLI app which just passes all CLI args to this to resolve.
// then setup 1 second delay on all network traffic via
//     tc qdisc add dev lo root netem delay 1000ms
// And setup logging to see what's going over to the net
//     iptables -A OUTPUT -o lo -j LOG
// Which may be visible via
//     tail -f /var/log/syslog
// cleanup via "tc qdisc del dev lo root netem" and "iptables -X OUTPUT" but YMMV
//
// With concurrent lookup the total time one lookup == the total time for 10 lookups.
// That proved to me that this works, but a unit test for that, isn't happening.

use futures::future;
use tokio::runtime::current_thread::Runtime;
use trust_dns_resolver::AsyncResolver;

pub struct ResolverWrapper {
    async_resolver: AsyncResolver,
    runtime: Runtime,
}

impl ResolverWrapper {
    pub fn new() -> Result<Self, std::io::Error> {
        let mut runtime = Runtime::new()?;
        let (async_resolver, background) = AsyncResolver::from_system_conf()?;
        runtime.spawn(background);
        Ok(Self {
            async_resolver,
            runtime,
        })
    }

    // Take a Vec<String> to resolve.
    // Sucessful resoltions call callback F(host,ips)
    // Failed resoltions call callback G(host,error_string)
    pub fn resolve<F, G>(&mut self, mut hosts: Vec<String>, mut f: F, mut g: G)
    where
        F: FnMut(String, Vec<String>), // F(host, ips)
        G: FnMut(String, String),      // G(host, error)
    {
        if hosts.is_empty() {
            return;
        }
        // create a future per lookup request
        let lookup_futures = hosts
            .iter()
            .map(|x| self.async_resolver.lookup_ip(x.as_str()));

        let mut lookup_futures = future::select_all(lookup_futures);
        // loops without explicit termination conditions make me nervous.
        // But every iteration reduces the size of results by 1, and we stop when empty.
        loop {
            // block_on() returns when one is ready
            let results = self.runtime.block_on(lookup_futures);
            let next_futures = match results {
                Ok(results) => {
                    // remove 'result' at 'index' from 'next_future' and repeat
                    let (result, index, next_futures) = results;
                    let hit_host = hosts.remove(index);
                    let ips = result.iter().map(|x| x.to_string()).collect();
                    f(hit_host, ips);
                    next_futures
                }
                Err(results) => {
                    let (e, index, next_futures) = results;
                    let hit_host = hosts.remove(index);
                    g(hit_host, e.to_string());
                    next_futures
                }
            };
            if next_futures.is_empty() {
                break;
            }
            lookup_futures = future::select_all(next_futures);
        }
    }
}
