//! Grants for the network's tests: the network and the local network, which a test sets, and
//! nothing else.

use std::future::pending;
use std::sync::{Arc, Mutex};

use waos_capabilities::{
    Answer, ChangedFuture, DomainRule, FileSystemGrant, NetworkGrant, ProgramGrants, Question,
};

pub(crate) fn any() -> NetworkGrant {
    NetworkGrant {
        on: true,
        domains: DomainRule::Any,
    }
}

pub(crate) fn off() -> NetworkGrant {
    NetworkGrant {
        on: false,
        domains: DomainRule::Any,
    }
}

pub(crate) fn only(names: &[&str]) -> NetworkGrant {
    NetworkGrant {
        on: true,
        domains: DomainRule::Only(names.iter().map(|name| name.parse().unwrap()).collect()),
    }
}

pub(crate) fn any_but(names: &[&str]) -> NetworkGrant {
    NetworkGrant {
        on: true,
        domains: DomainRule::AnyBut(names.iter().map(|name| name.parse().unwrap()).collect()),
    }
}

/// Grants of nothing but the network and the local network, which the test sets.
pub(crate) struct TestGrants {
    network: Mutex<NetworkGrant>,
    local_network: Mutex<bool>,
}

impl TestGrants {
    pub(crate) fn new(network: NetworkGrant, local_network: bool) -> Arc<TestGrants> {
        Arc::new(TestGrants {
            network: Mutex::new(network),
            local_network: Mutex::new(local_network),
        })
    }

    pub(crate) fn set_network(&self, network: NetworkGrant) {
        *self.network.lock().unwrap() = network;
    }

    pub(crate) fn set_local_network(&self, on: bool) {
        *self.local_network.lock().unwrap() = on;
    }
}

impl ProgramGrants for TestGrants {
    fn network(&self) -> NetworkGrant {
        self.network.lock().unwrap().clone()
    }

    fn local_network(&self) -> bool {
        *self.local_network.lock().unwrap()
    }

    fn clipboard_read(&self) -> bool {
        false
    }

    fn clipboard_write(&self) -> bool {
        false
    }

    fn file_system(&self) -> FileSystemGrant {
        FileSystemGrant::default()
    }

    fn answer(&self, _question: Question) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn changed(&self) -> ChangedFuture {
        Box::pin(pending())
    }
}
