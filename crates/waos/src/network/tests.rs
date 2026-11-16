use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use waos_capabilities::{DomainRule, NetworkGrant};

use super::test_grants::{TestGrants, any, any_but, off, only};
use super::{LOCAL_NETWORK, Lookup, LookupError, ProgramNetwork, Resolver};
use crate::access::{Cause, ProgramAccess, Refusal, Told};

/// An address of the internet's, which no test reaches.
const PUBLIC: &str = "93.184.216.34";

#[test]
fn a_name_is_looked_up_when_the_rule_allows_it_or_for_its_local_addresses() {
    let grants = TestGrants::new(only(&["example.com"]), false);
    let network = ProgramNetwork::new(ProgramAccess::new(grants.clone()));
    assert_eq!(
        network.plan_lookup("API.Example.com"),
        Ok(Lookup::Resolve {
            name: "api.example.com".to_owned(),
            by_rule: true
        })
    );
    assert_eq!(
        network.plan_lookup("tracker.org"),
        Err(LookupError::Refused(Refusal::by("domains")))
    );

    grants.set_local_network(true);
    assert_eq!(
        network.plan_lookup("tracker.org"),
        Ok(Lookup::Resolve {
            name: "tracker.org".to_owned(),
            by_rule: false
        }),
        "for its local addresses alone"
    );

    grants.set_network(NetworkGrant {
        on: false,
        domains: DomainRule::Any,
    });
    grants.set_local_network(false);
    assert_eq!(
        network.plan_lookup("example.com"),
        Err(LookupError::Refused(Refusal::by("network")))
    );
}

#[test]
fn an_address_answers_itself_and_text_that_names_no_host_is_invalid() {
    let network = ProgramNetwork::new(ProgramAccess::new(TestGrants::new(off(), false)));
    assert_eq!(
        network.plan_lookup(PUBLIC),
        Ok(Lookup::Known(vec![PUBLIC.parse().unwrap()]))
    );
    assert_eq!(
        network.plan_lookup("::1"),
        Ok(Lookup::Known(vec!["::1".parse().unwrap()]))
    );
    assert_eq!(network.plan_lookup("not a name"), Err(LookupError::Invalid));
}

#[test]
fn a_name_the_rule_allows_answers_every_address_and_records_it() {
    let network = ProgramNetwork::resolving_with(
        ProgramAccess::new(TestGrants::new(only(&["allowed.test"]), false)),
        answering(&["127.0.0.1", PUBLIC]),
    );
    let addresses = network.resolve("allowed.test", true).unwrap();
    assert_eq!(addresses, addresses_of(&["127.0.0.1", PUBLIC]));
    assert_eq!(
        network.record.names_of("127.0.0.1".parse().unwrap()),
        ["allowed.test"]
    );
}

#[test]
fn a_name_looked_up_for_the_local_network_answers_its_local_addresses_unrecorded() {
    let network = ProgramNetwork::resolving_with(
        ProgramAccess::new(TestGrants::new(off(), true)),
        answering(&["127.0.0.1", PUBLIC]),
    );
    let addresses = network.resolve("local.test", false).unwrap();
    assert_eq!(addresses, addresses_of(&["127.0.0.1"]));
    assert!(
        network
            .record
            .names_of("127.0.0.1".parse().unwrap())
            .is_empty()
    );
}

#[test]
fn a_localhost_name_is_this_computer_whatever_the_system_resolver_would_answer() {
    let network = ProgramNetwork::resolving_with(
        ProgramAccess::new(TestGrants::new(off(), true)),
        never_asked(),
    );
    for name in [
        "localhost",
        "LocalHost.",
        "app.localhost",
        "a.b.app.localhost.",
    ] {
        assert_eq!(
            network.plan_lookup(name),
            Ok(Lookup::Known(addresses_of(&["::1", "127.0.0.1"]))),
            "{name}"
        );
        assert_eq!(
            network.reachable(name, Cause::Program),
            Ok(addresses_of(&["::1", "127.0.0.1"])),
            "{name}"
        );
    }
    assert!(
        network
            .record
            .names_of("127.0.0.1".parse().unwrap())
            .is_empty(),
        "unrecorded"
    );
}

#[test]
fn a_localhost_name_is_refused_with_the_local_network_switch_off() {
    for network_grant in [any(), only(&["app.localhost"]), off()] {
        let network = ProgramNetwork::resolving_with(
            ProgramAccess::new(TestGrants::new(network_grant, false)),
            never_asked(),
        );
        assert_eq!(
            network.plan_lookup("app.localhost"),
            Err(LookupError::Refused(LOCAL_NETWORK))
        );
        assert_eq!(
            network.reachable("localhost", Cause::Program),
            Err(LookupError::Refused(LOCAL_NETWORK))
        );
    }
}

#[test]
fn the_domain_rule_neither_allows_nor_refuses_a_localhost_name() {
    for network_grant in [only(&["example.com"]), any_but(&["app.localhost"]), off()] {
        let network = ProgramNetwork::resolving_with(
            ProgramAccess::new(TestGrants::new(network_grant, true)),
            never_asked(),
        );
        assert_eq!(
            network.reachable("app.localhost", Cause::Program),
            Ok(addresses_of(&["::1", "127.0.0.1"]))
        );
    }
}

#[test]
fn a_name_with_a_localhost_label_elsewhere_is_resolved_by_the_system() {
    let network = ProgramNetwork::resolving_with(
        ProgramAccess::new(TestGrants::new(only(&["localhost.example"]), false)),
        answering(&[PUBLIC]),
    );
    assert_eq!(
        network.plan_lookup("localhost.example"),
        Ok(Lookup::Resolve {
            name: "localhost.example".to_owned(),
            by_rule: true
        })
    );
    assert_eq!(
        network.reachable("localhost.example", Cause::Program),
        Ok(addresses_of(&[PUBLIC]))
    );
}

/// A resolver answering `addresses` for every name.
fn answering(addresses: &[&str]) -> Resolver {
    let addresses = addresses_of(addresses);
    Arc::new(move |_name: &str| Ok(addresses.clone()))
}

/// A resolver a test must never ask, as the system's must not be for a localhost name.
fn never_asked() -> Resolver {
    Arc::new(|name: &str| panic!("the resolver was asked for {name}"))
}

fn addresses_of(addresses: &[&str]) -> Vec<IpAddr> {
    addresses
        .iter()
        .map(|address| address.parse().unwrap())
        .collect()
}

#[test]
fn a_local_address_is_reached_under_the_local_network_switch_alone() {
    let grants = TestGrants::new(any(), false);
    let network = ProgramNetwork::new(ProgramAccess::new(grants.clone()));
    for address in ["127.0.0.1", "192.168.1.20", "fe80::1"] {
        assert!(
            !network.reach(address.parse().unwrap()).is_ok(),
            "{address}"
        );
    }
    grants.set_local_network(true);
    grants.set_network(off());
    for address in ["127.0.0.1", "192.168.1.20", "fe80::1"] {
        assert!(network.reach(address.parse().unwrap()).is_ok(), "{address}");
    }
}

#[test]
fn an_address_of_the_internet_needs_the_network_switch_and_its_rule() {
    let grants = TestGrants::new(any(), true);
    let network = ProgramNetwork::new(ProgramAccess::new(grants.clone()));
    let public: IpAddr = PUBLIC.parse().unwrap();
    assert!(network.reach(public).is_ok(), "any domain");

    grants.set_network(only(&["example.com"]));
    assert!(
        !network.reach(public).is_ok(),
        "a bare address under only some"
    );
    network.record.keep("www.example.com", &[public]);
    assert!(
        network.reach(public).is_ok(),
        "looked up by an allowed name"
    );

    grants.set_network(only(&["example.org"]));
    assert!(
        !network.reach(public).is_ok(),
        "the rule is read at each check"
    );

    grants.set_network(any_but(&["example.com"]));
    assert!(
        !network.reach(public).is_ok(),
        "looked up by a refused name"
    );
    assert!(
        network.reach("8.8.8.8".parse().unwrap()).is_ok(),
        "a bare address"
    );

    grants.set_network(off());
    assert!(
        !network.reach("8.8.8.8".parse().unwrap()).is_ok(),
        "the switch off"
    );
}

#[test]
fn a_socket_listens_on_the_local_network_and_reaches_what_it_may() {
    let grants = TestGrants::new(any(), false);
    let network = ProgramNetwork::new(ProgramAccess::new(grants.clone()));
    let wildcard: SocketAddr = "0.0.0.0:0".parse().unwrap();
    let loopback: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let public: SocketAddr = format!("{PUBLIC}:443").parse().unwrap();
    assert!(!network.allows_listen(wildcard));
    assert!(!network.allows_connect(loopback));
    assert!(network.allows_connect(public));
    assert!(network.allows_send_to(public));
    assert!(!network.allows_receive_from(loopback));

    grants.set_local_network(true);
    assert!(network.allows_listen(wildcard));
    assert!(network.allows_accept(loopback));
}

#[test]
fn each_refusal_of_a_lookup_a_connection_a_listener_and_a_datagram_is_told_by_its_setting() {
    let grants = TestGrants::new(off(), false);
    let access = ProgramAccess::new(grants.clone());
    let network = ProgramNetwork::resolving_with(access.clone(), never_asked());
    let loopback: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let public: SocketAddr = format!("{PUBLIC}:443").parse().unwrap();

    assert!(network.look_up("localhost").is_err());
    assert!(network.look_up("example.com").is_err());
    assert!(!network.allows_connect(loopback));
    assert!(!network.allows_accept(public));
    assert!(!network.allows_listen(loopback));
    assert_eq!(
        access.refusals(),
        [
            "local-network",
            "network",
            "local-network",
            "network",
            "local-network"
        ]
    );

    grants.set_network(only(&["example.org"]));
    assert!(network.reachable(PUBLIC, Cause::MediaPlayer).is_err());
    assert_eq!(access.refusals().last(), Some(&"domains"));

    assert!(!network.allows_send_to(public));
    let noted = access.told().last().cloned();
    assert!(
        matches!(&noted, Some(Told::Noted(datagram)) if datagram.contains("domains")),
        "a datagram is noted with what refused it: {noted:?}"
    );
}
