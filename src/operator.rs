use std::cell::RefCell;

use itertools::{iproduct, Itertools};
use sha256::digest;

use crate::{logging::*, Error, Interface, Node, Result, Service, State};

pub trait Backend {
    fn read_state(&mut self);
    fn apply_rules(
        &mut self,
        rules: impl IntoIterator<Item = Rule>,
    ) -> Result<()>;
    fn delete_rules<P>(&mut self, pred: P) -> Result<()>
    where
        P: FnMut(&&str) -> bool;
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Rule {
    pub node: Node,
    pub service: Service,
    pub interface: Interface,
    pub num_nodes: usize,
    pub node_index: usize,
}

impl Rule {
    pub fn rule_id(&self) -> String {
        let mut rule_id = digest(format!(
            "{}::{}::{}::{}::{}::{}",
            self.service_id(),
            self.node.addr,
            self.num_nodes,
            self.node_index,
            self.interface.name,
            self.interface.is_external,
        ));
        rule_id.truncate(16);
        rule_id
    }

    pub fn service_id(&self) -> String {
        let mut svc_hash = digest(self.service.fqn());
        svc_hash.truncate(16);
        let port_hash = &self.service.external_ports.specs.iter().join("::");
        let mut service_id = digest(format!(
            "{svc_hash}{port_hash}{}{}",
            self.service.is_internal,
            self.service.allow_range.to_owned().unwrap_or_else(|| "".into())
        ));
        service_id.truncate(16);
        service_id
    }
}

pub struct Operator<B> {
    backend: RefCell<B>,
}

impl<B: Backend> Operator<B> {
    pub fn new(backend: B) -> Self { Self { backend: RefCell::new(backend) } }

    pub fn reconcile(&self, state: &State, prev_state: &State) -> Result<()> {
        let (added, removed) = state.diff(prev_state);
        if added.is_empty() && removed.is_empty() {
            return Ok(());
        }

        info!("added state: {added:?}");
        info!("removed state: {removed:?}");

        let mut backend = self.backend.borrow_mut();
        backend.read_state();

        // Case 1: same node set + same interfaces
        if state.get::<Node>() == prev_state.get::<Node>()
            && state.get::<Interface>() == prev_state.get::<Interface>()
        {
            let removed_service_ids =
                make_rules(&state.clone().with(removed.get::<Service>()))
                    .iter()
                    .map(Rule::service_id)
                    .collect::<Vec<_>>();

            backend
                .apply_rules(make_rules(
                    &state.clone().with(added.get::<Service>()),
                ))
                .map_err(|e| Error::OperatorError(Box::new(e)))?;

            return backend.delete_rules(|&rule| {
                removed_service_ids
                    .iter()
                    .any(|service_id| rule.contains(service_id))
            });
        }

        // Case 2: node or interface added or removed => full cycle
        let new_rules = make_rules(state);

        let new_rule_ids =
            new_rules.iter().map(Rule::rule_id).collect::<Vec<_>>();

        backend
            .apply_rules(new_rules)
            .map_err(|e| Error::OperatorError(Box::new(e)))?;

        backend
            .delete_rules(|&rule| {
                new_rule_ids
                    .iter()
                    .all(|new_rule_id| !rule.contains(new_rule_id))
            })
            .map_err(|e| Error::OperatorError(Box::new(e)))
    }

    pub fn cleanup(&self) -> Result<()> {
        let mut backend = self.backend.borrow_mut();
        backend.read_state();
        backend
            .delete_rules(|_| true)
            .map_err(|e| Error::OperatorError(Box::new(e)))
    }
}

fn make_rules(state: &State) -> Vec<Rule> {
    let num_nodes = state.get::<Node>().len();
    iproduct!(
        state.get::<Node>().iter().enumerate(),
        &state.get::<Service>(),
        &state.get::<Interface>()
    )
    .map(|((node_index, node), service, interface)| {
        if interface.is_external && service.is_internal {
            return None;
        }
        Some(Rule {
            node: node.to_owned(),
            service: service.to_owned(),
            interface: interface.to_owned(),
            num_nodes,
            node_index,
        })
    })
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{res::Proto, ExternalPorts, PortSpec};

    #[derive(Default)]
    struct TestBackend {
        rules: Vec<Rule>,
    }

    impl Operator<TestBackend> {
        fn get_rules(&self) -> Vec<Rule> {
            self.backend.borrow().rules.clone()
        }
    }

    impl Backend for TestBackend {
        fn read_state(&mut self) {
            // noop: we keep state in memory
        }

        fn apply_rules(
            &mut self,
            rules: impl IntoIterator<Item = Rule>,
        ) -> Result<()> {
            for rule in rules {
                self.rules.push(rule);
            }
            Ok(())
        }

        fn delete_rules<P>(&mut self, mut pred: P) -> Result<()>
        where
            P: FnMut(&&str) -> bool,
        {
            self.rules.retain(|r| {
                !pred(&format!("{} {}", r.rule_id(), r.service_id()).as_str())
            });
            Ok(())
        }
    }

    #[test]
    fn test_trivial() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let res = operator.reconcile(&State::default(), &State::default());
        assert!(res.is_ok());

        let rules = operator.get_rules();
        assert!(rules.is_empty());
    }

    #[test]
    fn it_replaces_svc_on_port_change() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let state0 = empty_state();

        let state1 = state0.clone().with([single_port_service(123, 456)]);
        operator.reconcile(&state1, &state0).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].service.external_ports,
            single_port_spec(123, 456)
        );

        let state2 = state1.clone().with([single_port_service(1234, 456)]);
        operator.reconcile(&state2, &state1).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].service.external_ports,
            single_port_spec(1234, 456)
        );
    }

    #[test]
    fn it_replaces_svc_on_interal_change() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let state0 = empty_state().with([Interface::new("eth0").external()]);

        let svc = single_port_service(123, 456);
        let state1 = state0.clone().with([svc.clone()]);
        operator.reconcile(&state1, &state0).unwrap();

        // A normal service should get a rule even for external interfaces
        let rules = operator.get_rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].service.external_ports,
            single_port_spec(123, 456)
        );

        // However once the service goes "internal" the rule should be gone
        let state2 = state1.clone().with([svc.internal()]);
        operator.reconcile(&state2, &state1).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 0);

        // When we make the interface internal again, the rule should pop up
        let state3 = state2.clone().with([Interface::new("eth0")]);
        operator.reconcile(&state3, &state2).unwrap();
        let rules = operator.get_rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].service.external_ports,
            single_port_spec(123, 456)
        );
    }

    #[test]
    fn it_deletes_all_rules_when_no_nodes_left() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let state0 = empty_state();

        let state1 = state0.clone().with([single_port_service(123, 456)]);
        operator.reconcile(&state1, &state0).unwrap();

        let state2 = state1.clone().with(Vec::<Node>::new());
        operator.reconcile(&state2, &state1).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 0);
    }

    #[test]
    fn it_handles_service_remove_node_add_correctly() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let state0 = empty_state();
        let state1 = state0.clone().with([
            single_port_service(123, 456),
            single_port_service(789, 654),
        ]);
        operator.reconcile(&state1, &state0).unwrap();

        // add a node, remove a service
        let state2 = state1
            .clone()
            .with([
                Node {
                    name: "foo".to_string(),
                    addr: "bar".to_string(),
                    is_active: true,
                },
                Node {
                    name: "foo_two".to_string(),
                    addr: "bar_two".to_string(),
                    is_active: true,
                },
            ])
            .with([single_port_service(789, 654)]);
        operator.reconcile(&state2, &state1).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 2);
        assert!(rules
            .iter()
            .all(|x| x.service.external_ports == single_port_spec(789, 654)));
    }

    #[test]
    fn it_removes_services() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let state0 = empty_state().with([single_port_service(123, 456)]);
        operator.reconcile(&state0, &empty_state()).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].service.external_ports,
            single_port_spec(123, 456)
        );

        let state1 = state0.clone().with(Vec::<Node>::new());
        operator.reconcile(&state1, &state0).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 0);
    }

    #[test]
    fn it_supports_multiple_ports() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let state0 = empty_state().with([single_port_service(123, 456)]);
        operator.reconcile(&state0, &empty_state()).unwrap();

        let state1 = state0.clone().with([service_with_ep(ExternalPorts {
            specs: vec![
                PortSpec::new_tcp(123, 456),
                PortSpec::new_tcp(321, 654),
            ],
        })]);
        operator.reconcile(&state1, &state0).unwrap();

        let rules = operator.get_rules();
        assert_eq!(
            rules[0].service.external_ports,
            ExternalPorts {
                specs: vec![
                    PortSpec::new_tcp(123, 456),
                    PortSpec::new_tcp(321, 654),
                ],
            }
        );
    }

    #[test]
    fn it_supports_udp() {
        let backend = TestBackend::default();
        let operator = Operator::new(backend);

        let state0 = empty_state().with([single_port_service(123, 456)]);
        operator.reconcile(&state0, &empty_state()).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].service.external_ports,
            single_port_spec(123, 456)
        );

        let state1 = state0.clone().with([service_with_ep(ExternalPorts {
            specs: vec![PortSpec {
                host_port: 123,
                node_port: 456,
                proto: Proto::Udp,
            }],
        })]);

        operator.reconcile(&state1, &state0).unwrap();

        let rules = operator.get_rules();
        assert_eq!(rules.len(), 1);

        assert_eq!(
            rules[0].service.external_ports,
            ExternalPorts {
                specs: vec![PortSpec {
                    host_port: 123,
                    node_port: 456,
                    proto: Proto::Udp,
                }],
            }
        );
    }

    fn empty_state() -> State {
        State::default().with(vec![Interface::new("eth0")]).with([Node {
            name: "foo".to_string(),
            addr: "bar".to_string(),
            is_active: true,
        }])
    }

    fn single_port_spec(host_port: u16, node_port: u16) -> ExternalPorts {
        ExternalPorts { specs: vec![PortSpec::new_tcp(host_port, node_port)] }
    }

    fn single_port_service(host_port: u16, node_port: u16) -> Service {
        service_with_ep(single_port_spec(host_port, node_port))
    }

    fn service_with_ep(external_ports: ExternalPorts) -> Service {
        Service {
            name: "foo".to_string(),
            namespace: "bar".to_string(),
            external_ports,
            is_internal: false,
            allow_range: None,
        }
    }
}
