use kube::runtime::watcher::Event;
use std::{collections::BTreeSet, ops::Sub, vec::IntoIter};

use crate::*;

pub type Interface = String;

#[derive(Clone, Default, Debug)]
pub struct State {
    pub interfaces: Vec<Interface>,
    pub services: BTreeSet<Service>,
    pub nodes: BTreeSet<Node>,
}

impl State {
    pub fn is_empty(&self) -> bool {
        self.services.is_empty() && self.nodes.is_empty()
    }
}

impl Sub for &State {
    type Output = State;

    fn sub(self, rhs: Self) -> Self::Output {
        State {
            interfaces: self.interfaces.clone(),
            services: &self.services - &rhs.services,
            nodes: &self.nodes - &rhs.nodes,
        }
    }
}

impl State {
    pub fn diff(&self, prev_state: &Self) -> (Self, Self) {
        let added = self - prev_state;
        let removed = prev_state - self;
        (added, removed)
    }

    pub fn with_interfaces(self, interfaces: Vec<Interface>) -> Self {
        Self { interfaces, ..self }
    }

    pub fn with_nodes(self, nodes: impl IntoIterator<Item = Node>) -> Self {
        Self {
            nodes: nodes.into_iter().collect(),
            ..self
        }
    }

    pub fn with_services(self, services: impl IntoIterator<Item = Service>) -> Self {
        Self {
            services: services.into_iter().collect(),
            ..self
        }
    }
}

#[derive(Debug, Clone)]
pub enum Op {
    NodeAdd(Node),
    NodeRemove(String),
    ServiceAdd(Service),
    ServiceRemove(String),
}

impl Op {
    pub fn apply(&self, state: &mut State) {
        match self {
            Op::NodeAdd(node) => {
                state.nodes.insert(node.to_owned());
            }
            Op::NodeRemove(node_name) => state.nodes.retain(|x| &x.name != node_name),
            Op::ServiceAdd(service) => {
                state.services.insert(service.to_owned());
            }
            Op::ServiceRemove(svc_fqn) => {
                state.services.retain(|s| &s.fqn() != svc_fqn);
            }
        }
    }
}

pub struct Ops(pub Vec<Op>);

impl IntoIterator for Ops {
    type Item = Op;
    type IntoIter = IntoIter<Op>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl From<Event<CoreService>> for Ops {
    fn from(event: Event<CoreService>) -> Self {
        let ops = match event {
            Event::Applied(obj) => Service::try_from(&obj).map(|svc| {
                let mut ret = vec![Op::ServiceRemove(svc.fqn())];
                if svc.has_external_port() {
                    ret.push(Op::ServiceAdd(svc))
                }
                ret
            }),
            Event::Restarted(objs) => Ok(objs
                .iter()
                .filter_map(|o| Service::try_from(o).ok())
                .filter(Service::has_external_port)
                .map(Op::ServiceAdd)
                .collect()),
            Event::Deleted(obj) => {
                Service::try_from(&obj).map(|svc| vec![Op::ServiceRemove(svc.fqn())])
            }
        };
        Ops(ops.unwrap_or_default())
    }
}

impl From<Event<CoreNode>> for Ops {
    fn from(event: Event<CoreNode>) -> Self {
        let ops = match event {
            Event::Applied(obj) => Node::try_from(&obj).map(|node| {
                let mut ret = vec![Op::NodeRemove(node.name.to_owned())];
                if node.is_active {
                    ret.push(Op::NodeAdd(node));
                }
                ret
            }),
            Event::Restarted(objs) => Ok(objs
                .iter()
                .filter_map(|o| Node::try_from(o).ok())
                .filter(|n| n.is_active)
                .map(Op::NodeAdd)
                .collect()),
            Event::Deleted(obj) => Node::try_from(&obj).map(|node| vec![Op::NodeRemove(node.name)]),
        };
        Ops(ops.unwrap_or_default())
    }
}
