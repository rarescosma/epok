use std::{any::TypeId, str::FromStr};

use anyhow::{anyhow, Context};
use enum_dispatch::enum_dispatch;
use k8s_openapi::api::core::v1::NodeStatus;
use kube::ResourceExt;

use crate::*;

#[enum_dispatch]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Resource {
    Interface,
    Node,
    Service,
}

#[enum_dispatch(Resource)]
pub trait ResourceLike {
    fn id(&self) -> String;
    fn type_id(&self) -> TypeId;
    fn is_active(&self) -> bool;
}

impl TryFrom<CoreService> for Resource {
    type Error = anyhow::Error;

    fn try_from(cs: CoreService) -> Result<Self, Self::Error> {
        Ok(Service {
            external_port: ExternalPort::try_from(cs.clone())?,
            name: cs.name_any(),
            namespace: cs.namespace().unwrap_or_default(),
        }
        .into())
    }
}

impl TryFrom<CoreNode> for Resource {
    type Error = anyhow::Error;

    fn try_from(cn: CoreNode) -> Result<Self, Self::Error> {
        let status = cn.status.clone().unwrap_or_default();
        let addr = node_ip(status.clone())?;
        let is_active = node_ready(status)
            && !cn.annotations().contains_key(NODE_EXCLUDE_ANNOTATION)
            && !cn.labels().contains_key(NODE_EXCLUDE_LABEL);

        Ok(Node {
            name: cn.name_any(),
            addr,
            is_active,
        }
        .into())
    }
}

pub type Interface = String;

impl ResourceLike for Interface {
    fn id(&self) -> String {
        self.to_owned()
    }

    fn type_id(&self) -> TypeId {
        TypeId::of::<Interface>()
    }

    fn is_active(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Service {
    pub name: String,
    pub namespace: String,
    pub external_port: ExternalPort,
}

impl Service {
    pub fn fqn(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }

    pub fn has_external_port(&self) -> bool {
        !matches!(self.external_port, ExternalPort::Absent)
    }

    pub fn get_ports(&self) -> Result<(u16, u16), anyhow::Error> {
        match self.external_port {
            ExternalPort::Spec {
                host_port,
                node_port,
            } => Ok((host_port, node_port)),
            ExternalPort::Absent => Err(anyhow!("invalid service")),
        }
    }
}

impl ResourceLike for Service {
    fn id(&self) -> String {
        self.fqn()
    }

    fn type_id(&self) -> TypeId {
        TypeId::of::<Service>()
    }

    fn is_active(&self) -> bool {
        self.has_external_port()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExternalPort {
    Spec { host_port: u16, node_port: u16 },
    Absent,
}

impl TryFrom<CoreService> for ExternalPort {
    type Error = anyhow::Error;

    fn try_from(cs: CoreService) -> Result<Self, Self::Error> {
        let anno = cs.annotations();
        if anno.contains_key(ANNOTATION) {
            return ExternalPort::from_str(&anno[ANNOTATION]);
        }
        Ok(ExternalPort::Absent)
    }
}

impl FromStr for ExternalPort {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts = s.split(':').collect::<Vec<_>>();
        match parts.len() {
            2 => Ok(Self::Spec {
                host_port: parts[0].parse()?,
                node_port: parts[1].parse()?,
            }),
            _ => Err(anyhow!("failed to parse annotation")),
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct Node {
    pub name: String,
    pub addr: String,
    pub is_active: bool,
}

impl ResourceLike for Node {
    fn id(&self) -> String {
        self.name.to_owned()
    }

    fn type_id(&self) -> TypeId {
        TypeId::of::<Node>()
    }

    fn is_active(&self) -> bool {
        self.is_active
    }
}

fn node_ip(status: NodeStatus) -> anyhow::Result<String> {
    for add in status.addresses.context("node missing addresses")? {
        if add.type_ == "InternalIP" {
            return Ok(add.address);
        }
    }
    Err(anyhow!("failed to extract node ip"))
}

fn node_ready(status: NodeStatus) -> bool {
    status
        .conditions
        .unwrap_or_default()
        .iter()
        .any(|c| c.type_ == "Ready" && c.status == "True")
}
