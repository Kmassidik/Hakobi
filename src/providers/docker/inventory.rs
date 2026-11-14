use std::collections::{BTreeMap, BTreeSet};

use super::{ContainerGroup, ContainerInventory, ContainerRow, ServiceGroup, StackGroup};

type RowsByService = BTreeMap<String, Vec<ContainerRow>>;

impl ContainerInventory {
    pub fn from_rows(rows: Vec<ContainerRow>) -> Self {
        let (groups, stack_map) = group_rows(&rows);
        let groups = container_groups(groups);
        let stacks = stack_groups(stack_map);
        let used: BTreeSet<u16> = rows
            .iter()
            .flat_map(|row| row.ports.iter().filter_map(|port| port.host_port))
            .collect();
        let suggested_ports = (18000..18200)
            .filter(|port| !used.contains(port))
            .take(5)
            .collect();
        Self {
            groups,
            stacks,
            suggested_ports,
        }
    }
}

fn group_rows(rows: &[ContainerRow]) -> (RowsByService, BTreeMap<String, RowsByService>) {
    let mut groups = RowsByService::new();
    let mut stack_map = BTreeMap::<String, RowsByService>::new();
    for row in rows {
        let group = row
            .compose_project
            .clone()
            .unwrap_or_else(|| "Standalone".into());
        groups.entry(group.clone()).or_default().push(row.clone());
        let service = row
            .compose_service
            .clone()
            .unwrap_or_else(|| row.name.clone());
        stack_map
            .entry(group)
            .or_default()
            .entry(service)
            .or_default()
            .push(row.clone());
    }
    (groups, stack_map)
}

fn container_groups(groups: RowsByService) -> Vec<ContainerGroup> {
    let mut groups = groups
        .into_iter()
        .map(|(name, containers)| ContainerGroup { name, containers })
        .collect::<Vec<_>>();
    groups.sort_by(|left, right| {
        let left_standalone = left.name == "Standalone";
        let right_standalone = right.name == "Standalone";
        left_standalone
            .cmp(&right_standalone)
            .then_with(|| left.name.cmp(&right.name))
    });
    groups
}

fn stack_groups(stack_map: BTreeMap<String, RowsByService>) -> Vec<StackGroup> {
    let mut stacks = stack_map
        .into_iter()
        .map(|(name, services)| stack_group(name, services))
        .collect::<Vec<_>>();
    stacks.sort_by(|left, right| {
        let left_unmanaged = left.name == "Standalone" || left.name == "Unmanaged";
        let right_unmanaged = right.name == "Standalone" || right.name == "Unmanaged";
        left_unmanaged
            .cmp(&right_unmanaged)
            .then_with(|| left.name.cmp(&right.name))
    });
    stacks
}

fn stack_group(name: String, services: RowsByService) -> StackGroup {
    let mut services = services
        .into_iter()
        .map(|(name, containers)| ServiceGroup { name, containers })
        .collect::<Vec<_>>();
    services.sort_by(|left, right| left.name.cmp(&right.name));
    let container_count = services
        .iter()
        .map(|service| service.containers.len())
        .sum();
    let running_count = services
        .iter()
        .flat_map(|service| &service.containers)
        .filter(|container| container.state == "running")
        .count();
    StackGroup {
        name,
        services,
        container_count,
        running_count,
    }
}
