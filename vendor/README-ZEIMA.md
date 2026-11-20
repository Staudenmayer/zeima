# vendor/extism

Extism 1.41.0 with one change, in `src/pdk.rs` (search for "Zeima patch"): the add-on HTTP host function builds its
agent with no redirects and a resolver that refuses addresses inside the machine or its network. The allow-list of an
add-on names hosts, and the stock library checks only the first URL and follows redirects, so an allowed host could
send an add-on to `127.0.0.1` or a cloud metadata address (security report PT5-02).

Hosts the person typed into a setting (`host_settings`) may be private: the app lists them in the plugin config key
`zeima.private_hosts`. Removing this patch needs the same behaviour from upstream.

Add-ons ignore proxy environment variables (`HTTP_PROXY` and friends): a proxy would resolve the target itself and skip
the address filter. The request URL must be http(s) without userinfo, and the parsed URL is what gets requested.
