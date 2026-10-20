use erisoverlaynetwork::net::Network;

#[test]
fn a_network_reads_from_its_name() {
    assert_eq!("".parse::<Network>().unwrap(), Network::Internet);
    assert_eq!("internet".parse::<Network>().unwrap(), Network::Internet);
    assert_eq!("loopback".parse::<Network>().unwrap(), Network::Loopback);
    let relay: Network = "http://127.0.0.1:3340".parse().unwrap();
    assert!(matches!(&relay, Network::Relay(url) if url.to_string().starts_with("http://127.0.0.1:3340")));
    assert_eq!(relay.to_string().parse::<Network>().unwrap(), relay);
    assert!("lan".parse::<Network>().is_err());
    assert!("https://".parse::<Network>().is_err());
}
